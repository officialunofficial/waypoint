use crate::proto::{HubEvent, hub_event};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;
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
    http_client: Arc<Client>,
    last_update: Arc<RwLock<SystemTime>>,
}

impl Default for SpamFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpamFilter {
    pub fn new() -> Self {
        Self {
            spam_fids: Arc::new(RwLock::new(HashSet::new())),
            http_client: Arc::new(Client::new()),
            last_update: Arc::new(RwLock::new(UNIX_EPOCH)),
        }
    }

    /// Start the spam filter updater in background and wait for initial load
    pub async fn start_updater(&self) -> Result<(), anyhow::Error> {
        // First, do an immediate update to load the initial spam list
        let new_fids = Self::fetch_spam_list(&self.http_client).await?;
        {
            let mut fids = self.spam_fids.write().await;
            *fids = new_fids;
            *self.last_update.write().await = SystemTime::now();
            info!("Initialized spam filter list with {} FIDs", fids.len());
        }

        // Now start the background updater
        let spam_fids = self.spam_fids.clone();
        let http_client = self.http_client.clone();
        let last_update = self.last_update.clone();

        tokio::spawn(async move {
            // Wait for 6 hours before the first background update
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;

            loop {
                match Self::fetch_spam_list(&http_client).await {
                    Ok(new_fids) => {
                        let mut fids = spam_fids.write().await;
                        *fids = new_fids;
                        *last_update.write().await = SystemTime::now();
                        info!("Updated spam filter list with {} FIDs", fids.len());
                    },
                    Err(e) => error!("Failed to update spam list: {:#}", e),
                }
                tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
            }
        });

        Ok(())
    }

    async fn fetch_spam_list(client: &Client) -> Result<HashSet<u64>> {
        let response = client
            .get("https://raw.githubusercontent.com/warpcast/labels/refs/heads/main/spam.jsonl")
            .send()
            .await
            .context("Failed to fetch spam list")?
            .text()
            .await
            .context("Failed to read response text")?;

        let mut spam_fids = HashSet::new();
        for line in response.lines() {
            if let Ok(label) = serde_json::from_str::<SpamLabel>(line) {
                if label.label_type == "spam" && (label.label_value == 0 || label.label_value == 1)
                {
                    spam_fids.insert(label.type_info.fid);
                }
            }
        }
        Ok(spam_fids)
    }

    pub async fn is_spam(&self, fid: u64) -> bool {
        self.spam_fids.read().await.contains(&fid)
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
