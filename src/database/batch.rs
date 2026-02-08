use crate::{
    core::{
        normalize::NormalizedEmbed,
        util::{sanitize_json_for_postgres, sanitize_string_for_postgres},
    },
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
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tracing::{debug, error, trace};

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
    pub parent_fid: Option<i64>,
    pub parent_hash: Option<&'a [u8]>,
    pub parent_url: Option<&'a str>,
    pub root_parent_fid: Option<i64>,
    pub root_parent_hash: Option<Vec<u8>>,
    pub root_parent_url: Option<String>,
    pub timestamp: OffsetDateTime,
    pub embeds: Value,
    pub mentions: Value,
    pub mentions_positions: Value,
}

impl<'a> CastInsert<'a> {
    /// Creates a CastInsert from a Message. Root parent fields are set to None
    /// and must be resolved separately before bulk insertion.
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

        let parent_fid = match &cast_body.parent {
            Some(Parent::ParentCastId(cast_id)) => Some(cast_id.fid as i64),
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
            parent_fid,
            parent_hash,
            parent_url,
            // Root parent fields are set to None here and should be resolved
            // before bulk insertion using resolve_root_parents_for_batch
            root_parent_fid: None,
            root_parent_hash: None,
            root_parent_url: None,
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
            // Using ON CONFLICT (hash) DO NOTHING to avoid duplicate key errors
            // and unnecessary retry cycles when reprocessing messages
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
                "hash",
                &[], // Empty update actions = DO NOTHING
            );

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each message
            for msg in chunk {
                if let Some(data) = &msg.data {
                    let ts = convert_timestamp(data.timestamp);
                    let raw_data = msg.data_bytes.as_deref().unwrap_or_default();
                    // Sanitize null bytes from JSON - PostgreSQL jsonb rejects \u0000
                    let body_json = serde_json::to_value(data)
                        .map(sanitize_json_for_postgres)
                        .unwrap_or(serde_json::Value::Null);

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
    /// Note: Root parent fields in CastInsert should be resolved before calling this.
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
                    "parent_fid",
                    "parent_hash",
                    "parent_url",
                    "root_parent_fid",
                    "root_parent_hash",
                    "root_parent_url",
                    "timestamp",
                    "embeds",
                    "mentions",
                    "mentions_positions",
                ],
                chunk.len(),
                "hash",
                &[
                    "text = EXCLUDED.text",
                    "parent_fid = EXCLUDED.parent_fid",
                    "parent_hash = EXCLUDED.parent_hash",
                    "parent_url = EXCLUDED.parent_url",
                    "root_parent_fid = EXCLUDED.root_parent_fid",
                    "root_parent_hash = EXCLUDED.root_parent_hash",
                    "root_parent_url = EXCLUDED.root_parent_url",
                    "timestamp = EXCLUDED.timestamp",
                    "embeds = EXCLUDED.embeds",
                    "mentions = EXCLUDED.mentions",
                    "mentions_positions = EXCLUDED.mentions_positions",
                ],
            );

