use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, warn};

/// Circuit breaker state
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Circuit is open, requests fail fast
    HalfOpen, // Testing if service has recovered
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub timeout: Duration,
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self { failure_threshold: 5, timeout: Duration::from_secs(30), success_threshold: 2 }
    }
}

/// Circuit breaker implementation for Hub connections
#[derive(Debug)]
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitBreakerState>>,
    config: CircuitBreakerConfig,
}

#[derive(Debug)]
struct CircuitBreakerState {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        let state = CircuitBreakerState {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
        };

        Self { state: Arc::new(Mutex::new(state)), config }
    }

    /// Check if request should be allowed through the circuit breaker
    pub async fn should_allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap();

        match state.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout period has passed
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() >= self.config.timeout {
                        debug!("Circuit breaker transitioning to half-open state");
                        state.state = CircuitState::HalfOpen;
                        state.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            },
            CircuitState::HalfOpen => {
                // Allow limited requests to test if service has recovered
                true
            },
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();

        match state.state {
            CircuitState::Closed => {
                state.failure_count = 0;
            },
            CircuitState::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.config.success_threshold {
                    debug!("Circuit breaker closing - service has recovered");
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                    state.success_count = 0;
                    state.last_failure_time = None;
                }
            },
            CircuitState::Open => {
                // This shouldn't happen, but reset just in case
                state.state = CircuitState::Closed;
                state.failure_count = 0;
                state.success_count = 0;
                state.last_failure_time = None;
            },
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        let mut state = self.state.lock().unwrap();

        match state.state {
            CircuitState::Closed => {
                state.failure_count += 1;
                if state.failure_count >= self.config.failure_threshold {
                    warn!("Circuit breaker opening due to {} failures", state.failure_count);
                    state.state = CircuitState::Open;
                    state.last_failure_time = Some(Instant::now());
                }
            },
            CircuitState::HalfOpen => {
                warn!("Circuit breaker opening again - service still failing");
                state.state = CircuitState::Open;
                state.failure_count += 1;
                state.last_failure_time = Some(Instant::now());
                state.success_count = 0;
            },
            CircuitState::Open => {
                state.failure_count += 1;
                state.last_failure_time = Some(Instant::now());
            },
        }
    }

    /// Get current circuit breaker state
    pub fn get_state(&self) -> CircuitState {
        self.state.lock().unwrap().state.clone()
    }

    /// Execute operation with circuit breaker protection
    pub async fn execute<F, R, E>(&self, operation: F) -> Result<R, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<R, E>>,
    {
        if !self.should_allow_request().await {
            return Err(CircuitBreakerError::CircuitOpen);
        }

        match operation.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            },
            Err(error) => {
                self.record_failure();
                Err(CircuitBreakerError::OperationFailed(error))
            },
        }
    }

    /// Execute with retry and backoff
    pub async fn execute_with_retry<F, R, E>(
        &self,
        mut operation: F,
        max_retries: u32,
    ) -> Result<R, CircuitBreakerError<E>>
    where
        F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<R, E>> + Send>>,
        E: std::fmt::Debug,
    {
        let mut retry_count = 0;

        loop {
            if !self.should_allow_request().await {
                return Err(CircuitBreakerError::CircuitOpen);
            }

            match operation().await {
                Ok(result) => {
                    self.record_success();
                    return Ok(result);
                },
                Err(error) => {
                    self.record_failure();
                    retry_count += 1;

                    if retry_count >= max_retries {
                        return Err(CircuitBreakerError::OperationFailed(error));
                    }

                    // Exponential backoff with jitter
                    let base_delay = 100 * (1 << retry_count.min(5));
                    let jitter = (retry_count * 37) % 100; // Simple deterministic jitter
                    let delay = Duration::from_millis((base_delay + jitter) as u64);

                    debug!(
                        "Retrying operation in {:?} (attempt {}/{})",
                        delay,
                        retry_count + 1,
                        max_retries
                    );
                    sleep(delay).await;
                },
            }
        }
    }
}

/// Circuit breaker error types
#[derive(Debug)]
pub enum CircuitBreakerError<E> {
    CircuitOpen,
    OperationFailed(E),
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::CircuitOpen => write!(f, "Circuit breaker is open"),
            CircuitBreakerError::OperationFailed(e) => write!(f, "Operation failed: {}", e),
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
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_circuit_breaker_basic_flow() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout: Duration::from_millis(100),
            success_threshold: 1,
        };
        let cb = CircuitBreaker::new(config);

        // Initial state should be closed
        assert_eq!(cb.get_state(), CircuitState::Closed);

        // Record failures to open circuit
        cb.record_failure();
        assert_eq!(cb.get_state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.get_state(), CircuitState::Open);

        // Should not allow requests when open
        assert!(!cb.should_allow_request().await);

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Should transition to half-open
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state(), CircuitState::HalfOpen);

        // Record success to close circuit
        cb.record_success();
        assert_eq!(cb.get_state(), CircuitState::Closed);
    }
}
