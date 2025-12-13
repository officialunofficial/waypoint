use crate::proto::{HubEvent, hub_event};
use color_eyre::eyre::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;
use tokio_util::io::StreamReader;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize)]
struct SpamLabel {
    provider: u64,
    #[serde(rename = "type")]
    type_info: LabelType,
    label_type: String,
    label_value: u32,
    timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct LabelType {
    target: String,
    fid: u64,
}

pub struct SpamFilter {
    spam_fids: Arc<RwLock<HashSet<u64>>>,
    nerfed_fids: Arc<RwLock<HashSet<u64>>>,
    http_client: Arc<Client>,
    last_update: Arc<RwLock<SystemTime>>,
}

impl Default for SpamFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of fetching and parsing the spam label list
struct SpamListResult {
    spam_fids: HashSet<u64>,
    nerfed_fids: HashSet<u64>,
}

/// Classification of a FID based on spam label
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FidClassification {
    Spam,
    Nerfed,
    None,
}

/// Parse a single JSONL line and return the FID classification
fn parse_spam_label_line(line: &str) -> Option<(u64, FidClassification)> {
    let label: SpamLabel = serde_json::from_str(line).ok()?;
    if label.label_type != "spam" {
        return None;
    }
    let classification = match label.label_value {
        0 => FidClassification::Spam,
        3 => FidClassification::Nerfed,
        _ => FidClassification::None,
    };
    if classification == FidClassification::None {
        return None;
    }
    Some((label.type_info.fid, classification))
}

impl SpamFilter {
    pub fn new() -> Self {
        Self {
            spam_fids: Arc::new(RwLock::new(HashSet::new())),
            nerfed_fids: Arc::new(RwLock::new(HashSet::new())),
            http_client: Arc::new(Client::new()),
            last_update: Arc::new(RwLock::new(UNIX_EPOCH)),
        }
    }

    /// Start the spam filter updater in background and wait for initial load
    pub async fn start_updater(&self) -> Result<()> {
        // First, do an immediate update to load the initial spam list
        let result = Self::fetch_spam_list(&self.http_client).await?;
        {
            let mut spam = self.spam_fids.write().await;
            let mut nerfed = self.nerfed_fids.write().await;
            info!(
                "Initialized filter lists: {} spam FIDs, {} nerfed FIDs",
                result.spam_fids.len(),
                result.nerfed_fids.len()
            );
            *spam = result.spam_fids;
            *nerfed = result.nerfed_fids;
            *self.last_update.write().await = SystemTime::now();
        }

        // Now start the background updater
        let spam_fids = self.spam_fids.clone();
        let nerfed_fids = self.nerfed_fids.clone();
        let http_client = self.http_client.clone();
        let last_update = self.last_update.clone();

        tokio::spawn(async move {
            // Wait for 6 hours before the first background update
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;

            loop {
                match Self::fetch_spam_list(&http_client).await {
                    Ok(result) => {
                        let mut spam = spam_fids.write().await;
                        let mut nerfed = nerfed_fids.write().await;
                        info!(
                            "Updated filter lists: {} spam FIDs, {} nerfed FIDs",
                            result.spam_fids.len(),
                            result.nerfed_fids.len()
                        );
                        *spam = result.spam_fids;
                        *nerfed = result.nerfed_fids;
                        *last_update.write().await = SystemTime::now();
                    },
                    Err(e) => error!("Failed to update spam list: {:#}", e),
                }
                tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
            }
        });

        Ok(())
    }

