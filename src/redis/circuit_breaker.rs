//! Circuit breaker for Redis operations
//!
//! Protects against cascading failures when Redis becomes unavailable or slow.
//! States:
//! - Closed: Normal operation, requests pass through
//! - Open: Circuit tripped, requests fail fast
//! - HalfOpen: Testing recovery, limited requests allowed

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - all requests pass through
    Closed,
    /// Circuit is open - requests fail fast
    Open,
    /// Testing if Redis has recovered - limited requests allowed
    HalfOpen,
}

/// Configuration for the Redis circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening the circuit
    pub failure_threshold: u32,
    /// Time to wait before transitioning from Open to HalfOpen
    pub open_timeout: Duration,
    /// Number of successful requests needed to close the circuit from HalfOpen
    pub success_threshold: u32,
    /// Timeout for considering an operation slow (triggers partial failure count)
    pub slow_call_threshold: Duration,
    /// Ratio of slow calls that triggers circuit open (0.0 - 1.0)
    pub slow_call_rate_threshold: f64,
    /// Minimum number of calls before evaluating slow call rate
    pub minimum_calls_for_rate: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_timeout: Duration::from_secs(30),
            success_threshold: 3,
            slow_call_threshold: Duration::from_secs(5),
            slow_call_rate_threshold: 0.5,
            minimum_calls_for_rate: 10,
        }
    }
}

/// Internal state tracking for the circuit breaker
struct InternalState {
    state: CircuitState,
    last_state_change: Instant,
    last_failure_time: Option<Instant>,
}

/// Circuit breaker for Redis operations
///
/// Thread-safe implementation using atomic counters and RwLock for state transitions.
pub struct RedisCircuitBreaker {
    config: CircuitBreakerConfig,
    /// Current circuit state (protected by RwLock for state transitions)
    internal_state: RwLock<InternalState>,
    /// Atomic counters for lock-free metrics
    failure_count: AtomicU32,
    success_count: AtomicU32,
    slow_call_count: AtomicU32,
    total_call_count: AtomicU32,
    /// Metrics for monitoring
    total_opens: AtomicU64,
    total_half_opens: AtomicU64,
    total_rejections: AtomicU64,
}

