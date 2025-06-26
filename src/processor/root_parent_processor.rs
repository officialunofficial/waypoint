use crate::{
    core::root_parent_hub::{RootParentInfo, find_root_parent_hub},
    hub::providers::FarcasterHubClient,
    proto::{Message, cast_add_body::Parent, message_data::Body::CastAddBody},
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

/// Processor that handles updating root parent information for casts
pub struct RootParentProcessor {
    db_pool: PgPool,
    hub_client: FarcasterHubClient,
}

impl RootParentProcessor {
    pub fn new(db_pool: PgPool, hub: Arc<Mutex<crate::hub::client::Hub>>) -> Self {
        Self { db_pool, hub_client: FarcasterHubClient::new(hub) }
    }

    /// Process a single cast message to update its root parent information
    pub async fn process_cast_message(
        &self,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(CastAddBody(cast_body)) = &data.body {
                let _fid = data.fid as i64;
                let hash = &msg.hash;

                // Extract parent information
                let (parent_fid, parent_hash, parent_url) = match &cast_body.parent {
                    Some(Parent::ParentCastId(cast_id)) => {
                        (Some(cast_id.fid as i64), Some(cast_id.hash.as_slice()), None)
                    },
                    Some(Parent::ParentUrl(url)) => (None, None, Some(url.as_str())),
                    None => (None, None, None),
                };

                // Find root parent if this cast has a parent
                let root_info = if parent_fid.is_some() || parent_url.is_some() {
                    find_root_parent_hub(&self.hub_client, parent_fid, parent_hash, parent_url)
                        .await?
                } else {
                    None
                };

                // Update the database with root parent information
                if let Some(root) = root_info {
                    self.update_cast_root_parent(hash, &root).await?;
                }
            }
        }
        Ok(())
    }

    /// Update a cast's root parent information in the database
    async fn update_cast_root_parent(
        &self,
        cast_hash: &[u8],
        root_info: &RootParentInfo,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query!(
            r#"
            UPDATE casts 
            SET 
                root_parent_fid = $2,
                root_parent_hash = $3,
                root_parent_url = $4
            WHERE hash = $1
            "#,
            cast_hash,
            root_info.root_parent_fid,
            root_info.root_parent_hash.as_deref(),
            root_info.root_parent_url.as_deref(),
        )
        .execute(&self.db_pool)
        .await?;

        debug!(
            "Updated root parent for cast hash={:?}, root_fid={:?}, root_hash={:?}, root_url={:?}",
            cast_hash,
            root_info.root_parent_fid,
            root_info.root_parent_hash,
            root_info.root_parent_url
        );

        Ok(())
    }

    /// Process a batch of casts to update their root parent information
    pub async fn process_cast_batch(
        &self,
        cast_hashes: Vec<Vec<u8>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Processing batch of {} casts for root parent updates", cast_hashes.len());

        for hash in cast_hashes {
            // Fetch cast details from database
            let cast_result = sqlx::query!(
                r#"
                SELECT fid, parent_fid, parent_hash, parent_url 
                FROM casts 
                WHERE hash = $1 AND deleted_at IS NULL
                "#,
                &hash
            )
            .fetch_optional(&self.db_pool)
            .await?;

            if let Some(cast) = cast_result {
                // Only process if it has a parent but no root parent info yet
                if cast.parent_fid.is_some() || cast.parent_url.is_some() {
                    match find_root_parent_hub(
                        &self.hub_client,
                        cast.parent_fid,
                        cast.parent_hash.as_deref(),
                        cast.parent_url.as_deref(),
                    )
                    .await
                    {
                        Ok(Some(root_info)) => {
                            if let Err(e) = self.update_cast_root_parent(&hash, &root_info).await {
                                error!("Failed to update root parent for cast {:?}: {}", hash, e);
                            }
                        },
                        Ok(None) => {
                            // Cast has no parent, it's a root itself
                        },
                        Err(e) => {
                            error!("Failed to find root parent for cast {:?}: {}", hash, e);
                        },
                    }
                }
            }
        }

        Ok(())
    }

    /// Find all casts that need root parent information updated
    pub async fn find_casts_needing_root_parent_update(
        &self,
        limit: i64,
    ) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query!(
            r#"
            SELECT hash 
            FROM casts 
            WHERE 
                (parent_fid IS NOT NULL OR parent_url IS NOT NULL)
                AND root_parent_fid IS NULL 
                AND root_parent_hash IS NULL 
                AND root_parent_url IS NULL
                AND deleted_at IS NULL
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(&self.db_pool)
        .await?;

        Ok(results.into_iter().map(|r| r.hash).collect())
    }
}
