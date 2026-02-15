//! Query-layer error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("data access error: {0}")]
    DataAccess(#[from] crate::core::data_context::DataAccessError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type QueryResult<T> = Result<T, QueryError>;