impl RedisCircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            internal_state: RwLock::new(InternalState {
                state: CircuitState::Closed,
                last_state_change: Instant::now(),
                last_failure_time: None,
            }),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            slow_call_count: AtomicU32::new(0),
            total_call_count: AtomicU32::new(0),
            total_opens: AtomicU64::new(0),
            total_half_opens: AtomicU64::new(0),
            total_rejections: AtomicU64::new(0),
        }
    }

    /// Create a circuit breaker with default configuration
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Check if a request should be allowed through
    pub async fn should_allow_request(&self) -> bool {
        let mut state = self.internal_state.write().await;

        match state.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has passed
                if state.last_state_change.elapsed() >= self.config.open_timeout {
                    debug!("Redis circuit breaker transitioning to half-open");
                    state.state = CircuitState::HalfOpen;
                    state.last_state_change = Instant::now();
                    self.success_count.store(0, Ordering::SeqCst);
                    self.total_half_opens.fetch_add(1, Ordering::Relaxed);
                    true
                } else {
                    self.total_rejections.fetch_add(1, Ordering::Relaxed);
                    false
                }
            },
            CircuitState::HalfOpen => {
                // Allow limited requests to test recovery
                true
            },
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self, duration: Duration) {
        // Check for slow calls
        if duration >= self.config.slow_call_threshold {
            self.slow_call_count.fetch_add(1, Ordering::SeqCst);
        }
        self.total_call_count.fetch_add(1, Ordering::SeqCst);

        let mut state = self.internal_state.write().await;

        match state.state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);

                // Check slow call rate
                let total = self.total_call_count.load(Ordering::SeqCst);
                if total >= self.config.minimum_calls_for_rate {
                    let slow = self.slow_call_count.load(Ordering::SeqCst);
                    let slow_rate = slow as f64 / total as f64;
                    if slow_rate >= self.config.slow_call_rate_threshold {
                        warn!(
                            "Redis circuit breaker opening due to slow call rate: {:.2}%",
                            slow_rate * 100.0
                        );
                        self.transition_to_open(&mut state);
                    }
                }
            },
            CircuitState::HalfOpen => {
                let count = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.config.success_threshold {
                    info!("Redis circuit breaker closing - Redis has recovered");
                    state.state = CircuitState::Closed;
                    state.last_state_change = Instant::now();
                    state.last_failure_time = None;
                    self.reset_counters();
                }
            },
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
                state.state = CircuitState::Closed;
                state.last_state_change = Instant::now();
                self.reset_counters();
            },
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        self.total_call_count.fetch_add(1, Ordering::SeqCst);

        let mut state = self.internal_state.write().await;

        match state.state {
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.config.failure_threshold {
                    warn!("Redis circuit breaker opening due to {} consecutive failures", count);
                    self.transition_to_open(&mut state);
                }
            },
            CircuitState::HalfOpen => {
                warn!("Redis circuit breaker re-opening - Redis still failing");
                self.transition_to_open(&mut state);
            },
            CircuitState::Open => {
                // Already open, just update failure time
                state.last_failure_time = Some(Instant::now());
            },
        }
    }

    /// Get the current circuit state
    pub async fn get_state(&self) -> CircuitState {
        self.internal_state.read().await.state
    }

    /// Get circuit breaker metrics
    pub fn get_metrics(&self) -> CircuitBreakerMetrics {
        CircuitBreakerMetrics {
            failure_count: self.failure_count.load(Ordering::Relaxed),
            success_count: self.success_count.load(Ordering::Relaxed),
            slow_call_count: self.slow_call_count.load(Ordering::Relaxed),
            total_call_count: self.total_call_count.load(Ordering::Relaxed),
            total_opens: self.total_opens.load(Ordering::Relaxed),
            total_half_opens: self.total_half_opens.load(Ordering::Relaxed),
            total_rejections: self.total_rejections.load(Ordering::Relaxed),
        }
    }

    /// Execute an operation with circuit breaker protection
    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        if !self.should_allow_request().await {
            return Err(CircuitBreakerError::CircuitOpen);
        }

        let start = Instant::now();
        match operation.await {
            Ok(result) => {
                self.record_success(start.elapsed()).await;
                Ok(result)
            },
            Err(error) => {
                self.record_failure().await;
                Err(CircuitBreakerError::OperationFailed(error))
            },
        }
    }

    /// Execute with automatic retry on failure
    pub async fn execute_with_retry<F, Fut, T, E>(
        &self,
        mut operation: F,
        max_retries: u32,
        retry_delay: Duration,
    ) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut attempts = 0;

        loop {
            if !self.should_allow_request().await {
                return Err(CircuitBreakerError::CircuitOpen);
            }

            let start = Instant::now();
            match operation().await {
                Ok(result) => {
                    self.record_success(start.elapsed()).await;
                    return Ok(result);
                },
                Err(error) => {
                    self.record_failure().await;
                    attempts += 1;

                    if attempts >= max_retries {
                        return Err(CircuitBreakerError::OperationFailed(error));
                    }

                    // Check if circuit is now open
                    if self.get_state().await == CircuitState::Open {
                        return Err(CircuitBreakerError::CircuitOpen);
                    }

                    // Exponential backoff with jitter
                    let backoff = retry_delay * (1 << attempts.min(5));
                    let jitter = Duration::from_millis((attempts as u64 * 17) % 100);
                    debug!(
                        "Redis operation failed, retrying in {:?} (attempt {}/{})",
                        backoff + jitter,
                        attempts,
                        max_retries
                    );
                    tokio::time::sleep(backoff + jitter).await;
                },
            }
        }
    }

    /// Force the circuit to open (for testing or manual intervention)
    pub async fn force_open(&self) {
        let mut state = self.internal_state.write().await;
        warn!("Redis circuit breaker force-opened");
        self.transition_to_open(&mut state);
    }

    /// Force the circuit to close (for testing or manual intervention)
    pub async fn force_close(&self) {
        let mut state = self.internal_state.write().await;
        info!("Redis circuit breaker force-closed");
        state.state = CircuitState::Closed;
        state.last_state_change = Instant::now();
        state.last_failure_time = None;
        self.reset_counters();
    }

    /// Transition to open state
    fn transition_to_open(&self, state: &mut InternalState) {
        state.state = CircuitState::Open;
        state.last_state_change = Instant::now();
        state.last_failure_time = Some(Instant::now());
        self.total_opens.fetch_add(1, Ordering::Relaxed);
        self.success_count.store(0, Ordering::SeqCst);
    }

    /// Reset all counters
    fn reset_counters(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        self.slow_call_count.store(0, Ordering::SeqCst);
        self.total_call_count.store(0, Ordering::SeqCst);
    }
}

