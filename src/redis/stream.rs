use crate::redis::{client::Redis, error::Error, types::AtomicStreamMetrics};
use std::{sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{trace, warn};

const MAX_RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(100);
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum number of attempts before sending to dead letter queue
const MAX_MESSAGE_RETRIES: u64 = 5;

#[derive(Clone)]
pub struct RedisStream {
    pub redis: Arc<Redis>,
    health_check_enabled: bool,
    /// Policy for handling messages that exceed max retries
    dead_letter_policy: crate::redis::types::DeadLetterPolicy,
    /// Lock-free metric tracking for this stream
    metrics: Arc<AtomicStreamMetrics>,
}

#[derive(Debug)]
pub struct StreamEntry {
    pub id: String,
    pub data: Vec<u8>,
    pub attempts: u64,
}

pub struct RedisPipeline {
    redis: Arc<Redis>,
    key: String,
    commands: Vec<Vec<u8>>,
    maxlen: Option<u64>,
}

impl RedisStream {
    pub fn new(redis: Arc<Redis>) -> Self {
        Self {
            redis,
            health_check_enabled: false,
            dead_letter_policy: crate::redis::types::DeadLetterPolicy::default(),
            metrics: Arc::new(AtomicStreamMetrics::default()),
        }
    }

    // Note: With fred, we don't need to manually get connections
    // The pool handles this internally

    /// Set a dead letter queue policy for handling failed messages
    pub fn with_dead_letter_queue(mut self, queue_name: String) -> Self {
        self.dead_letter_policy =
            crate::redis::types::DeadLetterPolicy::MoveToDeadLetter { queue_name };
        self
    }

    /// Get current metrics for this stream (lock-free snapshot)
    pub fn get_metrics(&self) -> crate::redis::types::StreamMetrics {
        self.metrics.snapshot()
    }

    /// Update metrics with a successful processing (lock-free)
    pub fn update_success_metrics(&self, processing_time_ms: u64) {
        self.metrics.increment_processed();
        self.metrics.update_latency(processing_time_ms);
    }

    /// Update metrics with an error (lock-free)
    pub fn update_error_metrics(&self) {
        self.metrics.increment_error();
    }

    /// Update metrics with a retry (lock-free)
    pub fn update_retry_metrics(&self) {
        self.metrics.increment_retry();
    }

    /// Update metrics with a dead letter event (lock-free)
    pub fn update_dead_letter_metrics(&self) {
        self.metrics.increment_dead_letter();
    }

    /// Start health check monitoring for the stream
    pub fn enable_health_check(mut self) -> Self {
        self.health_check_enabled = true;

        let redis = self.redis.clone();
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            let mut ticker = interval(HEALTH_CHECK_INTERVAL);
            loop {
                ticker.tick().await;

                // Get connection pool health
                let health = redis.get_pool_health();
                if health.1 == 0 {
                    warn!("Redis connection pool exhausted!");
                }

                // Log metrics periodically (lock-free snapshot)
                let current_metrics = metrics.snapshot();
                tracing::debug!(
                    "Stream metrics - Processed: {}, Errors: {}, Retries: {}, Dead Letter: {}, Rate: {:.2} msg/s",
                    current_metrics.processed_count,
                    current_metrics.error_count,
                    current_metrics.retry_count,
                    current_metrics.dead_letter_count,
                    current_metrics.processing_rate
                );
            }
        });

        self
    }

    /// Reserve messages from the stream (XREADGROUP)
    pub async fn reserve(
        &self,
        key: &str,
        group: &str,
        count: usize,
        consumer: Option<&str>,
    ) -> Result<Vec<StreamEntry>, Error> {
        let consumer_name = consumer.unwrap_or("default-consumer");

        trace!(
            "Reading from key='{}', group='{}', consumer='{}', count={}",
            key, group, consumer_name, count
        );

        // Use retries for transient failures
        let mut attempts = 0;
        loop {
            match self.redis.xreadgroup(group, consumer_name, key, count as u64).await {
                Ok(entries) => {
                    let stream_entries: Vec<StreamEntry> = entries
                        .into_iter()
                        .map(|(id, data)| StreamEntry { id, data, attempts: 0 })
                        .collect();

                    if stream_entries.is_empty() {
                        trace!("No messages available in key='{}'", key);
                    }

                    return Ok(stream_entries);
                },
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    /// Claim stale messages from other consumers
    pub async fn claim_stale(
        &self,
        key: &str,
        group: &str,
        min_idle_time: Duration,
        count: usize,
        consumer: Option<&str>,
    ) -> Result<Vec<StreamEntry>, Error> {
        let consumer_name = consumer.unwrap_or("default-consumer");

        // First, get pending messages
        let pending = self.redis.xpending(key, group, min_idle_time, count as u64).await?;

        if pending.is_empty() {
            return Ok(Vec::new());
        }

        // Extract message IDs
        let ids: Vec<String> = pending.into_iter().map(|p| p.id).collect();

        // Claim the messages
        let claimed = self.redis.xclaim(key, group, consumer_name, min_idle_time, &ids).await?;

        Ok(claimed
            .into_iter()
            .map(|(id, data)| StreamEntry {
                id,
                data,
                attempts: 1, // These are retries
            })
            .collect())
    }

    /// Acknowledge successfully processed messages (batched in single Redis command)
    pub async fn ack(&self, key: &str, group: &str, ids: Vec<String>) -> Result<(), Error> {
        if ids.is_empty() {
            return Ok(());
        }

        trace!("Acknowledging {} messages for key='{}', group='{}'", ids.len(), key, group);

        self.redis.xack(key, group, ids).await
    }

    /// Create a consumer group for the stream
    pub async fn create_group(&self, key: &str, group: &str) -> Result<(), Error> {
        use fred::prelude::*;

        // Try to create the group
        let result: Result<String, _> = self.redis.pool.xgroup_create(key, group, "$", true).await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                // Check if group already exists
                let error_str = e.to_string();
                if error_str.contains("BUSYGROUP") || error_str.contains("already exists") {
                    // Group already exists, that's fine
                    Ok(())
                } else {
                    Err(Error::RedisError(e))
                }
            },
        }
    }

    /// Start a pipeline for batched operations
    pub fn pipeline(&self, key: String) -> RedisPipeline {
        RedisPipeline { redis: self.redis.clone(), key, commands: Vec::new(), maxlen: None }
    }

    /// Delete a consumer from a group
    pub async fn delete_consumer(
        &self,
        key: &str,
        group: &str,
        consumer: &str,
    ) -> Result<u64, Error> {
        use fred::prelude::*;

        let result: u64 = self
            .redis
            .pool
            .xgroup_delconsumer(key, group, consumer)
            .await
            .map_err(Error::RedisError)?;

        Ok(result)
    }

    /// Get detailed information about a consumer group
    pub async fn group_info(
        &self,
        key: &str,
        group: &str,
    ) -> Result<crate::redis::types::ConsumerGroupHealth, Error> {
        self.redis.get_consumer_group_health(key, group).await
    }

    /// Process messages with automatic retry and dead letter handling
    pub async fn process_with_retry<F, Fut>(
        &self,
        key: &str,
        group: &str,
        consumer: &str,
        count: usize,
        processor: F,
    ) -> Result<usize, Error>
    where
        F: Fn(Vec<u8>) -> Fut,
        Fut: std::future::Future<Output = Result<(), Error>>,
    {
        let mut processed_count = 0;

        // Reserve messages
        let entries = self.reserve(key, group, count, Some(consumer)).await?;

        for entry in entries {
            let start = std::time::Instant::now();

            match processor(entry.data.clone()).await {
                Ok(_) => {
                    // Acknowledge the message
                    self.ack(key, group, vec![entry.id.clone()]).await?;
                    processed_count += 1;

                    let elapsed = start.elapsed().as_millis() as u64;
                    self.update_success_metrics(elapsed);
                },
                Err(e) => {
                    self.update_error_metrics();

                    // Check if message should be retried or sent to dead letter
                    if entry.attempts >= MAX_MESSAGE_RETRIES {
                        self.handle_dead_letter(key, &entry).await?;
                        // Still acknowledge to remove from pending
                        self.ack(key, group, vec![entry.id]).await?;
                    } else {
                        // Leave unacknowledged for retry
                        self.update_retry_metrics();
                        warn!("Failed to process message {}: {}", entry.id, e);
                    }
                },
            }
        }

        Ok(processed_count)
    }

    /// Handle dead letter policy for failed messages
    async fn handle_dead_letter(&self, key: &str, entry: &StreamEntry) -> Result<(), Error> {
        match &self.dead_letter_policy {
            crate::redis::types::DeadLetterPolicy::Discard => {
                warn!("Dropping message {} after max retries", entry.id);
            },
            crate::redis::types::DeadLetterPolicy::MoveToDeadLetter { queue_name: _ } => {
                // Add to dead letter queue with metadata
                let dead_letter_key = format!("{}:dead_letter", key);
                self.redis.xadd(&dead_letter_key, &entry.data).await?;
                self.update_dead_letter_metrics();
                warn!("Moved message {} to dead letter queue", entry.id);
            },
        }
        Ok(())
    }

    /// Trim old messages from the stream
    pub async fn trim_by_time(&self, key: &str, older_than: Duration) -> Result<u64, Error> {
        self.redis.xtrim(key, older_than).await
    }

    /// Get the length of the stream
    pub async fn len(&self, key: &str) -> Result<u64, Error> {
        self.redis.xlen(key).await
    }

    /// Add batch of messages with max length
    pub async fn add_batch_maxlen(
        &self,
        key: &str,
        maxlen: u64,
        messages: Vec<Vec<u8>>,
    ) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        for msg in messages {
            let id = self.redis.xadd_maxlen(key, Some(maxlen), &msg).await?;
            ids.push(id);
        }
        Ok(ids)
    }

    /// Trim stream based on time - remove messages older than specified duration
    pub async fn trim(&self, key: &str, older_than: Duration) -> Result<u64, Error> {
        self.redis.xtrim(key, older_than).await
    }

    /// Wait until the Redis connection is ready
    pub async fn wait_until_ready(&self, timeout: Duration) -> Result<(), Error> {
        use fred::prelude::*;

        tokio::time::timeout(timeout, self.redis.pool.wait_for_connect())
            .await
            .map_err(|_| Error::PoolError("Timeout waiting for Redis connection".to_string()))?
            .map_err(Error::RedisError)?;

        Ok(())
    }

    /// Get a stable consumer ID for this instance
    pub fn get_stable_consumer_id() -> String {
        use std::env;

        // Use hostname + process ID for a stable consumer ID
        #[allow(clippy::disallowed_methods)]
        let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "waypoint".to_string());
        let pid = std::process::id();
        format!("{}-{}", hostname, pid)
    }

    /// Start consumer rebalancing (no-op for now, fred handles this)
    pub async fn start_consumer_rebalancing(&self, _interval: Duration) -> Result<(), Error> {
        // Fred handles connection management and rebalancing internally
        // This is a no-op for compatibility
        Ok(())
    }
}

impl RedisPipeline {
    /// Set max length for stream trimming
    pub fn with_maxlen(mut self, maxlen: u64) -> Self {
        self.maxlen = Some(maxlen);
        self
    }

    /// Add a message to the pipeline
    pub fn add_message(mut self, data: Vec<u8>) -> Self {
        self.commands.push(data);
        self
    }

    /// Execute the pipeline
    pub async fn execute(self) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();

        for data in self.commands {
            let id = if let Some(maxlen) = self.maxlen {
                self.redis.xadd_maxlen(&self.key, Some(maxlen), &data).await?
            } else {
                self.redis.xadd(&self.key, &data).await?
            };
            ids.push(id);
        }

        Ok(ids)
    }
}
