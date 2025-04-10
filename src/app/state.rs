//! Application state management
use crate::{
    app::{AppError, Result},
    config::Config,
    database::client::Database,
    hub::client::Hub,
    redis::client::Redis,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state
pub struct AppState {
    /// Hub client
    pub hub: Arc<Mutex<Hub>>,
    /// Redis client
    pub redis: Arc<Redis>,
    /// Database client
    pub database: Arc<Database>,
}

/// State provider that initializes application components
pub struct StateProvider {
    config: Config,
}

impl StateProvider {
    /// Create a new state provider
    pub async fn new(config: &Config) -> Result<Self> {
        Ok(Self { config: config.clone() })
    }

    /// Initialize and provide the application state
    pub async fn provide(&self) -> Result<Arc<AppState>> {
        // Initialize core components
        let redis = Arc::new(
            Redis::new(&self.config.redis).await.map_err(|e| AppError::Redis(e.to_string()))?,
        );

        let hub_config = Arc::new(self.config.hub.clone());
        let hub =
            Arc::new(Mutex::new(Hub::new(hub_config).map_err(|e| AppError::Hub(e.to_string()))?));

        let database = Arc::new(
            Database::new(&self.config.database)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?,
        );

        // Test connections
        redis.check_connection().await.map_err(|e| AppError::Redis(e.to_string()))?;

        {
            let mut hub_guard = hub.lock().await;
            hub_guard.connect().await.map_err(|e| AppError::Hub(e.to_string()))?;
        }

        // Create the state
        let state = AppState { hub, redis, database };

        Ok(Arc::new(state))
    }
}
