use crate::proto::{FidsRequest, FidsResponse};
use crate::{
    config::HubConfig,
    hub::stream::EventStream,
    proto::{
        BlocksRequest, GetInfoRequest, GetInfoResponse, ShardChunksRequest, ShardChunksResponse,
        hub_service_client::HubServiceClient,
    },
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, RwLock};
use tokio_stream::Stream;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::{debug, info, warn};
use crate::metrics;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Not connected")]
    NotConnected,
    #[error("Transport error: {0}")]
    TransportError(#[from] tonic::transport::Error),
    #[error("gRPC status error: {0}")]
    StatusError(#[from] tonic::Status),
    #[error("Stream error: {0}")]
    StreamError(#[from] crate::redis::error::Error),
    #[error("Rate limited")]
    RateLimited,
}

/// Rate limiter for the Hub client that tracks per-endpoint usage with metrics
#[derive(Clone)]
struct HubRateLimiter {
    // Rate limit per endpoint - stores (count, reset_timestamp)
    limits: Arc<parking_lot::Mutex<std::collections::HashMap<String, (u32, i64)>>>,
    // Global limit for all endpoints
    max_per_second: u32,
    // Track metrics about rate limiting for monitoring
    metrics: Arc<parking_lot::RwLock<RateLimitMetrics>>,
}

struct RateLimitMetrics {
    // Track the number of requests limited per endpoint
    limited_count: std::collections::HashMap<String, u64>,
    // Track total requests per endpoint
    total_count: std::collections::HashMap<String, u64>,
    // Track last reset time
    last_metrics_push: std::time::Instant,
}

impl Default for RateLimitMetrics {
    fn default() -> Self {
        Self {
            limited_count: std::collections::HashMap::new(),
            total_count: std::collections::HashMap::new(),
            last_metrics_push: std::time::Instant::now(),
        }
    }
}

impl HubRateLimiter {
    fn new(max_per_second: u32) -> Self {
        // Spawn background task to periodically report metrics
        let metrics = Arc::new(parking_lot::RwLock::new(RateLimitMetrics::default()));
        let metrics_clone = Arc::clone(&metrics);
        
        // Spawn a background metrics reporter thread - fire and forget
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                
                // Report metrics if there's been rate limiting activity
                let totals;
                let limited;
                
                {
                    let metrics_guard = metrics_clone.read();
                    // Skip empty metrics or recently reported metrics
                    if metrics_guard.last_metrics_push.elapsed() < std::time::Duration::from_secs(30) || 
                       metrics_guard.total_count.is_empty() {
                        continue;
                    }
                    
                    // Clone the metrics for reporting
                    totals = metrics_guard.total_count.clone();
                    limited = metrics_guard.limited_count.clone();
                }
                
                // Report metrics
                for (endpoint, count) in &totals {
                    let limited_count = limited.get(endpoint).cloned().unwrap_or(0);
                    let limit_rate = if *count > 0 {
                        (limited_count as f64 / *count as f64) * 100.0
                    } else {
                        0.0
                    };
                    
                    // Log rate limiting metrics for monitoring
                    if limited_count > 0 {
                        info!(
                            "Hub rate limiting stats for {}: {}/{} requests limited ({:.2}%)",
                            endpoint, limited_count, count, limit_rate
                        );
                        
                        // Add StatsD metrics
                        let _ = metrics::gauge(
                            &format!("hub.rate_limit.percent.{}", endpoint.replace('/', "_")),
                            limit_rate,
                        );
                    }
                }
                
                // Reset metrics after reporting
                {
                    let mut metrics_guard = metrics_clone.write();
                    metrics_guard.limited_count.clear();
                    metrics_guard.total_count.clear();
                    metrics_guard.last_metrics_push = std::time::Instant::now();
                }
            }
        });
        
        Self {
            limits: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            max_per_second,
            metrics,
        }
    }
    
    fn check(&self, endpoint: &str) -> bool {
        let now = chrono::Utc::now().timestamp();
        
        // Update request metrics for this endpoint
        {
            let mut metrics_guard = self.metrics.write();
            *metrics_guard.total_count.entry(endpoint.to_string()).or_default() += 1;
        }
        
        // Use a more efficient lock (parking_lot) for better performance under contention
        let mut limits = self.limits.lock();
        
        let (count, reset_at) = limits.entry(endpoint.to_string())
            .or_insert((0, now + 1)); // Default: 0 requests, reset in 1 second
            
        // Reset counter if needed
        if now > *reset_at {
            *count = 0;
            *reset_at = now + 1; // Reset every second
        }
        
        // Check if under limit
        let allowed = *count < self.max_per_second;
        
        // Update count if allowed, otherwise record rate limit metric
        if allowed {
            *count += 1;
        } else {
            // Track rate limiting for metrics
            let mut metrics_guard = self.metrics.write();
            *metrics_guard.limited_count.entry(endpoint.to_string()).or_default() += 1;
            debug!("Rate limited request to endpoint: {}", endpoint);
        }
        
        allowed
    }
}

/// Hub client with rate limiting
pub struct Hub {
    client: Option<HubServiceClient<Channel>>,
    config: Arc<HubConfig>,
    host: String,
    rate_limiter: Arc<HubRateLimiter>,
    semaphore: Arc<Semaphore>,
    retries: Arc<RwLock<RetryState>>,
}

struct RetryState {
    max_attempts: u32,
    base_delay_ms: u64,
}

