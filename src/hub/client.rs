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
    collections::HashMap,
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
            if jitter_range > 0 { rand::rng().random_range(0..=jitter_range - 1) } else { 0 };

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

    /// Add custom headers to request
    fn add_custom_headers<T>(&self, request: tonic::Request<T>) -> tonic::Request<T> {
        crate::hub::add_custom_headers(request, &self.config.headers)
    }

    /// Create an empty hub instance for testing or mock purposes
    pub fn empty() -> Self {
        // Create default config
        let config = Arc::new(HubConfig {
            url: "snapchain.farcaster.xyz:3383".to_string(),
            headers: HashMap::new(),
            max_concurrent_connections: 5,
            max_requests_per_second: 10,
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
            shard_indices: Vec::new(),
            subscribe_to_all_shards: false,
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
        let (url_str, use_tls) = if self.config.url.starts_with("http://") {
            (self.config.url.clone(), false)
        } else if self.config.url.starts_with("https://") {
            (self.config.url.clone(), true)
        } else {
            // Default to HTTPS if no protocol is specified
            (format!("https://{}", self.config.url), true)
        };

        let channel_builder =
            Channel::from_shared(url_str).map_err(|e| Error::ConnectionError(e.to_string()))?;

        // Only configure TLS for HTTPS connections
        let channel_builder = if use_tls {
            channel_builder.tls_config(ClientTlsConfig::new().with_native_roots())?
        } else {
            channel_builder
        };

        let channel = match channel_builder
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_adaptive_window(true)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .connect()
            .await
        {
            Ok(ch) => ch,
            Err(e) => {
                // Provide helpful error message for common Docker networking mistakes
                let error_msg = if self.config.url.contains("localhost")
                    || self.config.url.contains("127.0.0.1")
                {
                    format!(
                        "Failed to connect to hub at {}: {}\n\n\
                        NOTE: If running in Docker, 'localhost' refers to the container itself, not other containers or the host.\n\
                        Try using:\n\
                        - Container name (e.g., 'http://snapchain:3381') for docker-compose on the same network\n\
                        - 'host.docker.internal' (e.g., 'http://host.docker.internal:3381') for Docker Desktop\n\
                        - Host network mode or container IP for other setups\n\
                        See the documentation for more details on Docker networking configuration.",
                        self.config.url, e
                    )
                } else {
                    format!("Failed to connect to hub at {}: {}", self.config.url, e)
                };
                return Err(Error::ConnectionError(error_msg));
            },
        };

        // Store the channel for future use
        self.channel = Some(channel.clone());

        // Create base client without middleware for operations that don't need retries
        let client = HubServiceClient::new(channel.clone());
        self.client = Some(client);

        // Test connection with info request
        // Get hub info without middleware first time to avoid double retry
        let info_request = self.add_custom_headers(tonic::Request::new(GetInfoRequest {}));
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

                // Provide helpful error message for common Docker networking mistakes
                if self.config.url.contains("localhost") || self.config.url.contains("127.0.0.1") {
                    let error_msg = format!(
                        "Failed to connect to hub at {}: {}\n\n\
                        NOTE: If running in Docker, 'localhost' refers to the container itself, not other containers or the host.\n\
                        Try using:\n\
                        - Container name (e.g., 'http://snapchain:3381') for docker-compose on the same network\n\
                        - 'host.docker.internal' (e.g., 'http://host.docker.internal:3381') for Docker Desktop\n\
                        - Host network mode or container IP for other setups\n\
                        See the documentation for more details on Docker networking configuration.",
                        self.config.url, e
                    );
                    return Err(Error::ConnectionError(error_msg));
                }

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
            // Create the request
            let request = tonic::Request::new(BlocksRequest {
                shard_id,
                start_block_number: start_block,
                stop_block_number: end_block,
            });
            let request_with_headers = self.add_custom_headers(request);

            // Get a mutable reference to the client
            let client = self.client.as_mut().ok_or(Error::NotConnected)?;

            // Try to execute the request
            match client.get_blocks(request_with_headers).await {
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
        let headers = self.config.headers.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(ShardChunksRequest {
                    shard_id,
                    start_block_number: start_block,
                    stop_block_number: end_block,
                });
                let request_with_headers = crate::hub::add_custom_headers(request, &headers);
                match client.get_shard_chunks(request_with_headers).await {
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
        let headers = self.config.headers.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(GetInfoRequest {});
                let request_with_headers = crate::hub::add_custom_headers(request, &headers);
                match client.get_info(request_with_headers).await {
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
        let headers = self.config.headers.clone();

        // Use retry_with_backoff
        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let page_token_clone = page_token.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request = tonic::Request::new(FidsRequest {
                    page_size,
                    page_token: page_token_clone,
                    reverse,
                    shard_id: 0,
                });
                let request_with_headers = crate::hub::add_custom_headers(request, &headers);
                match client.get_fids(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get casts by FID with retry logic and custom headers
    pub async fn get_casts_by_fid(
        &mut self,
        request: crate::proto::FidRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_casts_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get reactions by FID with retry logic and custom headers
    pub async fn get_reactions_by_fid(
        &mut self,
        request: crate::proto::ReactionsByFidRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_reactions_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get links by FID with retry logic and custom headers
    pub async fn get_links_by_fid(
        &mut self,
        request: crate::proto::LinksByFidRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_links_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get verifications by FID with retry logic and custom headers
    pub async fn get_verifications_by_fid(
        &mut self,
        request: crate::proto::FidRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_verifications_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get user data by FID with retry logic and custom headers
    pub async fn get_user_data_by_fid(
        &mut self,
        request: crate::proto::FidRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_user_data_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get all user data messages by FID with retry logic and custom headers
    pub async fn get_all_user_data_messages_by_fid(
        &mut self,
        request: crate::proto::FidTimestampRequest,
    ) -> Result<crate::proto::MessagesResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_all_user_data_messages_by_fid(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }

    /// Get on-chain events with retry logic and custom headers
    pub async fn get_on_chain_events(
        &mut self,
        request: crate::proto::OnChainEventRequest,
    ) -> Result<crate::proto::OnChainEventResponse, Error> {
        if self.client.is_none() {
            self.connect().await?;
        }

        let client_clone = self.client.clone();
        let headers = self.config.headers.clone();

        self.retry_with_backoff(|| {
            let client_clone = client_clone.clone();
            let request = request.clone();
            let headers = headers.clone();
            Box::pin(async move {
                let mut client = client_clone.ok_or(Error::NotConnected)?;
                let request_with_headers =
                    crate::hub::add_custom_headers(tonic::Request::new(request), &headers);
                match client.get_on_chain_events(request_with_headers).await {
                    Ok(response) => Ok(response.into_inner()),
                    Err(status) => Err(Error::StatusError(status)),
                }
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HubConfig;
    use std::collections::HashMap;

    #[test]
    fn test_url_protocol_detection_https() {
        let config = Arc::new(HubConfig {
            url: "https://snapchain.farcaster.xyz:3383".to_string(),
            headers: HashMap::new(),
            max_concurrent_connections: 5,
            max_requests_per_second: 10,
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
            shard_indices: vec![],
            subscribe_to_all_shards: false,
        });

        let hub = Hub::new(config).unwrap();
        assert_eq!(hub.host(), "https://snapchain.farcaster.xyz:3383");
    }

    #[test]
    fn test_url_protocol_detection_http() {
        let config = Arc::new(HubConfig {
            url: "http://localhost:3383".to_string(),
            headers: HashMap::new(),
            max_concurrent_connections: 5,
            max_requests_per_second: 10,
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
            shard_indices: vec![],
            subscribe_to_all_shards: false,
        });

        let hub = Hub::new(config).unwrap();
        assert_eq!(hub.host(), "http://localhost:3383");
    }

    #[test]
    fn test_url_protocol_detection_no_protocol() {
        let config = Arc::new(HubConfig {
            url: "snapchain.farcaster.xyz:3383".to_string(),
            headers: HashMap::new(),
            max_concurrent_connections: 5,
            max_requests_per_second: 10,
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
            shard_indices: vec![],
            subscribe_to_all_shards: false,
        });

        let hub = Hub::new(config).unwrap();
        // URL without protocol should remain as is in the host field
        assert_eq!(hub.host(), "snapchain.farcaster.xyz:3383");
    }

    #[test]
    fn test_retry_policy_backoff_calculation() {
        let policy = HubRetryPolicy::new(5, 100, 30000, 0.0);

        // With 0 jitter factor, we should get predictable exponential backoff
        assert_eq!(policy.calculate_backoff(0), Duration::from_millis(100));
        assert_eq!(policy.calculate_backoff(1), Duration::from_millis(200));
        assert_eq!(policy.calculate_backoff(2), Duration::from_millis(400));
        assert_eq!(policy.calculate_backoff(3), Duration::from_millis(800));
        assert_eq!(policy.calculate_backoff(4), Duration::from_millis(1600));

        // Should cap at max_delay
        assert_eq!(policy.calculate_backoff(10), Duration::from_millis(30000));
    }

    #[test]
    fn test_retry_policy_connection_error_detection() {
        // Test H2 protocol error
        let status = Status::internal("h2 protocol error: connection reset");
        assert!(HubRetryPolicy::is_connection_error(&status));

        // Test body read error
        let status = Status::internal("error reading a body from connection");
        assert!(HubRetryPolicy::is_connection_error(&status));

        // Test connection closed
        let status = Status::unavailable("connection closed");
        assert!(HubRetryPolicy::is_connection_error(&status));

        // Test timeout
        let status = Status::deadline_exceeded("request timed out");
        assert!(HubRetryPolicy::is_connection_error(&status));

        // Test non-connection error
        let status = Status::invalid_argument("invalid request");
        assert!(!HubRetryPolicy::is_connection_error(&status));
    }

    #[test]
    fn test_custom_headers() {
        let mut headers = HashMap::new();
        headers.insert("X_API_KEY".to_string(), "test-key".to_string());
        headers.insert("X_HUB_TOKEN".to_string(), "hub-token-123".to_string());

        let config = Arc::new(HubConfig {
            url: "localhost:3383".to_string(),
            headers,
            max_concurrent_connections: 5,
            max_requests_per_second: 10,
            retry_max_attempts: 5,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 30000,
            retry_jitter_factor: 0.25,
            retry_timeout_ms: 60000,
            conn_timeout_ms: 30000,
            shard_indices: vec![],
            subscribe_to_all_shards: false,
        });

        let hub = Hub::new(config).unwrap();

        // Test that custom headers are properly added to requests
        let request = tonic::Request::new(GetInfoRequest {});
        let request_with_headers = hub.add_custom_headers(request);

        let metadata = request_with_headers.metadata();
        assert_eq!(metadata.get("x-api-key").unwrap().to_str().unwrap(), "test-key");
        assert_eq!(metadata.get("x-hub-token").unwrap().to_str().unwrap(), "hub-token-123");
    }

    #[test]
    fn test_connect_url_formatting() {
        // Test that URL formatting logic works correctly
        // We extract and test just the URL formatting logic without connecting

        // Helper function that mimics the URL formatting logic in connect()
        fn format_url(url: &str) -> (String, bool) {
            if url.starts_with("http://") {
                (url.to_string(), false)
            } else if url.starts_with("https://") {
                (url.to_string(), true)
            } else {
                (format!("https://{}", url), true)
            }
        }

        // Test HTTPS URL
        let (url, use_tls) = format_url("https://example.com:3383");
        assert_eq!(url, "https://example.com:3383");
        assert!(use_tls);

        // Test HTTP URL
        let (url, use_tls) = format_url("http://localhost:3383");
        assert_eq!(url, "http://localhost:3383");
        assert!(!use_tls);

        // Test URL without protocol (should default to HTTPS)
        let (url, use_tls) = format_url("example.com:3383");
        assert_eq!(url, "https://example.com:3383");
        assert!(use_tls);

        // Test localhost without protocol
        let (url, use_tls) = format_url("localhost:2283");
        assert_eq!(url, "https://localhost:2283");
        assert!(use_tls);
    }
}
