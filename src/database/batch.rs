use crate::{
    core::normalize::NormalizedEmbed,
    database::models::Fid,
    proto::{
        Message,
        cast_add_body::Parent,
        link_body::Target as LinkTarget,
        message_data::Body::{
            CastAddBody, LinkBody, ReactionBody, UserDataBody, UsernameProofBody,
            VerificationAddAddressBody,
        },
        reaction_body::Target as ReactionTarget,
    },
};
use serde_json::Value;
use sqlx::{postgres::PgPool, types::time::OffsetDateTime};
use std::collections::HashMap;
use std::error::Error;
use tracing::{debug, error};

/// Default batch size for database operations
#[allow(dead_code)]
const DEFAULT_BATCH_SIZE: usize = 100;

/// Utility for building SQL for bulk inserts
pub fn build_insert_sql(
    table_name: &str,
    columns: &[&str],
    item_count: usize,
    conflict_target: &str,
    update_actions: &[&str],
) -> String {
    if item_count == 0 {
        return String::new();
    }

    let placeholders_per_row = columns.len();
    let values = (0..item_count)
        .map(|i| {
            let start_idx = i * placeholders_per_row + 1;
            let placeholders = (start_idx..start_idx + placeholders_per_row)
                .map(|j| format!("${}", j))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", placeholders)
        })
        .collect::<Vec<_>>()
        .join(",\n       ");

    let update_clause = if update_actions.is_empty() {
        "DO NOTHING".to_string()
    } else {
        format!("DO UPDATE SET\n    {}", update_actions.join(",\n    "))
    };

    format!(
        r#"INSERT INTO {} ({})
VALUES {}
ON CONFLICT ({}) {}"#,
        table_name,
        columns.join(", "),
        values,
        conflict_target,
        update_clause
    )
}

/// Convert from Farcaster timestamp to OffsetDateTime
pub fn convert_timestamp(timestamp: u32) -> OffsetDateTime {
    // Convert from farcaster time (seconds since epoch) to unix seconds
    let unix_time = crate::core::util::from_farcaster_time(timestamp) / 1000; // Convert ms to seconds
    OffsetDateTime::from_unix_timestamp(unix_time as i64).unwrap()
}

/// Structure for holding cast data for bulk inserts
#[derive(Debug)]
pub struct CastInsert<'a> {
    pub fid: Fid,
    pub hash: &'a [u8],
    pub text: &'a str,
    pub parent_hash: Option<&'a [u8]>,
    pub parent_url: Option<&'a str>,
    pub timestamp: OffsetDateTime,
    pub embeds: Value,
    pub mentions: Value,
    pub mentions_positions: Value,
}

impl<'a> CastInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let cast_body = match &data.body {
            Some(CastAddBody(body)) => body,
            _ => return None,
        };

        let parent_url = match &cast_body.parent {
            Some(Parent::ParentUrl(url)) => Some(url.as_str()),
            _ => None,
        };

        let parent_hash = match &cast_body.parent {
            Some(Parent::ParentCastId(cast_id)) => Some(cast_id.hash.as_slice()),
            _ => None,
        };

        let timestamp = convert_timestamp(data.timestamp);

        // Convert embeds to normalized form
        let embeds = serde_json::to_value(
            cast_body.embeds.iter().map(NormalizedEmbed::from_protobuf_embed).collect::<Vec<_>>(),
        )
        .ok()?;

        // Convert other fields
        let mentions = serde_json::to_value(&cast_body.mentions).ok()?;
        let mentions_positions = serde_json::to_value(&cast_body.mentions_positions).ok()?;

        Some(Self {
            fid: data.fid,
            hash: &msg.hash,
            text: &cast_body.text,
            parent_hash,
            parent_url,
            timestamp,
            embeds,
            mentions,
            mentions_positions,
        })
    }
}

/// Structure for holding reaction data for bulk inserts
#[derive(Debug)]
pub struct ReactionInsert<'a> {
    pub fid: Fid,
    pub target_fid: Fid,
    pub hash: &'a [u8],
    pub reaction_type: i16,
    pub target_hash: Option<&'a [u8]>,
    pub target_url: Option<&'a str>,
    pub timestamp: OffsetDateTime,
}

