use crate::proto::{FidsRequest, FidsResponse};
use crate::{
    config::HubConfig,
    hub::stream::EventStream,
    proto::{
        BlocksRequest, GetInfoRequest, GetInfoResponse, ShardChunksRequest, ShardChunksResponse,
        hub_service_client::HubServiceClient,
    },
};
use rand::Rng;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_stream::Stream;
use tonic::Status;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::{error, info, warn};

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
    #[error("Retry budget exceeded: {0}")]
    RetryBudgetExceeded(String),
}

/// A retry policy for gRPC requests that handles connection-related errors
/// with built-in exponential backoff and jitter
#[derive(Debug, Clone)]
struct HubRetryPolicy {
    max_retries: u32,
    current_retry: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
    jitter_factor: f32,
}

impl HubRetryPolicy {
    fn new(max_retries: u32, base_delay_ms: u64, max_delay_ms: u64, jitter_factor: f32) -> Self {
        Self { max_retries, current_retry: 0, base_delay_ms, max_delay_ms, jitter_factor }
    }

    fn from_config(config: &Arc<crate::config::HubConfig>) -> Self {
        Self::new(
            config.retry_max_attempts,
            config.retry_base_delay_ms,
            config.retry_max_delay_ms,
            config.retry_jitter_factor,
        )
    }

    /// Calculates backoff duration with jitter for a specific retry attempt
    fn calculate_backoff(&self, attempts: u32) -> Duration {
        let base_delay = self.base_delay_ms;
        let max_delay = self.max_delay_ms;
        let jitter_factor = self.jitter_factor;

        // Calculate exponential backoff
        let exp_backoff = std::cmp::min(base_delay * 2u64.saturating_pow(attempts), max_delay);

        // Apply jitter
        let jitter_range = (exp_backoff as f32 * jitter_factor) as u64;
        let jitter =
            if jitter_range > 0 { rand::thread_rng().gen_range(0..jitter_range) } else { 0 };

        Duration::from_millis(exp_backoff.saturating_add(jitter))
    }

    /// Checks if an error is a connection-related error that should be retried
    fn is_connection_error(status: &Status) -> bool {
        match status.code() {
            tonic::Code::Internal | tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => {
                // Check for specific connection error messages
                status.message().contains("h2 protocol error")
                    || status.message().contains("error reading a body from connection")
                    || status.message().contains("connection reset")
                    || status.message().contains("connection closed")
                    || status.message().contains("timed out")
            },
            _ => false,
        }
    }

    // This method determines if a retry should be attempted based on the error
    fn should_retry(&self, status: &Status) -> bool {
        if !Self::is_connection_error(status) {
            // Non-connection errors are not retried
            warn!("Non-connection error, not retrying: {}", status);
            return false;
        }

        if self.current_retry >= self.max_retries {
            // Too many retries
            error!("Max retries ({}) exceeded, giving up: {}", self.max_retries, status);
            return false;
        }

        // Connection error that should be retried
        true
    }

    // Next backoff duration with jitter
    fn next_backoff(&self) -> Duration {
        self.calculate_backoff(self.current_retry)
    }

    // Advance to the next retry state
    fn advance(&mut self) {
        self.current_retry += 1;
    }
}

#[derive(Clone)]
pub struct Hub {
    // Use a Channel directly for building services with middleware
    channel: Option<Channel>,
    // Raw client without middleware
    client: Option<HubServiceClient<Channel>>,
    config: Arc<HubConfig>,
    host: String,
    // Track consecutive errors for advanced retry behavior
    error_count: Arc<std::sync::atomic::AtomicU32>,
    last_success: Arc<std::sync::atomic::AtomicU64>,
}

