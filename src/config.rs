//! Configuration management for the application
use crate::eth::EthConfig;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Configuration errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to load configuration: {0}")]
    LoadError(String),

    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),

    #[error("Environment error: {0}")]
    EnvError(String),
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub timeout_seconds: u64,
    #[serde(default = "default_store_messages")]
    pub store_messages: bool,
    #[serde(default = "default_db_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_skip_migrations")]
    pub skip_migrations: bool,
}

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    #[serde(default = "default_max_pool_size")]
    pub max_pool_size: u32,
    #[serde(default = "default_redis_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_enable_dead_letter")]
    pub enable_dead_letter: bool,
    #[serde(default = "default_consumer_rebalance_interval")]
    pub consumer_rebalance_interval_seconds: u64,
    #[serde(default = "default_metrics_collection_interval")]
    pub metrics_collection_interval_seconds: u64,
    #[serde(default = "default_connection_timeout_ms")]
    pub connection_timeout_ms: u64,
    /// Circuit breaker configuration
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
}

/// Circuit breaker configuration for Redis operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Enable circuit breaker protection
    #[serde(default = "default_circuit_breaker_enabled")]
    pub enabled: bool,

    /// Number of consecutive failures before opening the circuit
    #[serde(default = "default_cb_failure_threshold")]
    pub failure_threshold: u32,

    /// Time to wait before transitioning from Open to HalfOpen (seconds)
    #[serde(default = "default_cb_open_timeout_secs")]
    pub open_timeout_secs: u64,

    /// Number of successful requests needed to close the circuit from HalfOpen
    #[serde(default = "default_cb_success_threshold")]
    pub success_threshold: u32,

    /// Timeout for considering an operation slow (milliseconds)
    #[serde(default = "default_cb_slow_call_threshold_ms")]
    pub slow_call_threshold_ms: u64,

    /// Ratio of slow calls that triggers circuit open (0.0 - 1.0)
    #[serde(default = "default_cb_slow_call_rate_threshold")]
    pub slow_call_rate_threshold: f64,

    /// Minimum number of calls before evaluating slow call rate
    #[serde(default = "default_cb_minimum_calls")]
    pub minimum_calls_for_rate: u32,
}

fn default_circuit_breaker_enabled() -> bool {
    true
}

fn default_cb_failure_threshold() -> u32 {
    5
}

fn default_cb_open_timeout_secs() -> u64 {
    30
}

fn default_cb_success_threshold() -> u32 {
    3
}

fn default_cb_slow_call_threshold_ms() -> u64 {
    5000 // 5 seconds
}

fn default_cb_slow_call_rate_threshold() -> f64 {
    0.5 // 50% slow calls triggers open
}

fn default_cb_minimum_calls() -> u32 {
    10
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: default_circuit_breaker_enabled(),
            failure_threshold: default_cb_failure_threshold(),
            open_timeout_secs: default_cb_open_timeout_secs(),
            success_threshold: default_cb_success_threshold(),
            slow_call_threshold_ms: default_cb_slow_call_threshold_ms(),
            slow_call_rate_threshold: default_cb_slow_call_rate_threshold(),
            minimum_calls_for_rate: default_cb_minimum_calls(),
        }
    }
}

/// Stream processor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProcessorConfig {
    /// Base consumer group name prefix
    #[serde(default = "default_stream_group_name")]
    pub group_name: String,

    /// Number of messages to fetch per batch
    #[serde(default = "default_stream_batch_size")]
    pub batch_size: u64,

    /// Maximum concurrent message processing tasks
    #[serde(default = "default_stream_concurrency")]
    pub concurrency: usize,

    /// Number of parallel consumers per stream type
    #[serde(default = "default_consumers_per_stream")]
    pub consumers_per_stream: usize,

    /// Timeout for processing a single event (seconds)
    #[serde(default = "default_event_processing_timeout_secs")]
    pub event_processing_timeout_secs: u64,

    /// How long to retain events in streams (seconds)
    #[serde(default = "default_event_retention_secs")]
    pub event_retention_secs: u64,

    /// Interval between cleanup runs (seconds)
    #[serde(default = "default_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,

    /// Threshold for considering a consumer extremely idle (ms)
    #[serde(default = "default_idle_consumer_threshold_ms")]
    pub idle_consumer_threshold_ms: u64,

    /// Maximum retry attempts for Redis operations
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,

    /// Delay between retries (ms)
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// Maximum retries for a message before dead letter
    #[serde(default = "default_max_message_retries")]
    pub max_message_retries: u64,

    /// Health check interval (seconds)
    #[serde(default = "default_health_check_interval_secs")]
    pub health_check_interval_secs: u64,

    /// Batch size for force reclaim operations
    #[serde(default = "default_reclaim_batch_size")]
    pub reclaim_batch_size: usize,

    /// Backpressure configuration
    #[serde(default)]
    pub backpressure: BackpressureConfig,
}

