//! Repository traits for domain abstractions
use crate::core::types::{Fid, Message, MessageId, MessageType};
use async_trait::async_trait;
use thiserror::Error;

/// Repository error type
#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Message not found: {0}")]
    NotFound(String),

    #[error("Search engine error: {0}")]
    Search(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Result type for repository operations
pub type Result<T> = std::result::Result<T, RepositoryError>;

/// Generic repository trait for message operations
#[async_trait]
pub trait MessageRepository: Send + Sync {
    /// Store a message
    async fn store_message(&self, message: Message) -> Result<()>;

    /// Get a message by ID
    async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message>;

    /// Get messages by FID and type
    async fn get_messages_by_fid(
        &self,
        fid: Fid,
        message_type: MessageType,
        limit: usize,
        cursor: Option<MessageId>,
    ) -> Result<Vec<Message>>;

    /// Delete a message
    async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()>;
}

/// Stream repository for publishing and consuming event streams
#[async_trait]
pub trait StreamRepository: Send + Sync {
    /// Publish a message to a stream
    async fn publish(&self, message: Message) -> Result<()>;

    /// Consume messages from a stream
    async fn consume(
        &self,
        message_type: MessageType,
        consumer_group: &str,
        consumer_name: &str,
        count: usize,
    ) -> Result<Vec<Message>>;

    /// Acknowledge processing of a message
    async fn ack(
        &self,
        message_type: MessageType,
        consumer_group: &str,
        id: &MessageId,
    ) -> Result<()>;

    /// Claim stale messages for reprocessing
    async fn claim_stale(
        &self,
        message_type: MessageType,
        consumer_group: &str,
        consumer_name: &str,
        min_idle_time: std::time::Duration,
        count: usize,
    ) -> Result<Vec<Message>>;
}

/// User profile repository for user data operations
#[async_trait]
pub trait UserProfileRepository: Send + Sync {
    /// Get a user profile by FID
    async fn get_profile(&self, fid: Fid) -> Result<Option<serde_json::Value>>;

    /// Update a user profile
    async fn update_profile(&self, fid: Fid, profile: serde_json::Value) -> Result<()>;

    /// Search for user profiles
    async fn search_profiles(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<serde_json::Value>>;
}
