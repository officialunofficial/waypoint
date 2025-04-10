//! Application module for composition and dependency management

mod processor;
mod service;
mod state;

pub use processor::{EventProcessor, ProcessorError, ProcessorRegistry, Result as ProcessorResult};
pub use service::{Service, ServiceBuilder, ServiceContext, ServiceError, ServiceHandle};
pub use state::{AppState, StateProvider};

use crate::{config::Config, core::RepositoryError, health::HealthServer};
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};

/// Application error type
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Hub error: {0}")]
    Hub(String),

    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),

    #[error("Service error: {0}")]
    Service(#[from] ServiceError),

    #[error("Other error: {0}")]
    Other(String),
}

/// Application result type
pub type Result<T> = std::result::Result<T, AppError>;

/// Core application struct
pub struct App {
    state: Arc<AppState>,
    services: Vec<Box<dyn Service>>,
    health_server: Option<HealthServer>,
}

impl App {
    /// Create a new application instance with the given configuration
    pub async fn new(config: Config) -> Result<Self> {
        // Validate configuration
        config.validate().map_err(AppError::from)?;

        // Initialize state provider (database, redis, hub)
        let state_provider = StateProvider::new(&config).await?;
        let state = state_provider.provide().await?;

        Ok(Self { state, services: Vec::new(), health_server: None })
    }

    /// Register a service with the application
    pub fn register_service<S: Service + 'static>(&mut self, service: S) -> &mut Self {
        self.services.push(Box::new(service));
        self
    }

    /// Configure a health server on the given port
    pub fn with_health_server(&mut self, port: u16) -> &mut Self {
        self.health_server = Some(HealthServer::new(port));
        self
    }

    /// Start the application and run until shutdown is requested
    pub async fn run_until_shutdown(self) -> Result<()> {
        // Start health server if configured
        let health_server_handle = if let Some(mut health_server) = self.health_server {
            let hub = self.state.hub.clone();
            let redis = self.state.redis.clone();
            let database = self.state.database.clone();

            let health_server_clone = health_server.clone();
            let handle = tokio::spawn(async move {
                if let Err(e) = health_server.run(database, redis, hub).await {
                    error!("Health server error: {}", e);
                }
            });

            Some((health_server_clone, handle))
        } else {
            None
        };

        // Start all services
        let mut service_handles = Vec::new();

        for service in self.services {
            let context = ServiceContext::new(self.state.clone());
            let handle = service.start(context).await?;
            service_handles.push(handle);
        }

        // Wait for shutdown signal
        wait_for_shutdown().await;

        info!("Shutdown initiated, first updating health probes...");

        // Step 1: Mark health checks as failing first to prevent new traffic
        if let Some((mut health_server, handle)) = health_server_handle {
            info!("Updating health probes to report shutdown");
            health_server.stopping.store(true, std::sync::atomic::Ordering::SeqCst);

            // Give k8s a chance to detect health change (typically 1-2 seconds)
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            // Step 2: Stop all application services
            info!("Stopping application services...");
            for service_handle in service_handles {
                service_handle.stop().await;
            }

            // Step 3: Fully shut down the health server last
            info!("Shutting down health server...");
            health_server.shutdown().await;
            let _ = handle.await;
        } else {
            // No health server, just stop services
            info!("Stopping application services...");
            for handle in service_handles {
                handle.stop().await;
            }
        }

        info!("Shutdown complete");
        Ok(())
    }
}

/// Wait for a shutdown signal (SIGTERM, SIGINT, or SIGHUP)
async fn wait_for_shutdown() {
    use std::time::Duration;
    use tokio::signal::unix::{SignalKind, signal};

    // Set up signal handlers
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to listen for SIGINT");
    let mut sighup = signal(SignalKind::hangup()).expect("Failed to listen for SIGHUP");

    // Wait for any signal
    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received, initiating graceful shutdown"),
        _ = sigint.recv() => info!("SIGINT received, initiating graceful shutdown"),
        _ = sighup.recv() => info!("SIGHUP received, initiating graceful shutdown"),
    }

    // Set shutdown timeout as a safety measure
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        info!("Shutdown timeout reached (30s), forcing exit");
        std::process::exit(0);
    });
}
