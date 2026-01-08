use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{error, info, warn};

use crate::{
    config::ServiceMode, database::client::Database, hub::client::Hub, redis::client::Redis,
};

#[derive(Clone)]
pub struct HealthServer {
    port: u16,
    shutdown_tx: Arc<parking_lot::Mutex<Option<oneshot::Sender<()>>>>,
    pub stopping: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone)]
struct AppState {
    /// Database client (optional for producer mode)
    database: Option<Arc<Database>>,
    /// Redis client (always required)
    redis: Arc<Redis>,
    /// Hub client (optional for consumer mode)
    hub: Option<Arc<Mutex<Hub>>>,
    /// Stopping flag for graceful shutdown
    stopping: Arc<std::sync::atomic::AtomicBool>,
    /// Current service mode
    mode: ServiceMode,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    database: bool,
    redis: bool,
    hub: bool,
    mode: String,
    details: Option<String>,
}

impl HealthServer {
    pub fn new(port: u16) -> Self {
        HealthServer {
            port,
            shutdown_tx: Arc::new(parking_lot::Mutex::new(None)),
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub async fn run(
        &mut self,
        database: Option<Arc<Database>>,
        redis: Arc<Redis>,
        hub: Option<Arc<Mutex<Hub>>>,
        mode: ServiceMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = AppState { database, redis, hub, stopping: self.stopping.clone(), mode };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/ready", get(readiness_check))
            .with_state(state);

        info!("Starting health check server on port {}", self.port);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], self.port));

        let (tx, rx) = oneshot::channel();
        *self.shutdown_tx.lock() = Some(tx);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).with_graceful_shutdown(shutdown_signal(rx)).await?;

        Ok(())
    }

    pub async fn shutdown(&mut self) {
        // Mark service as stopping - this affects health/readiness checks
        self.stopping.store(true, std::sync::atomic::Ordering::SeqCst);

        // Allow a grace period for health checks to start failing
        info!("Health server entering graceful shutdown phase");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Send shutdown signal to HTTP server
        if let Some(tx) = self.shutdown_tx.lock().take() {
            if tx.send(()).is_err() {
                error!("Failed to send shutdown signal to health server");
            }
        }

        // Let the server finish its shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        info!("Health server shutdown complete");
    }
}

// Liveness probe - just checks if the service is running
async fn health_check(State(state): State<AppState>) -> Response {
    // For liveness, we only return unhealthy if actively shutting down
    if state.stopping.load(std::sync::atomic::Ordering::SeqCst) {
        info!("Health check returning unhealthy because service is shutting down");
        return (StatusCode::SERVICE_UNAVAILABLE, "shutting down").into_response();
    }

    (StatusCode::OK, "healthy").into_response()
}

// Readiness probe - checks external dependencies based on mode
async fn readiness_check(State(state): State<AppState>) -> Response {
    // If shutting down, always return not ready to prevent new traffic
    if state.stopping.load(std::sync::atomic::Ordering::SeqCst) {
        info!("Readiness check returning not ready because service is shutting down");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "shutting_down".to_string(),
                database: false,
                redis: false,
                hub: false,
                mode: state.mode.to_string(),
                details: Some("Service is shutting down".to_string()),
            }),
        )
            .into_response();
    }

    // Redis is always required
    let redis_health = state.redis.check_connection().await;
    let (redis_ok, redis_error) = match redis_health {
        Ok(true) => (true, None),
        Ok(false) => {
            warn!("Redis connection check returned false");
            (false, Some("Redis connection check failed".to_string()))
        },
        Err(e) => {
            warn!("Redis health check error: {:?}", e);
            (false, Some(format!("Redis error: {}", e)))
        },
    };

    // Check database only if present (consumer/both modes)
    let (database_ok, db_error) = if let Some(ref database) = state.database {
        match database.check_connection().await {
            Ok(true) => (true, None),
            Ok(false) => {
                warn!("Database connection check returned false");
                (false, Some("Database connection check failed".to_string()))
            },
            Err(e) => {
                warn!("Database health check error: {:?}", e);
                (false, Some(format!("Database error: {}", e)))
            },
        }
    } else {
        // Database not configured (producer mode) - report as healthy/N/A
        (true, None)
    };

    // Check hub only if present (producer/both modes)
    let (hub_ok, hub_error) = if let Some(ref hub) = state.hub {
        let hub_health = {
            let mut hub_guard = hub.lock().await;
            hub_guard.check_connection().await
        };
        match hub_health {
            Ok(true) => (true, None),
            Ok(false) => {
                warn!("Hub connection check returned false");
                (false, Some("Hub connection check failed".to_string()))
            },
            Err(e) => {
                warn!("Hub health check error: {:?}", e);
                (false, Some(format!("Hub error: {}", e)))
            },
        }
    } else {
        // Hub not configured (consumer mode) - report as healthy/N/A
        (true, None)
    };

    // Determine first error for details
    let details = if !redis_ok {
        redis_error
    } else if !database_ok {
        db_error
    } else if !hub_ok {
        hub_error
    } else {
        None
    };

    // Determine overall health status based on mode
    let all_required_ok = match state.mode {
        ServiceMode::Producer => redis_ok && hub_ok,
        ServiceMode::Consumer => redis_ok && database_ok,
        ServiceMode::Both => redis_ok && hub_ok && database_ok,
    };

    let response = HealthResponse {
        status: if all_required_ok { "healthy".to_string() } else { "degraded".to_string() },
        database: database_ok,
        redis: redis_ok,
        hub: hub_ok,
        mode: state.mode.to_string(),
        details,
    };

    let status = if all_required_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    (status, Json(response)).into_response()
}

async fn shutdown_signal(rx: oneshot::Receiver<()>) {
    let _ = rx.await;
    info!("Health check server received shutdown signal");
}
