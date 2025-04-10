use crate::{
    hub::{error::Error, filter::SpamFilter, stats::ProcessingStats},
    proto::{
        GetInfoRequest, HubEvent, HubEventType, SubscribeRequest, hub_event,
        hub_service_client::HubServiceClient,
    },
    redis::{client::Redis, stream::RedisStream},
};
use dashmap::DashMap;
use futures::StreamExt;
use prost::Message as ProstMessage;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::{debug, error, info, instrument};

pub type PreProcessHandler = Arc<
    dyn Fn(&[HubEvent], &[Vec<u8>]) -> futures::future::BoxFuture<'static, Vec<bool>> + Send + Sync,
>;
pub type PostProcessHandler =
    Arc<dyn Fn(&[HubEvent], &[Vec<u8>]) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

struct BatchState {
    events: Vec<(HubEvent, Vec<u8>)>,
    current_bytes: usize,
    last_flush: Instant,
}

impl BatchState {
    fn new() -> Self {
        Self { events: Vec::new(), current_bytes: 0, last_flush: Instant::now() }
    }
}

#[derive(Clone)]
pub struct HubSubscriber {
    hub: HubServiceClient<Channel>,
    redis: Arc<Redis>,
    redis_stream: Arc<RedisStream>,
    stream_key: Arc<String>,
    redis_key: Arc<String>,
    event_types: Arc<Vec<i32>>,
    // Kept for compatibility but no longer used with the new protos
    total_shards: Option<u64>,
    shard_index: Option<u64>,
    batch_size: usize,
    max_batch_bytes: usize,
    max_flush_interval: Duration,
    stream_maxlen: u64,
    _before_process: Option<PreProcessHandler>,
    _after_process: Option<PostProcessHandler>,
    _stats: Arc<RwLock<ProcessingStats>>,
    shutdown: Arc<RwLock<bool>>,
    connection_timeout: Duration,
    spam_filter: Arc<SpamFilter>,
    // Track last successful Redis publish time for better connection monitoring
    last_successful_flush: Arc<RwLock<Option<Instant>>>,
}

impl HubSubscriber {
    pub async fn new(
        hub: HubServiceClient<Channel>,
        redis: Arc<Redis>,
        redis_stream: RedisStream,
        hub_host: String,
        shard_key: String,
        opts: SubscriberOptions,
    ) -> Self {
        let event_types = vec![
            HubEventType::MergeMessage as i32,
            HubEventType::PruneMessage as i32,
            HubEventType::RevokeMessage as i32,
            HubEventType::MergeUsernameProof as i32,
            HubEventType::MergeOnChainEvent as i32,
        ];

        // Create and initialize spam filter - this now blocks until initial load
        let spam_filter = Arc::new(SpamFilter::new());
        info!("Loading spam filter list before starting Hub subscriber...");
        if let Err(e) = spam_filter.start_updater().await {
            error!(
                "Failed to load initial spam filter list: {}. Hub subscription may include spam messages until filter loads.",
                e
            );
            // We'll continue anyway since the background task will retry
        } else {
            info!("Spam filter loaded - Hub subscription will filter spam messages");
        }

        Self {
            hub,
            redis,
            redis_stream: Arc::new(redis_stream),
            stream_key: Arc::new(crate::types::get_stream_key(
                &hub_host,
                "",
                if shard_key.is_empty() { None } else { Some(&shard_key) },
            )),
            redis_key: Arc::new(format!("{}:{}", hub_host, shard_key)),
            event_types: Arc::new(event_types),
            total_shards: opts.total_shards,
            shard_index: opts.shard_index,
            batch_size: 100,
            max_batch_bytes: 2_usize.pow(20), // 2 MiB
            max_flush_interval: Duration::from_millis(200),
            stream_maxlen: 3_000_000,
            _before_process: opts.before_process,
            _after_process: opts.after_process,
            _stats: Arc::new(RwLock::new(ProcessingStats::new())),
            shutdown: Arc::new(RwLock::new(false)),
            connection_timeout: Duration::from_millis(30000),
            spam_filter,
            last_successful_flush: Arc::new(RwLock::new(Some(Instant::now()))),
        }
    }

