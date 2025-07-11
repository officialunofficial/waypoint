//! Cast retry service for background processing of failed casts
use crate::{
    app::{Result, Service, ServiceContext, ServiceHandle},
    core::root_parent_hub::{
        CAST_RETRY_DEAD, CAST_RETRY_STREAM, MAX_RETRY_ATTEMPTS, find_root_parent_hub_with_retry,
    },
    database::client::Database,
    hub::{client::Hub, providers::FarcasterHubClient},
    redis::client::Redis,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, error, info, warn};

/// Data structure for retry queue messages
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetryData {
    cast_fid: Option<i64>,
    cast_hash: Option<Vec<u8>>,
    parent_fid: Option<i64>,
    parent_hash: Option<Vec<u8>>,
    parent_url: Option<String>,
    attempt: u64,
    error: String,
}

/// Retry worker for processing failed cast insertions
pub struct CastRetryWorker {
    redis: Arc<Redis>,
    #[allow(dead_code)]
    database: Arc<Database>,
    hub_client: FarcasterHubClient,
}

impl CastRetryWorker {
    pub fn new(redis: Arc<Redis>, database: Arc<Database>, hub: Arc<Mutex<Hub>>) -> Self {
        let hub_client = FarcasterHubClient::new(hub);
        Self { redis, database, hub_client }
    }

    /// Run the retry worker
    pub async fn run(&self) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting cast retry worker");

        // Ensure consumer group exists
        self.ensure_consumer_group().await?;

        loop {
            if let Err(e) = self.process_retry_batch().await {
                error!("Error processing retry batch: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }

            // Sleep between batches
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }

    /// Process a batch of retry messages
    async fn process_retry_batch(
        &self,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Read messages from retry stream
        let messages = self
            .redis
            .xreadgroup(
                "retry_workers",
                "worker_1",
                CAST_RETRY_STREAM,
                10, // Process 10 at a time
            )
            .await?;

        if messages.is_empty() {
            return Ok(());
        }

        info!("Processing {} retry messages", messages.len());

        for (msg_id, msg_data) in messages {
            if let Err(e) = self.process_retry_message(&msg_id, &msg_data).await {
                error!("Failed to process retry message {}: {}", msg_id, e);
                // Message will remain in pending and can be reclaimed later
            } else {
                // Acknowledge successful processing
                let _ = self.redis.xack(CAST_RETRY_STREAM, "retry_workers", &msg_id).await;
                debug!("Successfully processed and acked message {}", msg_id);
            }
        }

        Ok(())
    }

    /// Process a single retry message
    async fn process_retry_message(
        &self,
        msg_id: &str,
        msg_data: &[u8],
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Parse the message data from JSON serialization
        let retry_data: RetryData = serde_json::from_slice(msg_data)?;
        debug!("Processing retry message {}: {:?}", msg_id, retry_data);

        let cast_fid = retry_data.cast_fid;
        let cast_hash = &retry_data.cast_hash;
        let parent_fid = retry_data.parent_fid;
        let parent_hash = &retry_data.parent_hash;
        let parent_url = retry_data.parent_url.as_deref();
        let attempt = retry_data.attempt;

        // Try to resolve the root parent again
        let result = find_root_parent_hub_with_retry(
            &self.hub_client,
            &self.redis,
            parent_fid,
            parent_hash.as_ref().map(|h| h.as_slice()),
            parent_url,
            cast_fid,
            cast_hash.as_ref().map(|h| h.as_slice()),
        )
        .await;

        match result {
            Ok(Some(_root_info)) => {
                info!("Successfully resolved root parent for cast on retry attempt {}", attempt);
                // TODO: Update the database with the resolved parent info
                Ok(())
            },
            Ok(None) => {
                // No root parent needed, this is fine
                info!("No root parent needed for cast, retry successful");
                Ok(())
            },
            Err(e) => {
                let new_attempt = attempt + 1;
                warn!("Retry attempt {} failed: {}", new_attempt, e);

                if new_attempt >= MAX_RETRY_ATTEMPTS {
                    // Move to dead letter queue
                    warn!("Moving cast to dead letter queue after {} attempts", new_attempt);
                    self.move_to_dead_letter(&retry_data, &e.to_string()).await?;
                } else {
                    // Add back to retry queue with incremented attempt
                    let mut new_retry_data = retry_data;
                    new_retry_data.attempt = new_attempt;
                    self.add_back_to_retry(&new_retry_data).await?;
                }

                Err(e)
            },
        }
    }

    /// Move failed cast to dead letter queue
    async fn move_to_dead_letter(
        &self,
        retry_data: &RetryData,
        final_error: &str,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut dead_data = retry_data.clone();
        dead_data.error = final_error.to_string();

        let serialized_data = serde_json::to_vec(&dead_data)?;
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;

        let _: std::result::Result<String, _> = bb8_redis::redis::cmd("XADD")
            .arg(CAST_RETRY_DEAD)
            .arg("*")
            .arg("d")
            .arg(&serialized_data)
            .query_async(&mut *conn)
            .await;

        Ok(())
    }

    /// Add cast back to retry queue with incremented attempt count
    async fn add_back_to_retry(
        &self,
        retry_data: &RetryData,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let serialized_data = serde_json::to_vec(retry_data)?;
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;

        let _: std::result::Result<String, _> = bb8_redis::redis::cmd("XADD")
            .arg(CAST_RETRY_STREAM)
            .arg("*")
            .arg("d")
            .arg(&serialized_data)
            .query_async(&mut *conn)
            .await;

        debug!("Added cast back to retry queue: attempt={}", retry_data.attempt);
        Ok(())
    }

    /// Create consumer group if it doesn't exist
    async fn ensure_consumer_group(
        &self,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;

        // Try to create consumer group, ignore error if it already exists
        let _: std::result::Result<String, _> = bb8_redis::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(CAST_RETRY_STREAM)
            .arg("retry_workers")
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut *conn)
            .await;

        info!("Consumer group 'retry_workers' ensured for stream '{}'", CAST_RETRY_STREAM);
        Ok(())
    }
}

/// Service for processing failed cast retries in the background
pub struct CastRetryService {
    enabled: bool,
}

impl CastRetryService {
    /// Create a new cast retry service
    pub fn new() -> Self {
        Self { enabled: true }
    }