/// Metrics from the circuit breaker
#[derive(Debug, Clone, Default)]
pub struct CircuitBreakerMetrics {
    pub failure_count: u32,
    pub success_count: u32,
    pub slow_call_count: u32,
    pub total_call_count: u32,
    pub total_opens: u64,
    pub total_half_opens: u64,
    pub total_rejections: u64,
}

/// Error types for circuit breaker operations
#[derive(Debug)]
pub enum CircuitBreakerError<E> {
    /// Circuit is open, request was rejected
    CircuitOpen,
    /// Operation failed with the underlying error
    OperationFailed(E),
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::CircuitOpen => write!(f, "Redis circuit breaker is open"),
            CircuitBreakerError::OperationFailed(e) => write!(f, "Redis operation failed: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CircuitBreakerError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CircuitBreakerError::CircuitOpen => None,
            CircuitBreakerError::OperationFailed(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_initial_state() {
        let cb = RedisCircuitBreaker::with_defaults();
        assert_eq!(cb.get_state().await, CircuitState::Closed);
        assert!(cb.should_allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_opens_after_failures() {
        let config = CircuitBreakerConfig { failure_threshold: 3, ..Default::default() };
        let cb = RedisCircuitBreaker::new(config);

        // Record failures
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
        assert!(!cb.should_allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_transitions_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            open_timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let cb = RedisCircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Should transition to half-open
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            open_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let cb = RedisCircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;

        // Wait and transition to half-open
        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.should_allow_request().await;

        // Record successes
        cb.record_success(Duration::from_millis(10)).await;
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        cb.record_success(Duration::from_millis(10)).await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_reopens_on_failure_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            open_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let cb = RedisCircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;

        // Wait and transition to half-open
        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.should_allow_request().await;
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        // Failure in half-open should reopen
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_success_resets_failure_count() {
        let config = CircuitBreakerConfig { failure_threshold: 3, ..Default::default() };
        let cb = RedisCircuitBreaker::new(config);

        cb.record_failure().await;
        cb.record_failure().await;
        cb.record_success(Duration::from_millis(10)).await;

        // Failure count should be reset, so 2 more failures shouldn't open
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        // But one more should
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_execute_with_circuit_breaker() {
        let cb = RedisCircuitBreaker::with_defaults();

        // Successful operation
        let result = cb.execute(async { Ok::<i32, &str>(42) }).await;
        assert!(matches!(result, Ok(42)));

        // Failed operation
        let result = cb.execute(async { Err::<i32, &str>("error") }).await;
        assert!(matches!(result, Err(CircuitBreakerError::OperationFailed("error"))));
    }

    #[tokio::test]
    async fn test_metrics() {
        let config = CircuitBreakerConfig { failure_threshold: 3, ..Default::default() };
        let cb = RedisCircuitBreaker::new(config);

        cb.record_success(Duration::from_millis(10)).await;
        cb.record_failure().await;
        cb.record_failure().await;
        cb.record_failure().await;

        let metrics = cb.get_metrics();
        assert_eq!(metrics.total_call_count, 4);
        assert_eq!(metrics.failure_count, 3);
        assert_eq!(metrics.total_opens, 1);
    }

    #[tokio::test]
    async fn test_force_open_and_close() {
        let cb = RedisCircuitBreaker::with_defaults();

        cb.force_open().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
        assert!(!cb.should_allow_request().await);

        cb.force_close().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);
        assert!(cb.should_allow_request().await);
    }
}
