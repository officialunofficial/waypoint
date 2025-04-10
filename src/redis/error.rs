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
}