    async fn fetch_spam_list(client: &Client) -> Result<SpamListResult> {
        // List of spam.jsonl sources to fetch from
        let urls = [
            "https://media.githubusercontent.com/media/merkle-team/labels/main/spam.jsonl",
            "https://storage.googleapis.com/uno-spam-labels/spam.jsonl",
        ];

        let mut spam_fids = HashSet::new();
        let mut nerfed_fids = HashSet::new();

        for url in &urls {
            match Self::fetch_from_url(client, url).await {
                Ok(result) => {
                    info!(
                        "Fetched {} spam FIDs and {} nerfed FIDs from {}",
                        result.spam_fids.len(),
                        result.nerfed_fids.len(),
                        url
                    );
                    spam_fids.extend(result.spam_fids);
                    nerfed_fids.extend(result.nerfed_fids);
                },
                Err(e) => {
                    error!("Failed to fetch spam list from {}: {:#}", url, e);
                    // Continue with other sources even if one fails
                },
            }
        }

        if spam_fids.is_empty() && nerfed_fids.is_empty() {
            return Err(color_eyre::eyre::eyre!("Failed to fetch spam list from any source"));
        }

        Ok(SpamListResult { spam_fids, nerfed_fids })
    }

    async fn fetch_from_url(client: &Client, url: &str) -> Result<SpamListResult> {
        let response =
            client.get(url).send().await.context(format!("Failed to fetch from {}", url))?;

        // Stream the response to avoid loading ~96MB into memory at once
        let byte_stream =
            response.bytes_stream().map(|result| result.map_err(std::io::Error::other));
        let stream_reader = StreamReader::new(byte_stream);
        let buf_reader = BufReader::new(stream_reader);
        let mut lines = buf_reader.lines();

        let mut spam_fids = HashSet::new();
        let mut nerfed_fids = HashSet::new();

        while let Some(line) = lines.next_line().await.context("Failed to read line")? {
            if let Some((fid, classification)) = parse_spam_label_line(&line) {
                match classification {
                    FidClassification::Spam => {
                        spam_fids.insert(fid);
                    },
                    FidClassification::Nerfed => {
                        nerfed_fids.insert(fid);
                    },
                    FidClassification::None => {},
                }
            }
        }
        Ok(SpamListResult { spam_fids, nerfed_fids })
    }

    pub async fn is_spam(&self, fid: u64) -> bool {
        self.spam_fids.read().await.contains(&fid)
    }

    pub async fn is_nerfed(&self, fid: u64) -> bool {
        self.nerfed_fids.read().await.contains(&fid)
    }

    /// Remove a FID from the spam set (when label_value=2 is received)
    pub async fn remove_spam_fid(&self, fid: u64) -> bool {
        self.spam_fids.write().await.remove(&fid)
    }

    /// Remove a FID from the nerfed set
    pub async fn remove_nerfed_fid(&self, fid: u64) -> bool {
        self.nerfed_fids.write().await.remove(&fid)
    }

    /// Returns the current set of spam FIDs
    pub async fn get_spam_fids(&self) -> HashSet<u64> {
        self.spam_fids.read().await.clone()
    }

    /// Returns the current set of nerfed FIDs
    pub async fn get_nerfed_fids(&self) -> HashSet<u64> {
        self.nerfed_fids.read().await.clone()
    }

