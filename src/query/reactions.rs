//! Reaction query operations.

use crate::core::types::Fid;
use crate::query::WaypointQuery;
use crate::query::types::TimeRange;

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
    ) -> String {
        tracing::debug!("Query: Fetching reaction with FID: {} and type: {}", fid, reaction_type);

        // Use the data context to fetch the reaction
        match self
            .data_context
            .get_reaction(fid, reaction_type, target_cast_fid, target_cast_hash, target_url)
            .await
        {
            Ok(Some(message)) => {
                // Try to decode the message payload as MessageData
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;

                    // Process the reaction
                    if let Some(reaction_obj) =
                        super::utils::process_reaction_message(&message, &msg_data)
                    {
                        // Convert to JSON string
                        return serde_json::to_string_pretty(&reaction_obj).unwrap_or_else(|_| {
                            format!("Error formatting reaction for FID {}", fid)
                        });
                    }
                }

                format!("Reaction found but could not be processed for FID {}", fid)
            },
            Ok(None) => format!("No reaction found for FID {} with the specified parameters", fid),
            Err(e) => format!("Error fetching reaction: {}", e),
        }
    }

    /// Get reactions by FID
    pub async fn do_get_reactions_by_fid(
        &self,
        fid: Fid,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> String {
        tracing::debug!("Query: Fetching reactions for FID: {}", fid);

        // Use the data context to fetch reactions
        match self.data_context.get_reactions_by_fid(fid, reaction_type, limit).await {
            Ok(messages) => super::utils::format_reactions_response(messages, Some(fid)),
            Err(e) => format!("Error fetching reactions: {}", e),
        }
    }

    /// Get reactions by target (cast or URL)
    pub async fn do_get_reactions_by_target(
        &self,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> String {
        tracing::debug!("Query: Fetching reactions by target");

        // Use the data context to fetch reactions
        match self
            .data_context
            .get_reactions_by_target(
                target_cast_fid,
                target_cast_hash,
                target_url,
                reaction_type,
                limit,
            )
            .await
        {
            Ok(messages) => {
                // Format response based on target type
                let result = if messages.is_empty() {
                    if let Some(target_url) = target_url {
                        serde_json::json!({
                            "target_url": target_url,
                            "count": 0,
                            "reactions": []
                        })
                    } else if let (Some(tfid), Some(thash)) = (target_cast_fid, target_cast_hash) {
                        serde_json::json!({
                            "target_cast": {
                                "fid": tfid.value(),
                                "hash": hex::encode(thash)
                            },
                            "count": 0,
                            "reactions": []
                        })
                    } else {
                        serde_json::json!({
                            "count": 0,
                            "reactions": []
                        })
                    }
                } else {
                    // Process messages into reaction objects
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

                    if let Some(target_url) = target_url {
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
                    }
                };

                // Convert to JSON string
                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| "Error formatting reactions".to_string())
            },
            Err(e) => format!("Error fetching reactions by target: {}", e),
        }
    }

    /// Get all reactions by FID with timestamp filtering
    pub async fn do_get_all_reactions_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> String {
        tracing::debug!("Query: Fetching all reactions for FID: {} with time filtering", fid);

        // Use the data context to fetch reactions with time filtering
        match self.data_context.get_all_reactions_by_fid(fid, limit, start_time, end_time).await {
            Ok(messages) => {
                // Format the basic response
                let base_response = super::utils::format_reactions_response(messages, Some(fid));

                // If there are no reactions, return a time-specific message
                if base_response.starts_with("No reactions found") {
                    let time_range = TimeRange::new(start_time, end_time).describe();
                    return format!("No reactions found for FID {}{}", fid, time_range);
                }

                base_response
            },
            Err(e) => format!("Error fetching reactions with time filtering: {}", e),
        }
    }
}
