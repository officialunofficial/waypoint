use crate::{processor::stream::StreamProcessor, proto::HubEvent, redis::stream::RedisStream};
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, task::JoinHandle, time};
use tracing::error;

const BASE_GROUP_NAME: &str = "default";
const MAX_EVENTS_PER_FETCH: u64 = 50; // Increased from 10 to 50 for better throughput
const MESSAGE_PROCESSING_CONCURRENCY: usize = 250; // Increased from 200 to 250
// Number of parallel consumers per stream type for higher throughput
const CONSUMERS_PER_STREAM: usize = 3;
const EVENT_PROCESSING_TIMEOUT: Duration = Duration::from_secs(120);
const EVENT_DELETION_THRESHOLD: Duration = Duration::from_secs(24 * 60 * 60);

#[async_trait::async_trait]
pub trait EventProcessor: Send + Sync + 'static {
    async fn process_event(
        &self,
        event: HubEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Allows downcasting to concrete types for optimization
    fn as_any(&self) -> &dyn std::any::Any {
        &() // Default implementation returns empty Any
    }
}

pub struct Consumer {
    stream: Arc<RedisStream>,
    base_stream_key: String,
    processors: Vec<Arc<dyn EventProcessor>>,
    shutdown: Arc<RwLock<bool>>,
    stream_tasks: Vec<JoinHandle<()>>,
}

impl Consumer {
    /// Create a new consumer with the given Redis stream, hub host, and shard key
    ///
    /// # Arguments
    /// * `stream` - Redis stream for consuming events
    /// * `hub_host` - Hostname of the Hub server
    /// * `shard_key` - Optional shard key for partitioning (empty string means no sharding)
    pub fn new(stream: Arc<RedisStream>, hub_host: String, _shard_key: String) -> Self {
        // Use the shared stream key generation function with empty event_type
        // since this is just the base key - event types will be added later
        Self {
            stream,
            base_stream_key: crate::types::get_stream_key(&hub_host, ""),
            processors: Vec::new(),
            shutdown: Arc::new(RwLock::new(false)),
            stream_tasks: Vec::new(),
        }
    }

    fn get_stream_keys(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("casts", "casts"),
            ("reactions", "reactions"),
            ("links", "links"),
            ("verifications", "verifications"),
            ("user_data", "user_data"),
            ("username_proofs", "username_proofs"),
            ("onchain:signer", "onchain"),
            ("onchain:signer_migrated", "onchain"),
            ("onchain:id_register", "onchain"),
            ("onchain:storage_rent", "onchain"),
        ]
        .into_iter()
    }

    pub async fn start(&mut self) -> Result<(), crate::redis::error::Error> {
        self.stream.wait_until_ready(Duration::from_secs(5)).await?;

        let mut tasks = Vec::new();

        // Spawn tasks for each stream type
        for (event_type, group_suffix) in self.get_stream_keys() {
            // Extract hub_host from base_stream_key for use with the shared stream key function
            // Format: hub:snapchain.farcaster.xyz:evt:msg:
            let parts: Vec<&str> = self.base_stream_key.split(':').collect();
            // Must combine all parts that could be in the hostname (might include port number)
            let hub_host = if parts.len() >= 2 {
                // This handles hostnames that may contain port numbers with : in them
                parts[1]
            } else {
                "localhost"
            };
            // Use the same stream key format as in the subscriber
            let stream_key = crate::types::get_stream_key(hub_host, event_type);
            let group_name = format!("{}:{}", BASE_GROUP_NAME, group_suffix);

            // Immediately start consumer rebalancing to claim pending messages from idle consumers
            // This ensures we recover any messages that were in process during a previous shutdown
            let _consumer_id = crate::redis::stream::RedisStream::get_stable_consumer_id();
            let _ = self.stream
                .start_consumer_rebalancing(Duration::from_secs(30)) // Regular rebalance every 30 seconds
                .await;

            // Launch multiple parallel consumers for this stream type
            for consumer_num in 0..CONSUMERS_PER_STREAM {
                // Use a simpler consumer ID format to avoid reclamation issues
                let consumer_instance_id = format!("waypoint-{}", consumer_num + 1);

                let processor = StreamProcessor {
                    stream: Arc::clone(&self.stream),
                    stream_key: stream_key.clone(),
                    group_name: group_name.clone(),
                    processors: self.processors.clone(),
                    shutdown: Arc::clone(&self.shutdown),
                    max_events_per_fetch: MAX_EVENTS_PER_FETCH,
                    processing_concurrency: MESSAGE_PROCESSING_CONCURRENCY,
                    event_processing_timeout: EVENT_PROCESSING_TIMEOUT,
                    consumer_id: consumer_instance_id.clone(),
                };

                let handle = tokio::spawn(async move {
                    tracing::info!(
                        "Starting stream processor {}/{} with consumer ID: {}",
                        consumer_num + 1,
                        CONSUMERS_PER_STREAM,
                        consumer_instance_id
                    );
                    if let Err(e) = processor.process_stream().await {
                        error!(
                            "Stream processor error for consumer {}: {}",
                            consumer_instance_id, e
                        );
                    }
                });

                tasks.push(handle);
            }
        }

        self.stream_tasks = tasks;

        // Start cleanup task for old events
        for (event_type, _) in self.get_stream_keys() {
            let cleanup_stream = Arc::clone(&self.stream);
            // Extract hub_host for the cleanup key
            let parts: Vec<&str> = self.base_stream_key.split(':').collect();
            let hub_host = if parts.len() >= 2 { parts[1] } else { "localhost" };
            let cleanup_key = crate::types::get_stream_key(hub_host, event_type);
            let _cleanup_threshold = EVENT_DELETION_THRESHOLD;
            let cleanup_shutdown = Arc::clone(&self.shutdown);

            tokio::spawn(async move {
                let mut interval = time::interval(Duration::from_secs(60));
                while !*cleanup_shutdown.read().await {
                    interval.tick().await;
                    // Keep events from the last 24 hours
                    if let Err(e) =
                        cleanup_stream.trim(&cleanup_key, Duration::from_secs(24 * 60 * 60)).await
                    {
                        error!("Error clearing old events for {}: {}", cleanup_key, e);
                    }
                }
            });
        }

        Ok(())
    }

    pub fn add_processor<P: EventProcessor + 'static>(&mut self, processor: Arc<P>) {
        self.processors.push(processor);
    }

    pub async fn stop(&self) {
        *self.shutdown.write().await = true;

        for task in &self.stream_tasks {
            task.abort();
        }
    }
}
