use crate::{
    core::{normalize::NormalizedEmbed, util::from_farcaster_time},
    database::batch::BatchInserter,
    hub::subscriber::{PostProcessHandler, PreProcessHandler},
    metrics,
    processor::consumer::EventProcessor,
    proto::{
        CastId, HubEvent, Message, OnChainEvent, UserNameProof,
        cast_add_body::Parent,
        hub_event::Body,
        link_body::Target as LinkTarget,
        message_data::Body::{
            CastAddBody, CastRemoveBody, LendStorageBody, LinkBody, ReactionBody, UserDataBody,
            UsernameProofBody, VerificationAddAddressBody, VerificationRemoveBody,
        },
        reaction_body::Target as ReactionTarget,
    },
};
use async_trait::async_trait;
use futures::future::BoxFuture;
use rayon::prelude::*;
use sqlx::{postgres::PgPool, types::time::OffsetDateTime};
use std::hash::Hasher;
use std::sync::Arc;
use tracing::{debug, error, trace, warn};

/// Extracts parent info from a proto::Message (the Hub gRPC response type)
fn extract_parent_from_proto(msg: &Message) -> (Option<u64>, Option<Vec<u8>>, Option<String>) {
    if let Some(data) = &msg.data {
        if let Some(CastAddBody(cast_body)) = &data.body {
            let parent_fid = match &cast_body.parent {
                Some(Parent::ParentCastId(id)) => Some(id.fid),
                _ => None,
            };
            let parent_hash = match &cast_body.parent {
                Some(Parent::ParentCastId(id)) => Some(id.hash.clone()),
                _ => None,
            };
            let parent_url = match &cast_body.parent {
                Some(Parent::ParentUrl(url)) => Some(url.clone()),
                _ => None,
            };
            return (parent_fid, parent_hash, parent_url);
        }
    }
    (None, None, None)
}

#[derive(Clone)]
pub struct DatabaseProcessor {
    resources: Arc<super::AppResources>,
}

impl DatabaseProcessor {
    pub fn new(resources: Arc<super::AppResources>) -> Self {
        Self { resources }
    }

    fn convert_timestamp(timestamp: u32) -> OffsetDateTime {
        // Convert from farcaster time (seconds since epoch) to unix seconds
        let unix_time = from_farcaster_time(timestamp) / 1000; // Convert ms to seconds
        OffsetDateTime::from_unix_timestamp(unix_time as i64).unwrap()
    }

    /// Resolves root parent by looking up the parent cast.
    /// Priority: 1) Check DB for parent's root  2) Query Hub if not in DB  3) Fallback to parent as root
    async fn resolve_root_parent(
        &self,
        pool: &PgPool,
        parent_fid: Option<i64>,
        parent_hash: Option<&[u8]>,
        parent_url: Option<&str>,
    ) -> Result<
        (Option<i64>, Option<Vec<u8>>, Option<String>),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        // No parent = root cast
        if parent_hash.is_none() && parent_url.is_none() {
            return Ok((None, None, None));
        }

        // URL parent = root is the URL itself
        if let Some(url) = parent_url {
            return Ok((None, None, Some(url.to_string())));
        }

        // Cast parent = look up parent's root
        if let (Some(p_hash), Some(p_fid)) = (parent_hash, parent_fid) {
            // First check our database for the parent cast
            let db_result = sqlx::query!(
                r#"
                SELECT
                    COALESCE(root_parent_fid, fid) as "root_fid!: i64",
                    COALESCE(root_parent_hash, hash) as "root_hash!: Vec<u8>",
                    root_parent_url as "root_url: String"
                FROM casts
                WHERE hash = $1 AND deleted_at IS NULL
                "#,
                p_hash
            )
            .fetch_optional(pool)
            .await?;

            if let Some(row) = db_result {
                return Ok((Some(row.root_fid), Some(row.root_hash), row.root_url));
            }

            // Parent not in DB - query Hub and traverse chain to find root
            return self.resolve_root_from_hub(p_fid, p_hash).await;
        }

