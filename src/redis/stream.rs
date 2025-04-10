use crate::redis::{client::Redis, error::Error, types::PendingItem};
use bb8_redis::redis::{RedisError, RedisResult};
use std::{sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{error, warn};

const MAX_RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(100);
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum number of attempts before sending to dead letter queue
const MAX_MESSAGE_RETRIES: u64 = 5;

#[derive(Clone)]
pub struct RedisStream {
    redis: Arc<Redis>,
    health_check_enabled: bool,
    batch_size: usize,
    /// Policy for handling messages that exceed max retries
    dead_letter_policy: crate::redis::types::DeadLetterPolicy,
    /// Metric tracking for this stream
    metrics: Arc<tokio::sync::RwLock<crate::redis::types::StreamMetrics>>,
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
        // Get batch_size from Redis config or use default
        let batch_size = match redis.config() {
            Some(config) => config.batch_size,
            None => 100, // Default if config not available
        };

        Self {
            redis,
            health_check_enabled: false,
            batch_size,
            dead_letter_policy: crate::redis::types::DeadLetterPolicy::default(),
            metrics: Arc::new(tokio::sync::RwLock::new(
                crate::redis::types::StreamMetrics::default(),
            )),
        }
    }

    /// Set a dead letter queue policy for handling failed messages
    pub fn with_dead_letter_queue(mut self, queue_name: String) -> Self {
        self.dead_letter_policy =
            crate::redis::types::DeadLetterPolicy::MoveToDeadLetter { queue_name };
        self
    }

    /// Get current metrics for this stream
    /// Returns a copy of the metrics with primitive types (no deep cloning)
    pub async fn get_metrics(&self) -> crate::redis::types::StreamMetrics {
        let metrics_guard = self.metrics.read().await;
        crate::redis::types::StreamMetrics {
            processed_count: metrics_guard.processed_count,
            error_count: metrics_guard.error_count,
            retry_count: metrics_guard.retry_count,
            dead_letter_count: metrics_guard.dead_letter_count,
            processing_rate: metrics_guard.processing_rate,
            average_latency_ms: metrics_guard.average_latency_ms,
        }
    }

    /// Update metrics with a successful processing
    pub async fn update_success_metrics(&self, processing_time_ms: u64) {
        // Calculate new average latency and rate outside of any locks
        let (new_avg_latency, new_rate, update_rate) = {
            let metrics = self.metrics.read().await;
            let avg_latency = if metrics.average_latency_ms == 0.0 {
                processing_time_ms as f64
            } else {
                0.9 * metrics.average_latency_ms + 0.1 * processing_time_ms as f64
            };

            // Check if we need to update processing rate
            let (rate, update) = if (metrics.processed_count + 1) % 100 == 0 {
                let avg_time_per_msg = avg_latency / 1000.0; // seconds
                if avg_time_per_msg > 0.0 { (1.0 / avg_time_per_msg, true) } else { (0.0, false) }
            } else {
                (0.0, false)
            };

            (avg_latency, rate, update)
        };

        // Then acquire write lock for minimal time to update values
        let mut metrics = self.metrics.write().await;
        metrics.processed_count += 1;
        metrics.average_latency_ms = new_avg_latency;

        if update_rate {
            metrics.processing_rate = new_rate;
        }
    }

    /// Update metrics with an error - minimized lock time
    pub async fn update_error_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.error_count += 1;
    }

    /// Update metrics with a retry - minimized lock time
    pub async fn update_retry_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.retry_count += 1;
    }

    /// Update dead letter metrics - minimized lock time
    pub async fn update_dead_letter_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.dead_letter_count += 1;
    }

    pub fn enable_health_check(&mut self) {
        self.health_check_enabled = true;
    }

    async fn start_health_check(&self) {
        let redis = Arc::clone(&self.redis);
        tokio::spawn(async move {
            let mut interval = interval(HEALTH_CHECK_INTERVAL);
            loop {
                interval.tick().await;
                if let Err(e) = redis.check_connection().await {
                    error!("Health check failed: {}", e);
                }
            }
        });
    }

    /// Start consumer group rebalancing monitor
    pub async fn start_consumer_rebalancing(
        &self,
        stream_key: String,
        group_name: String,
        consumer_name: String,
        rebalance_interval: Duration,
    ) {
        let redis = Arc::clone(&self.redis);
        tokio::spawn(async move {
            let mut interval = interval(rebalance_interval);
            loop {
                interval.tick().await;

                // Check consumer group health
                match redis.get_consumer_group_health(&stream_key, &group_name).await {
                    Ok(health) => {
                        // Identify idle consumers
                        let idle_threshold = Duration::from_secs(60).as_millis() as u64;
                        let mut total_pending = 0;
                        let mut idle_consumers = Vec::new();
                        let mut active_consumers = Vec::new();

                        for consumer in &health.consumers {
                            total_pending += consumer.pending_count;
                            if consumer.idle_time > idle_threshold && consumer.name != consumer_name
                            {
                                idle_consumers.push(consumer.name.clone());
                            } else {
                                active_consumers.push(consumer.name.clone());
                            }
                        }

                        if !idle_consumers.is_empty() && total_pending > 0 {
                            tracing::info!(
                                "Detected {} idle consumers with {} total pending messages in group {} for stream {}",
                                idle_consumers.len(),
                                total_pending,
                                group_name,
                                stream_key
                            );

                            // Only claim messages if we're an active consumer
                            if active_consumers.contains(&consumer_name) {
                                // For each idle consumer, claim their pending messages
                                let mut conn = match redis.pool.get().await {
                                    Ok(conn) => conn,
                                    Err(e) => {
                                        error!("Failed to get Redis connection: {}", e);
                                        continue;
                                    },
                                };

                                // Reuse references to avoid unnecessary allocations
                                let sk = &stream_key;
                                let gn = &group_name;
                                let cn = &consumer_name;

                                for idle_consumer in &idle_consumers {
                                    // Get pending messages for the idle consumer
                                    // Define the complex type once
                                    type PendingResult =
                                        Vec<(String, String, u64, Vec<(String, u64)>)>;

                                    let pending_result: Result<
                                        PendingResult,
                                        bb8_redis::redis::RedisError,
                                    > = bb8_redis::redis::cmd("XPENDING")
                                            .arg(sk)
                                            .arg(gn)
                                            .arg("-")  // start ID
                                            .arg("+")  // end ID
                                            .arg(10)   // count (claim in batches)
                                            .arg(idle_consumer)
                                            .query_async(&mut *conn)
                                            .await;

                                    match pending_result {
                                        Ok(pending_msgs) if !pending_msgs.is_empty() => {
                                            let msg_ids: Vec<String> = pending_msgs
                                                .iter()
                                                .map(|(id, ..)| id.clone())
                                                .collect();

                                            // Claim the messages
                                            let _: Result<(), bb8_redis::redis::RedisError> =
                                                bb8_redis::redis::cmd("XCLAIM")
                                                    .arg(sk)
                                                    .arg(gn)
                                                    .arg(cn)
                                                    .arg(0) // min-idle-time of zero to claim immediately
                                                    .arg(&msg_ids)
                                                    .arg("JUSTID") // We just want to claim ownership
                                                    .query_async(&mut *conn)
                                                    .await;

                                            tracing::info!(
                                                "Rebalanced {} messages from idle consumer {} to {} for stream {}",
                                                msg_ids.len(),
                                                idle_consumer,
                                                cn,
                                                sk
                                            );
                                        },
                                        Ok(_) => {
                                            // No pending messages for this consumer
                                        },
                                        Err(e) => {
                                            error!(
                                                "Error getting pending messages for consumer {}: {}",
                                                idle_consumer, e
                                            );
                                        },
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to get consumer group health: {}", e);
                    },
                }
            }
        });
    }

    pub fn pipeline(&self, key: &str) -> RedisPipeline {
        RedisPipeline {
            redis: Arc::clone(&self.redis),
            key: key.to_string(),
            commands: Vec::new(),
            maxlen: None,
        }
    }

    pub fn pipeline_maxlen(&self, key: &str, maxlen: u64) -> RedisPipeline {
        RedisPipeline {
            redis: Arc::clone(&self.redis),
            key: key.to_string(),
            commands: Vec::new(),
            maxlen: Some(maxlen),
        }
    }

    pub async fn wait_until_ready(&self, timeout: Duration) -> Result<(), Error> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.redis.check_connection().await.is_ok() {
                if self.health_check_enabled {
                    self.start_health_check().await;
                }
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::RedisError(RedisError::from((
            bb8_redis::redis::ErrorKind::IoError,
            "Redis connection timeout",
        ))))
    }

    pub async fn create_group(&self, key: &str, group: &str) -> Result<(), Error> {
        let mut attempts = 0;
        loop {
            match self.try_create_group(key, group).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    warn!("Retrying group creation after error: {}", e);
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    async fn try_create_group(&self, key: &str, group: &str) -> Result<(), Error> {
        let mut conn = self.redis.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let create_result: RedisResult<()> = bb8_redis::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(key)
            .arg(group)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut *conn)
            .await;

        match create_result {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("BUSYGROUP") => Ok(()),
            Err(_) => {
                let info_result: RedisResult<Vec<Vec<String>>> = bb8_redis::redis::cmd("XINFO")
                    .arg("GROUPS")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await;

                match info_result {
                    Ok(groups) => {
                        if groups.iter().any(|group_info| group_info[1] == group) {
                            Ok(())
                        } else {
                            let retry_result: RedisResult<()> = bb8_redis::redis::cmd("XGROUP")
                                .arg("CREATE")
                                .arg(key)
                                .arg(group)
                                .arg("0")
                                .arg("MKSTREAM")
                                .query_async(&mut *conn)
                                .await;

                            match retry_result {
                                Ok(_) => Ok(()),
                                Err(e) if e.to_string().contains("BUSYGROUP") => Ok(()),
                                Err(e) => Err(Error::RedisError(e)),
                            }
                        }
                    },
                    Err(e) => Err(Error::RedisError(e)),
                }
            },
        }
    }

    pub async fn add(&self, key: &str, data: Vec<u8>) -> Result<String, Error> {
        let mut attempts = 0;
        loop {
            match self.redis.xadd(key, &data).await {
                Ok(id) => return Ok(id),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    warn!("Retrying add after error: {}", e);
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    pub async fn add_maxlen(&self, key: &str, maxlen: u64, data: Vec<u8>) -> Result<String, Error> {
        let mut attempts = 0;
        loop {
            match self.redis.xadd_maxlen(key, Some(maxlen), &data).await {
                Ok(id) => return Ok(id),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    warn!("Retrying add_maxlen after error: {}", e);
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    pub async fn add_batch(&self, key: &str, batch: Vec<Vec<u8>>) -> Result<Vec<String>, Error> {
        // Process in configurable batch sizes for better performance
        let mut all_ids = Vec::with_capacity(batch.len());

        for chunk in batch.chunks(self.batch_size) {
            let mut pipeline = self.pipeline(key);
            for data in chunk {
                pipeline.xadd(data);
            }
            let chunk_ids = pipeline.execute().await?;
            all_ids.extend(chunk_ids);
        }

        Ok(all_ids)
    }

    pub async fn add_batch_maxlen(
        &self,
        key: &str,
        maxlen: u64,
        batch: Vec<Vec<u8>>,
    ) -> Result<Vec<String>, Error> {
        // Process in configurable batch sizes for better performance
        let mut all_ids = Vec::with_capacity(batch.len());

        for chunk in batch.chunks(self.batch_size) {
            let mut pipeline = self.pipeline_maxlen(key, maxlen);
            for data in chunk {
                pipeline.xadd(data);
            }
            let chunk_ids = pipeline.execute().await?;
            all_ids.extend(chunk_ids);
        }

        Ok(all_ids)
    }

    pub async fn reserve(
        &self,
        key: &str,
        group: &str,
        count: u64,
    ) -> Result<Vec<StreamEntry>, Error> {
        let consumer_id = format!("consumer-{}", std::process::id());
        let entries = self.redis.xreadgroup(group, &consumer_id, key, count).await?;

        Ok(entries.into_iter().map(|(id, data)| StreamEntry { id, data, attempts: 0 }).collect())
    }

    pub async fn ack(&self, key: &str, group: &str, ids: Vec<String>) -> Result<(), Error> {
        if ids.is_empty() {
            return Ok(());
        }

        // Process in optimal batch sizes for better performance
        for chunk in ids.chunks(self.batch_size) {
            let mut attempts = 0;
            let chunk_ids = chunk.to_vec();

            loop {
                let mut pipeline = self.pipeline(key);
                for id in &chunk_ids {
                    pipeline.xack(group, id);
                }

                match pipeline.execute_acks().await {
                    Ok(_) => break,
                    Err(e) => {
                        attempts += 1;
                        if attempts >= MAX_RETRY_ATTEMPTS {
                            return Err(e);
                        }
                        warn!("Retrying ack after error: {}", e);
                        tokio::time::sleep(RETRY_DELAY).await;
                    },
                }
            }
        }

        Ok(())
    }

    pub async fn pending(
        &self,
        key: &str,
        group: &str,
        min_idle: Duration,
        count: u64,
    ) -> Result<Vec<PendingItem>, Error> {
        self.redis.xpending(key, group, min_idle, count).await
    }

    pub async fn claim_stale(
        &self,
        key: &str,
        group: &str,
        min_idle: Duration,
        count: u64,
    ) -> Result<Vec<StreamEntry>, Error> {
        let pending = self.pending(key, group, min_idle, count).await?;
        if pending.is_empty() {
            return Ok(vec![]);
        }

        // Process regular claims and check for messages exceeding retry count
        let mut regular_ids = Vec::new();
        let mut dead_letter_ids = Vec::new();

        for item in &pending {
            // Check if message exceeds retry limit
            if item.delivery_count >= MAX_MESSAGE_RETRIES {
                dead_letter_ids.push(item.id.clone());
                self.update_dead_letter_metrics().await;
            } else {
                regular_ids.push(item.id.clone());
                self.update_retry_metrics().await;
            }
        }

        // Handle dead letter items according to policy
        if !dead_letter_ids.is_empty() {
            match &self.dead_letter_policy {
                crate::redis::types::DeadLetterPolicy::Discard => {
                    // Just acknowledge them to remove from pending
                    if !dead_letter_ids.is_empty() {
                        tracing::warn!(
                            "Discarding {} messages that exceeded retry limit",
                            dead_letter_ids.len()
                        );
                        self.ack(key, group, dead_letter_ids).await?;
                    }
                },
                crate::redis::types::DeadLetterPolicy::MoveToDeadLetter { queue_name } => {
                    // First get the content of the messages to move
                    let mut conn =
                        self.redis.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

                    for id in &dead_letter_ids {
                        // Get message data
                        // Define the complex type once
                        type XRangeResult = Vec<(String, Vec<(String, Vec<u8>)>)>;

                        let result: Result<XRangeResult, bb8_redis::redis::RedisError> =
                            bb8_redis::redis::cmd("XRANGE")
                                .arg(key)
                                .arg(id)
                                .arg(id)
                                .query_async(&mut *conn)
                                .await;

                        if let Ok(entries) = result {
                            for (_, fields) in entries {
                                for (field, value) in fields {
                                    if field == "d" {
                                        // Add to dead letter queue with original message and metadata
                                        let mut dlq_data = value.clone();
                                        dlq_data.extend_from_slice(
                                            format!(
                                                "\nOriginal stream: {}\nFailed after: {} attempts",
                                                key, MAX_MESSAGE_RETRIES
                                            )
                                            .as_bytes(),
                                        );

                                        // Add to dead letter queue
                                        let _: Result<String, bb8_redis::redis::RedisError> =
                                            bb8_redis::redis::cmd("XADD")
                                                .arg(queue_name)
                                                .arg("*")
                                                .arg("d")
                                                .arg(&dlq_data)
                                                .arg("original_stream")
                                                .arg(key)
                                                .arg("original_id")
                                                .arg(id)
                                                .query_async(&mut *conn)
                                                .await;

                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Acknowledge the dead letter messages in the original stream
                    tracing::warn!(
                        "Moved {} messages to dead letter queue {}",
                        dead_letter_ids.len(),
                        queue_name
                    );
                    self.ack(key, group, dead_letter_ids).await?;
                },
            }
        }

        // Continue with regular processing for messages that haven't exceeded retry limit
        if regular_ids.is_empty() {
            return Ok(vec![]);
        }

        let claimed = self.redis.xclaim(key, group, "consumer", min_idle, &regular_ids).await?;

        Ok(claimed
            .into_iter()
            .zip(pending.iter().filter(|p| p.delivery_count < MAX_MESSAGE_RETRIES))
            .map(|((id, data), pending_item)| StreamEntry {
                id,
                data,
                attempts: pending_item.delivery_count,
            })
            .collect())
    }

    pub async fn stream_size(&self, key: &str) -> Result<u64, Error> {
        self.redis.xlen(key).await
    }

    pub async fn delete(&self, key: &str, id: &str) -> Result<(), Error> {
        let mut attempts = 0;
        loop {
            match self.redis.xdel(key, id).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    warn!("Retrying delete after error: {}", e);
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    pub async fn trim(&self, key: &str, timestamp: Duration) -> Result<u64, Error> {
        let mut attempts = 0;
        loop {
            match self.redis.xtrim(key, timestamp).await {
                Ok(count) => return Ok(count),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(e);
                    }
                    warn!("Retrying trim after error: {}", e);
                    tokio::time::sleep(RETRY_DELAY).await;
                },
            }
        }
    }

    pub async fn monitor_pending_messages(&self, key: String, group: String, alert_threshold: u64) {
        let redis = Arc::clone(&self.redis);
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                match redis.xinfo_groups(&key).await {
                    Ok(groups) => {
                        for group_info in groups {
                            if let Some(pending) = group_info.get("pending") {
                                if let Ok(count) = pending.parse::<u64>() {
                                    if count > alert_threshold {
                                        warn!(
                                            "High number of pending messages: {} for group {} in stream {}",
                                            count, group, key
                                        );
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => error!("Failed to monitor pending messages: {}", e),
                }
            }
        });
    }

    pub async fn close(&self) {
        // Release any active connections
        if let Ok(conn) = self.redis.pool.get().await {
            drop(conn);
        }
    }
}

impl RedisPipeline {
    pub fn xadd(&mut self, data: &[u8]) -> &mut Self {
        self.commands.push(data.to_vec());
        self
    }

    pub fn xack(&mut self, group: &str, id: &str) -> &mut Self {
        self.commands.push(format!("XACK {} {} {}", self.key, group, id).into_bytes());
        self
    }

    pub async fn execute(self) -> Result<Vec<String>, Error> {
        // Use individual commands regardless of batch size to avoid Redis errors
        let mut ids = Vec::with_capacity(self.commands.len());

        for cmd in self.commands {
            // Get a fresh connection for each command to avoid pipeline issues
            let mut conn =
                self.redis.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

            let mut redis_cmd = bb8_redis::redis::cmd("XADD");
            redis_cmd.arg(&self.key);

            if let Some(maxlen) = self.maxlen {
                redis_cmd.arg("MAXLEN").arg("~").arg(maxlen.to_string());
            }

            redis_cmd.arg("*").arg("d").arg(&cmd);

            // Execute each command individually
            let id: String = redis_cmd.query_async(&mut *conn).await.map_err(Error::RedisError)?;
            ids.push(id);
        }

        Ok(ids)
    }

    pub async fn execute_acks(self) -> Result<(), Error> {
        if self.commands.is_empty() {
            return Ok(());
        }

        // Use individual commands for ACKs instead of pipeline to avoid Redis errors
        for cmd in self.commands {
            let cmd_str = String::from_utf8_lossy(&cmd);
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();

            if parts[0] == "XACK" && parts.len() >= 4 {
                // Get a fresh connection for each ACK command
                let mut conn =
                    self.redis.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

                let mut redis_cmd = bb8_redis::redis::cmd(parts[0]);
                for part in &parts[1..] {
                    redis_cmd.arg(*part);
                }

                // Execute each ACK individually
                let _: () = redis_cmd.query_async(&mut *conn).await.map_err(Error::RedisError)?;
            }
        }

        Ok(())
    }
}