fn default_stream_group_name() -> String {
    "default".to_string()
}

fn default_stream_batch_size() -> u64 {
    50 // Optimized for throughput while maintaining reasonable memory usage
}

fn default_stream_concurrency() -> usize {
    250 // Maximum concurrent message processing tasks
}

fn default_consumers_per_stream() -> usize {
    3 // Parallel consumers per stream type for higher throughput
}

fn default_event_processing_timeout_secs() -> u64 {
    120 // 2 minutes timeout per event
}

fn default_event_retention_secs() -> u64 {
    24 * 60 * 60 // 24 hours retention
}

fn default_cleanup_interval_secs() -> u64 {
    30 * 60 // Run cleanup every 30 minutes
}

fn default_idle_consumer_threshold_ms() -> u64 {
    3_600_000 // 1 hour - consumers idle longer than this are considered stale
}

fn default_max_retry_attempts() -> u32 {
    3 // Retry Redis operations up to 3 times
}

fn default_retry_delay_ms() -> u64 {
    100 // Wait 100ms between retries
}

fn default_max_message_retries() -> u64 {
    5 // Move to dead letter after 5 failed processing attempts
}

fn default_health_check_interval_secs() -> u64 {
    60 // Check stream health every minute
}

fn default_reclaim_batch_size() -> usize {
    100 // Process 100 stale messages at a time during force reclaim
}

impl Default for StreamProcessorConfig {
    fn default() -> Self {
        Self {
            group_name: default_stream_group_name(),
            batch_size: default_stream_batch_size(),
            concurrency: default_stream_concurrency(),
            consumers_per_stream: default_consumers_per_stream(),
            event_processing_timeout_secs: default_event_processing_timeout_secs(),
            event_retention_secs: default_event_retention_secs(),
            cleanup_interval_secs: default_cleanup_interval_secs(),
            idle_consumer_threshold_ms: default_idle_consumer_threshold_ms(),
            max_retry_attempts: default_max_retry_attempts(),
            retry_delay_ms: default_retry_delay_ms(),
            max_message_retries: default_max_message_retries(),
            health_check_interval_secs: default_health_check_interval_secs(),
            reclaim_batch_size: default_reclaim_batch_size(),
            backpressure: BackpressureConfig::default(),
        }
    }
}

fn default_max_pool_size() -> u32 {
    50 // Fred's auto-pipelining means fewer connections needed than concurrent operations
}

fn default_redis_batch_size() -> usize {
    100 // Default batch size for Redis operations
}

fn default_connection_timeout_ms() -> u64 {
    5000 // 5 second connection timeout - balanced for pool contention scenarios
}

fn default_enable_dead_letter() -> bool {
    true // Enable dead letter queues by default
}

fn default_consumer_rebalance_interval() -> u64 {
    300 // Check for rebalancing every 5 minutes by default
}

fn default_metrics_collection_interval() -> u64 {
    60 // Collect metrics every minute by default
}

