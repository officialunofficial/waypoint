//! Implementation of the repository traits for PostgreSQL

use crate::core::{
    Fid, Message, MessageId, MessageType,
    repository::{MessageRepository, RepositoryError, Result, UserProfileRepository},
};
use crate::database::client::Database;
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::Row;
use std::sync::Arc;
use tracing::error;

/// PostgreSQL implementation of the message repository
pub struct PostgresMessageRepository {
    db: Arc<Database>,
}

impl PostgresMessageRepository {
    /// Create a new PostgreSQL message repository
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Get the table name for a message type
    fn table_for_message_type(&self, message_type: MessageType) -> &'static str {
        match message_type {
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
        }
    }
}

#[async_trait]
impl MessageRepository for PostgresMessageRepository {
    async fn store_message(&self, message: Message) -> Result<()> {
        let table_name = self.table_for_message_type(message.message_type);

        // Build SQL query
        let sql = format!(
            "INSERT INTO {} (id, payload, fid) VALUES ($1, $2, $3) ON CONFLICT (id) DO UPDATE SET payload = $2",
            table_name
        );

        // Extract FID from the message if available (implementation specific)
        let fid = message.id.value().parse::<i64>().unwrap_or(0);

        // Execute query using the underlying connection
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                match sqlx::query(&sql)
                    .bind(message.id.value())
                    .bind(&message.payload)
                    .bind(fid)
                    .execute(&mut *conn)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!("Failed to store message: {}", e);
                        Err(RepositoryError::Database(e))
                    },
                }
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }

    async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message> {
        let table_name = self.table_for_message_type(message_type);

        // Build SQL query
        let sql = format!("SELECT id, payload FROM {} WHERE id = $1", table_name);

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                let row = sqlx::query(&sql).bind(id.value()).fetch_one(&mut *conn).await.map_err(
                    |e| {
                        if e.to_string().contains("no rows") {
                            RepositoryError::NotFound(format!("Message with ID {} not found", id))
                        } else {
                            RepositoryError::Database(e)
                        }
                    },
                )?;

                // Extract data
                let id: String = row.try_get("id").map_err(RepositoryError::Database)?;

                let payload: Vec<u8> = row.try_get("payload").map_err(RepositoryError::Database)?;

                Ok(Message::new(id, message_type, payload))
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }

    async fn get_messages_by_fid(
        &self,
        fid: Fid,
        message_type: MessageType,
        limit: usize,
        cursor: Option<MessageId>,
    ) -> Result<Vec<Message>> {
        let table_name = self.table_for_message_type(message_type);

        // Convert FID to i64 for database
        let fid_value = fid.value() as i64;

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                let rows = match cursor {
                    Some(cursor) => {
                        // Build SQL query with cursor-based pagination
                        let sql = format!(
                            "SELECT id, payload FROM {} WHERE fid = $1 AND id > $2 ORDER BY id LIMIT $3",
                            table_name
                        );

                        sqlx::query(&sql)
                            .bind(fid_value)
                            .bind(cursor.value())
                            .bind(limit as i64)
                            .fetch_all(&mut *conn)
                            .await?
                    },
                    None => {
                        // Build SQL query without cursor
                        let sql = format!(
                            "SELECT id, payload FROM {} WHERE fid = $1 ORDER BY id LIMIT $2",
                            table_name
                        );

                        sqlx::query(&sql)
                            .bind(fid_value)
                            .bind(limit as i64)
                            .fetch_all(&mut *conn)
                            .await?
                    },
                };

                // Process results
                let mut messages = Vec::with_capacity(rows.len());
                for row in rows {
                    let id: String = row.try_get("id").map_err(RepositoryError::Database)?;

                    let payload: Vec<u8> =
                        row.try_get("payload").map_err(RepositoryError::Database)?;

                    messages.push(Message::new(id, message_type, payload));
                }

                Ok(messages)
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }

    async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()> {
        let table_name = self.table_for_message_type(message_type);

        // Build SQL query
        let sql = format!("DELETE FROM {} WHERE id = $1", table_name);

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => match sqlx::query(&sql).bind(id.value()).execute(&mut *conn).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("Failed to delete message: {}", e);
                    Err(RepositoryError::Database(e))
                },
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }
}

/// PostgreSQL implementation of the user profile repository
pub struct PostgresUserProfileRepository {
    db: Arc<Database>,
}

impl PostgresUserProfileRepository {
    /// Create a new PostgreSQL user profile repository
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserProfileRepository for PostgresUserProfileRepository {
    async fn get_profile(&self, fid: Fid) -> Result<Option<JsonValue>> {
        // Query user_data table
        let sql = "SELECT profile_data FROM user_profiles WHERE fid = $1";

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                let maybe_row =
                    sqlx::query(sql).bind(fid.value() as i64).fetch_optional(&mut *conn).await?;

                match maybe_row {
                    Some(row) => {
                        let profile_json: JsonValue =
                            row.try_get("profile_data").map_err(RepositoryError::Database)?;

                        Ok(Some(profile_json))
                    },
                    None => Ok(None),
                }
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }

    async fn update_profile(&self, fid: Fid, profile: JsonValue) -> Result<()> {
        // Insert or update profile
        let sql = r#"
        INSERT INTO user_profiles (fid, profile_data, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (fid) DO UPDATE
        SET profile_data = $2, updated_at = NOW()
        "#;

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                match sqlx::query(sql)
                    .bind(fid.value() as i64)
                    .bind(&profile)
                    .execute(&mut *conn)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!("Failed to update profile: {}", e);
                        Err(RepositoryError::Database(e))
                    },
                }
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }

    async fn search_profiles(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<JsonValue>> {
        // Full-text search query using PostgreSQL
        let sql = r#"
        SELECT profile_data FROM user_profiles
        WHERE to_tsvector('english', profile_data::text) @@ plainto_tsquery('english', $1)
        ORDER BY updated_at DESC
        LIMIT $2 OFFSET $3
        "#;

        // Execute query
        match self.db.pool.acquire().await {
            Ok(mut conn) => {
                let rows = sqlx::query(sql)
                    .bind(query)
                    .bind(limit as i64)
                    .bind(offset as i64)
                    .fetch_all(&mut *conn)
                    .await?;

                // Process results
                let mut profiles = Vec::with_capacity(rows.len());
                for row in rows {
                    let profile: JsonValue =
                        row.try_get("profile_data").map_err(RepositoryError::Database)?;

                    profiles.push(profile);
                }

                Ok(profiles)
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(RepositoryError::Database(e))
            },
        }
    }
}