    pub async fn get_last_event_id(&self) -> Result<Option<u64>, crate::redis::error::Error> {
        self.redis.get_last_processed_event(&self.redis_key).await
    }

    async fn wait_for_ready(&self) -> Result<(), Error> {
        // Simulate TS _waitForReadyHubClient
        let retry_delay = Duration::from_secs(5);
        let start = Instant::now();

        while start.elapsed() < retry_delay {
            if (self.hub.clone().get_info(tonic::Request::new(GetInfoRequest {})).await).is_err() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            return Ok(());
        }

        Err(Error::ConnectionError("Hub not ready after timeout".to_string()))
    }

    #[instrument(skip(self))]
    pub async fn try_start(&self) -> Result<(), Error> {
        info!("Starting hub subscriber");

        self.wait_for_ready().await?;
        info!("Connected to hub");

        let last_id = self.get_last_event_id().await?;
        if last_id > Some(0) {
            info!("Found last hub event ID: {}", last_id.unwrap());
        } else {
            info!("No last hub event ID found, starting from beginning");
        }

        let mut stream = self.connect_stream(last_id).await?;
        let mut batch_state = BatchState::new();
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 3;

        while !*self.shutdown.read().await {
            match stream.next().await {
                Some(Ok(event)) => {
                    // Reset error counter on successful event
                    consecutive_errors = 0;

                    // Update connection monitoring timestamp
                    *self.last_successful_flush.write().await = Some(Instant::now());

                    let bytes = event.encode_to_vec();
                    batch_state.current_bytes += bytes.len();
                    batch_state.events.push((event, bytes));

                    if self.should_flush(&batch_state).await {
                        match self.flush_batch_with_retry(&mut batch_state).await {
                            Ok(_) => {
                                // Update successful flush timestamp
                                *self.last_successful_flush.write().await = Some(Instant::now());
                            },
                            Err(e) => {
                                error!("Error flushing batch: {:?}", e);
                                consecutive_errors += 1;

                                if consecutive_errors >= max_consecutive_errors {
                                    error!(
                                        "Too many consecutive flush errors ({}), reconnecting stream",
                                        consecutive_errors
                                    );
                                    // Force reconnection rather than failing
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                    stream = self.connect_stream(last_id).await?;
                                    consecutive_errors = 0;
                                    continue;
                                }
                            },
                        }
                    }
                },
                Some(Err(e)) => {
                    error!("Stream error: {:?}", e);
                    consecutive_errors += 1;

                    if consecutive_errors >= max_consecutive_errors {
                        error!(
                            "Too many consecutive stream errors ({}), reconnecting",
                            consecutive_errors
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        stream = self.connect_stream(last_id).await?;
                        consecutive_errors = 0;
                    } else {
                        // For non-critical errors, continue without reconnecting
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                None => {
                    // Stream closed, try to reconnect after delay
                    info!("Hub stream closed, reconnecting after delay");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    stream = self.connect_stream(last_id).await?;
                    consecutive_errors = 0;
                },
            }

            // Check connection timeout - improved with more flexible monitoring
            let flush_timeout_exceeded = match *self.last_successful_flush.read().await {
                Some(timestamp) => timestamp.elapsed() > self.connection_timeout,
                None => false,
            };

            if flush_timeout_exceeded {
                error!(
                    "No successful operations for {:?}, forcing reconnection",
                    self.connection_timeout
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
                stream = self.connect_stream(last_id).await?;
                *self.last_successful_flush.write().await = Some(Instant::now());
                consecutive_errors = 0;
            }
        }

        Ok(())
    }

    pub async fn start(&self) -> Result<(), Error> {
        // Set up periodic cleanup
        let stream_key = self.stream_key.clone();
        let stream = self.redis_stream.clone();
        let cleanup_interval = Duration::from_secs(60);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;
                if let Err(e) = stream.trim(&stream_key, Duration::from_secs(24 * 60 * 60)).await {
                    error!("Error trimming events: {}", e);
                }
            }
        });

        // Main processing loop with retries
        let mut backoff = Duration::from_secs(5);
        let max_backoff = Duration::from_secs(60);

        while !*self.shutdown.read().await {
            match self.try_start().await {
                Ok(_) => break,
                Err(e) => {
                    error!("Stream error, reconnecting in {:?}: {:?}", backoff, e);
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                },
            }
        }

        info!("Event stream terminated");
        Ok(())
    }

    async fn connect_stream(
        &self,
        last_id: Option<u64>,
    ) -> Result<tonic::Streaming<HubEvent>, Error> {
        // Use a retry mechanism with exponential backoff for connection
        let mut backoff = Duration::from_millis(100);
        let max_backoff = Duration::from_secs(5);
        let max_attempts = 3;

        for attempt in 1..=max_attempts {
            // Create a fresh request for each attempt
            let req = tonic::Request::new(SubscribeRequest {
                from_id: last_id,
                event_types: self.event_types.as_ref().to_vec(),
                fid_partitions: self.total_shards,
                fid_partition_index: self.shard_index,
                shard_index: None,
            });

            // Clone hub client before making the call
            let mut hub_clone = self.hub.clone();
            match hub_clone.subscribe(req).await {
                Ok(response) => {
                    info!("Established gRPC stream connection");
                    return Ok(response.into_inner());
                },
                Err(e) => {
                    if attempt == max_attempts {
                        error!(
                            "Failed to connect to gRPC stream after {} attempts: {:?}",
                            max_attempts, e
                        );
                        return Err(Error::from(e));
                    }

                    error!(
                        "Error connecting to gRPC stream (attempt {}/{}): {:?}, retrying in {:?}",
                        attempt, max_attempts, e, backoff
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                },
            }
        }

        // This should never be reached due to the return in the loop above
        Err(Error::ConnectionError("Failed to connect to gRPC stream".to_string()))
    }

    async fn should_flush(&self, batch: &BatchState) -> bool {
        batch.events.len() >= self.batch_size
            || batch.current_bytes >= self.max_batch_bytes
            || batch.last_flush.elapsed() >= self.max_flush_interval
    }

    async fn flush_batch_with_retry(&self, batch: &mut BatchState) -> Result<(), Error> {
        let mut backoff = Duration::from_millis(100);
        let max_backoff = Duration::from_secs(5);
        let max_retries = 5;
        let mut attempts = 0;

        while attempts < max_retries {
            match self.flush_batch(batch).await {
                Ok(_) => return Ok(()),
                Err(Error::InternalRedisError(e)) => {
                    attempts += 1;
                    error!(
                        "Redis error when flushing batch (attempt {}/{}): {}",
                        attempts, max_retries, e
                    );

                    if attempts == max_retries {
                        error!("Max retries reached when flushing batch to Redis");
                        break;
                    }

                    debug!("Retrying batch flush in {:?}...", backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                },
                Err(e) => {
                    error!("Non-Redis error when flushing batch: {:?}", e);
                    return Err(e);
                },
            }
        }

        // Provide more detailed error message
        Err(Error::ProcessingError(format!(
            "Failed to flush batch after {} attempts - Redis connection may be unstable",
            max_retries
        )))
    }

    async fn flush_batch(&self, batch: &mut BatchState) -> Result<(), Error> {
        if batch.events.is_empty() {
            return Ok(());
        }

        // Record start time for performance monitoring
        let start_time = Instant::now();
        let event_count = batch.events.len();

        debug!("Flushing batch of {} events ({} bytes)", event_count, batch.current_bytes);

        // Filter spam events
        let keep_indices = self
            .spam_filter
            .filter_events(&batch.events.iter().map(|(e, _)| e.clone()).collect::<Vec<_>>())
            .await;

        let filtered_count = event_count - keep_indices.len();
        if filtered_count > 0 {
            debug!("Filtered {} spam events from batch", filtered_count);
        }

        let event_groups: DashMap<&str, Vec<Vec<u8>>> = DashMap::new();

        // Process and group non-spam events
        for &idx in &keep_indices {
            let (event, bytes) = &batch.events[idx];
            let event_type = match event.r#type {
                1 => {
                    // MERGE_MESSAGE
                    if let Some(hub_event::Body::MergeMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                2 => {
                    // PRUNE_MESSAGE
                    if let Some(hub_event::Body::PruneMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                3 => {
                    // REVOKE_MESSAGE
                    if let Some(hub_event::Body::RevokeMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                6 => "username_proofs", // MERGE_USERNAME_PROOF
                9 => {
                    // MERGE_ON_CHAIN_EVENT
                    if let Some(hub_event::Body::MergeOnChainEventBody(body)) = &event.body {
                        match body.on_chain_event.as_ref().map(|e| e.r#type) {
                            Some(1) => "onchain:signer",
                            Some(2) => "onchain:signer_migrated",
                            Some(3) => "onchain:id_register",
                            Some(4) => "onchain:storage_rent",
                            _ => "onchain:unknown",
                        }
                    } else {
                        "onchain:unknown"
                    }
                },
                _ => "unknown",
            };
            event_groups.entry(event_type).or_default().push(bytes.clone());
        }

        // Check if we have event groups to process
        if event_groups.is_empty() {
            debug!("No valid events to process in batch after filtering");
            batch.events.clear();
            batch.current_bytes = 0;
            batch.last_flush = Instant::now();
            return Ok(());
        }

        let mut handles = Vec::new();
        let mut group_count = 0;

        for (event_type, bytes) in event_groups {
            group_count += 1;
            let group_size = bytes.len();
            debug!("Processing group {}: {} {} events", group_count, group_size, event_type);

            let parts: Vec<&str> = self.stream_key.split(':').collect();
            let hub_host = if parts.len() >= 2 { parts[1] } else { "localhost" };
            let _shard_key = if parts.len() >= 5 { Some(parts[3]) } else { None };
            // Always use "evt" as the shard key for consistency with consumer
            let stream_key = crate::types::get_stream_key(hub_host, event_type, Some("evt"));
            let redis_stream = self.redis_stream.clone();
            let stream_maxlen = self.stream_maxlen;

            handles.push(tokio::spawn(async move {
                let result = redis_stream.add_batch_maxlen(&stream_key, stream_maxlen, bytes).await;
                if result.is_ok() {
                    debug!("Published {} events to {}", group_size, stream_key);
                }
                result
            }));
        }

        // Use a semaphore to limit concurrent Redis operations
        use futures::future::join_all;
        let results = join_all(handles).await;

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(_)) => {},
                Ok(Err(e)) => {
                    error!("Redis error for event group {}: {}", i + 1, e);
                    return Err(Error::InternalRedisError(e));
                },
                Err(e) => {
                    error!("Task error for event group {}: {}", i + 1, e);
                    return Err(Error::ProcessingError(format!("Task error: {}", e)));
                },
            }
        }

        // Update the last processed event ID
        if let Some(&last_idx) = keep_indices.last() {
            if let Some((last_event, _)) = batch.events.get(last_idx) {
                match self.redis.set_last_processed_event(&self.redis_key, last_event.id).await {
                    Ok(_) => {
                        debug!("Updated last processed event ID to {}", last_event.id);
                    },
                    Err(e) => {
                        error!("Failed to update last processed event ID: {}", e);
                        return Err(Error::InternalRedisError(e));
                    },
                }
            }
        }

        // Clean up and track timing
        batch.events.clear();
        batch.current_bytes = 0;
        batch.last_flush = Instant::now();

        let elapsed = start_time.elapsed();
        debug!("Finished flushing batch in {:?}", elapsed);

        // Update the last successful flush timestamp
        *self.last_successful_flush.write().await = Some(Instant::now());

        Ok(())
    }

    pub async fn stop(&self) -> Result<(), Error> {
        info!("Setting Hub subscriber shutdown flag...");
        *self.shutdown.write().await = true;
        info!("Hub subscriber shutdown signal sent");

        // Update the timestamp to avoid timeout reconnection attempts during shutdown
        *self.last_successful_flush.write().await = Some(Instant::now());

        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct SubscriberOptions {
    pub total_shards: Option<u64>,
    pub shard_index: Option<u64>,
    pub before_process: Option<PreProcessHandler>,
    pub after_process: Option<PostProcessHandler>,
}
