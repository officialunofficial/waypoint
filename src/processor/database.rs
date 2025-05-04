use crate::{
    core::{normalize::NormalizedEmbed, util::from_farcaster_time},
    database::batch::BatchInserter,
    hub::subscriber::{PostProcessHandler, PreProcessHandler},
    processor::consumer::EventProcessor,
    proto::{
        HubEvent, Message, OnChainEvent, UserNameProof,
        cast_add_body::Parent,
        hub_event::Body,
        link_body::Target as LinkTarget,
        message_data::Body::{
            CastAddBody, CastRemoveBody, LinkBody, ReactionBody, UserDataBody, UsernameProofBody,
            VerificationAddAddressBody, VerificationRemoveBody,
        },
        reaction_body::Target as ReactionTarget,
    },
};
use async_trait::async_trait;
use futures::future::BoxFuture;
use rayon::prelude::*;
use sqlx::{postgres::PgPool, types::time::OffsetDateTime};
use std::hash::Hasher;
use std::any::Any;
use std::sync::Arc;
use tracing::{debug, error};

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

    async fn add_cast(
        &self,
        pool: &PgPool,
        msg: &Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = &msg.data {
            if let Some(CastAddBody(cast_body)) = &data.body {
                let parent_url = match &cast_body.parent {
                    Some(Parent::ParentUrl(url)) => Some(url.as_str()),
                    _ => None,
                };

                let parent_hash = match &cast_body.parent {
                    Some(Parent::ParentCastId(cast_id)) => Some(&cast_id.hash),
                    _ => None,
                };

                let ts = Self::convert_timestamp(data.timestamp);

                sqlx::query!(
                r#"
                INSERT INTO casts (fid, hash, text, parent_hash, parent_url, timestamp, embeds, mentions, mentions_positions)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (hash) DO UPDATE SET
                    text = EXCLUDED.text,
                    parent_hash = EXCLUDED.parent_hash,
                    parent_url = EXCLUDED.parent_url,
                    timestamp = EXCLUDED.timestamp,
                    embeds = EXCLUDED.embeds,
                    mentions = EXCLUDED.mentions,
                    mentions_positions = EXCLUDED.mentions_positions
                "#,
                data.fid as i64,
                &msg.hash,
                &cast_body.text,
                parent_hash,
                parent_url,
                ts,
                serde_json::to_value(
                    cast_body.embeds
                        .iter()
                        .map(NormalizedEmbed::from_protobuf_embed)
                        .collect::<Vec<_>>()
                )?,
                serde_json::to_value(&cast_body.mentions)?,
                serde_json::to_value(&cast_body.mentions_positions)?,
            ).execute(pool).await?;
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
                INSERT INTO casts (fid, hash, deleted_at, timestamp, text, embeds, mentions, mentions_positions)
                VALUES ($1, $2, $3, $4, '', '[]'::json, '[]'::json, '[]'::json)
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
                let ts = Self::convert_timestamp(data.timestamp);

                sqlx::query!(
                    r#"
                INSERT INTO user_data (fid, type, hash, value, timestamp)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (fid, type) DO UPDATE SET
                    value = EXCLUDED.value,
                    hash = EXCLUDED.hash,
                    timestamp = EXCLUDED.timestamp
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

    async fn process_onchain_event(
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
                debug!("Successfully processed batch of {} messages", messages.len());
                Ok(())
            }
            Err(e) => {
                error!("Error in batch processing: {}", e);
                
                // If we encounter a database error, try processing messages individually as fallback
                if e.to_string().contains("EOF") || e.to_string().contains("timed out") {
                    error!("Fatal database error in batch processing: {}", e);
                    std::process::exit(1);
                }
                
                Err(e)
            }
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
