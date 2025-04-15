use axum::{
    Router,
    extract::{State, Request},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    middleware::{self, Next},
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{error, info, warn, debug};
use crate::metrics;

use crate::{database::client::Database, hub::client::Hub, redis::client::Redis};

/// Production-grade rate limiter for health endpoints with metrics tracking
#[derive(Clone)]
struct HealthRateLimiter {
    // Maximum number of requests per minute
    max_per_minute: u32,
    // Lock-free atomic counters for tracking request rates
    count: Arc<std::sync::atomic::AtomicU32>,
    reset_at: Arc<std::sync::atomic::AtomicI64>,
    // Per-IP tracking to detect and block abuse (limited size to prevent memory issues)
    ip_trackers: Arc<parking_lot::RwLock<lru::LruCache<String, RateLimitTracker>>>,
    // Health and monitoring metrics
    metrics: Arc<parking_lot::RwLock<HealthMetrics>>,
}

/// Track per-IP request rates
struct RateLimitTracker {
    count: u32,
    reset_at: i64,
    // Track historical rate limiting to detect abuse patterns
    limited_count: u32,
    last_limited: Option<i64>,
}

/// Track metrics about health endpoint usage
struct HealthMetrics {
    // Overall metrics
    total_requests: u64,
    limited_requests: u64,
    // Per-IP stats for potentially abusive clients
    ip_metrics: std::collections::HashMap<String, (u64, u64)>, // (requests, limited)
    // Last time metrics were reported
    last_push: std::time::Instant,
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            limited_requests: 0,
            ip_metrics: std::collections::HashMap::new(),
            last_push: std::time::Instant::now(),
        }
    }
}

impl HealthRateLimiter {
    fn new(max_per_minute: u32) -> Self {
        // Create a limited-size LRU cache for IP tracking to prevent memory issues
        // from too many unique IPs (defense against IP rotation attacks)
        let ip_trackers = Arc::new(parking_lot::RwLock::new(
            lru::LruCache::new(std::num::NonZeroUsize::new(1000).unwrap())
        ));
        
        // Set up metrics collection
        let metrics = Arc::new(parking_lot::RwLock::new(HealthMetrics::default()));
        let metrics_clone = Arc::clone(&metrics);
        
        // Spawn a background metrics reporter
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                
                let mut metrics_guard = metrics_clone.write();
                // Only log if we have recent activity and sufficient data
                if metrics_guard.last_push.elapsed() < std::time::Duration::from_secs(30) ||
                   metrics_guard.total_requests < 10 {
                    continue;
                }
                
                // Calculate and report overall rate limiting percentage
                let limit_rate = if metrics_guard.total_requests > 0 {
                    (metrics_guard.limited_requests as f64 / metrics_guard.total_requests as f64) * 100.0
                } else {
                    0.0
                };
                
                // Log overall stats
                if metrics_guard.limited_requests > 0 {
                    info!(
                        "Health endpoint rate limiting: {}/{} requests limited ({:.2}%)",
                        metrics_guard.limited_requests, metrics_guard.total_requests, limit_rate
                    );
                    
                    // Record metric for monitoring
                    let _ = metrics::gauge("health.rate_limit.percent", limit_rate);
                }
                
                // Log potentially abusive IPs
                let mut abusive_ips = metrics_guard.ip_metrics.iter()
                    .filter(|(_, (total, limited))| *limited > 10 && *total > 20)
                    .collect::<Vec<_>>();
                
                // Sort by most limited
                abusive_ips.sort_by(|a, b| (b.1).1.cmp(&(a.1).1));
                
                // Log top abusive IPs (limited to prevent log spam)
                for (i, (ip, (total, limited))) in abusive_ips.iter().take(5).enumerate() {
                    let ip_limit_rate = (*limited as f64 / *total as f64) * 100.0;
                    
                    warn!(
                        "Potentially abusive IP #{}: {} - {}/{} requests limited ({:.2}%)",
                        i+1, ip, limited, total, ip_limit_rate
                    );
                }
                
                // Reset metrics
                metrics_guard.total_requests = 0;
                metrics_guard.limited_requests = 0;
                metrics_guard.ip_metrics.clear();
                metrics_guard.last_push = std::time::Instant::now();
            }
        });
        
        Self {
            max_per_minute,
            count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            reset_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            ip_trackers,
            metrics,
        }
    }

    /// Check if a request should be allowed, using both global and per-IP rate limiting
    fn check(&self, ip: &str) -> bool {
        let now = chrono::Utc::now().timestamp();
        
        // Update overall metrics
        {
            let mut metrics = self.metrics.write();
            metrics.total_requests += 1;
            
            // Update per-IP metrics
            let entry = metrics.ip_metrics.entry(ip.to_string()).or_insert((0, 0));
            entry.0 += 1;
        }
        
        // Track IP-specific rate limit first (to detect abuse)
        let ip_limited = {
            let mut trackers = self.ip_trackers.write();
            
            // First check if IP exists
            let mut should_create = false;
            if !trackers.contains(ip) {
                should_create = true;
            }
            
            // If needed, insert new tracker
            if should_create {
                trackers.put(ip.to_string(), RateLimitTracker {
                    count: 0,
                    reset_at: now + 60,
                    limited_count: 0,
                    last_limited: None,
                });
            }
            
            // Now get the tracker
            let tracker = trackers.get_mut(ip).unwrap();
            
            // Reset counter if we're in a new minute
            if now > tracker.reset_at {
                tracker.count = 0;
                tracker.reset_at = now + 60;
            }
            
            // More aggressive rate limits for IPs that have been rate limited frequently
            let effective_limit = if tracker.limited_count > 10 {
                self.max_per_minute / 4  // 75% reduction for abusive IPs
            } else if tracker.limited_count > 5 {
                self.max_per_minute / 2  // 50% reduction for suspicious IPs
            } else {
                self.max_per_minute
            };
            
            // Check if they should be limited
            let should_limit = tracker.count >= effective_limit;
            if should_limit {
                tracker.limited_count += 1;
                tracker.last_limited = Some(now);
                
                // Update metrics for this IP
                let mut metrics = self.metrics.write();
                if let Some(entry) = metrics.ip_metrics.get_mut(ip) {
                    entry.1 += 1;
                }
            }
            
            // Count the request if not limited
            if !should_limit {
                tracker.count += 1;
            }
            
            should_limit
        };
        
        // If IP-specific limit triggered, return early
        if ip_limited {
            let mut metrics = self.metrics.write();
            metrics.limited_requests += 1;
            return false;
        }
        
        // Now check global rate limit
        let reset_at = self.reset_at.load(std::sync::atomic::Ordering::Relaxed);
        
        // Reset counter if we're in a new minute
        if now > reset_at {
            self.count.store(0, std::sync::atomic::Ordering::Relaxed);
            // Set reset time to the end of the current minute
            self.reset_at.store(now + 60, std::sync::atomic::Ordering::Relaxed);
        }
        
        // Check if we've exceeded the global rate limit
        let current = self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let allowed = current < self.max_per_minute;
        
        // Update metrics if limited
        if !allowed {
            let mut metrics = self.metrics.write();
            metrics.limited_requests += 1;
            
            // Update per-IP metrics for global limiting too
            if let Some(entry) = metrics.ip_metrics.get_mut(ip) {
                entry.1 += 1;
            }
        }
        
        allowed
    }
}