    pub async fn filter_events(&self, events: &[HubEvent]) -> Vec<usize> {
        let mut keep_indices = Vec::new();

        for (idx, event) in events.iter().enumerate() {
            let should_keep = match &event.body {
                Some(hub_event::Body::MergeMessageBody(body)) => match &body.message {
                    Some(msg) => match &msg.data {
                        Some(data) => !self.is_spam(data.fid).await,
                        None => true,
                    },
                    None => true,
                },
                Some(hub_event::Body::PruneMessageBody(body)) => match &body.message {
                    Some(msg) => match &msg.data {
                        Some(data) => !self.is_spam(data.fid).await,
                        None => true,
                    },
                    None => true,
                },
                Some(hub_event::Body::RevokeMessageBody(body)) => match &body.message {
                    Some(msg) => match &msg.data {
                        Some(data) => !self.is_spam(data.fid).await,
                        None => true,
                    },
                    None => true,
                },
                _ => true,
            };

            if should_keep {
                keep_indices.push(idx);
            }
        }

        keep_indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spam_filter_loads_fids() {
        let filter = SpamFilter::new();

        // Start the updater which will load the initial spam list
        let result = filter.start_updater().await;

        // Should not error
        assert!(result.is_ok(), "Failed to start spam filter: {:?}", result.err());

        // Give it a moment to ensure the write lock is released
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check that we loaded some FIDs
        let fid_count = filter.spam_fids.read().await.len();
        println!("Loaded {} spam FIDs", fid_count);

        // We expect to load hundreds of thousands of FIDs
        assert!(
            fid_count > 100000,
            "Expected to load more than 100k FIDs, but only loaded {}",
            fid_count
        );

        // Test that a known spam FID is detected
        // Using a FID from the sample data you provided
        assert!(filter.is_spam(568763).await, "Expected FID 568763 to be marked as spam");
    }

    #[tokio::test]
    async fn test_fetch_spam_list_directly() {
        let client = Client::new();
        let result = SpamFilter::fetch_spam_list(&client).await;

        assert!(result.is_ok(), "Failed to fetch spam list: {:?}", result.err());

        let list_result = result.unwrap();
        println!(
            "Fetched {} spam FIDs and {} nerfed FIDs directly from all sources",
            list_result.spam_fids.len(),
            list_result.nerfed_fids.len()
        );

        // Should have loaded many spam FIDs from multiple sources
        assert!(
            list_result.spam_fids.len() > 100000,
            "Expected more than 100k spam FIDs, got {}",
            list_result.spam_fids.len()
        );

        // Check for a specific FID from the data
        assert!(
            list_result.spam_fids.contains(&568763),
            "Expected to find FID 568763 in spam list"
        );
    }

    #[tokio::test]
    async fn test_fetch_from_multiple_sources() {
        let client = Client::new();

        // Test fetching from GitHub source
        let github_url =
            "https://media.githubusercontent.com/media/merkle-team/labels/main/spam.jsonl";
        let github_result = SpamFilter::fetch_from_url(&client, github_url).await;
        assert!(github_result.is_ok(), "Failed to fetch from GitHub: {:?}", github_result.err());

        let github_list = github_result.unwrap();
        println!(
            "Fetched {} spam FIDs and {} nerfed FIDs from GitHub source",
            github_list.spam_fids.len(),
            github_list.nerfed_fids.len()
        );

        // Test fetching from Google Cloud Storage source
        let gcs_url = "https://storage.googleapis.com/uno-spam-labels/spam.jsonl";
        let gcs_result = SpamFilter::fetch_from_url(&client, gcs_url).await;
        assert!(gcs_result.is_ok(), "Failed to fetch from GCS: {:?}", gcs_result.err());

        let gcs_list = gcs_result.unwrap();
        println!(
            "Fetched {} spam FIDs and {} nerfed FIDs from Google Cloud Storage source",
            gcs_list.spam_fids.len(),
            gcs_list.nerfed_fids.len()
        );

        // Both sources should have spam data
        assert!(!github_list.spam_fids.is_empty(), "GitHub source should have spam FIDs");
        assert!(!gcs_list.spam_fids.is_empty(), "GCS source should have spam FIDs");
    }

    #[tokio::test]
    async fn test_filter_events_keeps_onchain_events() {
        use crate::proto::{MergeOnChainEventBody, OnChainEvent};

        let filter = SpamFilter::new();

        // Manually add a spam FID for testing
        {
            let mut fids = filter.spam_fids.write().await;
            fids.insert(12345);
        }

        // Create a MergeOnChainEvent for a spam FID
        let onchain_event = HubEvent {
            id: 1,
            r#type: 9, // MERGE_ON_CHAIN_EVENT
            body: Some(hub_event::Body::MergeOnChainEventBody(MergeOnChainEventBody {
                on_chain_event: Some(OnChainEvent {
                    r#type: 1, // EVENT_TYPE_SIGNER
                    chain_id: 1,
                    block_number: 100,
                    block_hash: vec![],
                    block_timestamp: 0,
                    transaction_hash: vec![],
                    log_index: 0,
                    fid: 12345, // This is a spam FID
                    body: None,
                    tx_index: 0,
                    version: 0,
                }),
            })),
            block_number: 0,
            shard_index: 0,
            timestamp: 0,
        };

        let events = vec![onchain_event];
        let keep_indices = filter.filter_events(&events).await;

        // Onchain events should ALWAYS be kept, even for spam FIDs
        assert_eq!(
            keep_indices.len(),
            1,
            "Onchain events should not be filtered out, even for spam FIDs"
        );
        assert_eq!(keep_indices[0], 0, "The onchain event index should be kept");
    }

    #[tokio::test]
    async fn test_filter_events_filters_messages_from_spam_fids() {
        use crate::proto::{MergeMessageBody, Message, MessageData};

        let filter = SpamFilter::new();

        // Manually add a spam FID for testing
        {
            let mut fids = filter.spam_fids.write().await;
            fids.insert(99999);
        }

        // Create a MergeMessage event for a spam FID
        let spam_message_event = HubEvent {
            id: 1,
            r#type: 1, // MERGE_MESSAGE
            body: Some(hub_event::Body::MergeMessageBody(MergeMessageBody {
                message: Some(Message {
                    data: Some(MessageData {
                        r#type: 1,  // CAST_ADD
                        fid: 99999, // Spam FID
                        timestamp: 0,
                        network: 0,
                        body: None,
                    }),
                    hash: vec![],
                    hash_scheme: 0,
                    signature: vec![],
                    signature_scheme: 0,
                    signer: vec![],
                    data_bytes: None,
                }),
                deleted_messages: vec![],
            })),
            block_number: 0,
            shard_index: 0,
            timestamp: 0,
        };

        // Create a MergeMessage event for a non-spam FID
        let normal_message_event = HubEvent {
            id: 2,
            r#type: 1, // MERGE_MESSAGE
            body: Some(hub_event::Body::MergeMessageBody(MergeMessageBody {
                message: Some(Message {
                    data: Some(MessageData {
                        r#type: 1, // CAST_ADD
                        fid: 1,    // Non-spam FID
                        timestamp: 0,
                        network: 0,
                        body: None,
                    }),
                    hash: vec![],
                    hash_scheme: 0,
                    signature: vec![],
                    signature_scheme: 0,
                    signer: vec![],
                    data_bytes: None,
                }),
                deleted_messages: vec![],
            })),
            block_number: 0,
            shard_index: 0,
            timestamp: 0,
        };

        let events = vec![spam_message_event, normal_message_event];
        let keep_indices = filter.filter_events(&events).await;

        // Only the non-spam message should be kept
        assert_eq!(keep_indices.len(), 1, "Only non-spam messages should be kept");
        assert_eq!(keep_indices[0], 1, "Only the second event (non-spam) should be kept");
    }

    #[test]
    fn test_parse_spam_label_line_spam() {
        let line = r#"{"provider":1,"type":{"target":"fid","fid":12345},"label_type":"spam","label_value":0,"timestamp":1234567890}"#;
        let result = parse_spam_label_line(line);
        assert_eq!(result, Some((12345, FidClassification::Spam)));
    }

    #[test]
    fn test_parse_spam_label_line_nerfed() {
        let line = r#"{"provider":1,"type":{"target":"fid","fid":67890},"label_type":"spam","label_value":3,"timestamp":1234567890}"#;
        let result = parse_spam_label_line(line);
        assert_eq!(result, Some((67890, FidClassification::Nerfed)));
    }

    #[test]
    fn test_parse_spam_label_line_unknown_value() {
        let line = r#"{"provider":1,"type":{"target":"fid","fid":11111},"label_type":"spam","label_value":99,"timestamp":1234567890}"#;
        let result = parse_spam_label_line(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_spam_label_line_non_spam_type() {
        let line = r#"{"provider":1,"type":{"target":"fid","fid":22222},"label_type":"other","label_value":0,"timestamp":1234567890}"#;
        let result = parse_spam_label_line(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_spam_label_line_invalid_json() {
        let line = "not valid json";
        let result = parse_spam_label_line(line);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_spam_label_line_empty() {
        let result = parse_spam_label_line("");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_is_nerfed() {
        let filter = SpamFilter::new();

        // Manually add a nerfed FID
        {
            let mut nerfed = filter.nerfed_fids.write().await;
            nerfed.insert(54321);
        }

        assert!(filter.is_nerfed(54321).await);
        assert!(!filter.is_nerfed(99999).await);
    }

    #[tokio::test]
    async fn test_get_spam_fids() {
        let filter = SpamFilter::new();

        {
            let mut spam = filter.spam_fids.write().await;
            spam.insert(1);
            spam.insert(2);
            spam.insert(3);
        }

        let fids = filter.get_spam_fids().await;
        assert_eq!(fids.len(), 3);
        assert!(fids.contains(&1));
        assert!(fids.contains(&2));
        assert!(fids.contains(&3));
    }

    #[tokio::test]
    async fn test_get_nerfed_fids() {
        let filter = SpamFilter::new();

        {
            let mut nerfed = filter.nerfed_fids.write().await;
            nerfed.insert(100);
            nerfed.insert(200);
        }

        let fids = filter.get_nerfed_fids().await;
        assert_eq!(fids.len(), 2);
        assert!(fids.contains(&100));
        assert!(fids.contains(&200));
    }

    #[tokio::test]
    async fn test_spam_and_nerfed_are_separate() {
        let filter = SpamFilter::new();

        // Add different FIDs to spam and nerfed lists
        {
            let mut spam = filter.spam_fids.write().await;
            spam.insert(1000);
        }
        {
            let mut nerfed = filter.nerfed_fids.write().await;
            nerfed.insert(2000);
        }

        // Spam FID should only be in spam, not nerfed
        assert!(filter.is_spam(1000).await);
        assert!(!filter.is_nerfed(1000).await);

        // Nerfed FID should only be in nerfed, not spam
        assert!(!filter.is_spam(2000).await);
        assert!(filter.is_nerfed(2000).await);
    }

    #[tokio::test]
    async fn test_streaming_parse_multiple_lines() {
        use tokio::io::AsyncBufReadExt;

        // Simulate streaming JSONL content with mixed spam and nerfed entries
        let jsonl_content = b"{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":100},\"label_type\":\"spam\",\"label_value\":0,\"timestamp\":1}\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":200},\"label_type\":\"spam\",\"label_value\":3,\"timestamp\":2}\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":300},\"label_type\":\"spam\",\"label_value\":0,\"timestamp\":3}\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":400},\"label_type\":\"other\",\"label_value\":0,\"timestamp\":4}\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":500},\"label_type\":\"spam\",\"label_value\":3,\"timestamp\":5}\n\
invalid json line\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":600},\"label_type\":\"spam\",\"label_value\":99,\"timestamp\":6}\n\
{\"provider\":1,\"type\":{\"target\":\"fid\",\"fid\":700},\"label_type\":\"spam\",\"label_value\":0,\"timestamp\":7}";

        // Parse using async BufReader to simulate streaming
        let buf_reader = tokio::io::BufReader::new(&jsonl_content[..]);
        let mut lines = buf_reader.lines();

        let mut spam_fids = HashSet::new();
        let mut nerfed_fids = HashSet::new();

        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some((fid, classification)) = parse_spam_label_line(&line) {
                match classification {
                    FidClassification::Spam => {
                        spam_fids.insert(fid);
                    },
                    FidClassification::Nerfed => {
                        nerfed_fids.insert(fid);
                    },
                    FidClassification::None => {},
                }
            }
        }

        // Should have 3 spam FIDs: 100, 300, 700
        assert_eq!(spam_fids.len(), 3);
        assert!(spam_fids.contains(&100));
        assert!(spam_fids.contains(&300));
        assert!(spam_fids.contains(&700));

        // Should have 2 nerfed FIDs: 200, 500
        assert_eq!(nerfed_fids.len(), 2);
        assert!(nerfed_fids.contains(&200));
        assert!(nerfed_fids.contains(&500));

        // FID 400 should be skipped (wrong label_type)
        assert!(!spam_fids.contains(&400));
        assert!(!nerfed_fids.contains(&400));

        // FID 600 should be skipped (unknown label_value)
        assert!(!spam_fids.contains(&600));
        assert!(!nerfed_fids.contains(&600));
    }
}
