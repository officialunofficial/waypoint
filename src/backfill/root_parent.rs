//! Backfill worker for populating root_parent columns on existing casts.
//!
//! This module provides functionality to backfill the `root_parent_hash`,
//! `root_parent_fid`, and `root_parent_url` columns for casts that were
//! inserted before this feature was added.

use crate::{hub::client::Hub, proto::CastId};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};

/// Configuration for the root parent backfill process
#[derive(Debug, Clone)]
pub struct RootParentBackfillConfig {
    /// Number of casts to process per batch
    pub batch_size: usize,
    /// Maximum depth to traverse when looking for root
    pub max_depth: usize,
    /// Delay between batches to avoid overloading the Hub
    pub batch_delay_ms: u64,
}

impl Default for RootParentBackfillConfig {
    fn default() -> Self {
        Self { batch_size: 100, max_depth: 100, batch_delay_ms: 100 }
    }
}

/// Statistics for the backfill process
#[derive(Debug, Default, Clone)]
pub struct BackfillStats {
    pub casts_processed: u64,
    pub roots_found: u64,
    pub chains_broken: u64,
    pub errors: u64,
}

/// Backfill worker for root_parent columns
pub struct RootParentBackfill {
    pool: PgPool,
    hub: Arc<Mutex<Hub>>,
    config: RootParentBackfillConfig,
}

impl RootParentBackfill {
    pub fn new(pool: PgPool, hub: Arc<Mutex<Hub>>, config: RootParentBackfillConfig) -> Self {
        Self { pool, hub, config }
    }

    /// Run the backfill process for all casts missing root_parent
    pub async fn run(&self) -> Result<BackfillStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut stats = BackfillStats::default();

        info!("Starting root_parent backfill with batch_size={}", self.config.batch_size);