/// Hub configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub url: String,

    // Custom headers for authentication and other purposes
    #[serde(default)]
    pub headers: HashMap<String, String>,

    // Connection limits
    #[serde(default = "default_hub_max_concurrent_connections")]
    pub max_concurrent_connections: u32,

    #[serde(default = "default_hub_max_requests_per_second")]
    pub max_requests_per_second: u32,

    // Retry configuration
    #[serde(default = "default_retry_attempts")]
    pub retry_max_attempts: u32,

    #[serde(default = "default_retry_base_delay_ms")]
    pub retry_base_delay_ms: u64,

    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,

    #[serde(default = "default_retry_jitter_factor")]
    pub retry_jitter_factor: f32,

    #[serde(default = "default_retry_timeout_ms")]
    pub retry_timeout_ms: u64,

    #[serde(default = "default_conn_timeout_ms")]
    pub conn_timeout_ms: u64,

    // Shard configuration
    // List of shard indices to subscribe to (e.g., [0, 1, 2])
    // If empty, must set subscribe_to_all_shards=true
    #[serde(default)]
    pub shard_indices: Vec<u32>,

    // Optional: subscribe to all shards (temporary migration flag)
    #[serde(default = "default_subscribe_to_all_shards")]
    pub subscribe_to_all_shards: bool,
}

fn default_subscribe_to_all_shards() -> bool {
    true // Default to subscribing to all shards for backward compatibility
}

fn default_hub_max_concurrent_connections() -> u32 {
    20 // Increased from 5 to handle multiple concurrent stream reservations
}

fn default_hub_max_requests_per_second() -> u32 {
    50 // Increased from 10 to handle multiple concurrent stream reservations
}

fn default_retry_attempts() -> u32 {
    5 // Default to 5 retry attempts
}

fn default_retry_base_delay_ms() -> u64 {
    100 // Start with a 100ms delay
}

fn default_retry_max_delay_ms() -> u64 {
    30000 // Maximum 30 second delay
}

fn default_retry_jitter_factor() -> f32 {
    0.25 // 25% random jitter
}

fn default_retry_timeout_ms() -> u64 {
    60000 // 1 minute request timeout
}

fn default_conn_timeout_ms() -> u64 {
    30000 // 30 second connection timeout
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Logging format: "json" or "text"
    pub format: String,
    /// Default log level if no RUST_LOG is set
    pub default_level: String,
    /// Custom filter for dependency logs
    pub dependency_filter: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: "text".to_string(),
            default_level: "info".to_string(),
            dependency_filter: Some(
                "hyper=warn,h2=warn,tower=info,tokio_util=warn,mio=warn,rustls=warn,tonic=info,want=warn,warp=warn,sqlx=warn".to_string()
            ),
        }
    }
}

/// StatsD configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsdConfig {
    pub prefix: String,
    pub addr: String,
    pub use_tags: bool,
    pub enabled: bool,
}

impl Default for StatsdConfig {
    fn default() -> Self {
        Self {
            prefix: "way_read".to_string(),
            addr: "127.0.0.1:8125".to_string(),
            use_tags: false,
            enabled: false,
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,
    #[serde(default = "default_mcp_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_mcp_port")]
    pub port: u16,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_enabled(),
            bind_address: default_mcp_bind_address(),
            port: default_mcp_port(),
        }
    }
}

fn default_mcp_enabled() -> bool {
    true
}

fn default_mcp_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_mcp_port() -> u16 {
    8000
}

/// Default value for clear_db - default to false for safety
fn default_clear_db() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub hub: HubConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub backfill: BackfillConfig,
    #[serde(default)]
    pub statsd: StatsdConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub eth: EthConfig,
    #[serde(default)]
    pub stream: StreamProcessorConfig,
    #[serde(default = "default_clear_db")]
    pub clear_db: bool,
}

/// Backfill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillConfig {
    pub concurrency: Option<usize>,
    pub batch_size: Option<usize>,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self { concurrency: Some(40), batch_size: Some(50) } // Align with Docker default
    }
}

/// Default value for store_messages - default to true for backward compatibility
fn default_store_messages() -> bool {
    true // Store messages by default
}

/// Default batch size for database operations
fn default_db_batch_size() -> usize {
    100 // Default to 100 records per batch for database operations
}

/// Default value for skip_migrations - default to false to run migrations
fn default_skip_migrations() -> bool {
    false // Run migrations by default
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgresql://localhost/waypoint".to_string(),
            max_connections: 60, // Align with Docker default
            timeout_seconds: 30, // Align with Docker default
            store_messages: default_store_messages(),
            batch_size: default_db_batch_size(),
            skip_migrations: default_skip_migrations(),
        }
    }
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            max_pool_size: default_max_pool_size(),
            batch_size: default_redis_batch_size(),
            enable_dead_letter: default_enable_dead_letter(),
            consumer_rebalance_interval_seconds: default_consumer_rebalance_interval(),
            metrics_collection_interval_seconds: default_metrics_collection_interval(),
            connection_timeout_ms: default_connection_timeout_ms(),
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }
}

