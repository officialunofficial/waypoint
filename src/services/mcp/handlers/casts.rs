//! MCP handlers for Cast-related operations

use crate::core::types::Fid;
use crate::services::mcp::base::WaypointMcpService;
use crate::services::mcp::handlers::utils::format_casts_response;

use prost::Message as ProstMessage;

// Common types are used in the handler implementations

impl<DB, HC> WaypointMcpService<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get user data by FID using Hub client
    pub async fn do_get_cast(&self, fid: Fid, hash_hex: &str) -> String {
        tracing::info!("MCP: Fetching cast with FID: {} and hash: {}", fid, hash_hex);

        // Convert hex hash to bytes
        let hash_bytes = match hex::decode(hash_hex.trim_start_matches("0x")) {
            Ok(bytes) => bytes,
            Err(_) => return format!("Invalid hash format: {}", hash_hex),
        };

        // Use the data context to fetch the cast
        match self.data_context.get_cast(fid, &hash_bytes).await {
            Ok(Some(message)) => {
                // Try to decode the message payload as MessageData
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;

                    // Process the cast
                    if let Some(cast_obj) = super::utils::process_cast_message(&message, &msg_data) {
                        // Convert to JSON string
                        return serde_json::to_string_pretty(&cast_obj).unwrap_or_else(|_| {
                            format!("Error formatting cast for FID {} and hash {}", fid, hash_hex)
                        });
                    }
                }

                format!(
                    "Cast found but could not be processed for FID {} and hash {}",
                    fid, hash_hex
                )
            },
            Ok(None) => format!("No cast found for FID {} and hash {}", fid, hash_hex),
            Err(e) => format!("Error fetching cast: {}", e),
        }
    }

    /// Get casts by FID
    pub async fn do_get_casts_by_fid(&self, fid: Fid, limit: usize) -> String {
        tracing::info!("MCP: Fetching casts for FID: {}", fid);

        // Use the data context to fetch casts
        match self.data_context.get_casts_by_fid(fid, limit).await {
            Ok(messages) => format_casts_response(messages, Some(fid)),
            Err(e) => format!("Error fetching casts: {}", e),
        }
    }

    /// Get casts mentioning a user
    pub async fn do_get_casts_by_mention(&self, fid: Fid, limit: usize) -> String {
        tracing::info!("MCP: Fetching casts mentioning FID: {}", fid);

        // Use the data context to fetch mentions
        match self.data_context.get_casts_by_mention(fid, limit).await {
            Ok(messages) => {
                // Format the response with special metadata for mentions
                let formatted = format_casts_response(messages, None);

                if formatted.starts_with("No casts found") {
                    return format!("No casts found mentioning FID {}", fid);
                }

                formatted
            },
            Err(e) => format!("Error fetching cast mentions: {}", e),
        }
    }

    /// Get replies to a cast
    pub async fn do_get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash_hex: &str,
        limit: usize,
    ) -> String {
        tracing::info!(
            "MCP: Fetching replies to cast with FID: {} and hash: {}",
            parent_fid, parent_hash_hex
        );

        // Convert hex hash to bytes
        let parent_hash_bytes = match hex::decode(parent_hash_hex.trim_start_matches("0x")) {
            Ok(bytes) => bytes,
            Err(_) => return format!("Invalid hash format: {}", parent_hash_hex),
        };

        // Use the data context to fetch replies
        match self.data_context.get_casts_by_parent(parent_fid, &parent_hash_bytes, limit).await {
            Ok(messages) => {
                // Format the response with special metadata for replies
                let result = if messages.is_empty() {
                    serde_json::json!({
                        "parent": {
                            "fid": parent_fid.value(),
                            "hash": parent_hash_hex
                        },
                        "count": 0,
                        "replies": []
                    })
                } else {
                    // Process messages into cast objects
                    let replies: Vec<serde_json::Value> = messages
                        .iter()
                        .filter_map(|message| {
                            if let Ok(data) = ProstMessage::decode(&*message.payload) {
                                let msg_data: crate::proto::MessageData = data;
                                if let Some(cast_obj) =
                                    super::utils::process_cast_message(message, &msg_data)
                                {
                                    return Some(serde_json::Value::Object(cast_obj));
                                }
                            }
                            None
                        })
                        .collect();

                    serde_json::json!({
                        "parent": {
                            "fid": parent_fid.value(),
                            "hash": parent_hash_hex
                        },
                        "count": replies.len(),
                        "replies": replies
                    })
                };

                // Convert to JSON string
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!(
                        "Error formatting replies for parent cast with FID {} and hash {}",
                        parent_fid, parent_hash_hex
                    )
                })
            },
            Err(e) => format!("Error fetching cast replies: {}", e),
        }
    }

    /// Get replies to a URL
    pub async fn do_get_casts_by_parent_url(&self, parent_url: &str, limit: usize) -> String {
        tracing::info!("MCP: Fetching replies to URL: {}", parent_url);

        // Use the data context to fetch replies
        match self.data_context.get_casts_by_parent_url(parent_url, limit).await {
            Ok(messages) => {
                // Format the response with special metadata for URL replies
                let result = if messages.is_empty() {
                    serde_json::json!({
                        "parent_url": parent_url,
                        "count": 0,
                        "replies": []
                    })
                } else {
                    // Process messages into cast objects
                    let replies: Vec<serde_json::Value> = messages
                        .iter()
                        .filter_map(|message| {
                            if let Ok(data) = ProstMessage::decode(&*message.payload) {
                                let msg_data: crate::proto::MessageData = data;
                                if let Some(cast_obj) =
                                    super::utils::process_cast_message(message, &msg_data)
                                {
                                    return Some(serde_json::Value::Object(cast_obj));
                                }
                            }
                            None
                        })
                        .collect();

                    serde_json::json!({
                        "parent_url": parent_url,
                        "count": replies.len(),
                        "replies": replies
                    })
                };

                // Convert to JSON string
                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Error formatting replies for URL: {}", parent_url))
            },
            Err(e) => format!("Error fetching URL replies: {}", e),
        }
    }

    /// Get all casts by FID with timestamp filtering
    pub async fn do_get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> String {
        tracing::info!("MCP: Fetching all casts for FID: {} with time filtering", fid);

        // Use the data context to fetch casts with time filtering
        match self.data_context.get_all_casts_by_fid(fid, limit, start_time, end_time).await {
            Ok(messages) => {
                // Format the basic response
                let base_response = format_casts_response(messages, Some(fid));

                // If there are no casts, return a time-specific message
                if base_response.starts_with("No casts found") {
                    let time_range = match (start_time, end_time) {
                        (Some(start), Some(end)) => {
                            format!(" between timestamps {} and {}", start, end)
                        },
                        (Some(start), None) => format!(" after timestamp {}", start),
                        (None, Some(end)) => format!(" before timestamp {}", end),
                        (None, None) => "".to_string(),
                    };

                    return format!("No casts found for FID {}{}", fid, time_range);
                }

                base_response
            },
            Err(e) => format!("Error fetching casts with time filtering: {}", e),
        }
    }
}