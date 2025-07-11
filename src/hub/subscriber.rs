use crate::{
    config::HubConfig,
    hub::{error::Error, filter::SpamFilter, stats::ProcessingStats},
    proto::{
        GetInfoRequest, HubEvent, HubEventType, SubscribeRequest, hub_event,
        hub_service_client::HubServiceClient,
    },
    redis::{client::Redis, stream::RedisStream},
};
use dashmap::DashMap;
use futures::StreamExt;
use prost::Message as ProstMessage;
use rand::Rng;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::{error, info, instrument, trace, warn};

pub type PreProcessHandler = Arc<
    dyn Fn(&[HubEvent], &[Vec<u8>]) -> futures::future::BoxFuture<'static, Vec<bool>> + Send + Sync,
>;
pub type PostProcessHandler =
    Arc<dyn Fn(&[HubEvent], &[Vec<u8>]) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

struct BatchState {
    events: Vec<(HubEvent, Vec<u8>)>,
    current_bytes: usize,
    last_flush: Instant,
}

impl BatchState {
    fn new() -> Self {
        Self { events: Vec::new(), current_bytes: 0, last_flush: Instant::now() }
    }
}

#[derive(Clone)]
pub struct HubSubscriber {
    hub: HubServiceClient<Channel>,
    redis: Arc<Redis>,
    redis_stream: Arc<RedisStream>,
    stream_key: Arc<String>,
    redis_key: Arc<String>,
    event_types: Arc<Vec<i32>>,
    // The shard_index is used for SubscribeRequest
    shard_index: Option<u64>,
    batch_size: usize,
    max_batch_bytes: usize,
    max_flush_interval: Duration,
    stream_maxlen: u64,
    _before_process: Option<PreProcessHandler>,
    _after_process: Option<PostProcessHandler>,
    _stats: Arc<RwLock<ProcessingStats>>,
    shutdown: Arc<RwLock<bool>>,
    connection_timeout: Duration,
    spam_filter: Arc<SpamFilter>,
    spam_filter_enabled: bool,
    // Track last successful Redis publish time for better connection monitoring
    last_successful_flush: Arc<RwLock<Option<Instant>>>,
    // Enhanced connection tracking and retry configuration
    consecutive_errors: Arc<std::sync::atomic::AtomicU32>,
    last_success: Arc<std::sync::atomic::AtomicU64>,
    hub_config: Arc<HubConfig>,
}

impl HubSubscriber {
    pub async fn new(
        hub: HubServiceClient<Channel>,
        redis: Arc<Redis>,
        redis_stream: RedisStream,
        hub_host: String,
        shard_key: String,
        opts: SubscriberOptions,
    ) -> Self {
        let event_types = vec![
            HubEventType::MergeMessage as i32,
            HubEventType::PruneMessage as i32,
            HubEventType::RevokeMessage as i32,
            HubEventType::MergeUsernameProof as i32,
            HubEventType::MergeOnChainEvent as i32,
        ];

        // Create and initialize spam filter - this now blocks until initial load
        let spam_filter = Arc::new(SpamFilter::new());

        // Check for spam filter enable/disable flag in options
        let spam_filter_enabled = opts.spam_filter_enabled.unwrap_or(true);

        if spam_filter_enabled {
            info!("Loading spam filter list before starting Hub subscriber...");
            if let Err(e) = spam_filter.start_updater().await {
                error!(
                    "Failed to load initial spam filter list: {}. Hub subscription may include spam messages until filter loads.",
                    e
                );
                // We'll continue anyway since the background task will retry
            } else {
                info!("Spam filter loaded - Hub subscription will filter spam messages");
            }
        } else {
            info!("Spam filter disabled - all messages will be processed including spam");
        }

        // Get the current time in seconds since epoch for tracking the last successful operation
        let current_time =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        // Get connection timeout from HubConfig or use default
        let hub_config = if let Some(config) = &opts.hub_config {
            Arc::clone(config)
        } else {
            // Create a default config if none provided
            Arc::new(HubConfig {
                url: hub_host.clone(),
                headers: std::collections::HashMap::new(),
                max_concurrent_connections: 5,
                max_requests_per_second: 10,
                retry_max_attempts: 5,
                retry_base_delay_ms: 100,
                retry_max_delay_ms: 30000,
                retry_jitter_factor: 0.25,
                retry_timeout_ms: 60000,
                conn_timeout_ms: 30000,
            })
        };

        let connection_timeout = Duration::from_millis(hub_config.conn_timeout_ms);

        Self {
            hub,
            redis,
            redis_stream: Arc::new(redis_stream),
            stream_key: Arc::new(crate::types::get_stream_key(
                &hub_host,
                "",
                if shard_key.is_empty() { None } else { Some(&shard_key) },
            )),
            redis_key: Arc::new(format!("{}:{}", hub_host, shard_key)),
            event_types: Arc::new(event_types),
            shard_index: opts.shard_index,
            batch_size: 100,
            max_batch_bytes: 2_usize.pow(20), // 2 MiB
            max_flush_interval: Duration::from_millis(200),
            stream_maxlen: 3_000_000,
            _before_process: opts.before_process,
            _after_process: opts.after_process,
            _stats: Arc::new(RwLock::new(ProcessingStats::new())),
            shutdown: Arc::new(RwLock::new(false)),
            connection_timeout,
            spam_filter,
            spam_filter_enabled,
            last_successful_flush: Arc::new(RwLock::new(Some(Instant::now()))),
            consecutive_errors: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_success: Arc::new(std::sync::atomic::AtomicU64::new(current_time)),
            hub_config,
        }
    }

