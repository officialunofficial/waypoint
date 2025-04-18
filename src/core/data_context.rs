//! Data access abstractions and context
use crate::core::types::{Fid, Message, MessageId, MessageType};
use async_trait::async_trait;
use thiserror::Error;

/// Error type for data access operations
#[derive(Error, Debug)]
pub enum DataAccessError {
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

    #[error("Hub client error: {0}")]
    HubClient(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Result type for data access operations
pub type Result<T> = std::result::Result<T, DataAccessError>;

/// Generic trait for database operations
#[async_trait]
pub trait Database: Send + Sync {
    /// Get a message by ID and type
    async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message>;

    /// Get messages by FID
    async fn get_messages_by_fid(
        &self,
        fid: Fid,
        message_type: MessageType,
        limit: usize,
        cursor: Option<MessageId>,
    ) -> Result<Vec<Message>>;

    /// Store a message
    async fn store_message(&self, message: Message) -> Result<()>;

    /// Delete a message
    async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()>;
}

/// Generic trait for hub operations
#[async_trait]
pub trait HubClient: Send + Sync {
    /// Get user data by FID
    async fn get_user_data_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>>;

    /// Get specific user data type
    async fn get_user_data(&self, fid: Fid, data_type: &str) -> Result<Option<Message>>;

    /// Get username proofs
    async fn get_username_proofs_by_fid(&self, fid: Fid) -> Result<Vec<Message>>;

    /// Get verifications
    async fn get_verifications_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>>;

    /// Get casts by FID
    async fn get_casts_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>>;

    /// Get a specific cast by ID
    async fn get_cast(&self, fid: Fid, hash: &[u8]) -> Result<Option<Message>>;

    /// Get casts mentioning an FID
    async fn get_casts_by_mention(&self, fid: Fid, limit: usize) -> Result<Vec<Message>>;

    /// Get casts by parent
    async fn get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash: &[u8],
        limit: usize,
    ) -> Result<Vec<Message>>;

    /// Get casts by parent URL
    async fn get_casts_by_parent_url(&self, parent_url: &str, limit: usize)
    -> Result<Vec<Message>>;

    /// Get all casts by FID with timestamp filtering
    async fn get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<Message>>;
}

/// Generic data access context
#[derive(Debug, Clone)]
pub struct DataContext<DB, HC> {
    database: Option<DB>,
    hub_client: Option<HC>,
}

impl<DB, HC> DataContext<DB, HC>
where
    DB: Database,
    HC: HubClient,
{
    /// Get user data by FID, using Hub if available, falling back to database
    pub async fn get_user_data_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        // Try hub client first if available
        if let Some(hub) = &self.hub_client {
            match hub.get_user_data_by_fid(fid, limit).await {
                Ok(data) if !data.is_empty() => return Ok(data),
                _ => {},
            }
        }

        Err(DataAccessError::Other("No data source available".to_string()))
    }

    /// Get specific user data, with Hub priority
    pub async fn get_user_data(&self, fid: Fid, data_type: &str) -> Result<Option<Message>> {
        if let Some(hub) = &self.hub_client {
            match hub.get_user_data(fid, data_type).await {
                Ok(data) => return Ok(data),
                Err(_) => {}, // Fall through to database
            }
        }

        Err(DataAccessError::Other("No data source available".to_string()))
    }

    /// Get username proofs
    pub async fn get_username_proofs_by_fid(&self, fid: Fid) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_username_proofs_by_fid(fid).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Get verifications
    pub async fn get_verifications_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_verifications_by_fid(fid, limit).await;
        }

        if let Some(db) = &self.database {
            return db.get_messages_by_fid(fid, MessageType::Verification, limit, None).await;
        }

        Err(DataAccessError::Other("No data source available".to_string()))
    }

    /// Get casts by FID
    pub async fn get_casts_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_casts_by_fid(fid, limit).await;
        }

        if let Some(db) = &self.database {
            return db.get_messages_by_fid(fid, MessageType::Cast, limit, None).await;
        }

        Err(DataAccessError::Other("No data source available".to_string()))
    }

    /// Get a specific cast by ID
    pub async fn get_cast(&self, fid: Fid, hash: &[u8]) -> Result<Option<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_cast(fid, hash).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Get casts mentioning an FID
    pub async fn get_casts_by_mention(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_casts_by_mention(fid, limit).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Get casts by parent
    pub async fn get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash: &[u8],
        limit: usize,
    ) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_casts_by_parent(parent_fid, parent_hash, limit).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Get casts by parent URL
    pub async fn get_casts_by_parent_url(
        &self,
        parent_url: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_casts_by_parent_url(parent_url, limit).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Get all casts by FID with timestamp filtering
    pub async fn get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<Message>> {
        if let Some(hub) = &self.hub_client {
            return hub.get_all_casts_by_fid(fid, limit, start_time, end_time).await;
        }

        Err(DataAccessError::Other("Hub client not available".to_string()))
    }

    /// Generic database operation
    pub fn database(&self) -> Result<&DB> {
        self.database
            .as_ref()
            .ok_or_else(|| DataAccessError::Other("Database not configured".to_string()))
    }

    /// Generic hub operation
    pub fn hub(&self) -> Result<&HC> {
        self.hub_client
            .as_ref()
            .ok_or_else(|| DataAccessError::Other("Hub client not configured".to_string()))
    }
}

/// Builder for the data context
pub struct DataContextBuilder<DB, HC> {
    database: Option<DB>,
    hub_client: Option<HC>,
}

impl<DB, HC> Default for DataContextBuilder<DB, HC> {
    fn default() -> Self {
        Self { database: None, hub_client: None }
    }
}

impl<DB, HC> DataContextBuilder<DB, HC> {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a database
    pub fn with_database(mut self, database: DB) -> Self {
        self.database = Some(database);
        self
    }

    /// Add a Hub client
    pub fn with_hub_client(mut self, hub_client: HC) -> Self {
        self.hub_client = Some(hub_client);
        self
    }

    /// Build the context
    pub fn build(self) -> DataContext<DB, HC> {
        DataContext { database: self.database, hub_client: self.hub_client }
    }
}
