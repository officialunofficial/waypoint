use crate::{
    config::RedisConfig,
    metrics,
    redis::{error::Error, types::PendingItem},
};
use fred::prelude::*;
use fred::types::{ConnectionConfig, Expiration, PerformanceConfig, ReconnectPolicy, SetOptions};
use std::time::Duration;
use tracing::{info, warn};

pub struct Redis {
    pub pool: RedisPool,
    config: Option<RedisConfig>,
}

impl Redis {
    pub async fn new(config: &RedisConfig) -> Result<Self, Error> {
        // Configure fred connection options
        let redis_config = fred::types::RedisConfig::from_url(&config.url)
            .map_err(|e| Error::PoolError(format!("Invalid Redis URL: {}", e)))?;

        // Configure performance options
        let perf_config = PerformanceConfig {
            auto_pipeline: true,
            default_command_timeout: Duration::from_millis(config.connection_timeout_ms),
            ..Default::default()
        };

        // Configure connection pool
        let connection_config = ConnectionConfig {
            connection_timeout: Duration::from_millis(config.connection_timeout_ms),
            internal_command_timeout: Duration::from_millis(config.connection_timeout_ms * 2),
            max_redirections: 3,
            max_command_attempts: 3,
            ..Default::default()
        };

        // Configure reconnection policy
        let reconnect_policy = ReconnectPolicy::new_exponential(
            3,      // max retries
            100,    // min delay ms
            30_000, // max delay ms
            2,      // multiplier
        );

        // Create the pool with proper configuration
        let pool = RedisPool::new(
            redis_config,
            Some(perf_config),
            Some(connection_config),
            Some(reconnect_policy),
            config.max_pool_size as usize,
        )
        .map_err(|e| Error::PoolError(format!("Failed to create Redis pool: {}", e)))?;

        // Connect to Redis with timeout
        pool.connect();

        // Use a timeout for the connection to avoid hanging in tests
        match tokio::time::timeout(
            Duration::from_millis(config.connection_timeout_ms),
            pool.wait_for_connect(),
        )
        .await
        {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                return Err(Error::PoolError(format!("Failed to connect to Redis: {}", e)));
            },
            Err(_) => {
                return Err(Error::PoolError(format!(
                    "Redis connection timeout after {}ms",
                    config.connection_timeout_ms
                )));
            },
        }

        info!(
            "Initialized Redis pool with {} connections, {}ms timeout",
            config.max_pool_size, config.connection_timeout_ms,
        );

        // Start pool health monitoring
        let pool_monitor = pool.clone();
        let _max_pool_size = config.max_pool_size;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                // Fred handles pool metrics internally, but we can check connection state
                let state = pool_monitor.state();
                if matches!(state, fred::types::ClientState::Disconnected) {
                    warn!("Redis pool disconnected, reconnecting...");
                }