        Ok((None, None, None))
    }

    /// Traverses parent chain via Hub gRPC API to find the true root cast.
    /// Uses iterative approach with max depth to prevent infinite loops.
    /// Returns None for all fields if the root cannot be definitively determined.
    async fn resolve_root_from_hub(
        &self,
        start_fid: i64,
        start_hash: &[u8],
    ) -> Result<
        (Option<i64>, Option<Vec<u8>>, Option<String>),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        const MAX_DEPTH: usize = 100; // Prevent runaway traversal

        let mut current_fid = start_fid as u64;
        let mut current_hash = start_hash.to_vec();

        for depth in 0..MAX_DEPTH {
            // Fetch the cast from Hub via gRPC
            let mut hub = self.resources.hub.lock().await;

            // Ensure connected
            if !hub.check_connection().await.unwrap_or(false) {
                warn!("Hub not connected while resolving root parent, leaving root_parent NULL");
                return Ok((None, None, None));
            }

            let cast_id = CastId { fid: current_fid, hash: current_hash.clone() };

            let client = match hub.client() {
                Some(c) => c,
                None => {
                    warn!("Hub client not initialized, leaving root_parent NULL");
                    return Ok((None, None, None));
                },
            };

            let result = client.get_cast(tonic::Request::new(cast_id)).await;

            drop(hub); // Release lock before processing

            let proto_msg = match result {
                Ok(response) => response.into_inner(),
                Err(status) if status.code() == tonic::Code::NotFound => {
                    // Cast not found in Hub - we cannot determine the true root
                    // Leave root_parent NULL rather than incorrectly assume parent is root
                    trace!(
                        "Cast not found in Hub at depth {}, leaving root_parent NULL (chain broken)",
                        depth
                    );
                    return Ok((None, None, None));
                },
                Err(e) => {
                    warn!("Error fetching cast from Hub: {}, leaving root_parent NULL", e);
                    return Ok((None, None, None));
                },
            };

            // Extract parent info from protobuf
            let (parent_fid_opt, parent_hash_opt, parent_url_opt) =
                extract_parent_from_proto(&proto_msg);

            match (parent_hash_opt, parent_url_opt) {
                (None, None) => {
                    // Found root! This cast has no parent.
                    trace!("Found root cast at depth {}", depth);
                    return Ok((Some(current_fid as i64), Some(current_hash), None));
                },
                (None, Some(url)) => {
                    // Root is a URL (channel root)
                    trace!("Found URL root at depth {}: {}", depth, url);
                    return Ok((None, None, Some(url)));
                },
                (Some(p_hash), _) => {
                    // Continue traversing up
                    current_fid = parent_fid_opt.unwrap_or(current_fid);
                    current_hash = p_hash;
                },
            }
        }

        // Max depth reached - cannot determine root definitively
        warn!(
            "Max depth {} reached while resolving root parent, leaving root_parent NULL",
            MAX_DEPTH
        );
        Ok((None, None, None))
    }

    async fn add_cast(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(CastAddBody(cast_body)) = &data.body {
                metrics::increment_casts_processed();
                let parent_url = match &cast_body.parent {
                    Some(Parent::ParentUrl(url)) => Some(url.as_str()),
                    _ => None,
                };

                let parent_hash: Option<&[u8]> = match &cast_body.parent {
                    Some(Parent::ParentCastId(cast_id)) => Some(&cast_id.hash),
                    _ => None,
                };

                let parent_fid = match &cast_body.parent {
                    Some(Parent::ParentCastId(cast_id)) => Some(cast_id.fid as i64),
                    _ => None,
                };

                let ts = Self::convert_timestamp(data.timestamp);

                // Resolve root parent by traversing the parent chain
                let (root_parent_fid, root_parent_hash, root_parent_url) =
                    self.resolve_root_parent(pool, parent_fid, parent_hash, parent_url).await?;

                sqlx::query!(
                    r#"
                INSERT INTO casts (
                    fid, hash, text, parent_fid, parent_hash, parent_url,
                    root_parent_fid, root_parent_hash, root_parent_url,
                    timestamp, embeds, mentions, mentions_positions
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                ON CONFLICT (hash) DO UPDATE SET
                    text = EXCLUDED.text,
                    parent_fid = EXCLUDED.parent_fid,
                    parent_hash = EXCLUDED.parent_hash,
                    parent_url = EXCLUDED.parent_url,
                    root_parent_fid = EXCLUDED.root_parent_fid,
                    root_parent_hash = EXCLUDED.root_parent_hash,
                    root_parent_url = EXCLUDED.root_parent_url,
                    timestamp = EXCLUDED.timestamp,
                    embeds = EXCLUDED.embeds,
                    mentions = EXCLUDED.mentions,
                    mentions_positions = EXCLUDED.mentions_positions
                "#,
                    data.fid as i64,
                    &msg.hash,
                    &cast_body.text,
                    parent_fid,
                    parent_hash,
                    parent_url,
                    root_parent_fid,
                    root_parent_hash.as_deref(),
                    root_parent_url.as_deref(),
                    ts,
                    serde_json::to_value(
                        cast_body
                            .embeds
                            .iter()
                            .map(NormalizedEmbed::from_protobuf_embed)
                            .collect::<Vec<_>>()
                    )?,
                    serde_json::to_value(&cast_body.mentions)?,
                    serde_json::to_value(&cast_body.mentions_positions)?,
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn remove_cast(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(CastRemoveBody(remove_body)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                // Process cast removal with CRDT semantics
                // - Using timestamp-based conflict resolution: higher timestamp wins
                // - For equal timestamps, remove operation wins (remove-wins semantics)
                // - Handles out-of-order messages where removals arrive before additions
                sqlx::query!(
                    r#"
                INSERT INTO casts (fid, hash, deleted_at, timestamp, text, embeds, mentions, mentions_positions, parent_fid)
                VALUES ($1, $2, $3, $4, '', '[]'::json, '[]'::json, '[]'::json, NULL)
                ON CONFLICT (hash) DO UPDATE SET
                    deleted_at = CASE
                        WHEN EXCLUDED.timestamp >= COALESCE(casts.timestamp, EXCLUDED.timestamp) THEN EXCLUDED.deleted_at
                        ELSE casts.deleted_at
                    END,
                    timestamp = LEAST(COALESCE(casts.timestamp, EXCLUDED.timestamp), EXCLUDED.timestamp)
                "#,
                    data.fid as i64,
                    &remove_body.target_hash,
                    ts,
                    ts
                )
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    async fn add_reaction(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(ReactionBody(reaction)) = &data.body {
                metrics::increment_reactions_processed();
                let (target_cast_hash, target_url) = match &reaction.target {
                    Some(ReactionTarget::TargetCastId(cast_id)) => (Some(&cast_id.hash), None),
                    Some(ReactionTarget::TargetUrl(url)) => (None, Some(url.as_str())),
                    None => (None, None),
                };

                let ts = Self::convert_timestamp(data.timestamp);

                // First insert/update the reaction
                sqlx::query!(
                r#"
                INSERT INTO reactions (fid, hash, target_cast_hash, target_url, type, timestamp, deleted_at)
                VALUES ($1, $2, $3, $4, $5, $6, NULL)
                ON CONFLICT (hash) DO UPDATE SET
                    target_cast_hash = EXCLUDED.target_cast_hash,
                    target_url = EXCLUDED.target_url,
                    type = EXCLUDED.type,
                    deleted_at = NULL,
                    timestamp = GREATEST(reactions.timestamp, EXCLUDED.timestamp)
                "#,
                data.fid as i64,
                &msg.hash,
                target_cast_hash,
                target_url,
                reaction.r#type as i16,
                ts
            )
                    .execute(pool)
                    .await?;

                // Process reaction add
            }
        }
        Ok(())
    }

    async fn remove_reaction(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(ReactionBody(reaction)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                match &reaction.target {
                    Some(ReactionTarget::TargetCastId(cast_id)) => {
                        // Process reaction removal

                        // Then upsert reaction with deletion
                        // CRDT conflict resolution for reactions:
                        // - If timestamps are distinct, the message with the higher timestamp wins
                        // - If timestamps are identical, the removal operation wins (delete-wins)
                        // - Handles out-of-order messages with proper conflict resolution
                        sqlx::query!(
                        r#"
                        INSERT INTO reactions (fid, type, target_cast_hash, hash, timestamp, deleted_at)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (hash) DO UPDATE SET
                            deleted_at = CASE
                                WHEN EXCLUDED.timestamp >= reactions.timestamp THEN EXCLUDED.deleted_at
                                ELSE reactions.deleted_at
                            END,
                            timestamp = GREATEST(reactions.timestamp, EXCLUDED.timestamp)
                        "#,
                        data.fid as i64,
                        reaction.r#type as i16,
                        &cast_id.hash,
                        &msg.hash,
                        ts,
                        ts
                    ).execute(pool).await?;

                        // Follow CRDT semantics - no cross-entity updates
                    },
                    Some(ReactionTarget::TargetUrl(url)) => {
                        // Process URL-targeted reaction removal
                        sqlx::query!(
                        r#"
                        INSERT INTO reactions (fid, type, target_url, hash, timestamp, deleted_at)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (hash) DO UPDATE SET
                            deleted_at = CASE
                                WHEN EXCLUDED.timestamp >= reactions.timestamp THEN EXCLUDED.deleted_at
                                ELSE reactions.deleted_at
                            END,
                            timestamp = GREATEST(reactions.timestamp, EXCLUDED.timestamp)
                        "#,
                        data.fid as i64,
                        reaction.r#type as i16,
                        url.as_str(),
                        &msg.hash,
                        ts,
                        ts
                    )
                            .execute(pool)
                            .await?;
                    },
                    None => {},
                }
            }
        }
        Ok(())
    }

    async fn add_link(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(LinkBody(link)) = &data.body {
                metrics::increment_follows_processed();
                let target_fid = match link.target {
                    Some(LinkTarget::TargetFid(fid)) => fid,
                    _ => return Ok(()),
                };

                let ts = Self::convert_timestamp(data.timestamp);
                let display_ts = link.display_timestamp.map(Self::convert_timestamp);

                sqlx::query!(
                r#"
                INSERT INTO links (
                    fid,
                    target_fid,
                    type,
                    hash,
                    timestamp,
                    deleted_at,
                    display_timestamp
                )
                VALUES ($1, $2, $3, $4, $5, NULL, $6)
                ON CONFLICT (hash) DO UPDATE SET
                    deleted_at = NULL,
                    timestamp = LEAST(EXCLUDED.timestamp, links.timestamp),
                    display_timestamp = COALESCE(links.display_timestamp, EXCLUDED.display_timestamp)
                "#,
                data.fid as i64,
                target_fid as i64,
                &link.r#type,
                &msg.hash,
                ts,
                display_ts
            )
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    async fn remove_link(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(LinkBody(link)) = &data.body {
                let target_fid = match link.target {
                    Some(LinkTarget::TargetFid(fid)) => fid,
                    _ => return Ok(()),
                };

                let ts = Self::convert_timestamp(data.timestamp);
                let display_ts = link.display_timestamp.map(Self::convert_timestamp);

                // Process link removal

                // CRDT-compliant link removal:
                // - If timestamps are distinct, message with higher timestamp wins
                // - If timestamps match, removal operations win
                // - LEAST is used for timestamps to preserve earliest timestamp
                sqlx::query!(
                r#"
                INSERT INTO links (
                    fid,
                    target_fid,
                    type,
                    hash,
                    timestamp,
                    deleted_at,
                    display_timestamp
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (hash) DO UPDATE SET
                    deleted_at = CASE
                        WHEN EXCLUDED.timestamp >= links.timestamp THEN EXCLUDED.deleted_at
                        ELSE links.deleted_at
                    END,
                    timestamp = LEAST(EXCLUDED.timestamp, links.timestamp),
                    display_timestamp = COALESCE(links.display_timestamp, EXCLUDED.display_timestamp)
                "#,
                data.fid as i64,
                target_fid as i64,
                &link.r#type,
                &msg.hash,
                ts,
                ts,
                display_ts
            )
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }
    async fn insert_user_data(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(UserDataBody(user_data)) = &data.body {
                metrics::increment_user_data_processed();
                let ts = Self::convert_timestamp(data.timestamp);

                sqlx::query!(
                    r#"
                INSERT INTO user_data (fid, type, hash, value, timestamp)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (fid, type) DO UPDATE SET
                    hash = CASE WHEN EXCLUDED.timestamp >= user_data.timestamp THEN EXCLUDED.hash ELSE user_data.hash END,
                    value = CASE WHEN EXCLUDED.timestamp >= user_data.timestamp THEN EXCLUDED.value ELSE user_data.value END,
                    timestamp = GREATEST(user_data.timestamp, EXCLUDED.timestamp)
                "#,
                    data.fid as i64,
                    user_data.r#type as i16,
                    &msg.hash,
                    &user_data.value,
                    ts
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn insert_username_proof(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(UsernameProofBody(proof_body)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                // Extract the proof fields
                let name = String::from_utf8(proof_body.name.clone()).unwrap_or_default();

                sqlx::query!(
                    r#"
                INSERT INTO username_proofs (
                    fid,
                    username, 
                    timestamp, 
                    type,
                    signature,
                    owner
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (username, fid) 
                DO NOTHING
                "#,
                    data.fid as i64,
                    name,
                    ts,
                    proof_body.r#type as i16,
                    &proof_body.signature,
                    &proof_body.owner
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn process_username_proof(
        &self,
        proof: &UserNameProof,
        is_deleted: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Convert username from bytes to string
        let username = String::from_utf8(proof.name.clone()).unwrap_or_default();
        let ts = Self::convert_timestamp(proof.timestamp as u32);

        // Use a single UPSERT pattern with conditional handling of deleted_at
        // This handles both additions and deletions with a consistent approach
        sqlx::query!(
            r#"
            INSERT INTO username_proofs (
                fid,
                username,
                timestamp,
                type,
                signature,
                owner,
                deleted_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (username, fid) DO UPDATE SET
                deleted_at = CASE 
                    WHEN $7 IS NOT NULL THEN $7
                    ELSE username_proofs.deleted_at
                END
            "#,
            proof.fid as i64,
            username,
            ts,
            proof.r#type as i16,
            &proof.signature,
            &proof.owner,
            if is_deleted { Some(ts) } else { None::<OffsetDateTime> }
        )
        .execute(&self.resources.database.pool)
        .await?;

        Ok(())
    }

    pub async fn process_onchain_event(
        &self,
        event: &OnChainEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ts = OffsetDateTime::from_unix_timestamp(event.block_timestamp as i64).unwrap();

        // Generate a deterministic hash for this onchain event
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&event.transaction_hash, &mut hasher);
        std::hash::Hash::hash(&event.log_index, &mut hasher);
        let hash = hasher.finish().to_be_bytes().to_vec();

        sqlx::query!(
            r#"
            INSERT INTO onchain_events (
                fid,
                hash,
                type,
                timestamp,
                block_number,
                block_hash,
                log_index,
                tx_index,
                tx_hash,
                block_timestamp,
                chain_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (hash) DO NOTHING
            "#,
            event.fid as i64,
            &hash,
            event.r#type as i16,
            ts,
            event.block_number as i64,
            &event.block_hash,
            event.log_index as i32,
            event.tx_index as i32,
            &event.transaction_hash,
            ts,
            event.chain_id as i64
        )
        .execute(&self.resources.database.pool)
        .await?;

        // Handle specific event types
        match event.r#type {
            1 => {
                // EVENT_TYPE_SIGNER
                if let Some(crate::proto::on_chain_event::Body::SignerEventBody(signer_body)) =
                    &event.body
                {
                    sqlx::query!(
                        r#"
                        INSERT INTO signer_events (
                            fid,
                            key,
                            key_type,
                            event_type,
                            metadata,
                            metadata_type,
                            timestamp,
                            block_number,
                            block_hash,
                            log_index,
                            tx_index,
                            tx_hash,
                            block_timestamp,
                            chain_id
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                        ON CONFLICT (tx_hash, log_index) DO NOTHING
                        "#,
                        event.fid as i64,
                        &signer_body.key,
                        signer_body.key_type as i16,
                        signer_body.event_type as i16,
                        if signer_body.metadata.is_empty() {
                            None
                        } else {
                            Some(&signer_body.metadata[..])
                        },
                        Some(signer_body.metadata_type as i16),
                        ts,
                        event.block_number as i64,
                        &event.block_hash,
                        event.log_index as i32,
                        event.tx_index as i32,
                        &event.transaction_hash,
                        ts,
                        event.chain_id as i64
                    )
                    .execute(&self.resources.database.pool)
                    .await?;
                }
            },
            2 => {
                // EVENT_TYPE_SIGNER_MIGRATED
                if let Some(crate::proto::on_chain_event::Body::SignerMigratedEventBody(
                    migrated_body,
                )) = &event.body
                {
                    sqlx::query!(
                        r#"
                        INSERT INTO signer_migrated_events (
                            fid,
                            migrated_at,
                            timestamp,
                            block_number,
                            block_hash,
                            log_index,
                            tx_index,
                            tx_hash,
                            block_timestamp,
                            chain_id
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                        ON CONFLICT (tx_hash, log_index) DO NOTHING
                        "#,
                        event.fid as i64,
                        migrated_body.migrated_at as i64,
                        ts,
                        event.block_number as i64,
                        &event.block_hash,
                        event.log_index as i32,
                        event.tx_index as i32,
                        &event.transaction_hash,
                        ts,
                        event.chain_id as i64
                    )
                    .execute(&self.resources.database.pool)
                    .await?;
                }
            },
            3 => {
                // EVENT_TYPE_ID_REGISTER
                if let Some(crate::proto::on_chain_event::Body::IdRegisterEventBody(
                    id_register_body,
                )) = &event.body
                {
                    sqlx::query!(
                        r#"
                        INSERT INTO id_register_events (
                            fid,
                            to_address,
                            event_type,
                            from_address,
                            recovery_address,
                            timestamp,
                            block_number,
                            block_hash,
                            log_index,
                            tx_index,
                            tx_hash,
                            block_timestamp,
                            chain_id
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                        ON CONFLICT (tx_hash, log_index) DO NOTHING
                        "#,
                        event.fid as i64,
                        &id_register_body.to,
                        id_register_body.event_type as i16,
                        if id_register_body.from.is_empty() {
                            None
                        } else {
                            Some(&id_register_body.from[..])
                        },
                        if id_register_body.recovery_address.is_empty() {
                            None
                        } else {
                            Some(&id_register_body.recovery_address[..])
                        },
                        ts,
                        event.block_number as i64,
                        &event.block_hash,
                        event.log_index as i32,
                        event.tx_index as i32,
                        &event.transaction_hash,
                        ts,
                        event.chain_id as i64
                    )
                    .execute(&self.resources.database.pool)
                    .await?;
                }
            },
            4 => {
                // EVENT_TYPE_STORAGE_RENT
                if let Some(crate::proto::on_chain_event::Body::StorageRentEventBody(
                    storage_body,
                )) = &event.body
                {
                    sqlx::query!(
                        r#"
                        INSERT INTO storage_rent_events (
                            fid,
                            payer,
                            units,
                            expiry,
                            timestamp,
                            block_number,
                            block_hash,
                            log_index,
                            tx_index,
                            tx_hash,
                            block_timestamp,
                            chain_id
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                        ON CONFLICT (tx_hash, log_index) DO NOTHING
                        "#,
                        event.fid as i64,
                        &storage_body.payer,
                        storage_body.units as i32,
                        storage_body.expiry as i32,
                        ts,
                        event.block_number as i64,
                        &event.block_hash,
                        event.log_index as i32,
                        event.tx_index as i32,
                        &event.transaction_hash,
                        ts,
                        event.chain_id as i64
                    )
                    .execute(&self.resources.database.pool)
                    .await?;
                }
            },
            5 => {
                // EVENT_TYPE_TIER_PURCHASE
                if let Some(crate::proto::on_chain_event::Body::TierPurchaseEventBody(tier_body)) =
                    &event.body
                {
                    sqlx::query!(
                        r#"
                        INSERT INTO tier_purchases (
                            fid,
                            tier_type,
                            for_days,
                            payer,
                            timestamp,
                            block_number,
                            block_hash,
                            log_index,
                            tx_index,
                            tx_hash,
                            block_timestamp,
                            chain_id
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                        ON CONFLICT (tx_hash, log_index) DO NOTHING
                        "#,
                        event.fid as i64,
                        tier_body.tier_type as i16,
                        tier_body.for_days as i64,
                        &tier_body.payer,
                        ts,
                        event.block_number as i64,
                        &event.block_hash,
                        event.log_index as i32,
                        event.tx_index as i32,
                        &event.transaction_hash,
                        ts,
                        event.chain_id as i64
                    )
                    .execute(&self.resources.database.pool)
                    .await?;
                }
            },
            _ => {
                // Unknown event type, already stored in generic onchain_events table
            },
        }

        Ok(())
    }

    async fn add_verification(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(VerificationAddAddressBody(verification)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                sqlx::query!(
                    r#"
                INSERT INTO verifications (
                    fid,
                    hash,
                    signer_address,
                    block_hash,
                    signature,
                    protocol,
                    timestamp
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (signer_address, fid) DO UPDATE SET
                    hash = EXCLUDED.hash,
                    block_hash = EXCLUDED.block_hash,
                    signature = EXCLUDED.signature,
                    timestamp = EXCLUDED.timestamp
                "#,
                    data.fid as i64,
                    &msg.hash,
                    &verification.address,
                    &verification.block_hash,
                    &verification.claim_signature,
                    verification.protocol as i16,
                    ts
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn remove_verification(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(VerificationRemoveBody(verification)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                // Process verification removal with CRDT semantics:
                // - Conflict resolution based on timestamp ordering
                // - For equal timestamps, removal wins
                // - Default values for required fields when creating placeholder records
                sqlx::query!(
                    r#"
                INSERT INTO verifications (
                    fid,
                    signer_address,
                    hash,
                    block_hash,
                    signature,
                    protocol,
                    timestamp,
                    deleted_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (signer_address, fid) DO UPDATE SET
                    deleted_at = CASE
                        WHEN EXCLUDED.timestamp >= verifications.timestamp THEN EXCLUDED.deleted_at
                        ELSE verifications.deleted_at
                    END,
                    timestamp = GREATEST(verifications.timestamp, EXCLUDED.timestamp)
                "#,
                    data.fid as i64,
                    &verification.address,
                    &msg.hash,
                    // Default values for required columns
                    &verification.address, // Using address as block_hash placeholder
                    &verification.address, // Using address as signature placeholder
                    0 as i16,              // Default protocol value
                    ts,
                    ts
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn add_lend_storage(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(LendStorageBody(lend_storage)) = &data.body {
                let ts = Self::convert_timestamp(data.timestamp);

                sqlx::query!(
                    r#"
                INSERT INTO lend_storage (
                    fid,
                    to_fid,
                    num_units,
                    unit_type,
                    hash,
                    timestamp,
                    deleted_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, NULL)
                ON CONFLICT (hash) DO UPDATE SET
                    deleted_at = NULL,
                    timestamp = LEAST(EXCLUDED.timestamp, lend_storage.timestamp)
                "#,
                    data.fid as i64,
                    lend_storage.to_fid as i64,
                    lend_storage.num_units as i64,
                    lend_storage.unit_type as i16,
                    &msg.hash,
                    ts
                )
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    pub async fn process_message(
        &self,
        msg: &Message,
        operation: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            let ts = Self::convert_timestamp(data.timestamp);

            // Determine if we should store messages in the messages table
            let store_messages = self.resources.config.database.store_messages;

            // Store message in messages table only if configured to do so
            if store_messages {
                let raw_data = msg.data_bytes.as_deref().unwrap_or_default();

                // Store message in messages table with transaction
                let result = sqlx::query!(
                    r#"
            INSERT INTO messages (
                fid, type, timestamp, hash, hash_scheme, signature_scheme, signer, body, raw,
                deleted_at, pruned_at, revoked_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (hash, fid, type) DO UPDATE SET
                deleted_at = EXCLUDED.deleted_at,
                pruned_at = EXCLUDED.pruned_at,
                revoked_at = EXCLUDED.revoked_at
            "#,
                    data.fid as i64,
                    data.r#type as i16,
                    ts,
                    &msg.hash,
                    msg.hash_scheme as i16,
                    msg.signature_scheme as i16,
                    &msg.signer,
                    serde_json::to_value(data)?,
                    raw_data,
                    match operation {
                        "delete" => Some(OffsetDateTime::now_utc()),
                        _ => None,
                    },
                    match operation {
                        "prune" => Some(OffsetDateTime::now_utc()),
                        _ => None,
                    },
                    match operation {
                        "revoke" => Some(OffsetDateTime::now_utc()),
                        _ => None,
                    }
                )
                .execute(&self.resources.database.pool)
                .await;

                match result {
                    Ok(_) => {},
                    Err(e) => {
                        if e.to_string().contains("EOF") || e.to_string().contains("timed out") {
                            error!("Fatal database error in messages table: {}", e);
                            std::process::exit(1);
                        }
                        return Err(e.into());
                    },
                }
            }

            // Process message type-specific operations
            let type_result = match data.r#type {
                1 => self.add_cast(&self.resources.database.pool, msg).await,
                2 => self.remove_cast(&self.resources.database.pool, msg).await,
                3 => self.add_reaction(&self.resources.database.pool, msg).await,
                4 => self.remove_reaction(&self.resources.database.pool, msg).await,
                5 => self.add_link(&self.resources.database.pool, msg).await,
                6 => self.remove_link(&self.resources.database.pool, msg).await,
                7 => self.add_verification(&self.resources.database.pool, msg).await,
                8 => self.remove_verification(&self.resources.database.pool, msg).await,
                11 => self.insert_user_data(&self.resources.database.pool, msg).await,
                12 => self.insert_username_proof(&self.resources.database.pool, msg).await,
                15 => self.add_lend_storage(&self.resources.database.pool, msg).await,
                _ => Ok(()),
            };

            if let Err(e) = type_result {
                if e.to_string().contains("EOF") || e.to_string().contains("timed out") {
                    error!("Fatal database error in type-specific operation: {}", e);
                    std::process::exit(1);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Process a batch of messages in bulk for improved efficiency
    pub async fn process_message_batch(
        &self,
        messages: &[Message],
        operation: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if messages.is_empty() {
            return Ok(());
        }

        // Get the batch size from configuration
        let batch_size = self.resources.config.database.batch_size;

        // Create a batch inserter with our database pool and configured batch size
        let batch_inserter = BatchInserter::new(&self.resources.database.pool, batch_size);

        // For operations other than "merge" (like delete, prune, revoke),
        // we'll process messages individually since they have special handling
        if operation != "merge" {
            for msg in messages {
                if let Err(e) = self.process_message(msg, operation).await {
                    error!("Error processing message in batch (op={}): {}", operation, e);
                    return Err(e);
                }
            }
            return Ok(());
        }

        // For normal merge operations, use the batch inserter
        // This groups messages by type and inserts them in bulk
        match batch_inserter.process_message_batch(messages).await {
            Ok(_) => {
                trace!("Successfully processed batch of {} messages", messages.len());
                Ok(())
            },
            Err(e) => {
                error!("Error in batch processing: {}", e);

                // If we encounter a database error, try processing messages individually as fallback
                if e.to_string().contains("EOF") || e.to_string().contains("timed out") {
                    error!("Fatal database error in batch processing: {}", e);
                    std::process::exit(1);
                }

                Err(e)
            },
        }
    }

    pub fn create_handlers(
        processor: Arc<Self>,
    ) -> (Option<PreProcessHandler>, Option<PostProcessHandler>) {
        let pre_process = Arc::new(move |events: &[HubEvent], _: &[Vec<u8>]| {
            let events = events.to_owned();
            let processor = processor.clone();

            Box::pin(async move {
                // Create a vector to store processing tasks
                let mut processing_tasks = Vec::with_capacity(events.len());

                // Create a semaphore to limit concurrent database operations
                // This prevents overwhelming the database connection pool
                // The value is chosen to match approximately half of a typical PostgreSQL
                // connection pool size, allowing other operations to proceed
                let db_semaphore = Arc::new(tokio::sync::Semaphore::new(10));

                // Prepare processing tasks in parallel using rayon
                let task_data: Vec<_> = events
                    .par_iter()
                    .enumerate()
                    .map(|(idx, event)| {
                        let event_type = event.r#type;
                        let event_body = event.body.clone();
                        (idx, event_type, event_body)
                    })
                    .collect();

                // Process the prepared tasks asynchronously with controlled concurrency
                for (_, event_type, event_body) in task_data {
                    // Clone the semaphore for each task
                    let semaphore = Arc::clone(&db_semaphore);
                    match event_type {
                        1 => {
                            // MERGE_MESSAGE
                            if let Some(Body::MergeMessageBody(body)) = event_body {
                                if let Some(msg) = body.message {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        // This will block if too many tasks are already accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_message(&msg, "merge").await {
                                            error!("Error processing message: {}", e);
                                        }
                                        // Permit is automatically dropped when this task completes
                                    });
                                    processing_tasks.push(task);
                                }
                                // Process deleted messages
                                for deleted_msg in body.deleted_messages {
                                    let proc = processor.clone();
                                    let msg = deleted_msg.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_message(&msg, "delete").await {
                                            error!("Error processing deleted message: {}", e);
                                        }
                                    });
                                    processing_tasks.push(task);
                                }
                            }
                        },
                        2 => {
                            // PRUNE_MESSAGE
                            if let Some(Body::PruneMessageBody(body)) = event_body {
                                if let Some(msg) = body.message {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_message(&msg, "prune").await {
                                            error!("Error processing message: {}", e);
                                        }
                                    });
                                    processing_tasks.push(task);
                                }
                            }
                        },
                        3 => {
                            // REVOKE_MESSAGE
                            if let Some(Body::RevokeMessageBody(body)) = event_body {
                                if let Some(msg) = body.message {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_message(&msg, "revoke").await {
                                            error!("Error processing message: {}", e);
                                        }
                                    });
                                    processing_tasks.push(task);
                                }
                            }
                        },
                        6 => {
                            // MERGE_USERNAME_PROOF
                            if let Some(Body::MergeUsernameProofBody(body)) = event_body {
                                // Process username proof message if available
                                if let Some(msg) = body.username_proof_message {
                                    let proc = processor.clone();
                                    let task = tokio::spawn(async move {
                                        if let Err(e) = proc.process_message(&msg, "merge").await {
                                            error!(
                                                "Error processing username proof message: {}",
                                                e
                                            );
                                        }
                                    });
                                    processing_tasks.push(task);
                                }

                                // Process deleted username proof message if available
                                if let Some(msg) = body.deleted_username_proof_message {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_message(&msg, "delete").await {
                                            error!(
                                                "Error processing deleted username proof message: {}",
                                                e
                                            );
                                        }
                                    });
                                    processing_tasks.push(task);
                                }

                                // Process the username proof directly
                                if let Some(proof) = body.username_proof {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) =
                                            proc.process_username_proof(&proof, false).await
                                        {
                                            error!("Error processing username proof: {}", e);
                                        }
                                    });
                                    processing_tasks.push(task);
                                }

                                // Process deleted username proof
                                if let Some(proof) = body.deleted_username_proof {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) =
                                            proc.process_username_proof(&proof, true).await
                                        {
                                            error!(
                                                "Error processing deleted username proof: {}",
                                                e
                                            );
                                        }
                                    });
                                    processing_tasks.push(task);
                                }
                            }
                        },
                        9 => {
                            // MERGE_ON_CHAIN_EVENT
                            if let Some(Body::MergeOnChainEventBody(body)) = event_body {
                                if let Some(event) = body.on_chain_event {
                                    let proc = processor.clone();
                                    let semaphore_clone = Arc::clone(&semaphore);
                                    let task = tokio::spawn(async move {
                                        // Acquire a permit from the semaphore before accessing the database
                                        let _permit = semaphore_clone.acquire().await.unwrap();

                                        if let Err(e) = proc.process_onchain_event(&event).await {
                                            error!("Error processing onchain event: {}", e);
                                        }
                                    });
                                    processing_tasks.push(task);
                                }
                            }
                        },
                        _ => {},
                    }
                }

                // Wait for all tasks to complete
                for task in processing_tasks {
                    let _ = task.await;
                }

                // Return results (all false since we're not filtering)
                events.iter().map(|_| false).collect()
            }) as BoxFuture<'static, Vec<bool>>
        });

        // Post-process handler - collects metrics on processed events
        let post_process = Arc::new(move |events: &[HubEvent], _: &[Vec<u8>]| {
            let events_count = events.len();
            let event_types = events.iter().map(|e| e.r#type).fold(
                std::collections::HashMap::new(),
                |mut acc, t| {
                    *acc.entry(t).or_insert(0) += 1;
                    acc
                },
            );

            Box::pin(async move {
                // Log metrics occasionally (e.g., every 1000 batches)
                static COUNTER: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                if count % 1000 == 0 && events_count > 0 {
                    debug!(
                        "Processed batch #{}: {} events with type distribution: {:?}",
                        count, events_count, event_types
                    );
                }

                // Could be extended to:
                // - Track processing rate
                // - Monitor event type distribution
                // - Record metrics for monitoring systems
                // - Detect anomalies in processing patterns
            }) as BoxFuture<'static, ()>
        });

        (Some(pre_process), Some(post_process))
    }
}

#[async_trait]
impl EventProcessor for DatabaseProcessor {
    /// Implementation of the as_any method to allow downcasting
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn process_event(
        &self,
        event: HubEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &event.body {
            Some(Body::MergeMessageBody(body)) => {
                // Process the main message
                if let Some(msg) = &body.message {
                    // For individual messages, still use the single message approach
                    self.process_message(msg, "merge").await?;
                }

                // Process all deleted messages
                let deleted_messages: Vec<_> = body.deleted_messages.iter().collect();
                if !deleted_messages.is_empty() {
                    // For multiple deleted messages, use batch processing
                    if deleted_messages.len() > 1 {
                        self.process_message_batch(&body.deleted_messages, "delete").await?;
                    } else {
                        // For a single deleted message, use the single message approach
                        self.process_message(deleted_messages[0], "delete").await?;
                    }
                }
            },
            Some(Body::PruneMessageBody(body)) => {
                if let Some(msg) = &body.message {
                    self.process_message(msg, "prune").await?;
                }
            },
            Some(Body::RevokeMessageBody(body)) => {
                if let Some(msg) = &body.message {
                    self.process_message(msg, "revoke").await?;
                }
            },
            Some(Body::MergeUsernameProofBody(body)) => {
                // Process username proof message if available
                if let Some(msg) = &body.username_proof_message {
                    self.process_message(msg, "merge").await?;
                }

                // Process deleted username proof message if available
                if let Some(msg) = &body.deleted_username_proof_message {
                    self.process_message(msg, "delete").await?;
                }

                // Process the username proof directly
                if let Some(proof) = &body.username_proof {
                    self.process_username_proof(proof, false).await?;
                }

                // Process deleted username proof
                if let Some(proof) = &body.deleted_username_proof {
                    self.process_username_proof(proof, true).await?;
                }
            },
            Some(Body::MergeOnChainEventBody(body)) => {
                if let Some(event) = &body.on_chain_event {
                    self.process_onchain_event(event).await?;
                }
            },
            _ => {},
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{TierPurchaseBody, TierType};

    #[test]
    fn test_convert_timestamp() {
        // Test conversion from farcaster time to OffsetDateTime
        let farcaster_timestamp = 100000000u32; // Example farcaster timestamp
        let result = DatabaseProcessor::convert_timestamp(farcaster_timestamp);

        // Verify it's a valid timestamp
        assert!(result.unix_timestamp() > 0);
    }

    #[test]
    fn test_tier_purchase_event_creation() {
        // Create a tier purchase body
        let tier_body = TierPurchaseBody {
            tier_type: TierType::Pro as i32,
            for_days: 365,
            payer: vec![0x01, 0x02, 0x03], // Example payer address
        };

        // Create an onchain event with tier purchase
        let event = OnChainEvent {
            r#type: 5, // EVENT_TYPE_TIER_PURCHASE
            fid: 12345,
            chain_id: 10, // Optimism
            block_number: 1000000,
            block_hash: vec![0xaa; 32],
            block_timestamp: 1700000000,
            transaction_hash: vec![0xbb; 32],
            log_index: 5,
            tx_index: 10,
            version: 1,
            body: Some(crate::proto::on_chain_event::Body::TierPurchaseEventBody(tier_body)),
        };

        // Verify the event is properly constructed
        assert_eq!(event.r#type, 5);
        assert_eq!(event.fid, 12345);

        if let Some(crate::proto::on_chain_event::Body::TierPurchaseEventBody(body)) = &event.body {
            assert_eq!(body.tier_type, TierType::Pro as i32);
            assert_eq!(body.for_days, 365);
            assert_eq!(body.payer, vec![0x01, 0x02, 0x03]);
        } else {
            panic!("Expected tier purchase body");
        }
    }

    #[test]
    fn test_onchain_event_hash_generation() {
        use std::hash::Hasher;

        // Test deterministic hash generation for onchain events
        let tx_hash = vec![0xaa; 32];
        let log_index = 5u32;

        let mut hasher1 = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&tx_hash, &mut hasher1);
        std::hash::Hash::hash(&log_index, &mut hasher1);
        let hash1 = hasher1.finish().to_be_bytes().to_vec();

        let mut hasher2 = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&tx_hash, &mut hasher2);
        std::hash::Hash::hash(&log_index, &mut hasher2);
        let hash2 = hasher2.finish().to_be_bytes().to_vec();

        // Verify deterministic hashing
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 8); // 64-bit hash as 8 bytes
    }

    #[test]
    fn test_extract_parent_from_proto_no_parent() {
        // Create a cast message with no parent (root cast)
        let msg = Message {
            hash: vec![0x01; 20],
            data: Some(crate::proto::MessageData {
                fid: 12345,
                timestamp: 1700000000,
                network: 1,
                r#type: 1, // CastAdd
                body: Some(CastAddBody(crate::proto::CastAddBody {
                    text: "Hello world".to_string(),
                    parent: None,
                    embeds: vec![],
                    mentions: vec![],
                    mentions_positions: vec![],
                    embeds_deprecated: vec![],
                    r#type: 0,
                })),
            }),
            hash_scheme: 1,
            signature: vec![],
            signature_scheme: 1,
            signer: vec![],
            data_bytes: None,
        };

        let (parent_fid, parent_hash, parent_url) = super::extract_parent_from_proto(&msg);
        assert!(parent_fid.is_none());
        assert!(parent_hash.is_none());
        assert!(parent_url.is_none());
    }

    #[test]
    fn test_extract_parent_from_proto_with_cast_parent() {
        use crate::proto::CastId;
        use crate::proto::cast_add_body::Parent;

        // Create a cast message with a cast parent
        let msg = Message {
            hash: vec![0x02; 20],
            data: Some(crate::proto::MessageData {
                fid: 12345,
                timestamp: 1700000000,
                network: 1,
                r#type: 1, // CastAdd
                body: Some(CastAddBody(crate::proto::CastAddBody {
                    text: "Reply to cast".to_string(),
                    parent: Some(Parent::ParentCastId(CastId { fid: 67890, hash: vec![0xAA; 20] })),
                    embeds: vec![],
                    mentions: vec![],
                    mentions_positions: vec![],
                    embeds_deprecated: vec![],
                    r#type: 0,
                })),
            }),
            hash_scheme: 1,
            signature: vec![],
            signature_scheme: 1,
            signer: vec![],
            data_bytes: None,
        };

        let (parent_fid, parent_hash, parent_url) = super::extract_parent_from_proto(&msg);
        assert_eq!(parent_fid, Some(67890));
        assert_eq!(parent_hash, Some(vec![0xAA; 20]));
        assert!(parent_url.is_none());
    }

    #[test]
    fn test_extract_parent_from_proto_with_url_parent() {
        use crate::proto::cast_add_body::Parent;

        // Create a cast message with a URL parent (channel post)
        let msg = Message {
            hash: vec![0x03; 20],
            data: Some(crate::proto::MessageData {
                fid: 12345,
                timestamp: 1700000000,
                network: 1,
                r#type: 1, // CastAdd
                body: Some(CastAddBody(crate::proto::CastAddBody {
                    text: "Channel post".to_string(),
                    parent: Some(Parent::ParentUrl(
                        "https://warpcast.com/~/channel/rust".to_string(),
                    )),
                    embeds: vec![],
                    mentions: vec![],
                    mentions_positions: vec![],
                    embeds_deprecated: vec![],
                    r#type: 0,
                })),
            }),
            hash_scheme: 1,
            signature: vec![],
            signature_scheme: 1,
            signer: vec![],
            data_bytes: None,
        };

        let (parent_fid, parent_hash, parent_url) = super::extract_parent_from_proto(&msg);
        assert!(parent_fid.is_none());
        assert!(parent_hash.is_none());
        assert_eq!(parent_url, Some("https://warpcast.com/~/channel/rust".to_string()));
    }
}