    pub async fn get_last_event_id(&self) -> Result<Option<u64>, crate::redis::error::Error> {
        self.redis.get_last_processed_event(&self.redis_key).await
    }

    /// Add custom headers to request
    fn add_custom_headers<T>(&self, request: tonic::Request<T>) -> tonic::Request<T> {
        crate::hub::add_custom_headers(request, &self.hub_config.headers)
    }

    async fn wait_for_ready(&self) -> Result<(), Error> {
        // Enhanced wait_for_ready with configurable retries and exponential backoff
        let max_attempts = self.hub_config.retry_max_attempts;
        let mut current_attempt = 0;
        let mut backoff = Duration::from_millis(self.hub_config.retry_base_delay_ms);
        let max_backoff = Duration::from_millis(self.hub_config.retry_max_delay_ms);
        let jitter_factor = self.hub_config.retry_jitter_factor;

        while current_attempt < max_attempts {
            current_attempt += 1;

            // Try to get hub info with custom headers
            let request = tonic::Request::new(GetInfoRequest {});
            let request = self.add_custom_headers(request);

            match self.hub.clone().get_info(request).await {
                Ok(_) => {
                    // Success - reset error counter and update last success time
                    self.consecutive_errors.store(0, std::sync::atomic::Ordering::SeqCst);
                    self.last_success.store(
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    return Ok(());
                },
                Err(e) => {
                    // Apply jitter to backoff to prevent thundering herd
                    let jitter_ms = if jitter_factor > 0.0 {
                        let jitter_range = (backoff.as_millis() as f32 * jitter_factor) as u64;
                        if jitter_range > 0 {
                            rand::rng().random_range(0..=jitter_range - 1)
                        } else {
                            0
                        }
                    } else {
                        0
                    };

                    let backoff_with_jitter = backoff + Duration::from_millis(jitter_ms);

                    if current_attempt < max_attempts {
                        warn!(
                            "Hub not ready (attempt {}/{}) [status: {:?}], retrying in {:?}ms",
                            current_attempt,
                            max_attempts,
                            e.code(),
                            backoff_with_jitter.as_millis()
                        );
                        tokio::time::sleep(backoff_with_jitter).await;

                        // Exponential backoff
                        backoff = std::cmp::min(backoff * 2, max_backoff);
                    } else {
                        error!("Hub not ready after {} attempts, giving up: {:?}", max_attempts, e);
                    }
                },
            }
        }

        // Increment consecutive errors counter
        self.consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Err(Error::ConnectionError(format!(
            "Hub not ready after {} attempts with max backoff of {}ms",
            max_attempts,
            max_backoff.as_millis()
        )))
    }

    #[instrument(skip(self))]
    pub async fn try_start(&self) -> Result<(), Error> {
        info!("Starting hub subscriber");

        self.wait_for_ready().await?;
        info!("Connected to hub");

        let last_id = self.get_last_event_id().await?;
        if last_id > Some(0) {
            trace!("Found last hub event ID: {}", last_id.unwrap());
        } else {
            trace!("No last hub event ID found, starting from beginning");
        }

        let mut stream = self.connect_stream(last_id).await?;
        let mut batch_state = BatchState::new();

        // Use atomic counter for consistent error tracking
        let max_consecutive_errors = 3;

        while !*self.shutdown.read().await {
            match stream.next().await {
                Some(Ok(event)) => {
                    // Reset error counter on successful event
                    self.consecutive_errors.store(0, std::sync::atomic::Ordering::SeqCst);

                    // Update last successful operation timestamp
                    self.last_success.store(
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                        std::sync::atomic::Ordering::SeqCst,
                    );

                    // Update connection monitoring timestamp
                    *self.last_successful_flush.write().await = Some(Instant::now());

                    let bytes = event.encode_to_vec();
                    batch_state.current_bytes += bytes.len();
                    batch_state.events.push((event, bytes));

                    if self.should_flush(&batch_state).await {
                        match self.flush_batch_with_retry(&mut batch_state).await {
                            Ok(_) => {
                                // Update successful flush timestamp
                                *self.last_successful_flush.write().await = Some(Instant::now());

                                // Also update global success counter
                                self.last_success.store(
                                    SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    std::sync::atomic::Ordering::SeqCst,
                                );
                            },
                            Err(e) => {
                                error!("Error flushing batch: {:?}", e);
                                let current_errors = self
                                    .consecutive_errors
                                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                                    + 1;

                                if current_errors >= max_consecutive_errors {
                                    error!(
                                        "Too many consecutive flush errors ({}), reconnecting stream with exponential backoff",
                                        current_errors
                                    );

                                    // Use configured backoff with jitter
                                    let backoff_ms = Self::calculate_backoff_with_jitter(
                                        current_errors,
                                        self.hub_config.retry_base_delay_ms,
                                        self.hub_config.retry_max_delay_ms,
                                        self.hub_config.retry_jitter_factor,
                                    );

                                    info!(
                                        "Waiting for {:?}ms before reconnection attempt",
                                        backoff_ms
                                    );
                                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                                    // Try to reconnect - with specific error handling for connection errors
                                    match self.connect_stream(last_id).await {
                                        Ok(new_stream) => {
                                            stream = new_stream;
                                            self.consecutive_errors
                                                .store(0, std::sync::atomic::Ordering::SeqCst);
                                            continue;
                                        },
                                        Err(reconnect_error) => {
                                            error!(
                                                "Failed to reconnect: {:?}, will retry on next iteration",
                                                reconnect_error
                                            );
                                            // We'll try again on the next iteration with increased backoff
                                            tokio::time::sleep(Duration::from_millis(100)).await;
                                        },
                                    }
                                }
                            },
                        }
                    }
                },
                Some(Err(e)) => {
                    // Identify h2 protocol errors specifically
                    let is_h2_error = e.message().contains("h2 protocol error")
                        || e.message().contains("error reading a body from connection");

                    if is_h2_error {
                        error!("H2 protocol error in stream: {:?}", e);
                    } else {
                        error!("Stream error: {:?}", e);
                    }

                    let current_errors =
                        self.consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            + 1;

                    if current_errors >= max_consecutive_errors || is_h2_error {
                        // Always reconnect immediately for h2 protocol errors
                        let reason = if is_h2_error {
                            "H2 protocol error detected".to_string()
                        } else {
                            format!("Too many consecutive stream errors ({})", current_errors)
                        };

                        error!("{}, reconnecting with backoff", reason);

                        // Use configured backoff with jitter
                        let backoff_ms = Self::calculate_backoff_with_jitter(
                            current_errors,
                            self.hub_config.retry_base_delay_ms,
                            self.hub_config.retry_max_delay_ms,
                            self.hub_config.retry_jitter_factor,
                        );

                        info!("Waiting for {:?}ms before reconnection attempt", backoff_ms);
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                        match self.connect_stream(last_id).await {
                            Ok(new_stream) => {
                                stream = new_stream;
                                self.consecutive_errors
                                    .store(0, std::sync::atomic::Ordering::SeqCst);
                            },
                            Err(reconnect_error) => {
                                error!(
                                    "Failed to reconnect: {:?}, will continue retrying",
                                    reconnect_error
                                );
                                // Use longer backoff for failed reconnection
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            },
                        }
                    } else {
                        // For non-critical errors with low count, continue without reconnecting
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                None => {
                    // Stream closed, try to reconnect after delay with proper backoff
                    warn!("Hub stream closed unexpectedly, reconnecting after delay");

                    let current_errors =
                        self.consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            + 1;
                    let backoff_ms = Self::calculate_backoff_with_jitter(
                        current_errors,
                        self.hub_config.retry_base_delay_ms,
                        self.hub_config.retry_max_delay_ms,
                        self.hub_config.retry_jitter_factor,
                    );

                    info!("Waiting for {:?}ms before reconnection attempt", backoff_ms);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                    match self.connect_stream(last_id).await {
                        Ok(new_stream) => {
                            stream = new_stream;
                            self.consecutive_errors.store(0, std::sync::atomic::Ordering::SeqCst);
                        },
                        Err(reconnect_error) => {
                            error!(
                                "Failed to reconnect after stream closure: {:?}, will retry",
                                reconnect_error
                            );
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        },
                    }
                },
            }

            // Check connection timeout - improved with more flexible monitoring
            let flush_timeout_exceeded = match *self.last_successful_flush.read().await {
                Some(timestamp) => timestamp.elapsed() > self.connection_timeout,
                None => false,
            };

            // Also check the global success timestamp
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let last_success = self.last_success.load(std::sync::atomic::Ordering::SeqCst);
            let global_timeout_exceeded =
                now.saturating_sub(last_success) * 1000 > self.hub_config.conn_timeout_ms;

            if flush_timeout_exceeded || global_timeout_exceeded {
                let reason = if flush_timeout_exceeded {
                    format!("No successful flush for {:?}", self.connection_timeout)
                } else {
                    format!("No successful operations for {:?}ms", self.hub_config.conn_timeout_ms)
                };

                error!("{}, forcing reconnection", reason);

                // Use backoff for reconnection
                let current_errors =
                    self.consecutive_errors.load(std::sync::atomic::Ordering::SeqCst);
                let backoff_ms = Self::calculate_backoff_with_jitter(
                    current_errors,
                    self.hub_config.retry_base_delay_ms,
                    self.hub_config.retry_max_delay_ms,
                    self.hub_config.retry_jitter_factor,
                );

                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                match self.connect_stream(last_id).await {
                    Ok(new_stream) => {
                        stream = new_stream;
                        // Reset error tracking
                        self.consecutive_errors.store(0, std::sync::atomic::Ordering::SeqCst);
                        *self.last_successful_flush.write().await = Some(Instant::now());
                        self.last_success.store(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                    },
                    Err(reconnect_error) => {
                        error!(
                            "Failed to reconnect after timeout: {:?}, will retry",
                            reconnect_error
                        );
                        // Still set last flush time to avoid tight reconnection loops
                        *self.last_successful_flush.write().await = Some(Instant::now());
                    },
                }
            }
        }

        Ok(())
    }

    // Helper function to calculate backoff with jitter
    fn calculate_backoff_with_jitter(
        attempt: u32,
        base_delay_ms: u64,
        max_delay_ms: u64,
        jitter_factor: f32,
    ) -> u64 {
        // Ensure attempt is at least 1 to prevent underflow
        let safe_attempt = std::cmp::max(1, attempt);

        // Calculate exponential backoff
        let exponential_backoff =
            std::cmp::min(base_delay_ms * 2u64.saturating_pow(safe_attempt - 1), max_delay_ms);

        // Apply jitter
        if jitter_factor > 0.0 {
            let jitter_range = (exponential_backoff as f32 * jitter_factor) as u64;
            if jitter_range > 0 {
                let jitter = rand::rng().random_range(0..=jitter_range - 1);
                exponential_backoff.saturating_add(jitter)
            } else {
                exponential_backoff
            }
        } else {
            exponential_backoff
        }
    }

    pub async fn start(&self) -> Result<(), Error> {
        // Set up periodic cleanup
        let stream_key = self.stream_key.clone();
        let stream = self.redis_stream.clone();
        let cleanup_interval = Duration::from_secs(60);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;
                if let Err(e) = stream.trim(&stream_key, Duration::from_secs(24 * 60 * 60)).await {
                    error!("Error trimming events: {}", e);
                }
            }
        });

        // Main processing loop with retries
        // Use configured values for more consistent behavior
        let hub_config = self.hub_config.clone();
        let consecutive_errors = self.consecutive_errors.clone();

        while !*self.shutdown.read().await {
            match self.try_start().await {
                Ok(_) => break,
                Err(e) => {
                    // Increment error counter
                    let current_errors =
                        consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;

                    // Check if error is related to H2 protocol
                    let is_h2_error = match &e {
                        Error::StatusError(status) => {
                            status.message().contains("h2 protocol error")
                                || status.message().contains("error reading a body from connection")
                        },
                        Error::ConnectionError(msg) => {
                            msg.contains("h2 protocol error") || msg.contains("connection reset")
                        },
                        _ => false,
                    };

                    if is_h2_error {
                        error!("H2 protocol error, applying specialized retry logic: {:?}", e);
                    } else {
                        error!("Stream error, reconnecting with exponential backoff: {:?}", e);
                    }

                    // Calculate backoff with jitter for more natural retry patterns
                    let backoff_ms = Self::calculate_backoff_with_jitter(
                        current_errors,
                        hub_config.retry_base_delay_ms,
                        hub_config.retry_max_delay_ms,
                        hub_config.retry_jitter_factor,
                    );

                    info!(
                        "Waiting for {:?}ms before retry attempt #{}",
                        backoff_ms, current_errors
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                },
            }
        }

        info!("Event stream terminated");
        Ok(())
    }

    async fn connect_stream(
        &self,
        last_id: Option<u64>,
    ) -> Result<tonic::Streaming<HubEvent>, Error> {
        // Use enhanced retry mechanism with exponential backoff and jitter for connection
        let mut backoff = Duration::from_millis(self.hub_config.retry_base_delay_ms);
        let max_backoff = Duration::from_millis(self.hub_config.retry_max_delay_ms);
        let max_attempts = self.hub_config.retry_max_attempts;
        let jitter_factor = self.hub_config.retry_jitter_factor;

        for attempt in 1..=max_attempts {
            // Create a fresh request for each attempt
            let req = tonic::Request::new(SubscribeRequest {
                from_id: last_id,
                event_types: self.event_types.as_ref().to_vec(),
                shard_index: self.shard_index.map(|s| s as u32),
            });
            let req = self.add_custom_headers(req);

            // Clone hub client before making the call
            let mut hub_clone = self.hub.clone();
            match hub_clone.subscribe(req).await {
                Ok(response) => {
                    info!("Established gRPC stream connection");

                    // Reset error tracking on success
                    self.consecutive_errors.store(0, std::sync::atomic::Ordering::SeqCst);
                    self.last_success.store(
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                        std::sync::atomic::Ordering::SeqCst,
                    );

                    return Ok(response.into_inner());
                },
                Err(e) => {
                    // Apply jitter to backoff to prevent thundering herd
                    let jitter_ms = if jitter_factor > 0.0 {
                        let jitter_range = (backoff.as_millis() as f32 * jitter_factor) as u64;
                        if jitter_range > 0 {
                            rand::rng().random_range(0..=jitter_range - 1)
                        } else {
                            0
                        }
                    } else {
                        0
                    };

                    let backoff_with_jitter = backoff + Duration::from_millis(jitter_ms);

                    // Increment consecutive errors counter
                    self.consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                    // Check if error appears to be a connection failure
                    let is_conn_error = e.code() == tonic::Code::Internal
                        && (e.message().contains("h2 protocol error")
                            || e.message().contains("error reading a body from connection")
                            || e.message().contains("connection reset")
                            || e.message().contains("connection closed"));

                    // Log detailed error information for better diagnostics
                    if attempt == max_attempts {
                        error!(
                            "Failed to connect to gRPC stream after {} attempts: {:?}",
                            max_attempts, e
                        );

                        if is_conn_error {
                            error!(
                                "This appears to be a connection error. Will try to establish new connection."
                            );
                            // Return specific error type for connection errors
                            return Err(Error::ConnectionError(format!(
                                "H2 protocol error: {}",
                                e
                            )));
                        }

                        return Err(Error::from(e));
                    }

                    // More detailed logging with additional context
                    if is_conn_error {
                        warn!(
                            "H2 protocol error connecting to gRPC stream (attempt {}/{}): {}, retrying in {:?}ms",
                            attempt,
                            max_attempts,
                            e,
                            backoff_with_jitter.as_millis()
                        );
                    } else {
                        warn!(
                            "Error connecting to gRPC stream (attempt {}/{}): {:?}, retrying in {:?}ms",
                            attempt,
                            max_attempts,
                            e,
                            backoff_with_jitter.as_millis()
                        );
                    }

                    tokio::time::sleep(backoff_with_jitter).await;
                    // Exponential backoff
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                },
            }
        }

        // This should never be reached due to the return in the loop above, but handle it anyway
        Err(Error::ConnectionError(format!(
            "Failed to connect to gRPC stream after {} attempts with max backoff of {}ms",
            max_attempts,
            max_backoff.as_millis()
        )))
    }

    async fn should_flush(&self, batch: &BatchState) -> bool {
        batch.events.len() >= self.batch_size
            || batch.current_bytes >= self.max_batch_bytes
            || batch.last_flush.elapsed() >= self.max_flush_interval
    }

    async fn flush_batch_with_retry(&self, batch: &mut BatchState) -> Result<(), Error> {
        let mut backoff = Duration::from_millis(100);
        let max_backoff = Duration::from_secs(5);
        let max_retries = 5;
        let mut attempts = 0;

        while attempts < max_retries {
            match self.flush_batch(batch).await {
                Ok(_) => return Ok(()),
                Err(Error::InternalRedisError(e)) => {
                    attempts += 1;
                    error!(
                        "Redis error when flushing batch (attempt {}/{}): {}",
                        attempts, max_retries, e
                    );

                    if attempts == max_retries {
                        error!("Max retries reached when flushing batch to Redis");
                        break;
                    }

                    trace!("Retrying batch flush in {:?}...", backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                },
                Err(e) => {
                    error!("Non-Redis error when flushing batch: {:?}", e);
                    return Err(e);
                },
            }
        }

        // Provide more detailed error message
        Err(Error::ProcessingError(format!(
            "Failed to flush batch after {} attempts - Redis connection may be unstable",
            max_retries
        )))
    }

    async fn flush_batch(&self, batch: &mut BatchState) -> Result<(), Error> {
        if batch.events.is_empty() {
            return Ok(());
        }

        // Record start time for performance monitoring
        let start_time = Instant::now();
        let event_count = batch.events.len();

        trace!("Flushing batch of {} events ({} bytes)", event_count, batch.current_bytes);

        // Filter spam events if enabled
        let keep_indices = if self.spam_filter_enabled {
            self.spam_filter
                .filter_events(&batch.events.iter().map(|(e, _)| e.clone()).collect::<Vec<_>>())
                .await
        } else {
            // If spam filter is disabled, keep all events
            (0..batch.events.len()).collect()
        };

        let filtered_count = event_count - keep_indices.len();
        if filtered_count > 0 {
            trace!("Filtered {} spam events from batch", filtered_count);
        }

        let event_groups: DashMap<&str, Vec<Vec<u8>>> = DashMap::new();

        // Process and group non-spam events
        for &idx in &keep_indices {
            let (event, bytes) = &batch.events[idx];
            let event_type = match event.r#type {
                1 => {
                    // MERGE_MESSAGE
                    if let Some(hub_event::Body::MergeMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            Some(13) => "frame_actions",
                            Some(14) => "link_compact_states",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                2 => {
                    // PRUNE_MESSAGE
                    if let Some(hub_event::Body::PruneMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            Some(13) => "frame_actions",
                            Some(14) => "link_compact_states",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                3 => {
                    // REVOKE_MESSAGE
                    if let Some(hub_event::Body::RevokeMessageBody(body)) = &event.body {
                        match body
                            .message
                            .as_ref()
                            .and_then(|m| m.data.as_ref().map(|data| data.r#type))
                        {
                            Some(1) | Some(2) => "casts",
                            Some(3) | Some(4) => "reactions",
                            Some(5) | Some(6) => "links",
                            Some(7) | Some(8) => "verifications",
                            Some(11) => "user_data",
                            Some(12) => "username_proofs",
                            Some(13) => "frame_actions",
                            Some(14) => "link_compact_states",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    }
                },
                6 => "username_proofs", // MERGE_USERNAME_PROOF
                9 => {
                    // MERGE_ON_CHAIN_EVENT
                    if let Some(hub_event::Body::MergeOnChainEventBody(body)) = &event.body {
                        match body.on_chain_event.as_ref().map(|e| e.r#type) {
                            Some(1) => "onchain:signer",
                            Some(2) => "onchain:signer_migrated",
                            Some(3) => "onchain:id_register",
                            Some(4) => "onchain:storage_rent",
                            Some(5) => "onchain:tier_purchase",
                            _ => "onchain:unknown",
                        }
                    } else {
                        "onchain:unknown"
                    }
                },
                10 => "merge_failures", // MERGE_FAILURE
                _ => "unknown",
            };
            event_groups.entry(event_type).or_default().push(bytes.clone());
        }

        // Check if we have event groups to process
        if event_groups.is_empty() {
            trace!("No valid events to process in batch after filtering");
            batch.events.clear();
            batch.current_bytes = 0;
            batch.last_flush = Instant::now();
            return Ok(());
        }

        let mut handles = Vec::new();
        let mut group_count = 0;

        for (event_type, bytes) in event_groups {
            group_count += 1;
            let group_size = bytes.len();
            trace!("Processing group {}: {} {} events", group_count, group_size, event_type);

            let parts: Vec<&str> = self.stream_key.split(':').collect();
            let hub_host = if parts.len() >= 2 { parts[1] } else { "localhost" };
            let _shard_key = if parts.len() >= 5 { Some(parts[3]) } else { None };
            // Always use "evt" as the shard key for consistency with consumer
            let stream_key = crate::types::get_stream_key(hub_host, event_type, Some("evt"));
            let redis_stream = self.redis_stream.clone();
            let stream_maxlen = self.stream_maxlen;

            handles.push(tokio::spawn(async move {
                let result = redis_stream.add_batch_maxlen(&stream_key, stream_maxlen, bytes).await;
                if result.is_ok() {
                    trace!("Published {} events to {}", group_size, stream_key);
                }
                result
            }));
        }

        // Use a semaphore to limit concurrent Redis operations
        use futures::future::join_all;
        let results = join_all(handles).await;

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(_)) => {},
                Ok(Err(e)) => {
                    error!("Redis error for event group {}: {}", i + 1, e);
                    return Err(Error::InternalRedisError(e));
                },
                Err(e) => {
                    error!("Task error for event group {}: {}", i + 1, e);
                    return Err(Error::ProcessingError(format!("Task error: {}", e)));
                },
            }
        }

        // Update the last processed event ID
        if let Some(&last_idx) = keep_indices.last() {
            if let Some((last_event, _)) = batch.events.get(last_idx) {
                match self.redis.set_last_processed_event(&self.redis_key, last_event.id).await {
                    Ok(_) => {
                        trace!("Updated last processed event ID to {}", last_event.id);
                    },
                    Err(e) => {
                        error!("Failed to update last processed event ID: {}", e);
                        return Err(Error::InternalRedisError(e));
                    },
                }
            }
        }

        // Clean up and track timing
        batch.events.clear();
        batch.current_bytes = 0;
        batch.last_flush = Instant::now();

        let elapsed = start_time.elapsed();
        trace!("Finished flushing batch in {:?}", elapsed);

        // Update the last successful flush timestamp
        *self.last_successful_flush.write().await = Some(Instant::now());

        Ok(())
    }

    pub async fn stop(&self) -> Result<(), Error> {
        info!("Setting Hub subscriber shutdown flag...");
        *self.shutdown.write().await = true;
        info!("Hub subscriber shutdown signal sent");

        // Update the timestamp to avoid timeout reconnection attempts during shutdown
        *self.last_successful_flush.write().await = Some(Instant::now());

        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct SubscriberOptions {
    pub shard_index: Option<u64>,
    pub before_process: Option<PreProcessHandler>,
    pub after_process: Option<PostProcessHandler>,
    pub hub_config: Option<Arc<HubConfig>>,
    pub spam_filter_enabled: Option<bool>,
}
