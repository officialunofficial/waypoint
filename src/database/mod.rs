//! Database module for PostgreSQL interactions
pub mod client;
pub mod error;
pub mod models;
pub mod providers;

// Re-export most commonly used types
pub use client::Database;
pub use error::Error;
pub use providers::PostgresDatabaseClient;
