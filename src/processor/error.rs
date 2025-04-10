use crate::hub::error;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Processor error: {0}")]
    ProcessorError(String),

    #[error("Hub error: {0}")]
    HubError(#[from] error::Error),

    #[error("Redis error: {0}")]
    RedisError(#[from] crate::redis::error::Error),

    #[error("Database error: {0}")]
    DatabaseError(#[from] crate::database::error::Error),
}
