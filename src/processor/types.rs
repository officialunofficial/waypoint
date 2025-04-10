use crate::{database::client::Database, hub::client::Hub, redis::client::Redis};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application resources
///
/// This struct holds references to the core shared components
/// of the application that are used by processors.
pub struct AppResources {
    pub hub: Arc<Mutex<Hub>>,
    pub redis: Arc<Redis>,
    pub database: Arc<Database>,
}

impl AppResources {
    pub fn new(hub: Arc<Mutex<Hub>>, redis: Arc<Redis>, database: Arc<Database>) -> Self {
        Self { hub, redis, database }
    }
}

impl Clone for AppResources {
    fn clone(&self) -> Self {
        // Efficiently clone by only cloning the Arc pointers, not the underlying data
        Self {
            hub: Arc::clone(&self.hub),
            redis: Arc::clone(&self.redis),
            database: Arc::clone(&self.database),
        }
    }
}
