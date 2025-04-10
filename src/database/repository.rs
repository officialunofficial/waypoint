//! Database repository implementations
use crate::{
    core::{
        Fid, Message, MessageId, MessageType,
        repository::{MessageRepository, RepositoryError, Result, UserProfileRepository},
    },
    database::client::Database,
};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info};

/// PostgreSQL implementation of the message repository
pub struct PostgresMessageRepository {
    database: Arc<Database>,
}

impl PostgresMessageRepository {
    /// Create a new PostgreSQL message repository
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

#[async_trait]
impl MessageRepository for PostgresMessageRepository {
    async fn store_message(&self, message: Message) -> Result<()> {
        let table_name = match message.message_type {
            MessageType::Cast => "casts",
            MessageType::Reaction => "reactions",
            MessageType::Link => "links",
            MessageType::Verification => "verifications",
            MessageType::UserData => "user_data",
            MessageType::UsernameProof => "username_proofs",
            MessageType::OnchainSigner => "onchain_events",
            MessageType::OnchainSignerMigrated => "onchain_events",
            MessageType::OnchainIdRegister => "onchain_events",
            MessageType::OnchainStorageRent => "onchain_events",
        };
        
        // Build SQL query
        let sql = format!(
            "INSERT INTO {} (id, payload) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET payload = $2",
            table_name
        );
        
        // Execute query
        match self.database.execute(&sql, &[&message.id.value(), &message.payload]).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to store message: {}", e);
                Err(RepositoryError::Database(e))
            }
        }
    }
    
    async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message> {
        let table_name = match message_type {
            MessageType::Cast => "casts",
            MessageType::Reaction => "reactions",
            MessageType::Link => "links",
            MessageType::Verification => "verifications",
            MessageType::UserData => "user_data",
            MessageType::UsernameProof => "username_proofs",
            MessageType::OnchainSigner => "onchain_events",
            MessageType::OnchainSignerMigrated => "onchain_events",
            MessageType::OnchainIdRegister => "onchain_events",
            MessageType::OnchainStorageRent => "onchain_events",
        };
        
        // Build SQL query
        let sql = format!("SELECT id, payload FROM {} WHERE id = $1", table_name);
        
        // Execute query
        let row = self.database.query_one(&sql, &[&id.value()]).await
            .map_err(|e| {
                if e.to_string().contains("no rows") {
                    RepositoryError::NotFound(format!("Message with ID {} not found", id))
                } else {
                    RepositoryError::Database(e)
                }
            })?;
        
        // Extract data
        let id: String = row.get(0);
        let payload: Vec<u8> = row.get(1);
        
        Ok(Message::new(id, message_type, payload))
    }
    
    async fn get_messages_by_fid(
        &self,
        fid: Fid,
        message_type: MessageType,
        limit: usize,
        cursor: Option<MessageId>,
    ) -> Result<Vec<Message>> {
        let table_name = match message_type {
            MessageType::Cast => "casts",
            MessageType::Reaction => "reactions",
            MessageType::Link => "links",
            MessageType::Verification => "verifications",
            MessageType::UserData => "user_data",
            MessageType::UsernameProof => "username_proofs",
            MessageType::OnchainSigner => "onchain_events",
            MessageType::OnchainSignerMigrated => "onchain_events",
            MessageType::OnchainIdRegister => "onchain_events",
            MessageType::OnchainStorageRent => "onchain_events",
        };
        
        // Build SQL query with cursor-based pagination
        let sql = match cursor {
            Some(cursor) => format!(
                "SELECT id, payload FROM {} WHERE fid = $1 AND id > $2 ORDER BY id LIMIT $3",
                table_name
            ),
            None => format!(
                "SELECT id, payload FROM {} WHERE fid = $1 ORDER BY id LIMIT $2",
                table_name
            ),
        };
        
        // Execute query
        let rows = match cursor {
            Some(cursor) => {
                self.database.query(&sql, &[&fid.value(), &cursor.value(), &(limit as i64)]).await
            }
            None => {
                self.database.query(&sql, &[&fid.value(), &(limit as i64)]).await
            }
        }.map_err(|e| RepositoryError::Database(e))?;
        
        // Process results
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get(0);
            let payload: Vec<u8> = row.get(1);
            messages.push(Message::new(id, message_type, payload));
        }
        
        Ok(messages)
    }
    
    async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()> {
        let table_name = match message_type {
            MessageType::Cast => "casts",
            MessageType::Reaction => "reactions",
            MessageType::Link => "links",
            MessageType::Verification => "verifications",
            MessageType::UserData => "user_data",
            MessageType::UsernameProof => "username_proofs",
            MessageType::OnchainSigner => "onchain_events",
            MessageType::OnchainSignerMigrated => "onchain_events",
            MessageType::OnchainIdRegister => "onchain_events",
            MessageType::OnchainStorageRent => "onchain_events",
        };
        
        // Build SQL query
        let sql = format!("DELETE FROM {} WHERE id = $1", table_name);
        
        // Execute query
        match self.database.execute(&sql, &[&id.value()]).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to delete message: {}", e);
                Err(RepositoryError::Database(e))
            }
        }
    }
}

/// PostgreSQL implementation of the user profile repository
pub struct PostgresUserProfileRepository {
    database: Arc<Database>,
}

impl PostgresUserProfileRepository {
    /// Create a new PostgreSQL user profile repository
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

#[async_trait]
impl UserProfileRepository for PostgresUserProfileRepository {
    async fn get_profile(&self, fid: Fid) -> Result<Option<serde_json::Value>> {
        // Query user_data table
        let sql = "SELECT profile_data FROM user_profiles WHERE fid = $1";
        
        // Execute query
        let maybe_row = self.database.query_opt(&sql, &[&fid.value()]).await
            .map_err(|e| RepositoryError::Database(e))?;
        
        match maybe_row {
            Some(row) => {
                let profile_json: serde_json::Value = row.get(0);
                Ok(Some(profile_json))
            }
            None => Ok(None),
        }
    }
    
    async fn update_profile(&self, fid: Fid, profile: serde_json::Value) -> Result<()> {
        // Insert or update profile
        let sql = "INSERT INTO user_profiles (fid, profile_data, updated_at)
                  VALUES ($1, $2, NOW())
                  ON CONFLICT (fid) DO UPDATE
                  SET profile_data = $2, updated_at = NOW()";
        
        // Execute query
        match self.database.execute(&sql, &[&fid.value(), &profile]).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to update profile: {}", e);
                Err(RepositoryError::Database(e))
            }
        }
    }
    
    async fn search_profiles(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<serde_json::Value>> {
        // Full-text search query using PostgreSQL
        let sql = "SELECT profile_data FROM user_profiles
                  WHERE to_tsvector('english', profile_data::text) @@ plainto_tsquery('english', $1)
                  ORDER BY updated_at DESC
                  LIMIT $2 OFFSET $3";
        
        // Execute query
        let rows = self.database.query(&sql, &[&query, &(limit as i64), &(offset as i64)]).await
            .map_err(|e| RepositoryError::Database(e))?;
        
        // Process results
        let mut profiles = Vec::with_capacity(rows.len());
        for row in rows {
            let profile: serde_json::Value = row.get(0);
            profiles.push(profile);
        }
        
        Ok(profiles)
    }
}