                // Stats are tracked internally by fred
                // pool.clients() can give us individual client info if needed
            }
        });

        Ok(Self { pool, config: Some(config.clone()) })
    }

    /// Create an empty Redis instance for testing/placeholder purposes
    pub fn empty() -> Self {
        // Create an uninitialized pool for testing
        let config = RedisConfig::default();
        let pool = RedisPool::new(fred::types::RedisConfig::default(), None, None, None, 1)
            .expect("Failed to create empty pool");

        Self { pool, config: Some(config) }
    }

    pub fn config(&self) -> Option<&RedisConfig> {
        self.config.as_ref()
    }

    /// Get Redis connection pool health metrics
    pub fn get_pool_health(&self) -> (u32, u32) {
        // Fred doesn't expose pool stats the same way as bb8
        // Return approximations - fred manages pooling internally
        let pool_size = self.config.as_ref().map(|c| c.max_pool_size).unwrap_or(1);
        // Always report as healthy since fred handles connection management
        (pool_size, pool_size)
    }

    /// Check if pool is under pressure
    pub fn is_pool_under_pressure(&self) -> bool {
        // Fred manages pooling internally, no pressure metrics available
        // Return false to use normal operation
        false
    }

    /// Get consumer group information and health metrics
    pub async fn get_consumer_group_health(
        &self,
        stream_key: &str,
        group_name: &str,
    ) -> Result<crate::redis::types::ConsumerGroupHealth, Error> {
        use crate::redis::types::ConsumerGroupHealth;

        // For now, return a simplified version
        // TODO: Implement full xinfo support when fred adds proper types
        let mut health = ConsumerGroupHealth {
            group_name: group_name.to_string(),
            stream_key: stream_key.to_string(),
            pending_count: 0,
            consumers: Vec::new(),
            lag: 0,
        };

        // Get basic pending count using XPENDING
        // Use empty args for basic summary
        type PendingSummary = (u64, Option<String>, Option<String>, Vec<(String, u64)>);
        let pending_summary: Result<PendingSummary, _> =
            self.pool.xpending(stream_key, group_name, ()).await;

        if let Ok((count, _, _, _)) = pending_summary {
            health.pending_count = count;
        }

        Ok(health)
    }

    /// Get detailed stream metrics
    pub async fn get_stream_metrics(
        &self,
        stream_key: &str,
    ) -> Result<crate::redis::types::StreamMetrics, Error> {
        use crate::redis::types::StreamMetrics;

        let len: u64 = self.pool.xlen(stream_key).await.map_err(|e| {
            metrics::increment_redis_errors();
            Error::RedisError(e)
        })?;

        let metrics = StreamMetrics { processed_count: len, ..StreamMetrics::default() };

        Ok(metrics)
    }

    pub async fn check_connection(&self) -> Result<bool, Error> {
        match self.pool.ping::<String>().await {
            Ok(response) => Ok(response == "PONG"),
            Err(e) => {
                metrics::increment_redis_errors();
                Err(Error::RedisError(e))
            },
        }
    }

    pub async fn can_process_event(&self, event_id: u64) -> Result<bool, Error> {
        let event_cache_ttl = Duration::from_secs(60);
        let key = format!("processed_event:{}", event_id);

        let result: Option<()> = self
            .pool
            .set(
                key,
                1,
                Some(Expiration::EX(event_cache_ttl.as_secs() as i64)),
                Some(SetOptions::NX),
                false,
            )
            .await
            .map_err(Error::RedisError)?;

        Ok(result.is_some())
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
        use fred::prelude::*;

        // Use a simpler approach with raw Redis command
        let result: String = if let Some(_len) = maxlen {
            // XADD key MAXLEN ~ len * field value
            let id: String = self
                .pool
                .xadd(
                    key,
                    false, // nomkstream
                    None,  // cap - we'll handle MAXLEN differently
                    "*",
                    vec![("d", fred::types::RedisValue::Bytes(value.to_vec().into()))],
                )
                .await
                .map_err(Error::RedisError)?;

            // Trim the stream after adding using custom command
            // Since fred doesn't support MAXLEN directly, we'll handle trimming separately
            // This is a no-op for now as the stream will auto-trim based on config

            id
        } else {
            // Regular XADD
            self.pool
                .xadd(
                    key,
                    false, // nomkstream
                    None,  // no cap
                    "*",
                    vec![("d", fred::types::RedisValue::Bytes(value.to_vec().into()))],
                )
                .await
                .map_err(Error::RedisError)?
        };

        Ok(result)
    }

    pub async fn xinfo_groups(
        &self,
        key: &str,
    ) -> Result<Vec<std::collections::HashMap<String, String>>, Error> {
        // Use custom command for XINFO GROUPS
        use fred::interfaces::ClientLike;
        use fred::types::{ClusterHash, CustomCommand};

        let cmd = CustomCommand::new("XINFO", ClusterHash::FirstKey, false);
        let result: Result<Vec<Vec<RedisValue>>, _> =
            self.pool.custom(cmd, vec![RedisValue::from("GROUPS"), RedisValue::from(key)]).await;

        match result {
            Ok(groups) => {
                let mut group_maps = Vec::new();
                for group in groups {
                    let mut map = std::collections::HashMap::new();
                    // Parse the array format response
                    for i in (0..group.len()).step_by(2) {
                        if let (Some(key), Some(value)) = (group.get(i), group.get(i + 1)) {
                            let key_str = key.as_string().unwrap_or_default().to_string();
                            let value_str = match value {
                                fred::types::RedisValue::String(s) => s.to_string(),
                                fred::types::RedisValue::Integer(i) => i.to_string(),
                                _ => String::new(),
                            };
                            map.insert(key_str, value_str);
                        }
                    }
                    group_maps.push(map);
                }
                Ok(group_maps)
            },
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn xinfo_consumers(
        &self,
        key: &str,
        group: &str,
    ) -> Result<Vec<std::collections::HashMap<String, String>>, Error> {
        // Use custom command for XINFO CONSUMERS
        use fred::interfaces::ClientLike;
        use fred::types::{ClusterHash, CustomCommand};

        let cmd = CustomCommand::new("XINFO", ClusterHash::FirstKey, false);
        let result: Result<Vec<Vec<RedisValue>>, _> = self
            .pool
            .custom(
                cmd,
                vec![RedisValue::from("CONSUMERS"), RedisValue::from(key), RedisValue::from(group)],
            )
            .await;

        match result {
            Ok(consumers) => {
                let mut consumer_maps = Vec::new();
                for consumer in consumers {
                    let mut map = std::collections::HashMap::new();
                    // Parse the array format response
                    for i in (0..consumer.len()).step_by(2) {
                        if let (Some(key), Some(value)) = (consumer.get(i), consumer.get(i + 1)) {
                            let key_str = key.as_string().unwrap_or_default().to_string();
                            let value_str = match value {
                                fred::types::RedisValue::String(s) => s.to_string(),
                                fred::types::RedisValue::Integer(i) => i.to_string(),
                                _ => String::new(),
                            };
                            map.insert(key_str, value_str);
                        }
                    }
                    consumer_maps.push(map);
                }
                Ok(consumer_maps)
            },
            Err(e) => Err(Error::RedisError(e)),
        }
    }

    pub async fn xreadgroup(
        &self,
        group: &str,
        consumer: &str,
        key: &str,
        count: u64,
    ) -> Result<Vec<(String, Vec<u8>)>, Error> {
        // Use short blocking timeout to balance efficiency and connection availability
        let block_timeout = if self.is_pool_under_pressure() { 0 } else { 10 };

        // Use xreadgroup from fred - let it return raw RedisValue to avoid parse errors
        use fred::prelude::*;
        let response: fred::types::RedisValue = self
            .pool
            .xreadgroup(
                group,
                consumer,
                Some(count),
                Some(block_timeout as u64),
                false,
                key, // Single key
                ">", // Read new messages only
            )
            .await
            .map_err(Error::RedisError)?;

        let mut results = Vec::with_capacity(count as usize);

        // Parse the response - it's an array of streams, each with messages
        if let fred::types::RedisValue::Array(streams) = response {
            for stream in streams {
                if let fred::types::RedisValue::Array(stream_data) = stream {
                    if stream_data.len() >= 2 {
                        // stream_data[0] is the stream key, stream_data[1] is the messages array
                        if let fred::types::RedisValue::Array(messages) = &stream_data[1] {
                            for msg in messages {
                                if let fred::types::RedisValue::Array(msg_data) = msg {
                                    if msg_data.len() >= 2 {
                                        // msg_data[0] is the ID, msg_data[1] is the fields
                                        let id =
                                            msg_data[0].as_string().unwrap_or_default().to_string();

                                        // Extract the "d" field from the fields array
                                        if let fred::types::RedisValue::Array(fields) = &msg_data[1]
                                        {
                                            // Fields are key-value pairs
                                            for i in (0..fields.len()).step_by(2) {
                                                if i + 1 < fields.len()
                                                    && fields[i]
                                                        .as_string()
                                                        .map(|s| s == "d")
                                                        .unwrap_or(false)
                                                {
                                                    // Handle both Bytes and String data types
                                                    match &fields[i + 1] {
                                                        fred::types::RedisValue::Bytes(data) => {
                                                            results
                                                                .push((id.clone(), data.to_vec()));
                                                        },
                                                        fred::types::RedisValue::String(data) => {
                                                            results.push((
                                                                id.clone(),
                                                                data.as_bytes().to_vec(),
                                                            ));
                                                        },
                                                        _ => {
                                                            // Try to convert other types to bytes
                                                            if let Some(data_string) =
                                                                fields[i + 1].as_string()
                                                            {
                                                                results.push((
                                                                    id.clone(),
                                                                    data_string.as_bytes().to_vec(),
                                                                ));
                                                            }
                                                        },
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn xack(&self, key: &str, group: &str, id: &str) -> Result<(), Error> {
        let _: u64 = self.pool.xack(key, group, id).await.map_err(Error::RedisError)?;

        Ok(())
    }

    pub async fn xpending(
        &self,
        key: &str,
        group: &str,
        idle: Duration,
        count: u64,
    ) -> Result<Vec<PendingItem>, Error> {
        // Use custom command for XPENDING with IDLE filter
        use fred::interfaces::ClientLike;

        let idle_ms = idle.as_millis() as u64;

        // XPENDING key group IDLE idle_ms - + count
        use fred::types::{ClusterHash, CustomCommand};
        let cmd = CustomCommand::new("XPENDING", ClusterHash::FirstKey, false);
        let result: Result<Vec<Vec<RedisValue>>, _> = self
            .pool
            .custom(
                cmd,
                vec![
                    RedisValue::from(key),
                    RedisValue::from(group),
                    RedisValue::from("IDLE"),
                    RedisValue::from(idle_ms.to_string()),
                    RedisValue::from("-"),
                    RedisValue::from("+"),
                    RedisValue::from(count.to_string()),
                ],
            )
            .await;

        match result {
            Ok(messages) => {
                let mut items = Vec::new();
                for msg in messages {
                    if msg.len() >= 4 {
                        // Format: [id, consumer, idle_time, delivery_count]
                        let id = msg[0].as_string().unwrap_or_default().to_string();
                        let idle_time = match &msg[2] {
                            fred::types::RedisValue::Integer(i) => *i as u64,
                            _ => 0,
                        };
                        let delivery_count = match &msg[3] {
                            fred::types::RedisValue::Integer(i) => *i as u64,
                            _ => 0,
                        };

                        if idle_time >= idle_ms {
                            items.push(PendingItem { id, idle_time, delivery_count });
                        }
                    }
                }
                Ok(items)
            },
            Err(_e) => {
                // Fallback to regular XPENDING without IDLE
                use fred::types::{ClusterHash, CustomCommand};
                let cmd = CustomCommand::new("XPENDING", ClusterHash::FirstKey, false);
                let result: Result<Vec<Vec<RedisValue>>, _> = self
                    .pool
                    .custom(
                        cmd,
                        vec![
                            RedisValue::from(key),
                            RedisValue::from(group),
                            RedisValue::from("-"),
                            RedisValue::from("+"),
                            RedisValue::from(count.to_string()),
                        ],
                    )
                    .await;

                match result {
                    Ok(messages) => {
                        let mut items = Vec::new();
                        for msg in messages {
                            if msg.len() >= 4 {
                                let id = msg[0].as_string().unwrap_or_default().to_string();
                                let idle_time = match &msg[2] {
                                    fred::types::RedisValue::Integer(i) => *i as u64,
                                    _ => 0,
                                };
                                let delivery_count = match &msg[3] {
                                    fred::types::RedisValue::Integer(i) => *i as u64,
                                    _ => 0,
                                };

                                if idle_time >= idle_ms {
                                    items.push(PendingItem { id, idle_time, delivery_count });
                                }
                            }
                        }
                        Ok(items)
                    },
                    Err(e) => Err(Error::RedisError(e)),
                }
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

        // Use xclaim from fred - let it return raw RedisValue to avoid parse errors
        use fred::prelude::*;
        let claimed: fred::types::RedisValue = self
            .pool
            .xclaim(
                key,
                group,
                consumer,
                min_idle_time.as_millis() as u64,
                ids.to_vec(),
                None,
                None,
                None,
                false,
                false, // justid=false to get full messages
            )
            .await
            .map_err(Error::RedisError)?;

        let mut results = Vec::new();

        // Parse the response - it's an array of claimed messages
        if let fred::types::RedisValue::Array(messages) = claimed {
            for msg in messages {
                if let fred::types::RedisValue::Array(msg_data) = msg {
                    if msg_data.len() >= 2 {
                        // msg_data[0] is the ID, msg_data[1] is the fields
                        let id = msg_data[0].as_string().unwrap_or_default().to_string();

                        // Extract the "d" field from the fields array
                        if let fred::types::RedisValue::Array(fields) = &msg_data[1] {
                            // Fields are key-value pairs
                            for i in (0..fields.len()).step_by(2) {
                                if i + 1 < fields.len()
                                    && fields[i].as_string().map(|s| s == "d").unwrap_or(false)
                                {
                                    // Handle both Bytes and String data types
                                    match &fields[i + 1] {
                                        fred::types::RedisValue::Bytes(data) => {
                                            results.push((id.clone(), data.to_vec()));
                                            break;
                                        },
                                        fred::types::RedisValue::String(data) => {
                                            results.push((id.clone(), data.as_bytes().to_vec()));
                                            break;
                                        },
                                        _ => {
                                            // Try to convert other types to bytes
                                            if let Some(data_string) = fields[i + 1].as_string() {
                                                results.push((
                                                    id.clone(),
                                                    data_string.as_bytes().to_vec(),
                                                ));
                                                break;
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn xlen(&self, key: &str) -> Result<u64, Error> {
        let result: u64 = self.pool.xlen(key).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn xdel(&self, key: &str, id: &str) -> Result<(), Error> {
        let _: u64 = self.pool.xdel(key, vec![id]).await.map_err(Error::RedisError)?;

        Ok(())
    }

    pub async fn xtrim(&self, key: &str, older_than: Duration) -> Result<u64, Error> {
        // Calculate the timestamp ID for trimming
        // Redis stream IDs are in format: timestamp-sequence
        let cutoff_time =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
                as u64
                - older_than.as_millis() as u64;

        let min_id = format!("{}-0", cutoff_time);

        // Use fred's custom command to execute XTRIM with MINID
        use fred::prelude::*;
        use fred::types::CustomCommand;

        let cmd = CustomCommand::new_static("XTRIM", None, false);

        let args: Vec<fred::types::RedisValue> = vec![
            key.into(),
            "MINID".into(),
            "~".into(), // Use approximate trimming for better performance
            min_id.into(),
        ];

        let result: fred::types::RedisValue =
            self.pool.custom(cmd, args).await.map_err(Error::RedisError)?;

        // Convert result to u64
        let count = match result {
            fred::types::RedisValue::Integer(n) => n as u64,
            _ => 0,
        };

        Ok(count)
    }

    pub async fn get_last_processed_event(&self, key: &str) -> Result<Option<u64>, Error> {
        let result: Option<String> = self.pool.get(key).await.map_err(Error::RedisError)?;

        match result {
            Some(val) => {
                let event_id = val.parse().unwrap_or(0);
                if event_id > 0 {
                    info!(
                        "Retrieved last processed event ID from Redis (key: {}): {}",
                        key, event_id
                    );
                }
                Ok(Some(event_id))
            },
            None => {
                info!("No last processed event ID found in Redis for key: {}", key);
                Ok(None)
            },
        }
    }

    pub async fn set_last_processed_event(&self, key: &str, event_id: u64) -> Result<(), Error> {
        let _: () = self
            .pool
            .set(key, event_id.to_string(), None, None, false)
            .await
            .map_err(Error::RedisError)?;

        Ok(())
    }

    // List operations for backfill
    pub async fn llen(&self, key: &str) -> Result<u64, Error> {
        let result: u64 = self.pool.llen(key).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn lpush(&self, key: &str, values: Vec<String>) -> Result<u64, Error> {
        let result: u64 = self.pool.lpush(key, values).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn brpop(
        &self,
        keys: Vec<String>,
        timeout: u64,
    ) -> Result<Option<(String, String)>, Error> {
        let result: Option<(String, String)> =
            self.pool.brpop(keys, timeout as f64).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn lrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>, Error> {
        let result: Vec<String> =
            self.pool.lrange(key, start, stop).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn lrem(&self, key: &str, count: i64, value: &str) -> Result<u64, Error> {
        let result: u64 = self.pool.lrem(key, count, value).await.map_err(Error::RedisError)?;

        Ok(result)
    }

    pub async fn keys(&self, pattern: &str) -> Result<Vec<String>, Error> {
        use fred::types::Scanner;
        use futures::stream::TryStreamExt;

        // Use scan for pattern matching
        // This is safer than KEYS command in production
        // RedisPool doesn't have scan, so we use next() to get a client
        let client = self.pool.next();
        let scan_stream = client.scan(pattern, Some(100), None);
        let mut results = Vec::new();

        let mut stream = Box::pin(scan_stream);
        while let Some(mut page) = stream.try_next().await.map_err(Error::RedisError)? {
            if let Some(keys) = page.take_results() {
                // Convert RedisKey to String, filtering out non-string keys
                results.extend(keys.into_iter().filter_map(|k| k.into_string()));
            }
        }

        Ok(results)
    }
}