impl<'a> ReactionInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let reaction_body = match &data.body {
            Some(ReactionBody(body)) => body,
            _ => return None,
        };

        let target_hash = match &reaction_body.target {
            Some(ReactionTarget::TargetCastId(cast_id)) => Some(cast_id.hash.as_slice()),
            _ => None,
        };

        let target_url = match &reaction_body.target {
            Some(ReactionTarget::TargetUrl(url)) => Some(url.as_str()),
            _ => None,
        };

        let target_fid = match &reaction_body.target {
            Some(ReactionTarget::TargetCastId(cast_id)) => cast_id.fid,
            _ => 0, // Default value for URL targets
        };

        let timestamp = convert_timestamp(data.timestamp);

        Some(Self {
            fid: data.fid,
            target_fid,
            hash: &msg.hash,
            reaction_type: reaction_body.r#type as i16,
            target_hash,
            target_url,
            timestamp,
        })
    }
}

/// Structure for holding link data for bulk inserts
#[derive(Debug)]
pub struct LinkInsert<'a> {
    pub fid: Fid,
    pub target_fid: Fid,
    pub hash: &'a [u8],
    pub link_type: &'a str,
    pub timestamp: OffsetDateTime,
    pub display_timestamp: OffsetDateTime,
}

impl<'a> LinkInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let link_body = match &data.body {
            Some(LinkBody(body)) => body,
            _ => return None,
        };

        let target_fid = match &link_body.target {
            Some(LinkTarget::TargetFid(fid)) => *fid,
            _ => return None, // Links require a target FID
        };

        let timestamp = convert_timestamp(data.timestamp);

        Some(Self {
            fid: data.fid,
            target_fid,
            hash: &msg.hash,
            link_type: &link_body.r#type,
            timestamp,
            display_timestamp: timestamp, // Same as timestamp for now
        })
    }
}

/// Structure for holding user data for bulk inserts
#[derive(Debug)]
pub struct UserDataInsert<'a> {
    pub fid: Fid,
    pub hash: &'a [u8],
    pub user_data_type: i16,
    pub value: &'a str,
    pub timestamp: OffsetDateTime,
}

impl<'a> UserDataInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let user_data_body = match &data.body {
            Some(UserDataBody(body)) => body,
            _ => return None,
        };

        let timestamp = convert_timestamp(data.timestamp);

        Some(Self {
            fid: data.fid,
            hash: &msg.hash,
            user_data_type: user_data_body.r#type as i16,
            value: &user_data_body.value,
            timestamp,
        })
    }
}

/// Structure for holding verification data for bulk inserts
#[derive(Debug)]
pub struct VerificationInsert<'a> {
    pub fid: Fid,
    pub hash: &'a [u8],
    pub signer_address: &'a [u8],
    pub block_hash: &'a [u8],
    pub signature: &'a [u8],
    pub protocol: i16,
    pub timestamp: OffsetDateTime,
}

impl<'a> VerificationInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let verification_body = match &data.body {
            Some(VerificationAddAddressBody(body)) => body,
            _ => return None,
        };

        let timestamp = convert_timestamp(data.timestamp);

        Some(Self {
            fid: data.fid,
            hash: &msg.hash,
            signer_address: &verification_body.address,
            block_hash: &verification_body.block_hash,
            signature: &verification_body.claim_signature,
            protocol: 0, // Default to Ethereum
            timestamp,
        })
    }
}

/// Structure for holding username proof data for bulk inserts
#[derive(Debug)]
pub struct UsernameProofInsert<'a> {
    pub fid: Fid,
    pub hash: &'a [u8],
    pub username: &'a [u8],
    pub timestamp: OffsetDateTime,
    pub signature: &'a [u8],
}

impl<'a> UsernameProofInsert<'a> {
    pub fn from_message(msg: &'a Message) -> Option<Self> {
        let data = msg.data.as_ref()?;
        let proof_body = match &data.body {
            Some(UsernameProofBody(body)) => body,
            _ => return None,
        };

        let timestamp = convert_timestamp(data.timestamp);

        Some(Self {
            fid: data.fid,
            hash: &msg.hash,
            username: &proof_body.name,
            timestamp,
            signature: &proof_body.signature,
        })
    }
}

