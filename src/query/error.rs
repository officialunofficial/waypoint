//! Query-layer error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("data access error: {0}")]
    DataAccess(#[from] crate::core::data_context::DataAccessError),

    #[error("processing error: {0}")]
    Processing(String),
}

pub type QueryResult<T> = Result<T, QueryError>;
