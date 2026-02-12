//! Data providers for PostgreSQL database

use crate::core::{
    data_context::{DataAccessError, Database, Result},
    types::{Fid, Message, MessageId, MessageType},
};
use crate::database::client::Database as DbPool;
use async_trait::async_trait;
use sqlx::Row;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, info};

/// No-op database provider for hub-only read paths.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullDb;

#[async_trait]
impl Database for NullDb {
    async fn get_message(&self, _id: &MessageId, _message_type: MessageType) -> Result<Message> {
        Err(DataAccessError::NotFound("NullDb does not store messages".to_string()))
    }

    async fn get_messages_by_fid(
        &self,
        _fid: Fid,
        _message_type: MessageType,
        _limit: usize,
        _cursor: Option<MessageId>,
    ) -> Result<Vec<Message>> {
        Ok(Vec::new())
    }

    async fn store_message(&self, _message: Message) -> Result<()> {
        Ok(())
    }

    async fn delete_message(&self, _id: &MessageId, _message_type: MessageType) -> Result<()> {
        Ok(())
    }
}

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
            MessageType::OnchainSigner => "signer_events",
            MessageType::OnchainSignerMigrated => "signer_migrated_events",
            MessageType::OnchainIdRegister => "id_register_events",
            MessageType::OnchainStorageRent => "storage_rent_events",
            MessageType::OnchainTierPurchase => "tier_purchases",
            MessageType::LendStorage => "lend_storage",
            MessageType::Messages => "messages",
        }
    }

    /// Batch upsert spammy users efficiently using unnest
    /// Inserts new FIDs and clears deleted_at for existing ones
    /// Soft-deletes FIDs that are no longer in the set
    pub async fn sync_spammy_users(
        &self,
        fids: &HashSet<u64>,
        source: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let fids_vec: Vec<i64> = fids.iter().map(|&f| f as i64).collect();

        // Upsert all current FIDs (insert or clear deleted_at)
        sqlx::query!(
            r#"
            INSERT INTO spammy_users (fid, source)
            SELECT unnest($1::bigint[]), $2
            ON CONFLICT (fid) DO UPDATE SET
                deleted_at = NULL,
                updated_at = CURRENT_TIMESTAMP
            "#,
            &fids_vec,
            source
        )
        .execute(&self.db.pool)
        .await?;

        // Soft-delete FIDs that are no longer in the set
        sqlx::query!(
            r#"
            UPDATE spammy_users
            SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
            WHERE deleted_at IS NULL
              AND source = $1
              AND fid != ALL($2::bigint[])
            "#,
            source,
            &fids_vec
        )
        .execute(&self.db.pool)
        .await?;

        info!("Synced {} spammy users from source '{}'", fids.len(), source);
        Ok(())
    }

    /// Remove a single user from the spammy_users table (soft-delete)
    /// Called when a label_value=2 is received, indicating the user is no longer spammy
    pub async fn remove_spammy_user(
        &self,
        fid: u64,
        source: &str,
    ) -> std::result::Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            r#"
            UPDATE spammy_users
            SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
            WHERE fid = $1 AND source = $2 AND deleted_at IS NULL
            "#,
            fid as i64,
            source
        )
        .execute(&self.db.pool)
        .await?;

        let removed = result.rows_affected() > 0;
        if removed {
            info!("Removed spammy user fid={} from source '{}'", fid, source);
        }
        Ok(removed)
    }

    /// Batch upsert nerfed users efficiently using unnest
    /// Inserts new FIDs and clears deleted_at for existing ones
    /// Soft-deletes FIDs that are no longer in the set
    pub async fn sync_nerfed_users(
        &self,
        fids: &HashSet<u64>,
        source: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let fids_vec: Vec<i64> = fids.iter().map(|&f| f as i64).collect();

        // Upsert all current FIDs (insert or clear deleted_at)
        sqlx::query!(
            r#"
            INSERT INTO nerfed_users (fid, source)
            SELECT unnest($1::bigint[]), $2
            ON CONFLICT (fid) DO UPDATE SET
                deleted_at = NULL,
                updated_at = CURRENT_TIMESTAMP
            "#,
            &fids_vec,
            source
        )
        .execute(&self.db.pool)
        .await?;

        // Soft-delete FIDs that are no longer in the set
        sqlx::query!(
            r#"
            UPDATE nerfed_users
            SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
            WHERE deleted_at IS NULL
              AND source = $1
              AND fid != ALL($2::bigint[])
            "#,
            source,
            &fids_vec
        )
        .execute(&self.db.pool)
        .await?;

        info!("Synced {} nerfed users from source '{}'", fids.len(), source);
        Ok(())
    }

    /// Remove a single user from the nerfed_users table (soft-delete)
    /// Called when a user is no longer considered nerfed
    pub async fn remove_nerfed_user(
        &self,
        fid: u64,
        source: &str,
    ) -> std::result::Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            r#"
            UPDATE nerfed_users
            SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
            WHERE fid = $1 AND source = $2 AND deleted_at IS NULL
            "#,
            fid as i64,
            source
        )
        .execute(&self.db.pool)
        .await?;

        let removed = result.rows_affected() > 0;
        if removed {
            info!("Removed nerfed user fid={} from source '{}'", fid, source);
        }
        Ok(removed)
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
