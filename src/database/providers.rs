//! Data providers for PostgreSQL database

use crate::core::{
    data_context::{DataAccessError, Database, Result},
    types::{Fid, Message, MessageId, MessageType},
};
use crate::database::client::Database as DbPool;
use async_trait::async_trait;
use sqlx::Row;
use std::sync::Arc;
use tracing::error;

/// Read-only PostgreSQL data provider
#[derive(Clone)]
pub struct PostgresDatabaseClient {
    db: Arc<DbPool>,
}

impl std::fmt::Debug for PostgresDatabaseClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresDatabaseClient").field("db", &"<Arc<Database>>").finish()
    }
}

impl PostgresDatabaseClient {
    /// Create a new PostgreSQL database
    pub fn new(db: Arc<DbPool>) -> Self {
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
impl Database for PostgresDatabaseClient {
    async fn store_message(&self, _message: Message) -> Result<()> {
        Err(DataAccessError::Other("Write operations not supported".to_string()))
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
                            DataAccessError::NotFound(format!("Message with ID {} not found", id))
                        } else {
                            DataAccessError::Database(e)
                        }
                    },
                )?;

                // Extract data
                let id: String = row.try_get("id").map_err(DataAccessError::Database)?;

                let payload: Vec<u8> = row.try_get("payload").map_err(DataAccessError::Database)?;

                Ok(Message::new(id, message_type, payload))
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(DataAccessError::Database(e))
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
                    let id: String = row.try_get("id").map_err(DataAccessError::Database)?;

                    let payload: Vec<u8> =
                        row.try_get("payload").map_err(DataAccessError::Database)?;

                    messages.push(Message::new(id, message_type, payload));
                }

                Ok(messages)
            },
            Err(e) => {
                error!("Failed to acquire database connection: {}", e);
                Err(DataAccessError::Database(e))
            },
        }
    }

    async fn delete_message(&self, _id: &MessageId, _message_type: MessageType) -> Result<()> {
        Err(DataAccessError::Other("Write operations not supported".to_string()))
    }
}