    /// Configure whether the retry service is enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Default for CastRetryService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Service for CastRetryService {
    fn name(&self) -> &str {
        "cast_retry"
    }

    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> Result<ServiceHandle> {
        if !self.enabled {
            info!("Cast retry service disabled");
            let (stop_tx, stop_rx) = oneshot::channel();
            let join_handle = tokio::spawn(async move {
                let _ = stop_rx.await;
            });
            return Ok(ServiceHandle::new(stop_tx, join_handle));
        }

        info!("Starting cast retry service");

        // Get resources from the service context
        let redis = Arc::clone(&context.state.redis);
        let database = Arc::clone(&context.state.database);
        let hub = Arc::clone(&context.state.hub);

        // Create the retry worker
        let worker = CastRetryWorker::new(redis, database, hub);

        // Set up shutdown channel
        let (stop_tx, mut stop_rx) = oneshot::channel();

        // Spawn the worker task
        let join_handle = tokio::spawn(async move {
            let mut worker_task = tokio::spawn(async move {
                if let Err(e) = worker.run().await {
                    error!("Cast retry worker error: {}", e);
                }
            });

            // Wait for shutdown signal
            tokio::select! {
                _ = &mut stop_rx => {
                    info!("Cast retry service received shutdown signal");
                    worker_task.abort();
                }
                _ = &mut worker_task => {
                    error!("Cast retry worker exited unexpectedly");
                }
            }

            info!("Cast retry service stopped");
        });

        Ok(ServiceHandle::new(stop_tx, join_handle))
    }
}
