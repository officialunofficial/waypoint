use crate::redis::{
    client::Redis,
    error::Error,
    types::{AtomicStreamMetrics, DeadLetterMetadata, DeadLetterReason},
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{trace, warn};

/// Context for dead letter handling
pub struct DeadLetterContext<'a> {
    /// Stream key
    pub key: &'a str,
    /// Consumer group name
    pub group: &'a str,
    /// Consumer name
    pub consumer: &'a str,
    /// Reason for dead-lettering
    pub reason: DeadLetterReason,
    /// Optional error message from processing
    pub error_message: Option<String>,
}

// Default values (used when config not provided)
const DEFAULT_MAX_RETRY_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_DELAY_MS: u64 = 100;
const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 60;
const DEFAULT_MAX_MESSAGE_RETRIES: u64 = 5;

#[derive(Clone)]
pub struct RedisStream {
    pub redis: Arc<Redis>,
    health_check_enabled: bool,
    /// Policy for handling messages that exceed max retries
    dead_letter_policy: crate::redis::types::DeadLetterPolicy,
    /// Lock-free metric tracking for this stream
    metrics: Arc<AtomicStreamMetrics>,
    /// Maximum retry attempts for Redis operations
    max_retry_attempts: u32,
    /// Delay between retries
    retry_delay: Duration,
    /// Health check interval
    health_check_interval: Duration,
    /// Maximum message retries before dead letter
    max_message_retries: u64,
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
            max_retry_attempts: DEFAULT_MAX_RETRY_ATTEMPTS,
            retry_delay: Duration::from_millis(DEFAULT_RETRY_DELAY_MS),
            health_check_interval: Duration::from_secs(DEFAULT_HEALTH_CHECK_INTERVAL_SECS),
            max_message_retries: DEFAULT_MAX_MESSAGE_RETRIES,
        }
    }

    /// Configure stream with values from StreamProcessorConfig
    pub fn with_config(mut self, config: &crate::config::StreamProcessorConfig) -> Self {
        self.max_retry_attempts = config.max_retry_attempts;
        self.retry_delay = Duration::from_millis(config.retry_delay_ms);
        self.health_check_interval = Duration::from_secs(config.health_check_interval_secs);
        self.max_message_retries = config.max_message_retries;
        self
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
        let health_check_interval = self.health_check_interval;

        tokio::spawn(async move {
            let mut ticker = interval(health_check_interval);
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
                    // When reading with ">", this is the first delivery of each message
                    let stream_entries: Vec<StreamEntry> = entries
                        .into_iter()
                        .map(|(id, data)| StreamEntry { id, data, attempts: 1 })
                        .collect();

                    if stream_entries.is_empty() {
                        trace!("No messages available in key='{}'", key);
                    }

                    return Ok(stream_entries);
                },
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.max_retry_attempts {
                        return Err(e);
                    }
                    tokio::time::sleep(self.retry_delay).await;
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

        // Get pending messages with their delivery counts
        let pending = self.redis.xpending(key, group, min_idle_time, count as u64).await?;

        if pending.is_empty() {
            return Ok(Vec::new());
        }

        // Single pass: build delivery count map and extract IDs with pre-allocated capacity
        let mut delivery_counts = HashMap::with_capacity(pending.len());
        let mut ids = Vec::with_capacity(pending.len());

        for p in pending {
            delivery_counts.insert(p.id.clone(), p.delivery_count);
            ids.push(p.id);
        }

        // Claim the messages
        let claimed = self.redis.xclaim(key, group, consumer_name, min_idle_time, &ids).await?;

        Ok(claimed
            .into_iter()
            .map(|(id, data)| {
                // Look up actual delivery count from xpending, default to 1 if not found
                let attempts = delivery_counts.get(&id).copied().unwrap_or(1);
                StreamEntry { id, data, attempts }
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
                    if entry.attempts >= self.max_message_retries {
                        let ctx = DeadLetterContext {
                            key,
                            group,
                            consumer,
                            reason: DeadLetterReason::MaxRetriesExceeded,
                            error_message: Some(e.to_string()),
                        };
                        self.handle_dead_letter_with_context(&entry, ctx).await?;
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

    /// Handle dead letter policy for failed messages with full metadata
    pub async fn handle_dead_letter_with_context(
        &self,
        entry: &StreamEntry,
        ctx: DeadLetterContext<'_>,
    ) -> Result<(), Error> {
        match &self.dead_letter_policy {
            crate::redis::types::DeadLetterPolicy::Discard => {
                warn!(
                    "Dropping message {} after {} attempts (reason: {})",
                    entry.id, entry.attempts, ctx.reason
                );
            },
            crate::redis::types::DeadLetterPolicy::MoveToDeadLetter { queue_name } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                // Estimate first delivery time based on attempts
                // Each retry typically takes a few hundred ms
                let estimated_first_delivery = now.saturating_sub(entry.attempts * 1000);

                let metadata = DeadLetterMetadata {
                    original_id: entry.id.clone(),
                    source_stream: ctx.key.to_string(),
                    group_name: ctx.group.to_string(),
                    consumer_name: ctx.consumer.to_string(),
                    delivery_count: entry.attempts,
                    first_delivery_time: estimated_first_delivery,
                    dead_letter_time: now,
                    reason: ctx.reason.clone(),
                    error_message: ctx.error_message,
                };

                // Use configured queue name or default pattern
                let dead_letter_key = if queue_name.is_empty() {
                    format!("{}:dead_letter", ctx.key)
                } else {
                    queue_name.clone()
                };

                self.redis.xadd_dead_letter(&dead_letter_key, &entry.data, &metadata).await?;
                self.update_dead_letter_metrics();

                warn!(
                    "Moved message {} to dead letter queue {} (reason: {}, attempts: {})",
                    entry.id, dead_letter_key, ctx.reason, entry.attempts
                );
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