#[derive(Clone)]
pub struct HealthServer {
    port: u16,
    shutdown_tx: Arc<parking_lot::Mutex<Option<oneshot::Sender<()>>>>,
    pub stopping: Arc<std::sync::atomic::AtomicBool>,
    rate_limiter: Arc<HealthRateLimiter>,
}

#[derive(Clone)]
struct AppState {
    database: Arc<Database>,
    redis: Arc<Redis>,
    hub: Arc<Mutex<Hub>>,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    rate_limiter: Arc<HealthRateLimiter>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    database: bool,
    redis: bool,
    hub: bool,
    details: Option<String>,
}

impl HealthServer {
    pub fn new(port: u16) -> Self {
        // Create rate limiter: 300 requests per minute (5 per second)
        // Use configurable rate limit from environment or config
        let max_per_minute = std::env::var("WAYPOINT_HEALTH_RATE_LIMIT")
            .ok()
            .and_then(|val| val.parse::<u32>().ok())
            .unwrap_or(300); // Default to 300 requests per minute
        
        let rate_limiter = Arc::new(HealthRateLimiter::new(max_per_minute));
        
        info!("Health server rate limit set to {} requests per minute", max_per_minute);
        
        HealthServer {
            port,
            shutdown_tx: Arc::new(parking_lot::Mutex::new(None)),
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rate_limiter,
        }
    }

    pub async fn run(
        &mut self,
        database: Arc<Database>,
        redis: Arc<Redis>,
        hub: Arc<Mutex<Hub>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = AppState { 
            database, 
            redis, 
            hub, 
            stopping: self.stopping.clone(),
            rate_limiter: self.rate_limiter.clone(),
        };

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/ready", get(readiness_check))
            .layer(middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
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

// Readiness probe - checks external dependencies
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
                details: Some("Service is shutting down".to_string()),
            }),
        )
            .into_response();
    }

    let db_health = state.database.check_connection().await;
    let redis_health = state.redis.check_connection().await;

    // Check hub connection
    let hub_health = {
        let mut hub_guard = state.hub.lock().await;
        hub_guard.check_connection().await
    };

    let (database_ok, db_error) = match db_health {
        Ok(true) => (true, None),
        Ok(false) => {
            warn!("Database connection check returned false");
            (false, Some("Database connection check failed".to_string()))
        },
        Err(e) => {
            warn!("Database health check error: {:?}", e);
            (false, Some(format!("Database error: {}", e)))
        },
    };

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

    let (hub_ok, hub_error) = match hub_health {
        Ok(true) => (true, None),
        Ok(false) => {
            warn!("Hub connection check returned false");
            (false, Some("Hub connection check failed".to_string()))
        },
        Err(e) => {
            warn!("Hub health check error: {:?}", e);
            (false, Some(format!("Hub error: {}", e)))
        },
    };

    let details = if !database_ok {
        db_error
    } else if !redis_ok {
        redis_error
    } else if !hub_ok {
        hub_error
    } else {
        None
    };

    let response = HealthResponse {
        status: if database_ok && redis_ok && hub_ok {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        database: database_ok,
        redis: redis_ok,
        hub: hub_ok,
        details,
    };

    let status = if database_ok || redis_ok || hub_ok {
        // Service can run in degraded state if at least one dependency is available
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(response)).into_response()
}

// Rate limit middleware for health endpoints
async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Get client IP address for rate limiting key
    let ip = req.extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|connect_info| connect_info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Apply rate limiting
    if state.rate_limiter.check(&ip) {
        // Request is allowed
        let response = next.run(req).await;
        Ok(response)
    } else {
        // Request is rate limited
        debug!("Rate limited request from IP: {}", ip);
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

async fn shutdown_signal(rx: oneshot::Receiver<()>) {
    let _ = rx.await;
    info!("Health check server received shutdown signal");
}
