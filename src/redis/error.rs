#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    #[error("Redis error: {0}")]
    RedisError(#[from] bb8_redis::redis::RedisError),
    #[error("Pool error: {0}")]
    PoolError(String),
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Type conversion error: {0}")]
    TypeConversionError(String),
    #[error("Consumer not found error: {0}")]
    ConsumerNotFoundError(String),
    #[error("Stream not found error: {0}")]
    StreamNotFoundError(String),
    #[error("Invalid stream format: {0}")]
    InvalidStreamFormat(String),
    #[error("Operation timeout: {0}")]
    OperationTimeout(String),
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    #[error("Connection pool exhausted")]
    PoolExhausted,
    #[error("Circuit breaker open")]
    CircuitBreakerOpen,
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Backpressure detected - service degraded")]
    BackpressureDetected,
}

impl Error {
    /// Check if this error is recoverable (should retry)
    pub fn is_recoverable(&self) -> bool {
        match self {
            Error::RedisError(redis_err) => {
                // Check if it's a connection error that might recover
                matches!(redis_err.kind(), 
                    bb8_redis::redis::ErrorKind::IoError | 
                    bb8_redis::redis::ErrorKind::BusyLoadingError | 
                    bb8_redis::redis::ErrorKind::TryAgain | 
                    bb8_redis::redis::ErrorKind::ClusterDown
                )
            }
            Error::PoolError(_) => true,
            Error::OperationTimeout(_) => true,
            Error::PoolExhausted => true,
            Error::CircuitBreakerOpen => false, // Circuit breaker handles its own recovery
            Error::RateLimitExceeded => true,
            Error::BackpressureDetected => true,
            // Data/configuration errors are generally not recoverable
            _ => false,
        }
    }

    /// Check if this error should trigger circuit breaker
    pub fn should_trigger_circuit_breaker(&self) -> bool {
        match self {
            Error::RedisError(redis_err) => {
                matches!(redis_err.kind(), 
                    bb8_redis::redis::ErrorKind::IoError | 
                    bb8_redis::redis::ErrorKind::AuthenticationFailed | 
                    bb8_redis::redis::ErrorKind::ClientError
                )
            }
            Error::PoolError(_) => true,
            Error::OperationTimeout(_) => true,
            Error::PoolExhausted => true,
            _ => false,
        }
    }

    /// Get suggested retry delay in milliseconds
    pub fn suggested_retry_delay(&self) -> Option<u64> {
        match self {
            Error::PoolExhausted => Some(500),
            Error::RateLimitExceeded => Some(1000),
            Error::BackpressureDetected => Some(200),
            Error::OperationTimeout(_) => Some(100),
            Error::RedisError(redis_err) => {
                match redis_err.kind() {
                    bb8_redis::redis::ErrorKind::BusyLoadingError => Some(1000),
                    bb8_redis::redis::ErrorKind::TryAgain => Some(100),
                    bb8_redis::redis::ErrorKind::IoError => Some(500),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
