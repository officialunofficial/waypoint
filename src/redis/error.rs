use error_stack::{Context, Report};
use std::fmt::{self, Display};

/// Redis operation error context
#[derive(Debug)]
pub struct RedisError;

impl Display for RedisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Redis operation failed")
    }
}

impl Context for RedisError {}

/// Specific Redis error types
#[derive(Debug)]
pub enum RedisErrorKind {
    Deserialization,
    Serialization,
    Connection,
    Pool,
    TypeConversion,
    ConsumerNotFound,
    StreamNotFound,
    InvalidStreamFormat,
    OperationTimeout,
    Configuration,
    PoolExhausted,
    CircuitBreakerOpen,
    RateLimitExceeded,
    BackpressureDetected,
}

impl Display for RedisErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedisErrorKind::Deserialization => write!(f, "Failed to deserialize Redis data"),
            RedisErrorKind::Serialization => write!(f, "Failed to serialize data for Redis"),
            RedisErrorKind::Connection => write!(f, "Redis connection error"),
            RedisErrorKind::Pool => write!(f, "Connection pool error"),
            RedisErrorKind::TypeConversion => write!(f, "Type conversion error"),
            RedisErrorKind::ConsumerNotFound => write!(f, "Consumer not found"),
            RedisErrorKind::StreamNotFound => write!(f, "Stream not found"),
            RedisErrorKind::InvalidStreamFormat => write!(f, "Invalid stream format"),
            RedisErrorKind::OperationTimeout => write!(f, "Operation timed out"),
            RedisErrorKind::Configuration => write!(f, "Configuration error"),
            RedisErrorKind::PoolExhausted => write!(f, "Connection pool exhausted"),
            RedisErrorKind::CircuitBreakerOpen => write!(f, "Circuit breaker is open"),
            RedisErrorKind::RateLimitExceeded => write!(f, "Rate limit exceeded"),
            RedisErrorKind::BackpressureDetected => {
                write!(f, "Backpressure detected - service degraded")
            },
        }
    }
}

/// Result type for Redis operations
pub type Result<T> = error_stack::Result<T, RedisError>;

/// Helper functions for Redis errors
pub struct ErrorHelpers;

impl ErrorHelpers {
    /// Check if an error is recoverable (should retry)
    pub fn is_recoverable(report: &Report<RedisError>) -> bool {
        // Check if any attachment indicates a recoverable error
        for frame in report.frames() {
            if let Some(kind) = frame.downcast_ref::<RedisErrorKind>() {
                match kind {
                    RedisErrorKind::Connection
                    | RedisErrorKind::Pool
                    | RedisErrorKind::OperationTimeout
                    | RedisErrorKind::PoolExhausted
                    | RedisErrorKind::RateLimitExceeded
                    | RedisErrorKind::BackpressureDetected => return true,
                    RedisErrorKind::CircuitBreakerOpen => return false,
                    _ => continue,
                }
            }

            // Check for Redis-specific errors
            if let Some(redis_err) = frame.downcast_ref::<fred::error::Error>() {
                // Fred errors are simpler - most are recoverable
                match redis_err.kind() {
                    fred::error::ErrorKind::IO
                    | fred::error::ErrorKind::Timeout
                    | fred::error::ErrorKind::Backpressure => return true,
                    fred::error::ErrorKind::Auth | fred::error::ErrorKind::Config => {
                        return false;
                    },
                    _ => continue,
                }
            }
        }
        false
    }

    /// Check if error should trigger circuit breaker
    pub fn should_trigger_circuit_breaker(report: &Report<RedisError>) -> bool {
        for frame in report.frames() {
            if let Some(kind) = frame.downcast_ref::<RedisErrorKind>() {
                match kind {
                    RedisErrorKind::Connection
                    | RedisErrorKind::Pool
                    | RedisErrorKind::OperationTimeout
                    | RedisErrorKind::PoolExhausted => return true,
                    _ => continue,
                }
            }

            if let Some(redis_err) = frame.downcast_ref::<fred::error::Error>() {
                match redis_err.kind() {
                    fred::error::ErrorKind::IO
                    | fred::error::ErrorKind::Auth
                    | fred::error::ErrorKind::Timeout => return true,
                    _ => continue,
                }
            }
        }
        false
    }

    /// Get suggested retry delay in milliseconds
    pub fn suggested_retry_delay(report: &Report<RedisError>) -> Option<u64> {
        for frame in report.frames() {
            if let Some(kind) = frame.downcast_ref::<RedisErrorKind>() {
                match kind {
                    RedisErrorKind::PoolExhausted => return Some(500),
                    RedisErrorKind::RateLimitExceeded => return Some(1000),
                    RedisErrorKind::BackpressureDetected => return Some(200),
                    RedisErrorKind::OperationTimeout => return Some(100),
                    _ => continue,
                }
            }

            if let Some(redis_err) = frame.downcast_ref::<fred::error::Error>() {
                match redis_err.kind() {
                    fred::error::ErrorKind::Backpressure => return Some(1000),
                    fred::error::ErrorKind::Timeout => return Some(500),
                    fred::error::ErrorKind::IO => return Some(500),
                    _ => continue,
                }
            }
        }
        None
    }
}

/// Extension trait for converting Redis errors
pub trait IntoRedisError<T> {
    fn into_redis_error(self, kind: RedisErrorKind) -> Result<T>;
}

impl<T, E> IntoRedisError<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn into_redis_error(self, kind: RedisErrorKind) -> Result<T> {
        self.map_err(|e| {
            Report::new(RedisError).attach_printable(kind).attach_printable(e.to_string())
        })
    }
}

// Re-export for convenience
pub use RedisErrorKind as ErrorKind;

// Temporary compatibility shim for migration
// TODO: Remove this once all code is migrated to error-stack
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    #[error("Redis error: {0}")]
    RedisError(#[from] fred::error::Error),
    #[error("Pool error: {0}")]
    PoolError(String),
}

impl From<Error> for Report<RedisError> {
    fn from(err: Error) -> Self {
        let kind = match &err {
            Error::DeserializationError(_) => RedisErrorKind::Deserialization,
            Error::RedisError(_) => RedisErrorKind::Connection,
            Error::PoolError(_) => RedisErrorKind::Pool,
        };

        Report::new(RedisError).attach_printable(kind).attach_printable(err.to_string())
    }
}