            // Pre-sanitize all text fields - PostgreSQL text columns reject \x00
            let sanitized: Vec<_> = chunk
                .iter()
                .map(|c| {
                    (
                        sanitize_string_for_postgres(c.text),
                        c.parent_url.map(|u| sanitize_string_for_postgres(u).into_owned()),
                        c.root_parent_url
                            .as_deref()
                            .map(|u| sanitize_string_for_postgres(u).into_owned()),
                    )
                })
                .collect();

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each cast
            for (cast, (text, parent_url, root_parent_url)) in chunk.iter().zip(sanitized.iter()) {
                query = query
                    .bind(cast.fid as i64)
                    .bind(cast.hash)
                    .bind(text.as_ref())
                    .bind(cast.parent_fid)
                    .bind(cast.parent_hash)
                    .bind(parent_url.as_deref())
                    .bind(cast.root_parent_fid)
                    .bind(cast.root_parent_hash.as_deref())
                    .bind(root_parent_url.as_deref())
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
                    "target_cast_fid",
                    "hash",
                    "type",
                    "target_cast_hash",
                    "target_url",
                    "timestamp",
                ],
                chunk.len(),
                "hash",
                &[
                    "target_cast_fid = EXCLUDED.target_cast_fid",
                    "type = EXCLUDED.type",
                    "target_cast_hash = EXCLUDED.target_cast_hash",
                    "target_url = EXCLUDED.target_url",
                    "timestamp = EXCLUDED.timestamp",
                ],
            );

            // Pre-sanitize target_url - PostgreSQL text columns reject \x00
            let sanitized_urls: Vec<_> = chunk
                .iter()
                .map(|r| r.target_url.map(|u| sanitize_string_for_postgres(u).into_owned()))
                .collect();

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each reaction
            for (reaction, sanitized_url) in chunk.iter().zip(sanitized_urls.iter()) {
                query = query
                    .bind(reaction.fid as i64)
                    .bind(reaction.target_fid as i64)
                    .bind(reaction.hash)
                    .bind(reaction.reaction_type)
                    .bind(reaction.target_hash)
                    .bind(sanitized_url.as_deref())
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

            // Pre-sanitize link_type - PostgreSQL text columns reject \x00
            let sanitized_types: Vec<_> =
                chunk.iter().map(|l| sanitize_string_for_postgres(l.link_type)).collect();

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each link
            for (link, sanitized_type) in chunk.iter().zip(sanitized_types.iter()) {
                query = query
                    .bind(link.fid as i64)
                    .bind(link.target_fid as i64)
                    .bind(link.hash)
                    .bind(sanitized_type.as_ref())
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

            // Pre-sanitize all values - PostgreSQL text columns reject \x00
            let sanitized_values: Vec<_> =
                chunk.iter().map(|d| sanitize_string_for_postgres(d.value)).collect();

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each user data entry
            for (data, sanitized_value) in chunk.iter().zip(sanitized_values.iter()) {
                query = query
                    .bind(data.fid as i64)
                    .bind(data.hash)
                    .bind(data.user_data_type)
                    .bind(sanitized_value.as_ref())
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

            // Pre-sanitize all usernames - PostgreSQL text columns reject \x00
            let sanitized_usernames: Vec<_> = chunk
                .iter()
                .map(|p| {
                    let username_str = String::from_utf8_lossy(p.username);
                    sanitize_string_for_postgres(&username_str).into_owned()
                })
                .collect();

            let mut query = sqlx::query(&sql);

            // Bind all parameters for each username proof
            for (proof, sanitized_username) in chunk.iter().zip(sanitized_usernames.iter()) {
                query = query
                    .bind(proof.fid as i64)
                    .bind(proof.hash)
                    .bind(sanitized_username.as_str())
                    .bind(proof.timestamp)
                    .bind(proof.signature);
            }

            let result = query.execute(self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Resolve root parent fields for a batch of CastInserts using a single DB query.
    /// - URL-parented casts: root_parent_url = parent_url (trivial, no lookup)
    /// - Cast-parented casts: batch query DB for parent's root_parent fields
    /// - Unresolvable: left as None (RootParentBackfill handles these)
    async fn resolve_root_parents_batch(&self, casts: &mut [CastInsert<'_>]) {
        // 1. Handle URL parents (trivial — root is the URL itself)
        for cast in casts.iter_mut() {
            if let Some(url) = cast.parent_url {
                cast.root_parent_url = Some(url.to_string());
            }
        }

        // 2. Collect unique parent hashes that need DB lookup
        let mut seen = HashSet::new();
        let parent_hashes: Vec<Vec<u8>> = casts
            .iter()
            .filter(|c| c.root_parent_url.is_none())
            .filter_map(|c| c.parent_hash)
            .map(|h| h.to_vec())
            .filter(|h| seen.insert(h.clone()))
            .collect();

        if parent_hashes.is_empty() {
            return;
        }

        // 3. Single batch query to resolve root parents from existing casts
        let rows = match sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, Option<String>)>(
            r#"
            SELECT
                hash,
                COALESCE(root_parent_fid, fid) as root_fid,
                COALESCE(root_parent_hash, hash) as root_hash,
                root_parent_url
            FROM casts
            WHERE hash = ANY($1) AND deleted_at IS NULL
            "#,
        )
        .bind(&parent_hashes)
        .fetch_all(self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                debug!("Error resolving root parents in batch: {}, leaving as NULL", e);
                return;
            },
        };

        // 4. Build lookup map from hash → (root_fid, root_hash, root_url)
        let lookup: HashMap<Vec<u8>, (i64, Vec<u8>, Option<String>)> = rows
            .into_iter()
            .map(|(hash, root_fid, root_hash, root_url)| (hash, (root_fid, root_hash, root_url)))
            .collect();

        let resolved = lookup.len();

        // 5. Apply resolved roots to CastInserts
        for cast in casts.iter_mut() {
            if cast.root_parent_url.is_some() {
                continue; // Already resolved via URL parent
            }
            if let Some(parent_hash) = cast.parent_hash
                && let Some((root_fid, root_hash, root_url)) = lookup.get(parent_hash)
            {
                cast.root_parent_fid = Some(*root_fid);
                cast.root_parent_hash = Some(root_hash.clone());
                cast.root_parent_url = root_url.clone();
            }
        }

        if resolved > 0 {
            debug!(
                "Resolved root parents for {}/{} unique parent hashes from DB",
                resolved,
                parent_hashes.len()
            );
        }
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

        // Insert into the messages table (all types go here regardless of specific type tables)
        match self.bulk_insert_messages(messages).await {
            Ok(count) => trace!("Bulk inserted {} messages", count),
            Err(e) => error!("Error bulk inserting messages: {}", e),
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

        // Resolve root parents for casts before bulk insert
        if !cast_inserts.is_empty() {
            self.resolve_root_parents_batch(&mut cast_inserts).await;
        }

        // Execute bulk inserts for each type
        if !cast_inserts.is_empty()
            && let Err(e) = self.bulk_insert_casts(cast_inserts).await
        {
            error!("Error in bulk insert of casts: {}", e);
            return Err(e);
        }

        if !reaction_inserts.is_empty()
            && let Err(e) = self.bulk_insert_reactions(reaction_inserts).await
        {
            error!("Error in bulk insert of reactions: {}", e);
            return Err(e);
        }

        if !link_inserts.is_empty()
            && let Err(e) = self.bulk_insert_links(link_inserts).await
        {
            error!("Error in bulk insert of links: {}", e);
            return Err(e);
        }

        if !user_data_inserts.is_empty()
            && let Err(e) = self.bulk_insert_user_data(user_data_inserts).await
        {
            error!("Error in bulk insert of user data: {}", e);
            return Err(e);
        }

        if !verification_inserts.is_empty()
            && let Err(e) = self.bulk_insert_verifications(verification_inserts).await
        {
            error!("Error in bulk insert of verifications: {}", e);
            return Err(e);
        }

        if !username_proof_inserts.is_empty()
            && let Err(e) = self.bulk_insert_username_proofs(username_proof_inserts).await
        {
            error!("Error in bulk insert of username proofs: {}", e);
            return Err(e);
        }

        // All bulk inserts have been completed

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_timestamp_late_2025() {
        // This test verifies that a late 2025 Farcaster timestamp converts correctly
        // and does NOT produce a 1974 date (which would happen if epoch isn't applied)

        // Farcaster timestamp for approximately late Dec 2025
        // Farcaster epoch: Jan 1, 2021 00:00:00 UTC = 1609459200 Unix seconds
        // Late Dec 2025 ~= 1767244800 Unix seconds
        // Farcaster time = 1767244800 - 1609459200 = 157785600 seconds since FC epoch
        let farcaster_timestamp: u32 = 157785600;

        let result = convert_timestamp(farcaster_timestamp);

        // The result should be in 2025, not 1974
        let year = result.year();
        assert!(
            (2025..=2026).contains(&year),
            "Expected year 2025 or 2026, got {}. Unix timestamp: {}",
            year,
            result.unix_timestamp()
        );

        // Specifically verify we're NOT getting 1974 (which would happen without epoch)
        assert_ne!(year, 1974, "Got 1974 - Farcaster epoch is not being applied!");
        assert_ne!(year, 1975, "Got 1975 - Farcaster epoch is not being applied!");

        // Verify the Unix timestamp is correct: should be ~1767 billion, not ~157 million
        let unix_ts = result.unix_timestamp();
        assert!(
            unix_ts > 1_700_000_000,
            "Unix timestamp {} is too low - epoch not applied correctly",
            unix_ts
        );
    }

    #[test]
    fn test_batch_and_processor_timestamps_match() {
        // Ensure both convert_timestamp implementations produce identical results
        use crate::processor::database::DatabaseProcessor;

        let farcaster_timestamp: u32 = 157785600;

        let batch_result = convert_timestamp(farcaster_timestamp);
        let processor_result = DatabaseProcessor::convert_timestamp(farcaster_timestamp);

        assert_eq!(
            batch_result.unix_timestamp(),
            processor_result.unix_timestamp(),
            "Batch and processor convert_timestamp produce different results!"
        );
    }
}