/// Batch inserter for Farcaster message data
pub struct BatchInserter<'a> {
    pool: &'a PgPool,
    batch_size: usize,
}

impl<'a> BatchInserter<'a> {
    /// Create a new batch inserter with the given pool and batch size
    pub fn new(pool: &'a PgPool, batch_size: usize) -> Self {
        Self { pool, batch_size }
    }

    /// Set the batch size for this inserter
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Group messages by their message type
    pub fn group_messages_by_type(messages: &[Message]) -> HashMap<i32, Vec<&Message>> {
        let mut grouped = HashMap::new();

        for msg in messages {
            if let Some(data) = &msg.data {
                grouped.entry(data.r#type).or_insert_with(Vec::new).push(msg);
            }
        }

        grouped
    }

    /// Bulk insert messages into the messages table
    pub async fn bulk_insert_messages(
        &self,
        messages: &[Message],
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if messages.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in messages.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "messages",
                &[
                    "fid",
                    "type",
                    "timestamp",
                    "hash",
                    "hash_scheme",
                    "signature_scheme",
                    "signer",
                    "body",
                    "raw",
                    "deleted_at",
                    "pruned_at",
                    "revoked_at",
                ],
                chunk.len(),
                "hash, fid, type",
                &[
                    "deleted_at = EXCLUDED.deleted_at",
                    "pruned_at = EXCLUDED.pruned_at",
                    "revoked_at = EXCLUDED.revoked_at",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each message
            for msg in chunk {
                if let Some(data) = &msg.data {
                    let ts = convert_timestamp(data.timestamp);
                    let raw_data = msg.data_bytes.as_deref().unwrap_or_default();
                    let body_json = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);

                    query = query
                        .bind(data.fid as i64)
                        .bind(data.r#type as i16)
                        .bind(ts)
                        .bind(&msg.hash)
                        .bind(msg.hash_scheme as i16)
                        .bind(msg.signature_scheme as i16)
                        .bind(&msg.signer)
                        .bind(body_json)
                        .bind(raw_data)
                        .bind::<Option<OffsetDateTime>>(None) // deleted_at
                        .bind::<Option<OffsetDateTime>>(None) // pruned_at
                        .bind::<Option<OffsetDateTime>>(None); // revoked_at
                }
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert casts
    pub async fn bulk_insert_casts(
        &self,
        casts: Vec<CastInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if casts.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in casts.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "casts",
                &[
                    "fid",
                    "hash",
                    "text",
                    "parent_hash",
                    "parent_url",
                    "timestamp",
                    "embeds",
                    "mentions",
                    "mentions_positions",
                ],
                chunk.len(),
                "hash",
                &[
                    "text = EXCLUDED.text",
                    "parent_hash = EXCLUDED.parent_hash",
                    "parent_url = EXCLUDED.parent_url",
                    "timestamp = EXCLUDED.timestamp",
                    "embeds = EXCLUDED.embeds",
                    "mentions = EXCLUDED.mentions",
                    "mentions_positions = EXCLUDED.mentions_positions",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each cast
            for cast in chunk {
                query = query
                    .bind(cast.fid as i64)
                    .bind(cast.hash)
                    .bind(cast.text)
                    .bind(cast.parent_hash)
                    .bind(cast.parent_url)
                    .bind(cast.timestamp)
                    .bind(&cast.embeds)
                    .bind(&cast.mentions)
                    .bind(&cast.mentions_positions);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert reactions
    pub async fn bulk_insert_reactions(
        &self,
        reactions: Vec<ReactionInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if reactions.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in reactions.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "reactions",
                &[
                    "fid",
                    "target_fid",
                    "hash",
                    "reaction_type",
                    "target_hash",
                    "target_url",
                    "timestamp",
                ],
                chunk.len(),
                "hash",
                &[
                    "target_fid = EXCLUDED.target_fid",
                    "reaction_type = EXCLUDED.reaction_type",
                    "target_hash = EXCLUDED.target_hash",
                    "target_url = EXCLUDED.target_url",
                    "timestamp = EXCLUDED.timestamp",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each reaction
            for reaction in chunk {
                query = query
                    .bind(reaction.fid as i64)
                    .bind(reaction.target_fid as i64)
                    .bind(reaction.hash)
                    .bind(reaction.reaction_type)
                    .bind(reaction.target_hash)
                    .bind(reaction.target_url)
                    .bind(reaction.timestamp);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert links
    pub async fn bulk_insert_links(
        &self,
        links: Vec<LinkInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if links.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in links.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "links",
                &["fid", "target_fid", "hash", "type", "timestamp", "display_timestamp"],
                chunk.len(),
                "hash",
                &[
                    "type = EXCLUDED.type",
                    "timestamp = EXCLUDED.timestamp",
                    "display_timestamp = EXCLUDED.display_timestamp",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each link
            for link in chunk {
                query = query
                    .bind(link.fid as i64)
                    .bind(link.target_fid as i64)
                    .bind(link.hash)
                    .bind(link.link_type)
                    .bind(link.timestamp)
                    .bind(link.display_timestamp);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert user data
    pub async fn bulk_insert_user_data(
        &self,
        user_data: Vec<UserDataInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if user_data.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in user_data.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "user_data",
                &["fid", "hash", "type", "value", "timestamp"],
                chunk.len(),
                "fid, type",
                &[
                    "hash = CASE WHEN EXCLUDED.timestamp >= user_data.timestamp THEN EXCLUDED.hash ELSE user_data.hash END",
                    "value = CASE WHEN EXCLUDED.timestamp >= user_data.timestamp THEN EXCLUDED.value ELSE user_data.value END",
                    "timestamp = GREATEST(user_data.timestamp, EXCLUDED.timestamp)",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each user data entry
            for data in chunk {
                query = query
                    .bind(data.fid as i64)
                    .bind(data.hash)
                    .bind(data.user_data_type)
                    .bind(data.value)
                    .bind(data.timestamp);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert verifications
    pub async fn bulk_insert_verifications(
        &self,
        verifications: Vec<VerificationInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if verifications.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in verifications.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "verifications",
                &[
                    "fid",
                    "hash",
                    "signer_address",
                    "block_hash",
                    "signature",
                    "protocol",
                    "timestamp",
                    "deleted_at",
                ],
                chunk.len(),
                "signer_address, fid",
                &[
                    "hash = CASE WHEN EXCLUDED.timestamp >= verifications.timestamp THEN EXCLUDED.hash ELSE verifications.hash END",
                    "block_hash = CASE WHEN EXCLUDED.timestamp >= verifications.timestamp THEN EXCLUDED.block_hash ELSE verifications.block_hash END",
                    "signature = CASE WHEN EXCLUDED.timestamp >= verifications.timestamp THEN EXCLUDED.signature ELSE verifications.signature END",
                    "protocol = CASE WHEN EXCLUDED.timestamp >= verifications.timestamp THEN EXCLUDED.protocol ELSE verifications.protocol END",
                    "timestamp = GREATEST(verifications.timestamp, EXCLUDED.timestamp)",
                    "deleted_at = NULL",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each verification
            for verification in chunk {
                query = query
                    .bind(verification.fid as i64)
                    .bind(verification.hash)
                    .bind(verification.signer_address)
                    .bind(verification.block_hash)
                    .bind(verification.signature)
                    .bind(verification.protocol)
                    .bind(verification.timestamp)
                    .bind::<Option<OffsetDateTime>>(None); // deleted_at
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Bulk insert username proofs
    pub async fn bulk_insert_username_proofs(
        &self,
        proofs: Vec<UsernameProofInsert<'_>>,
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if proofs.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in proofs.chunks(self.batch_size) {
            let sql = build_insert_sql(
                "username_proofs",
                &["fid", "hash", "username", "timestamp", "signature"],
                chunk.len(),
                "username, fid",
                &[
                    "hash = CASE WHEN EXCLUDED.timestamp >= username_proofs.timestamp THEN EXCLUDED.hash ELSE username_proofs.hash END",
                    "signature = CASE WHEN EXCLUDED.timestamp >= username_proofs.timestamp THEN EXCLUDED.signature ELSE username_proofs.signature END",
                    "timestamp = GREATEST(username_proofs.timestamp, EXCLUDED.timestamp)",
                ],
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each username proof
            for proof in chunk {
                query = query
                    .bind(proof.fid as i64)
                    .bind(proof.hash)
                    .bind(proof.username)
                    .bind(proof.timestamp)
                    .bind(proof.signature);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Process a batch of messages, grouping them by type and inserting in bulk
    pub async fn process_message_batch(
        &self,
        messages: &[Message],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if messages.is_empty() {
            return Ok(());
        }

        // Group messages by type to batch insert similar message types
        let grouped = Self::group_messages_by_type(messages);

        // Insert messages table records if configured
        // We'll process this separately since all messages go into the messages table
        if !messages.is_empty() {
            match self.bulk_insert_messages(messages).await {
                Ok(count) => debug!("Bulk inserted {} messages", count),
                Err(e) => error!("Error bulk inserting messages: {}", e),
            }
        }

        // Process each message type in parallel
        let mut cast_inserts = Vec::new();
        let mut reaction_inserts = Vec::new();
        let mut link_inserts = Vec::new();
        let mut user_data_inserts = Vec::new();
        let mut verification_inserts = Vec::new();
        let mut username_proof_inserts = Vec::new();

        // Collect all inserts for each type
        for (msg_type, msgs) in grouped {
            match msg_type {
                1 => {
                    // CastAdd
                    for msg in msgs {
                        if let Some(cast) = CastInsert::from_message(msg) {
                            cast_inserts.push(cast);
                        }
                    }
                },
                3 => {
                    // ReactionAdd
                    for msg in msgs {
                        if let Some(reaction) = ReactionInsert::from_message(msg) {
                            reaction_inserts.push(reaction);
                        }
                    }
                },
                5 => {
                    // LinkAdd
                    for msg in msgs {
                        if let Some(link) = LinkInsert::from_message(msg) {
                            link_inserts.push(link);
                        }
                    }
                },
                7 => {
                    // VerificationAdd
                    for msg in msgs {
                        if let Some(verification) = VerificationInsert::from_message(msg) {
                            verification_inserts.push(verification);
                        }
                    }
                },
                11 => {
                    // UserDataAdd
                    for msg in msgs {
                        if let Some(user_data) = UserDataInsert::from_message(msg) {
                            user_data_inserts.push(user_data);
                        }
                    }
                },
                12 => {
                    // UsernameProof
                    for msg in msgs {
                        if let Some(proof) = UsernameProofInsert::from_message(msg) {
                            username_proof_inserts.push(proof);
                        }
                    }
                },
                // Add other message type handlers as needed
                _ => {}, // Skip unsupported message types
            }
        }

        // Execute bulk inserts for each type
        if !cast_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_casts(cast_inserts).await {
                error!("Error in bulk insert of casts: {}", e);
                return Err(e);
            }
        }

        if !reaction_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_reactions(reaction_inserts).await {
                error!("Error in bulk insert of reactions: {}", e);
                return Err(e);
            }
        }

        if !link_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_links(link_inserts).await {
                error!("Error in bulk insert of links: {}", e);
                return Err(e);
            }
        }

        if !user_data_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_user_data(user_data_inserts).await {
                error!("Error in bulk insert of user data: {}", e);
                return Err(e);
            }
        }

        if !verification_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_verifications(verification_inserts).await {
                error!("Error in bulk insert of verifications: {}", e);
                return Err(e);
            }
        }

        if !username_proof_inserts.is_empty() {
            if let Err(e) = self.bulk_insert_username_proofs(username_proof_inserts).await {
                error!("Error in bulk insert of username proofs: {}", e);
                return Err(e);
            }
        }

        // All bulk inserts have been completed

        Ok(())
    }
}