        loop {
            // Fetch a batch of casts that need root_parent populated
            let casts = self.fetch_casts_needing_backfill().await?;

            if casts.is_empty() {
                info!("Root parent backfill complete: {:?}", stats);
                break;
            }

            info!(
                "Processing batch of {} casts (total processed: {})",
                casts.len(),
                stats.casts_processed
            );

            for (hash, parent_fid, parent_hash) in casts {
                match self.resolve_and_update_root(&hash, parent_fid, &parent_hash).await {
                    Ok(true) => stats.roots_found += 1,
                    Ok(false) => stats.chains_broken += 1,
                    Err(e) => {
                        error!("Error processing cast {}: {}", hex::encode(&hash), e);
                        stats.errors += 1;
                    },
                }
                stats.casts_processed += 1;
            }

            // Delay between batches
            if self.config.batch_delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(self.config.batch_delay_ms))
                    .await;
            }
        }

        Ok(stats)
    }

    /// Fetch casts that have parent_hash but no root_parent_hash
    async fn fetch_casts_needing_backfill(
        &self,
    ) -> Result<Vec<(Vec<u8>, i64, Vec<u8>)>, Box<dyn std::error::Error + Send + Sync>> {
        let rows = sqlx::query!(
            r#"
            SELECT hash, parent_fid, parent_hash
            FROM casts
            WHERE parent_hash IS NOT NULL
              AND root_parent_hash IS NULL
              AND deleted_at IS NULL
            LIMIT $1
            "#,
            self.config.batch_size as i64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let hash = row.hash;
                let parent_fid = row.parent_fid?;
                let parent_hash = row.parent_hash?;
                Some((hash, parent_fid, parent_hash))
            })
            .collect())
    }

    /// Resolve the root parent for a cast and update the database
    /// Returns true if root was found, false if chain was broken
    async fn resolve_and_update_root(
        &self,
        cast_hash: &[u8],
        parent_fid: i64,
        parent_hash: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // First check if parent exists in DB and has root_parent set
        let db_result = sqlx::query!(
            r#"
            SELECT
                COALESCE(root_parent_fid, fid) as "root_fid!: i64",
                COALESCE(root_parent_hash, hash) as "root_hash!: Vec<u8>",
                root_parent_url as "root_url: String"
            FROM casts
            WHERE hash = $1 AND deleted_at IS NULL
            "#,
            parent_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        let (root_fid, root_hash, root_url) = if let Some(row) = db_result {
            (Some(row.root_fid), Some(row.root_hash), row.root_url)
        } else {
            // Parent not in DB - traverse Hub
            self.resolve_root_from_hub(parent_fid, parent_hash).await?
        };

        // Update the cast with resolved root_parent
        if root_hash.is_some() || root_url.is_some() {
            sqlx::query!(
                r#"
                UPDATE casts
                SET root_parent_fid = $1,
                    root_parent_hash = $2,
                    root_parent_url = $3
                WHERE hash = $4
                "#,
                root_fid,
                root_hash.as_deref(),
                root_url.as_deref(),
                cast_hash
            )
            .execute(&self.pool)
            .await?;

            trace!("Updated cast {} with root_parent", hex::encode(cast_hash));
            Ok(true)
        } else {
            debug!("Could not resolve root for cast {} (broken chain)", hex::encode(cast_hash));
            Ok(false)
        }
    }

    /// Traverse parent chain via Hub to find the root
    async fn resolve_root_from_hub(
        &self,
        start_fid: i64,
        start_hash: &[u8],
    ) -> Result<
        (Option<i64>, Option<Vec<u8>>, Option<String>),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        use crate::proto::{cast_add_body::Parent, message_data::Body::CastAddBody};

        let mut current_fid = start_fid as u64;
        let mut current_hash = start_hash.to_vec();

        for depth in 0..self.config.max_depth {
            let mut hub = self.hub.lock().await;

            if !hub.check_connection().await.unwrap_or(false) {
                warn!("Hub not connected during backfill");
                return Ok((None, None, None));
            }

            let cast_id = CastId { fid: current_fid, hash: current_hash.clone() };

            let client = match hub.client() {
                Some(c) => c,
                None => {
                    warn!("Hub client not initialized during backfill");
                    return Ok((None, None, None));
                },
            };

            let result = client.get_cast(tonic::Request::new(cast_id)).await;
            drop(hub);

            let proto_msg = match result {
                Ok(response) => response.into_inner(),
                Err(status) if status.code() == tonic::Code::NotFound => {
                    trace!("Cast not found in Hub at depth {}", depth);
                    return Ok((None, None, None));
                },
                Err(e) => {
                    warn!("Hub error during backfill: {}", e);
                    return Ok((None, None, None));
                },
            };

            // Extract parent info
            if let Some(data) = &proto_msg.data {
                if let Some(CastAddBody(cast_body)) = &data.body {
                    match &cast_body.parent {
                        None => {
                            // Found root!
                            return Ok((Some(current_fid as i64), Some(current_hash), None));
                        },
                        Some(Parent::ParentUrl(url)) => {
                            // URL root
                            return Ok((None, None, Some(url.clone())));
                        },
                        Some(Parent::ParentCastId(parent_id)) => {
                            // Continue traversing
                            current_fid = parent_id.fid;
                            current_hash = parent_id.hash.clone();
                        },
                    }
                } else {
                    return Ok((None, None, None));
                }
            } else {
                return Ok((None, None, None));
            }
        }

        warn!("Max depth {} reached during backfill", self.config.max_depth);
        Ok((None, None, None))
    }
}

/// Also backfill URL parent casts (they have parent_url but need root_parent_url set)
pub async fn backfill_url_parents(
    pool: &PgPool,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let result = sqlx::query!(
        r#"
        UPDATE casts
        SET root_parent_url = parent_url
        WHERE parent_url IS NOT NULL
          AND root_parent_url IS NULL
          AND deleted_at IS NULL
        "#
    )
    .execute(pool)
    .await?;

    let updated = result.rows_affected();
    info!("Backfilled {} casts with URL parents", updated);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RootParentBackfillConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.max_depth, 100);
        assert_eq!(config.batch_delay_ms, 100);
    }
}
