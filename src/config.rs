//! Configuration management for the application
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
}

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: u32,
    #[serde(default = "default_redis_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_enable_dead_letter")]
    pub enable_dead_letter: bool,
    #[serde(default = "default_consumer_rebalance_interval")]
    pub consumer_rebalance_interval_seconds: u64,
    #[serde(default = "default_metrics_collection_interval")]
    pub metrics_collection_interval_seconds: u64,
}

fn default_redis_batch_size() -> usize {
    100 // Default batch size for Redis operations
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub hub: HubConfig,
    pub logging: LoggingConfig,
    pub backfill: BackfillConfig,
    pub statsd: StatsdConfig,
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

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgresql://localhost/waypoint".to_string(),
            max_connections: 20,
            timeout_seconds: 30,
        }
    }
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            pool_size: 5,
            batch_size: default_redis_batch_size(),
            enable_dead_letter: default_enable_dead_letter(),
            consumer_rebalance_interval_seconds: default_consumer_rebalance_interval(),
            metrics_collection_interval_seconds: default_metrics_collection_interval(),
        }
    }
}

impl Default for HubConfig {
    fn default() -> Self {
        Self { url: "hub.farcaster.xyz:2283".to_string() }
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
