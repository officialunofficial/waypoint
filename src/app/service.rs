//! Service abstractions and lifecycle management
use crate::{
    app::{AppState, Result},
    config::Config,
};
use async_trait::async_trait;
use once_cell;
use std::sync::Arc;
use thiserror::Error;

/// Service error type
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Service initialization error: {0}")]
    Initialization(String),

    #[error("Service start error: {0}")]
    Start(String),

    #[error("Service stop error: {0}")]
    Stop(String),
}

/// Service context provided to each service
pub struct ServiceContext<'a> {
    /// Application state
    pub state: Arc<AppState>,
    /// Application configuration (reference)
    pub config: &'a Config,
    // DataContext replaced the repository factory pattern
}

impl<'a> ServiceContext<'a> {
    /// Create a new service context with default config
    pub fn new(state: Arc<AppState>) -> Self {
        // This is a workaround that creates a static Config - rarely used in practice
        // as most code will use with_config instead
        static DEFAULT_CONFIG: once_cell::sync::Lazy<Config> =
            once_cell::sync::Lazy::new(Config::default);

        Self { state, config: &DEFAULT_CONFIG }
    }

    /// Create a new service context with configuration reference
    pub fn with_config(state: Arc<AppState>, config: &'a Config) -> Self {
        // DataContext has replaced the repository factory

        Self { state, config }
    }

    // Repository factory was replaced with DataContext
}

/// Service handle for controlling a running service
pub struct ServiceHandle {
    stop_tx: tokio::sync::oneshot::Sender<()>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl ServiceHandle {
    /// Create a new service handle
    pub fn new(
        stop_tx: tokio::sync::oneshot::Sender<()>,
        join_handle: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self { stop_tx, join_handle }
    }

    /// Stop the service
    pub async fn stop(self) {
        // Send stop signal, ignore errors if receiver is dropped
        let _ = self.stop_tx.send(());

        // Wait for service to complete with timeout
        let timeout_result =
            tokio::time::timeout(std::time::Duration::from_secs(3), self.join_handle).await;

        if timeout_result.is_err() {
            // If timeout occurs, log but don't wait further
            tracing::warn!("Service did not shut down within timeout period");
        }
    }
}

/// Service trait defining lifecycle methods
#[async_trait]
pub trait Service: Send + Sync {
    /// Get the service name
    fn name(&self) -> &str;

    /// Start the service
    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> Result<ServiceHandle>;
}

/// Builder for creating services
pub struct ServiceBuilder<S> {
    service: S,
}

impl<S: Default> Default for ServiceBuilder<S> {
    fn default() -> Self {
        Self { service: S::default() }
    }
}

impl<S> ServiceBuilder<S> {
    /// Create a new service builder with the given service
    pub fn new(service: S) -> Self {
        Self { service }
    }

    /// Build the service
    pub fn build(self) -> S {
        self.service
    }
}
