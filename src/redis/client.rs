use crate::{
    config::RedisConfig,
    redis::{error::Error, types::PendingItem},
};
use bb8_redis::{RedisConnectionManager, redis::RedisResult};
use std::time::Duration;
use tracing::{info, warn};

pub struct Redis {
    pub pool: bb8::Pool<RedisConnectionManager>,
    config: Option<RedisConfig>,
}

impl Redis {
    pub async fn new(config: &RedisConfig) -> Result<Self, Error> {
        let manager =
            RedisConnectionManager::new(config.url.as_str()).map_err(Error::RedisError)?;

        let builder = bb8::Pool::builder()
            .retry_connection(true)
            .max_size(config.pool_size)
            .connection_timeout(Duration::from_millis(config.connection_timeout_ms))
            .idle_timeout(Some(Duration::from_secs(config.idle_timeout_secs)))
            .max_lifetime(Some(Duration::from_secs(config.max_connection_lifetime_secs)));

        let pool = builder.build(manager).await.map_err(|e| Error::PoolError(e.to_string()))?;

        info!(
            "Initialized Redis pool with {} max connections, {}ms connection timeout, {}s idle timeout, {}s max lifetime",
            config.pool_size,
            config.connection_timeout_ms,
            config.idle_timeout_secs,
            config.max_connection_lifetime_secs
        );

        // Start pool health monitoring
        let pool_monitor = pool.clone();
        let max_pool_size = config.pool_size;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let state = pool_monitor.state();
                let idle_pct = if state.connections > 0 {
                    (state.idle_connections as f32 / state.connections as f32) * 100.0
                } else {
                    0.0
                };

                info!(
                    "Redis pool health: {} total connections, {} idle ({:.1}%), {} pending",
                    state.connections,
                    state.idle_connections,
                    idle_pct,
                    state.connections - state.idle_connections
                );

                if idle_pct < 20.0 && state.connections >= max_pool_size {
                    warn!("Redis pool under pressure: only {:.1}% idle connections", idle_pct);
                }
            }
        });

        Ok(Self { pool, config: Some(config.clone()) })
    }

    /// Create an empty Redis instance for testing/placeholder purposes
    /// This should not be used in production code
    pub fn empty() -> Self {
        let url = "redis://localhost:6379".to_string();
        let manager = RedisConnectionManager::new(url).expect("Failed to create Redis manager");
        let pool = bb8::Pool::builder().max_size(1).build_unchecked(manager);

        Self { pool, config: None }
    }

    pub fn config(&self) -> Option<&RedisConfig> {
        self.config.as_ref()
    }

    /// Get Redis connection pool health metrics
    pub fn get_pool_health(&self) -> (u32, u32) {
        let state = self.pool.state();
        (state.connections, state.idle_connections)
    }

    /// Check if pool is under pressure (low available connections)
    pub fn is_pool_under_pressure(&self) -> bool {
        let state = self.pool.state();
        let available = state.idle_connections;
        let total = state.connections;

        // Consider under pressure if < 20% connections available
        if total > 0 {
            let available_percentage = (available as f32 / total as f32) * 100.0;
            available_percentage < 20.0
        } else {
            true
        }
    }

    /// Get a connection with timeout and backpressure awareness
    pub async fn get_connection_with_timeout(
        &self,
        timeout_ms: u64,
    ) -> Result<bb8::PooledConnection<'_, RedisConnectionManager>, Error> {
        let timeout = Duration::from_millis(timeout_ms);

        tokio::time::timeout(timeout, self.pool.get())
            .await
            .map_err(|_| Error::PoolError("Connection timeout".to_string()))?
            .map_err(|e| Error::PoolError(e.to_string()))
    }

    /// Get consumer group information and health metrics
    pub async fn get_consumer_group_health(
        &self,
        stream_key: &str,
        group_name: &str,
    ) -> Result<crate::redis::types::ConsumerGroupHealth, Error> {
        use crate::redis::types::{ConsumerGroupHealth, ConsumerInfo};
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        // Get stream information for calculating lag
        let xinfo_stream: RedisResult<Vec<(String, String)>> = bb8_redis::redis::cmd("XINFO")
            .arg("STREAM")
            .arg(stream_key)
            .query_async(&mut *conn)
            .await;

        let last_id = match xinfo_stream {
            Ok(info) => info
                .iter()
                .find(|(key, _)| key == "last-generated-id")
                .map(|(_, val)| val.clone())
                .unwrap_or_default(),
            Err(_) => String::new(),
        };

        // Get group information
        let xinfo_groups: RedisResult<Vec<Vec<String>>> = bb8_redis::redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(stream_key)
            .query_async(&mut *conn)
            .await;

        let mut health = ConsumerGroupHealth {
            group_name: group_name.to_string(),
            stream_key: stream_key.to_string(),
            pending_count: 0,
            consumers: Vec::new(),
            lag: 0,
        };

        if let Ok(groups) = xinfo_groups {
            for group in groups {
                if group.len() >= 9 && group[1] == group_name {
                    // Parse pending count
                    if let Ok(pending) = group[5].parse::<u64>() {
                        health.pending_count = pending;
                    }

                    // Parse lag based on last-delivered-id vs last-generated-id
                    if !last_id.is_empty() && group.len() >= 9 {
                        let last_delivered = &group[7];
                        health.lag = self.calculate_id_lag(&last_id, last_delivered);
                    }

                    // Get consumer information
                    let xinfo_consumers: RedisResult<Vec<Vec<String>>> =
                        bb8_redis::redis::cmd("XINFO")
                            .arg("CONSUMERS")
                            .arg(stream_key)
                            .arg(group_name)
                            .query_async(&mut *conn)
                            .await;

                    if let Ok(consumers) = xinfo_consumers {
                        for consumer in consumers {
                            if consumer.len() >= 7 {
                                let name = consumer[1].clone();
                                let pending = consumer[3].parse::<u64>().unwrap_or(0);
                                let idle = consumer[5].parse::<u64>().unwrap_or(0);

                                health.consumers.push(ConsumerInfo {
                                    name,
                                    pending_count: pending,
                                    idle_time: idle,
                                });
                            }
                        }
                    }

                    break;
                }
            }
        }

        Ok(health)
    }

    /// Calculate lag between two Redis stream IDs
    fn calculate_id_lag(&self, current_id: &str, delivered_id: &str) -> u64 {
        if delivered_id == "0-0" || current_id.is_empty() || delivered_id.is_empty() {
            return 0;
        }

        let parse_id = |id: &str| -> (u64, u64) {
            let parts: Vec<&str> = id.split('-').collect();
            if parts.len() == 2 {
                let timestamp = parts[0].parse::<u64>().unwrap_or(0);
                let sequence = parts[1].parse::<u64>().unwrap_or(0);
                (timestamp, sequence)
            } else {
                (0, 0)
            }
        };

        let (current_ts, current_seq) = parse_id(current_id);
        let (delivered_ts, delivered_seq) = parse_id(delivered_id);

        // Calculate rough message lag - this is approximate
        if current_ts > delivered_ts {
            let ts_diff = current_ts - delivered_ts;
            // Rough estimate: each millisecond might have multiple messages
            ts_diff * 10 + (current_seq - delivered_seq).min(100)
        } else if current_ts == delivered_ts && current_seq > delivered_seq {
            current_seq - delivered_seq
        } else {
            0
        }
    }

    /// Get detailed stream metrics
    pub async fn get_stream_metrics(
        &self,
        stream_key: &str,
    ) -> Result<crate::redis::types::StreamMetrics, Error> {
        use crate::redis::types::StreamMetrics;
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        // Get stream length
        let len: RedisResult<u64> =
            bb8_redis::redis::cmd("XLEN").arg(stream_key).query_async(&mut *conn).await;

        let mut metrics = StreamMetrics::default();

        // Set processed count based on stream length
        if let Ok(count) = len {
            metrics.processed_count = count;
        }

        // More sophisticated metrics would require custom tracking
        // This would be implemented in the stream processor

        Ok(metrics)
    }

    pub async fn check_connection(&self) -> Result<bool, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let response: RedisResult<String> =
            bb8_redis::redis::cmd("PING").query_async(&mut *conn).await;

        match response {
            Ok(s) => Ok(s == "PONG"),
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn can_process_event(&self, event_id: u64) -> Result<bool, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let event_cache_ttl = Duration::from_secs(60).as_secs();

        let key = format!("processed_event:{}", event_id);
        let result: RedisResult<bool> = bb8_redis::redis::cmd("SET")
            .arg(&key)
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(event_cache_ttl)
            .query_async(&mut *conn)
            .await;

        result.map_err(Error::RedisError)
    }

    pub async fn xadd(&self, key: &str, value: &[u8]) -> Result<String, Error> {
        self.xadd_maxlen(key, None, value).await
    }

    pub async fn xadd_maxlen(
        &self,
        key: &str,
        maxlen: Option<u64>,
        value: &[u8],
    ) -> Result<String, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let mut cmd = bb8_redis::redis::cmd("XADD");
        cmd.arg(key);

        // Add MAXLEN if specified - always use MINID for better performance
        if let Some(len) = maxlen {
            cmd.arg("MAXLEN")
                .arg("!") // Use exact trimming but with NOMKSTREAM to avoid creating the stream if it doesn't exist
                .arg(len);
        }

        let result: RedisResult<String> =
            cmd.arg("*").arg("d").arg(value).query_async(&mut *conn).await;

        result.map_err(Error::RedisError)
    }

    pub async fn xinfo_groups(
        &self,
        key: &str,
    ) -> Result<Vec<std::collections::HashMap<String, String>>, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<Vec<Vec<String>>> =
            bb8_redis::redis::cmd("XINFO").arg("GROUPS").arg(key).query_async(&mut *conn).await;

        result
            .map(|groups| {
                groups
                    .into_iter()
                    .map(|group| {
                        let mut map = std::collections::HashMap::new();
                        for chunk in group.chunks(2) {
                            if chunk.len() == 2 {
                                map.insert(chunk[0].clone(), chunk[1].clone());
                            }
                        }
                        map
                    })
                    .collect()
            })
            .map_err(Error::RedisError)
    }

    pub async fn xreadgroup(
        &self,
        group: &str,
        consumer: &str,
        key: &str,
        count: u64,
    ) -> Result<Vec<(String, Vec<u8>)>, Error> {
        // Check pool pressure and use shorter connection timeout if needed
        let conn_timeout = if self.is_pool_under_pressure() { 1000 } else { 2000 };
        let mut conn = self.get_connection_with_timeout(conn_timeout).await?;

        type StreamResponse = Vec<(String, Vec<(String, Vec<(String, Vec<u8>)>)>)>;

        // Use short blocking timeout to balance efficiency and connection availability
        // 10ms blocking is more efficient than polling while still releasing connections quickly
        let block_timeout = if self.is_pool_under_pressure() { 0 } else { 10 };

        let result: RedisResult<Option<StreamResponse>> = bb8_redis::redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(group)
            .arg(consumer)
            .arg("COUNT")
            .arg(count)
            .arg("BLOCK")
            .arg(block_timeout) // Short block or non-blocking under pressure
            .arg("STREAMS")
            .arg(key)
            .arg(">")
            .query_async(&mut *conn)
            .await;

        match result {
            Ok(Some(streams)) => {
                // Process results more efficiently with capacity pre-allocation
                let mut results = Vec::with_capacity(count as usize);

                for (_, entries) in streams {
                    for (id, entry_data) in entries {
                        // Fast path for the common case where field name is "d"
                        for (field, value) in entry_data {
                            if field == "d" {
                                results.push((id.clone(), value));
                                break;
                            }
                        }
                    }
                }

                Ok(results)
            },
            Ok(None) => Ok(Vec::new()), // Return empty vec instead of error
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn xack(&self, key: &str, group: &str, id: &str) -> Result<(), Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<()> =
            bb8_redis::redis::cmd("XACK").arg(key).arg(group).arg(id).query_async(&mut *conn).await;

        result.map_err(Error::RedisError)
    }

    pub async fn xpending(
        &self,
        key: &str,
        group: &str,
        idle: Duration,
        count: u64,
    ) -> Result<Vec<PendingItem>, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        // First try with IDLE parameter (Redis 6.2+)
        let items_with_idle: RedisResult<Vec<(String, String, String, String)>> =
            bb8_redis::redis::cmd("XPENDING")
                .arg(key)
                .arg(group)
                .arg("IDLE")
                .arg(idle.as_millis() as u64)
                .arg("-")
                .arg("+")
                .arg(count)
                .query_async(&mut *conn)
                .await;

        match items_with_idle {
            Ok(items) => Ok(items
                .into_iter()
                .map(|(id, _, idle_time, count)| PendingItem {
                    id,
                    idle_time: idle_time.parse().unwrap_or(0),
                    delivery_count: count.parse().unwrap_or(0),
                })
                .collect()),
            Err(_) => {
                // Fallback to standard XPENDING without IDLE (older Redis versions)
                // We'll have to filter by idle time manually
                let items: RedisResult<Vec<(String, String, String, String)>> =
                    bb8_redis::redis::cmd("XPENDING")
                        .arg(key)
                        .arg(group)
                        .arg("-")
                        .arg("+")
                        .arg(count * 2) // Get more to account for filtering
                        .query_async(&mut *conn)
                        .await;

                items
                    .map(|items| {
                        let idle_threshold = idle.as_millis() as u64;
                        items
                            .into_iter()
                            .filter_map(|(id, _, idle_time, count)| {
                                let idle_ms = idle_time.parse().unwrap_or(0);
                                if idle_ms >= idle_threshold {
                                    Some(PendingItem {
                                        id,
                                        idle_time: idle_ms,
                                        delivery_count: count.parse().unwrap_or(0),
                                    })
                                } else {
                                    None
                                }
                            })
                            .take(count as usize)
                            .collect()
                    })
                    .map_err(Error::RedisError)
            },
        }
    }

    pub async fn xclaim(
        &self,
        key: &str,
        group: &str,
        consumer: &str,
        min_idle_time: Duration,
        ids: &[String],
    ) -> Result<Vec<(String, Vec<u8>)>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        // Use JUSTID to reduce network traffic when claiming many messages
        // This avoids transferring the full message content which we'll fetch separately
        // Add FORCE option to forcibly claim messages regardless of idle time (Redis 7.0+)
        let claimed_ids: RedisResult<Vec<String>> = bb8_redis::redis::cmd("XCLAIM")
            .arg(key)
            .arg(group)
            .arg(consumer)
            .arg(min_idle_time.as_millis() as u64)
            .arg(ids)
            .arg("FORCE")  // Force claim even if messages don't satisfy idle time
            .arg("JUSTID") // Only return the message ID, not the full content
            .query_async(&mut *conn)
            .await;

        match claimed_ids {
            Ok(ids) => {
                if ids.is_empty() {
                    return Ok(Vec::new());
                }

                // Now fetch the actual message content for the claimed IDs
                // Use XRANGE to fetch the messages in a single command
                // Define the complex type once
                type XRangeResult = Vec<(String, Vec<(String, Vec<u8>)>)>;

                let result: RedisResult<XRangeResult> = bb8_redis::redis::cmd("XRANGE")
                    .arg(key)
                    .arg(ids.first().unwrap())
                    .arg(ids.last().unwrap())
                    .query_async(&mut *conn)
                    .await;

                match result {
                    Ok(entries) => {
                        let mut results = Vec::with_capacity(entries.len());
                        for (id, entry_data) in entries {
                            if ids.contains(&id) {
                                // Ensure we only include claimed messages
                                for (field, value) in entry_data {
                                    if field == "d" {
                                        results.push((id.clone(), value));
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(results)
                    },
                    Err(e) => Err(Error::RedisError(e)),
                }
            },
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn xlen(&self, key: &str) -> Result<u64, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<u64> =
            bb8_redis::redis::cmd("XLEN").arg(key).query_async(&mut *conn).await;

        result.map_err(Error::RedisError)
    }

    pub async fn xdel(&self, key: &str, id: &str) -> Result<(), Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<()> =
            bb8_redis::redis::cmd("XDEL").arg(key).arg(id).query_async(&mut *conn).await;

        result.map_err(Error::RedisError)
    }

    pub async fn xtrim(&self, key: &str, timestamp: Duration) -> Result<u64, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let minid = timestamp.as_millis() as u64;

        let result: RedisResult<u64> = bb8_redis::redis::cmd("XTRIM")
            .arg(key)
            .arg("MINID")
            .arg("~") // Approximate trimming for better performance
            .arg(minid)
            .query_async(&mut *conn)
            .await;

        result.map_err(Error::RedisError)
    }

    pub async fn get_last_processed_event(&self, key: &str) -> Result<Option<u64>, Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<Option<String>> =
            bb8_redis::redis::cmd("GET").arg(key).query_async(&mut *conn).await;

        info!("key, {}, result: {:?}", key, result);

        match result {
            Ok(Some(val)) => Ok(Some(val.parse().unwrap_or(0))),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn set_last_processed_event(&self, key: &str, event_id: u64) -> Result<(), Error> {
        let mut conn = self.pool.get().await.map_err(|e| Error::PoolError(e.to_string()))?;

        let result: RedisResult<()> = bb8_redis::redis::cmd("SET")
            .arg(key)
            .arg(event_id.to_string())
            .query_async(&mut *conn)
            .await;

        result.map_err(Error::RedisError)
    }
}
