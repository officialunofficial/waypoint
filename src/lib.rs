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

// Include proto definitions - either from build-time generation or pre-generated file
#[cfg(build_protos)]
pub mod proto {
    tonic::include_proto!("_");
}

#[cfg(not(build_protos))]
#[path = "proto.gen.rs"]
pub mod proto;
