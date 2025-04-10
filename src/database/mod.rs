//! Database module for PostgreSQL interactions
pub mod client;
pub mod error;
pub mod models;
pub mod repository_impl;

// Re-export most commonly used types
pub use client::Database;
pub use error::Error;
pub use repository_impl::{PostgresMessageRepository, PostgresUserProfileRepository};