impl CircuitBreakerConfig {
    /// Convert to the circuit breaker's internal config format
    pub fn to_circuit_breaker_config(&self) -> crate::redis::circuit_breaker::CircuitBreakerConfig {
        crate::redis::circuit_breaker::CircuitBreakerConfig {
            failure_threshold: self.failure_threshold,
            open_timeout: std::time::Duration::from_secs(self.open_timeout_secs),
            success_threshold: self.success_threshold,
            slow_call_threshold: std::time::Duration::from_millis(self.slow_call_threshold_ms),
            slow_call_rate_threshold: self.slow_call_rate_threshold,
            minimum_calls_for_rate: self.minimum_calls_for_rate,
        }
    }
}

/// Backpressure configuration for stream processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// Enable backpressure control
    #[serde(default = "default_backpressure_enabled")]
    pub enabled: bool,

    /// Pending messages threshold for Light pressure
    #[serde(default = "default_bp_light_threshold")]
    pub light_threshold: u64,

    /// Pending messages threshold for Moderate pressure
    #[serde(default = "default_bp_moderate_threshold")]
    pub moderate_threshold: u64,

    /// Pending messages threshold for Heavy pressure
    #[serde(default = "default_bp_heavy_threshold")]
    pub heavy_threshold: u64,

    /// Pending messages threshold for Critical pressure
    #[serde(default = "default_bp_critical_threshold")]
    pub critical_threshold: u64,

    /// Base delay in milliseconds when under pressure
    #[serde(default = "default_bp_base_delay_ms")]
    pub base_delay_ms: u64,

    /// How often to evaluate backpressure (milliseconds)
    #[serde(default = "default_bp_evaluation_interval_ms")]
    pub evaluation_interval_ms: u64,

    /// Enable adaptive rate limiting based on latency
    #[serde(default = "default_bp_adaptive_rate_limit")]
    pub adaptive_rate_limit: bool,

    /// Target latency in milliseconds for adaptive rate limiting
    #[serde(default = "default_bp_target_latency_ms")]
    pub target_latency_ms: u64,
}

fn default_backpressure_enabled() -> bool {
    true
}

fn default_bp_light_threshold() -> u64 {
    1000
}

fn default_bp_moderate_threshold() -> u64 {
    5000
}

fn default_bp_heavy_threshold() -> u64 {
    10000
}

fn default_bp_critical_threshold() -> u64 {
    50000
}

fn default_bp_base_delay_ms() -> u64 {
    50
}

fn default_bp_evaluation_interval_ms() -> u64 {
    1000
}

fn default_bp_adaptive_rate_limit() -> bool {
    true
}

fn default_bp_target_latency_ms() -> u64 {
    100
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            enabled: default_backpressure_enabled(),
            light_threshold: default_bp_light_threshold(),
            moderate_threshold: default_bp_moderate_threshold(),
            heavy_threshold: default_bp_heavy_threshold(),
            critical_threshold: default_bp_critical_threshold(),
            base_delay_ms: default_bp_base_delay_ms(),
            evaluation_interval_ms: default_bp_evaluation_interval_ms(),
            adaptive_rate_limit: default_bp_adaptive_rate_limit(),
            target_latency_ms: default_bp_target_latency_ms(),
        }
    }
}

