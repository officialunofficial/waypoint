//! Configuration management for the application
use crate::eth::EthConfig;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
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
}

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    #[serde(default = "default_redis_pool_size")]
    pub pool_size: u32,
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
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_max_connection_lifetime_secs")]
    pub max_connection_lifetime_secs: u64,
}

fn default_redis_pool_size() -> u32 {
    20 // Balanced pool size - enough for concurrency without overwhelming hub
}

fn default_redis_batch_size() -> usize {
    100 // Default batch size for Redis operations
}

fn default_connection_timeout_ms() -> u64 {
    5000 // 5 second connection timeout
}

fn default_idle_timeout_secs() -> u64 {
    300 // 5 minute idle timeout
}

fn default_max_connection_lifetime_secs() -> u64 {
    1800 // 30 minute max connection lifetime
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
}

fn default_hub_max_concurrent_connections() -> u32 {
    5 // Conservative limit to avoid overwhelming hub
}

fn default_hub_max_requests_per_second() -> u32 {
    10 // Conservative rate limit
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
        Self { concurrency: Some(50), batch_size: Some(50) }
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

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgresql://localhost/waypoint".to_string(),
            max_connections: 20,
            timeout_seconds: 30,
            store_messages: default_store_messages(),
            batch_size: default_db_batch_size(),
        }
    }
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            pool_size: default_redis_pool_size(),
            batch_size: default_redis_batch_size(),
            enable_dead_letter: default_enable_dead_letter(),
            consumer_rebalance_interval_seconds: default_consumer_rebalance_interval(),
            metrics_collection_interval_seconds: default_metrics_collection_interval(),
            connection_timeout_ms: default_connection_timeout_ms(),
            idle_timeout_secs: default_idle_timeout_secs(),
            max_connection_lifetime_secs: default_max_connection_lifetime_secs(),
        }
    }
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            url: "snapchain.farcaster.xyz:3383".to_string(),
            max_concurrent_connections: default_hub_max_concurrent_connections(),
            max_requests_per_second: default_hub_max_requests_per_second(),
            retry_max_attempts: default_retry_attempts(),
            retry_base_delay_ms: default_retry_base_delay_ms(),
            retry_max_delay_ms: default_retry_max_delay_ms(),
            retry_jitter_factor: default_retry_jitter_factor(),
            retry_timeout_ms: default_retry_timeout_ms(),
            conn_timeout_ms: default_conn_timeout_ms(),
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
