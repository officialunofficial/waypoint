//! Core domain modules
pub mod normalize;
pub mod repository;
pub mod types;
pub mod util;

#[cfg(test)]
mod repository_tests;

// Re-export common types
pub use repository::{
    MessageRepository, RepositoryError, Result, StreamRepository, UserProfileRepository,
};
pub use types::{FARCASTER_EPOCH, Fid, Message, MessageId, MessageType};