impl BackpressureConfig {
    /// Convert to the backpressure controller's internal config format
    pub fn to_backpressure_config(&self) -> crate::redis::backpressure::BackpressureConfig {
        crate::redis::backpressure::BackpressureConfig {
            light_threshold: self.light_threshold,
            moderate_threshold: self.moderate_threshold,
            heavy_threshold: self.heavy_threshold,
            critical_threshold: self.critical_threshold,
            base_delay_ms: self.base_delay_ms,
            evaluation_interval_ms: self.evaluation_interval_ms,
            rate_window_secs: 60,
            max_processing_rate: None,
            adaptive_rate_limit: self.adaptive_rate_limit,
            target_latency_ms: self.target_latency_ms,
        }
    }
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            url: "snapchain.farcaster.xyz:3383".to_string(),
            headers: HashMap::new(),
            max_concurrent_connections: default_hub_max_concurrent_connections(),
            max_requests_per_second: default_hub_max_requests_per_second(),
            retry_max_attempts: default_retry_attempts(),
            retry_base_delay_ms: default_retry_base_delay_ms(),
            retry_max_delay_ms: default_retry_max_delay_ms(),
            retry_jitter_factor: default_retry_jitter_factor(),
            retry_timeout_ms: default_retry_timeout_ms(),
            conn_timeout_ms: default_conn_timeout_ms(),
            shard_indices: Vec::new(),
            subscribe_to_all_shards: default_subscribe_to_all_shards(),
        }
    }
}

impl Config {
    /// Load configuration from environment variables and optional config file
    pub fn load() -> Result<Self, ConfigError> {
        // Load .env file if it exists
        let _ = dotenvy::dotenv().ok();

        let mut figment = Figment::new()
            .merge(Serialized::defaults(Config::default()))
            .merge(Env::prefixed("WAYPOINT_").split("__"));

        // Optionally load from config file if WAYPOINT_CONFIG is set
        // Note: We use std::env::var_os here directly because this is the bootstrapping
        // part of our config system; we have to use env vars to find the config file
        // This is exempt from the clippy lint for std::env::var since it's part of config loading
        if let Some(config_path) = std::env::var_os("WAYPOINT_CONFIG") {
            if let Some(path_str) = config_path.to_str() {
                let path = Path::new(path_str);
                if path.exists() {
                    figment = figment.merge(Toml::file(path));
                }
            }
        }

        figment.extract().map_err(|e| ConfigError::LoadError(e.to_string()))
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate database config
        if self.database.url.is_empty() {
            return Err(ConfigError::MissingConfig("Database URL is required".to_string()));
        }

        // Validate Redis config
        if self.redis.url.is_empty() {
            return Err(ConfigError::MissingConfig("Redis URL is required".to_string()));
        }

        // Validate Hub config
        if self.hub.url.is_empty() {
            return Err(ConfigError::MissingConfig("Hub URL is required".to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_processor_config_defaults() {
        let config = StreamProcessorConfig::default();

        assert_eq!(config.group_name, "default");
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.concurrency, 250);
        assert_eq!(config.consumers_per_stream, 3);
        assert_eq!(config.event_processing_timeout_secs, 120);
        assert_eq!(config.event_retention_secs, 24 * 60 * 60); // 24 hours
        assert_eq!(config.cleanup_interval_secs, 30 * 60); // 30 minutes
        assert_eq!(config.idle_consumer_threshold_ms, 3_600_000); // 1 hour
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.retry_delay_ms, 100);
        assert_eq!(config.max_message_retries, 5);
        assert_eq!(config.health_check_interval_secs, 60);
        assert_eq!(config.reclaim_batch_size, 100);
    }

    #[test]
    fn test_stream_processor_config_in_main_config() {
        let config = Config::default();

        // Verify stream config is included and has correct defaults
        assert_eq!(config.stream.batch_size, 50);
        assert_eq!(config.stream.concurrency, 250);
        assert_eq!(config.stream.consumers_per_stream, 3);
    }

    #[test]
    fn test_stream_processor_config_serialization() {
        let config = StreamProcessorConfig::default();

        // Test that config can be serialized/deserialized
        let json = serde_json::to_string(&config).expect("Should serialize");
        let deserialized: StreamProcessorConfig =
            serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(config.batch_size, deserialized.batch_size);
        assert_eq!(config.concurrency, deserialized.concurrency);
        assert_eq!(config.max_retry_attempts, deserialized.max_retry_attempts);
    }

    #[test]
    fn test_stream_processor_config_partial_override() {
        // Test that partial JSON works with defaults for missing fields
        let partial_json = r#"{"batch_size": 100, "concurrency": 500}"#;
        let config: StreamProcessorConfig =
            serde_json::from_str(partial_json).expect("Should deserialize partial config");

        // Overridden values
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.concurrency, 500);

        // Default values for non-specified fields
        assert_eq!(config.consumers_per_stream, 3);
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.group_name, "default");
    }
}
