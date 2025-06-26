use crate::{
    core::root_parent_hub::find_root_parent_hub, database::client::Database,
    hub::providers::FarcasterHubClient,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Backfill processor for updating root parent information on existing casts
pub struct RootParentBackfill {
    database: Arc<Database>,
    hub_client: FarcasterHubClient,
    batch_size: i64,
}

impl RootParentBackfill {
    pub fn new(database: Arc<Database>, hub: Arc<Mutex<crate::hub::client::Hub>>) -> Self {
        Self { database, hub_client: FarcasterHubClient::new(hub), batch_size: 100 }
    }

    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Run the backfill process
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting root parent backfill process");

        let mut total_processed = 0;
        let mut total_updated = 0;

        loop {
            // Find casts that need root parent information
            let casts = self.find_casts_needing_update().await?;

            if casts.is_empty() {
                info!("No more casts to process");
                break;
            }

            info!("Processing batch of {} casts", casts.len());

            for cast in &casts {
                total_processed += 1;

                match self.process_cast(cast).await {
                    Ok(updated) => {
                        if updated {
                            total_updated += 1;
                        }
                    },
                    Err(e) => {
                        error!("Failed to process cast {:?}: {}", cast.hash, e);
                    },
                }

                // Log progress every 1000 casts
                if total_processed % 1000 == 0 {
                    info!("Progress: processed={}, updated={}", total_processed, total_updated);
                }
            }
        }

        info!(
            "Root parent backfill completed: processed={}, updated={}",
            total_processed, total_updated
        );

        Ok(())
    }

    /// Find casts that need root parent information updated
    async fn find_casts_needing_update(
        &self,
    ) -> Result<Vec<CastInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query_as!(
            CastInfo,
            r#"
            SELECT 
                hash,
                fid as "fid!",
                parent_fid as "parent_fid?",
                parent_hash as "parent_hash?",
                parent_url as "parent_url?"
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
            self.batch_size
        )
        .fetch_all(&self.database.pool)
        .await?;

        Ok(results)
    }

    /// Process a single cast to update its root parent information
    async fn process_cast(
        &self,
        cast: &CastInfo,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Find root parent
        let root_info = match find_root_parent_hub(
            &self.hub_client,
            cast.parent_fid,
            cast.parent_hash.as_deref(),
            cast.parent_url.as_deref(),
        )
        .await
        {
            Ok(info) => info,
            Err(e) => {
                warn!("Failed to find root parent for cast {:?}: {}", cast.hash, e);
                return Ok(false);
            },
        };

        // Update the database if we found root parent info
        if let Some(root) = root_info {
            sqlx::query!(
                r#"
                UPDATE casts 
                SET 
                    root_parent_fid = $2,
                    root_parent_hash = $3,
                    root_parent_url = $4
                WHERE hash = $1
                "#,
                &cast.hash,
                root.root_parent_fid,
                root.root_parent_hash.as_deref(),
                root.root_parent_url.as_deref(),
            )
            .execute(&self.database.pool)
            .await?;

            Ok(true)
        } else {
            // Cast has no parent, nothing to update
            Ok(false)
        }
    }
}

#[derive(Debug)]
struct CastInfo {
    hash: Vec<u8>,
    #[allow(dead_code)]
    fid: i64,
    parent_fid: Option<i64>,
    parent_hash: Option<Vec<u8>>,
    parent_url: Option<String>,
}