impl Hub {
    pub fn new(config: impl Into<Arc<HubConfig>>) -> Result<Self, Error> {
        let config = config.into();
        let host = config.url.clone();
        
        // Create rate limiter based on configuration
        let rate_limiter = Arc::new(HubRateLimiter::new(
            config.rate_limit_per_second
        ));
        
        // Create semaphore for limiting concurrent requests
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_requests as usize));
        
        // Initialize retry state
        let retries = Arc::new(RwLock::new(RetryState {
            max_attempts: config.retry_max_attempts,
            base_delay_ms: config.retry_base_delay_ms,
        }));
        
        Ok(Hub { 
            client: None, 
            config, 
            host, 
            rate_limiter,
            semaphore,
            retries,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        info!("Connecting to Farcaster hub at {}", self.config.url);

        let channel = Channel::from_shared(format!("https://{}", self.config.url))
            .map_err(|e| Error::ConnectionError(e.to_string()))?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_adaptive_window(true)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .timeout(Duration::from_secs(30))
            .connect()
            .await?;

        let client = HubServiceClient::new(channel);
        self.client = Some(client);

        let hub_info = self.get_hub_info().await?;
        info!("Connected to Farcaster hub: {:?}", hub_info);

        Ok(())
    }

    pub fn client(&mut self) -> Option<&mut HubServiceClient<Channel>> {
        self.client.as_mut()
    }

    /// Execute a request with rate limiting, concurrency control, and retries
    async fn execute_with_control<F, Fut, T>(&self, request_name: &str, f: F) -> Result<T, Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, Error>>,
    {
        // Check if we're allowed to make this request based on rate limits
        if !self.rate_limiter.check(request_name) {
            debug!("Rate limited for {}", request_name);
            return Err(Error::RateLimited);
        }

        // Acquire semaphore permit to limit concurrent requests
        let _permit = match self.semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => return Err(Error::ConnectionError("Failed to acquire semaphore".to_string())),
        };

        // Read retry configuration
        let retry_state = self.retries.read().await;
        let max_attempts = retry_state.max_attempts;
        let base_delay = retry_state.base_delay_ms;
        drop(retry_state);

        // Execute with retries
        let mut attempt = 0;
        loop {
            attempt += 1;
            match f().await {
                Ok(result) => {
                    return Ok(result);
                }
                Err(err) => {
                    if attempt >= max_attempts {
                        warn!("Failed {} after {} attempts: {:?}", request_name, attempt, err);
                        return Err(err);
                    }
                    
                    // Exponential backoff with jitter
                    let delay = base_delay * 2u64.pow(attempt - 1);
                    let jitter = rand::random::<u64>() % (delay / 2);
                    let backoff = Duration::from_millis(delay + jitter);
                    
                    debug!("Retrying {} after {}ms (attempt {}/{})", 
                          request_name, backoff.as_millis(), attempt, max_attempts);
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    pub async fn stream(&mut self) -> Result<EventStream, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        Ok(EventStream::new(client))
    }

    pub async fn get_blocks(
        &mut self,
        shard_id: u32,
        start_block: u64,
        end_block: Option<u64>,
    ) -> Result<impl Stream<Item = Result<crate::proto::Block, tonic::Status>>, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        let client_ref = client.clone();

        let request = BlocksRequest {
            shard_id,
            start_block_number: start_block,
            stop_block_number: end_block,
        };

        self.execute_with_control("get_blocks", move || {
            let mut client_clone = client_ref.clone();
            let req_clone = request;
            async move {
                match client_clone.get_blocks(tonic::Request::new(req_clone)).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            }
        }).await
    }

    pub async fn get_shard_chunks(
        &mut self,
        shard_id: u32,
        start_block: u64,
        end_block: Option<u64>,
    ) -> Result<ShardChunksResponse, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        let client_ref = client.clone();

        let request = ShardChunksRequest {
            shard_id,
            start_block_number: start_block,
            stop_block_number: end_block,
        };

        self.execute_with_control("get_shard_chunks", move || {
            let mut client_clone = client_ref.clone();
            let req_clone = request;
            async move {
                match client_clone.get_shard_chunks(tonic::Request::new(req_clone)).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            }
        }).await
    }

    pub async fn disconnect(&mut self) -> Result<(), Error> {
        self.client = None;
        Ok(())
    }

    pub async fn get_hub_info(&mut self) -> Result<GetInfoResponse, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        let client_ref = client.clone();

        self.execute_with_control("get_hub_info", move || {
            let mut client_clone = client_ref.clone();
            async move {
                match client_clone.get_info(tonic::Request::new(GetInfoRequest {})).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            }
        }).await
    }

    /// Check hub connection by requesting hub info
    pub async fn check_connection(&mut self) -> Result<bool, Error> {
        match self.get_hub_info().await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Try to reconnect on errors
                match self.connect().await {
                    Ok(_) => Ok(true),
                    Err(_) => Err(e), // Return original error
                }
            },
        }
    }

    /// Get all FIDs from the hub
    pub async fn get_fids(
        &mut self,
        page_size: Option<u32>,
        page_token: Option<Vec<u8>>,
        reverse: Option<bool>,
    ) -> Result<FidsResponse, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        let client_ref = client.clone();

        let request = FidsRequest { page_size, page_token, reverse };

        self.execute_with_control("get_fids", move || {
            let mut client_clone = client_ref.clone();
            let req_clone = request.clone();
            async move {
                match client_clone.get_fids(tonic::Request::new(req_clone)).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            }
        }).await
    }
}