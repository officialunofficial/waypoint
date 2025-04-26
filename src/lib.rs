//! Waypoint - Snapchain synchronization tool
//!
//! Waypoint provides a streaming synchronization service combined with backfill capabilities
//! to process historical data from Snapchain.

// Core application modules
pub mod app;
pub mod backfill;
pub mod config;
pub mod core;
pub mod database;
pub mod error;
pub mod eth;
pub mod health;
pub mod hub;
pub mod metrics;
pub mod processor;
pub mod redis;
pub mod services;
pub mod types;

// Include all proto files in the src/proto directory with empty package name
pub mod proto {
    tonic::include_proto!("_"); // use "farcaster" as the package name
}
