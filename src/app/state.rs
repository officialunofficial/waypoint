//! Application state management
use crate::{
    app::{AppError, Result},
    config::{Config, ServiceMode},
    database::client::Database,
    hub::client::Hub,
    redis::client::Redis,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state
///
/// Components are optional based on service mode:
/// - Producer mode: hub + redis (no database)
/// - Consumer mode: redis + database (no hub)
/// - Both mode: all components
pub struct AppState {
    /// Hub client (required for producer/both modes)
    pub hub: Option<Arc<Mutex<Hub>>>,
    /// Redis client (always required - the message bus)
    pub redis: Arc<Redis>,
    /// Database client (required for consumer/both modes)
    pub database: Option<Arc<Database>>,
    /// Current service mode
    pub mode: ServiceMode,
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

    /// Initialize and provide the application state for the default (Both) mode
    ///
    /// This maintains backward compatibility with existing code.
    pub async fn provide(&self) -> Result<Arc<AppState>> {
        self.provide_for_mode(ServiceMode::Both).await
    }

    /// Initialize and provide the application state for a specific mode
    ///
    /// Only initializes the components required for the given mode:
    /// - Producer: Hub + Redis
    /// - Consumer: Redis + Database
    /// - Both: All components
    pub async fn provide_for_mode(&self, mode: ServiceMode) -> Result<Arc<AppState>> {
        // Redis is always required - it's the message bus
        let redis = Arc::new(
            Redis::new(&self.config.redis).await.map_err(|e| AppError::Redis(e.to_string()))?,
        );
        redis.check_connection().await.map_err(|e| AppError::Redis(e.to_string()))?;

        // Initialize Hub only for Producer or Both modes
        let hub = if matches!(mode, ServiceMode::Producer | ServiceMode::Both) {
            let hub_config = Arc::new(self.config.hub.clone());
            let hub = Arc::new(Mutex::new(
                Hub::new(hub_config).map_err(|e| AppError::Hub(e.to_string()))?,
            ));

            {
                let mut hub_guard = hub.lock().await;
                hub_guard.connect().await.map_err(|e| AppError::Hub(e.to_string()))?;
            }

            Some(hub)
        } else {
            None
        };

        // Initialize Database only for Consumer or Both modes
        let database = if matches!(mode, ServiceMode::Consumer | ServiceMode::Both) {
            let db = Arc::new(
                Database::new(&self.config.database)
                    .await
                    .map_err(|e| AppError::Database(e.to_string()))?,
            );
            db.log_connection_info();
            Some(db)
        } else {
            None
        };

        // Create the state
        let state = AppState { hub, redis, database, mode };

        Ok(Arc::new(state))
    }
}
