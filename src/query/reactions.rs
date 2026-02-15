//! Reaction query operations.

use crate::core::types::Fid;
use crate::query::{JsonMap, QueryError, QueryResult, WaypointQuery};

use prost::Message as ProstMessage;

// Common types are used in the handler implementations

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get a specific reaction
    pub async fn do_get_reaction(
        &self,
        fid: Fid,
        reaction_type: u8,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
    ) -> QueryResult<JsonMap> {
        tracing::debug!("Query: Fetching reaction with FID: {} and type: {}", fid, reaction_type);

        match self
            .data_context
            .get_reaction(fid, reaction_type, target_cast_fid, target_cast_hash, target_url)
            .await
        {
            Ok(Some(message)) => {
                let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload)
                    .map_err(|err| {
                    QueryError::Processing(format!("failed to decode reaction payload: {}", err))
                })?;

                super::utils::process_reaction_message(&message, &msg_data).ok_or_else(|| {
                    QueryError::Processing(format!(
                        "reaction payload could not be processed for FID {}",
                        fid
                    ))
                })
            },
            Ok(None) => {
                let mut result = serde_json::Map::new();
                result.insert("fid".to_string(), serde_json::json!(fid.value()));
                result.insert("reaction_type_id".to_string(), serde_json::json!(reaction_type));
                result.insert("found".to_string(), serde_json::json!(false));
                result.insert("error".to_string(), serde_json::json!("Reaction not found"));

                if let (Some(target_fid), Some(target_hash)) = (target_cast_fid, target_cast_hash) {
                    result.insert(
                        "target_cast".to_string(),
                        serde_json::json!({
                            "fid": target_fid.value(),
                            "hash": hex::encode(target_hash)
                        }),
                    );
                }

                if let Some(target_url) = target_url {
                    result.insert("target_url".to_string(), serde_json::json!(target_url));
                }

                Ok(result)
            },
            Err(e) => Err(e.into()),
        }
    }

    /// Get reactions by FID
    pub async fn do_get_reactions_by_fid(
        &self,
        fid: Fid,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching reactions for FID: {}", fid);

        let messages = self.data_context.get_reactions_by_fid(fid, reaction_type, limit).await?;
        Ok(super::utils::format_reactions_response(messages, Some(fid)))
    }

    /// Get reactions by target (cast or URL)
    pub async fn do_get_reactions_by_target(
        &self,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching reactions by target");

        let messages = self
            .data_context
            .get_reactions_by_target(
                target_cast_fid,
                target_cast_hash,
                target_url,
                reaction_type,
                limit,
            )
            .await?;

        let reactions: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|message| {
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;
                    if let Some(reaction_obj) =
                        super::utils::process_reaction_message(message, &msg_data)
                    {
                        return Some(serde_json::Value::Object(reaction_obj));
                    }
                }
                None
            })
            .collect();

        let result = if let Some(target_url) = target_url {
            serde_json::json!({
                "target_url": target_url,
                "count": reactions.len(),
                "reactions": reactions
            })
        } else if let (Some(tfid), Some(thash)) = (target_cast_fid, target_cast_hash) {
            serde_json::json!({
                "target_cast": {
                    "fid": tfid.value(),
                    "hash": hex::encode(thash)
                },
                "count": reactions.len(),
                "reactions": reactions
            })
        } else {
            serde_json::json!({
                "count": reactions.len(),
                "reactions": reactions
            })
        };

        Ok(result)
    }

    /// Get all reactions by FID with timestamp filtering
    pub async fn do_get_all_reactions_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching all reactions for FID: {} with time filtering", fid);

        let messages =
            self.data_context.get_all_reactions_by_fid(fid, limit, _start_time, _end_time).await?;
        Ok(super::utils::format_reactions_response(messages, Some(fid)))
    }
}
