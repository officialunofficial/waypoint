use crate::{config::Config, database::client::Database, hub::client::Hub, redis::client::Redis};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application resources
///
/// This struct holds references to the core shared components
/// of the application that are used by processors.
///
/// Hub is optional to support consumer-only mode where we don't need
/// a hub connection for database writes.
pub struct AppResources {
    /// Hub client (optional for consumer-only mode)
    pub hub: Option<Arc<Mutex<Hub>>>,
    pub redis: Arc<Redis>,
    pub database: Arc<Database>,
    pub config: Config,
}

impl AppResources {
    /// Create resources with all components (backward compatible)
    pub fn new(hub: Arc<Mutex<Hub>>, redis: Arc<Redis>, database: Arc<Database>) -> Self {
        // Create a default config for backward compatibility
        Self { hub: Some(hub), redis, database, config: Config::default() }
    }

    /// Create resources with all components and custom config
    pub fn with_config(
        hub: Arc<Mutex<Hub>>,
        redis: Arc<Redis>,
        database: Arc<Database>,
        config: Config,
    ) -> Self {
        Self { hub: Some(hub), redis, database, config }
    }

    /// Create resources without hub (for consumer-only mode)
    pub fn with_config_consumer_only(
        redis: Arc<Redis>,
        database: Arc<Database>,
        config: Config,
    ) -> Self {
        Self { hub: None, redis, database, config }
    }
}

impl Clone for AppResources {
    fn clone(&self) -> Self {
        // Efficiently clone by only cloning the Arc pointers, not the underlying data
        Self {
            hub: self.hub.as_ref().map(Arc::clone),
            redis: Arc::clone(&self.redis),
            database: Arc::clone(&self.database),
            config: self.config.clone(),
        }
    }
}
