//! Core domain modules
pub mod data_context;
pub mod normalize;
pub mod types;
pub mod util;

#[cfg(test)]
mod data_context_tests;

// Re-export common types
pub use data_context::{
    DataAccessError, DataContext, DataContextBuilder, Database, HubClient, Result,
};
pub use types::{FARCASTER_EPOCH, Fid, Message, MessageId, MessageType};