impl Hub {
    pub fn new(config: impl Into<Arc<HubConfig>>) -> Result<Self, Error> {
        let config = config.into();
        let host = config.url.clone();
        Ok(Hub {
            channel: None,
            client: None,
            config,
            host,
            error_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_success: Arc::new(std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )),
        })
    }

    /// Create an empty hub instance for testing or mock purposes
    pub fn empty() -> Self {
        // Create default config
        let config = Arc::new(HubConfig {
            url: "snapchain.farcaster.xyz:3383".to_string(),
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
        });

        // Create empty hub with default config
        let config_clone = config.clone();
        Hub {
            channel: None,
            client: None,
            config,
            host: config_clone.url.clone(),
            error_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_success: Arc::new(std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        info!("Connecting to Farcaster hub at {}", self.config.url);

        // Create channel with connection settings
        // Check if URL already has a scheme (http:// or https://)
        let url_str =
            if self.config.url.starts_with("http://") || self.config.url.starts_with("https://") {
                self.config.url.clone()
            } else {
                format!("https://{}", self.config.url)
            };

        let channel = Channel::from_shared(url_str)
            .map_err(|e| Error::ConnectionError(e.to_string()))?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_adaptive_window(true)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .connect()
            .await?;

        // Store the channel for future use
        self.channel = Some(channel.clone());

        // Create base client without middleware for operations that don't need retries
        let client = HubServiceClient::new(channel.clone());
        self.client = Some(client);

        // We're not using Tower for retries anymore - since Tower doesn't handle streaming
        // RPCs well for our use case. Instead we'll use our retry_with_backoff helper for retries.

        // Test connection with info request
        // Get hub info without middleware first time to avoid double retry
        let info_request = tonic::Request::new(GetInfoRequest {});
        match self.client.as_mut().unwrap().get_info(info_request).await {
            Ok(response) => {
                let hub_info = response.into_inner();
                info!("Connected to Farcaster hub: {:?}", hub_info);

                // Reset error count and update last success on successful connection
                self.error_count.store(0, std::sync::atomic::Ordering::SeqCst);
                self.last_success.store(
                    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                    std::sync::atomic::Ordering::SeqCst,
                );

                Ok(())
            },
            Err(e) => {
                // If there's an error, clean up client and channel
                self.client = None;
                self.channel = None;
                Err(Error::StatusError(e))
            },
        }
    }

    /// Helper method to handle retries with proper error handling and backoff
    async fn retry_with_backoff<T, F>(&self, mut operation: F) -> Result<T, Error>
    where
        F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, Error>> + Send>>
            + Send,
    {
        // Create retry policy with config values
        let mut retry_policy = HubRetryPolicy::from_config(&self.config);

        loop {
            // Execute the operation
            match operation().await {
                Ok(result) => {
                    // Success - reset error counter and update success timestamp
                    self.error_count.store(0, std::sync::atomic::Ordering::SeqCst);
                    self.last_success.store(
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                        std::sync::atomic::Ordering::SeqCst,
                    );

                    return Ok(result);
                },
                Err(error) => {
                    // Extract status if this is a status error
                    let (status, is_connection_error) = match &error {
                        Error::StatusError(status) => {
                            (Some(status), HubRetryPolicy::is_connection_error(status))
                        },
                        Error::ConnectionError(_) => (None, true),
                        Error::TransportError(_) => (None, true),
                        _ => (None, false),
                    };

                    // Check if we should retry based on error type and retry count
                    let should_retry = if let Some(status) = status {
                        retry_policy.should_retry(status)
                    } else {
                        // For other errors, just retry if we have attempts left
                        retry_policy.current_retry < retry_policy.max_retries
                    };

                    // Increment error counter
                    self.error_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                    // If we shouldn't retry, return the error
                    if !should_retry {
                        return Err(error);
                    }

                    // Calculate backoff with jitter
                    let backoff = retry_policy.next_backoff();

                    // Advance retry state
                    retry_policy.advance();

                    // If this is likely a connection error, log it
                    // We can't directly reconnect here since we only have an immutable reference
                    if is_connection_error {
                        warn!(
                            "Connection error detected (retry {}/{}), will retry after backoff",
                            retry_policy.current_retry, retry_policy.max_retries
                        );
                    }

                    // Log the retry attempt
                    warn!(
                        "Retrying request (attempt {}/{}), backing off for {:?}ms: {}",
                        retry_policy.current_retry,
                        retry_policy.max_retries,
                        backoff.as_millis(),
                        error
                    );

                    // Wait before retrying
                    tokio::time::sleep(backoff).await;
                },
            }
        }
    }

    pub fn client(&mut self) -> Option<&mut HubServiceClient<Channel>> {
        self.client.as_mut()
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
        // For streaming RPCs, we need to use the regular client
        // Tower's retry middleware is not designed for streaming responses
        // So we'll use a simplified retry approach for this specific method

        // If not connected, connect first
        if self.client.is_none() {
            self.connect().await?;
        }

        let mut attempts = 0;
        let max_attempts = self.config.retry_max_attempts;

        loop {
            // Get a mutable reference to the client
            let client = self.client.as_mut().ok_or(Error::NotConnected)?;

            // Create the request
            let request = tonic::Request::new(BlocksRequest {
                shard_id,
                start_block_number: start_block,
                stop_block_number: end_block,
            });

            // Try to execute the request
            match client.get_blocks(request).await {
                Ok(response) => {
                    // Reset error count and update success timestamp
                    self.error_count.store(0, std::sync::atomic::Ordering::SeqCst);
                    self.last_success.store(
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                        std::sync::atomic::Ordering::SeqCst,
                    );

                    // Extract inner stream and return
                    return Ok(response.into_inner());
                },
                Err(status) => {
                    // Check if this is a connection error that should be retried
                    if !HubRetryPolicy::is_connection_error(&status) {
                        // Non-connection errors are not retried
                        warn!("Non-connection error, not retrying: {}", status);
                        return Err(Error::StatusError(status));
                    }

                    // Increment attempt counter
                    attempts += 1;
                    self.error_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                    // Check if we've exceeded max attempts
                    if attempts >= max_attempts {
                        error!("Max retries ({}) exceeded, giving up: {}", max_attempts, status);
                        return Err(Error::StatusError(status));
                    }

                    // Calculate backoff with jitter
                    let retry_policy = HubRetryPolicy::from_config(&self.config);
                    let backoff = retry_policy.calculate_backoff(attempts);

                    // Log the retry attempt
                    warn!(
                        "Connection error detected (retry {}/{}), backing off for {:?}ms: {}",
                        attempts,
                        max_attempts,
                        backoff.as_millis(),
                        status
                    );

                    // Wait before retrying
                    tokio::time::sleep(backoff).await;
                },
            }
        }
    }

    pub async fn get_shard_chunks(
        &mut self,
        shard_id: u32,
        start_block: u64,
        end_block: Option<u64>,
    ) -> Result<ShardChunksResponse, Error> {
        // If not connected, connect first
        if self.client.is_none() {
            self.connect().await?;
        }

        // Clone what we need for the closure to avoid borrowing self
        let client_clone = self.client.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(ShardChunksRequest {
                    shard_id,
                    start_block_number: start_block,
                    stop_block_number: end_block,
                });
                match client.get_shard_chunks(request).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    pub async fn disconnect(&mut self) -> Result<(), Error> {
        self.client = None;
        self.channel = None;
        Ok(())
    }

    pub async fn get_hub_info(&mut self) -> Result<GetInfoResponse, Error> {
        // If not connected, connect first
        if self.client.is_none() {
            self.connect().await?;
        }

        // Clone what we need for the closure to avoid borrowing self
        let client_clone = self.client.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(GetInfoRequest {});
                match client.get_info(request).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Check hub connection by requesting hub info
    /// Uses the enhanced retry logic in get_hub_info
    pub async fn check_connection(&mut self) -> Result<bool, Error> {
        // get_hub_info already has the retry logic built in
        match self.get_hub_info().await {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// Get all FIDs from the hub with retry logic
    pub async fn get_fids(
        &mut self,
        page_size: Option<u32>,
        page_token: Option<Vec<u8>>,
        reverse: Option<bool>,
    ) -> Result<FidsResponse, Error> {
        // If not connected, connect first
        if self.client.is_none() {
            self.connect().await?;
        }

        // Clone what we need for the closure to avoid borrowing self
        let client_clone = self.client.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let page_token_clone = page_token.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(FidsRequest {
                    page_size,
                    page_token: page_token_clone,
                    reverse,
                });
                match client.get_fids(request).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }
}
