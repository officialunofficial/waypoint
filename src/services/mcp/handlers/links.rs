//! MCP handlers for Link-related operations

use crate::core::types::Fid;
use crate::services::mcp::base::WaypointMcpService;

use prost::Message as ProstMessage;

// Common types are used in the handler implementations

impl<DB, HC> WaypointMcpService<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get a specific link
    pub async fn do_get_link(&self, fid: Fid, link_type: &str, target_fid: Fid) -> String {
        tracing::info!(
            "MCP: Fetching link with FID: {}, type: {}, target: {}",
            fid,
            link_type,
            target_fid
        );

        // Use the data context to fetch the link
        match self.data_context.get_link(fid, link_type, target_fid).await {
            Ok(Some(message)) => {
                // Try to decode the message payload as MessageData
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;

                    // Process the link
                    if let Some(link_obj) = super::utils::process_link_message(&message, &msg_data)
                    {
                        // Convert to JSON string
                        return serde_json::to_string_pretty(&link_obj)
                            .unwrap_or_else(|_| format!("Error formatting link for FID {}", fid));
                    }
                }

                format!("Link found but could not be processed for FID {}", fid)
            },
            Ok(None) => format!(
                "No link found for FID {} with type {} to target {}",
                fid, link_type, target_fid
            ),
            Err(e) => format!("Error fetching link: {}", e),
        }
    }

    /// Get links by FID
    pub async fn do_get_links_by_fid(
        &self,
        fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> String {
        tracing::info!("MCP: Fetching links for FID: {}", fid);

        // Use the data context to fetch links
        match self.data_context.get_links_by_fid(fid, link_type, limit).await {
            Ok(messages) => super::utils::format_links_response(messages, Some(fid)),
            Err(e) => format!("Error fetching links: {}", e),
        }
    }

    /// Get links by target
    pub async fn do_get_links_by_target(
        &self,
        target_fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> String {
        tracing::info!("MCP: Fetching links to target FID: {}", target_fid);

        // Use the data context to fetch links
        match self.data_context.get_links_by_target(target_fid, link_type, limit).await {
            Ok(messages) => {
                // Format response
                let result = if messages.is_empty() {
                    serde_json::json!({
                        "target_fid": target_fid.value(),
                        "count": 0,
                        "links": []
                    })
                } else {
                    // Process messages into link objects
                    let links: Vec<serde_json::Value> = messages
                        .iter()
                        .filter_map(|message| {
                            if let Ok(data) = ProstMessage::decode(&*message.payload) {
                                let msg_data: crate::proto::MessageData = data;
                                if let Some(link_obj) =
                                    super::utils::process_link_message(message, &msg_data)
                                {
                                    return Some(serde_json::Value::Object(link_obj));
                                }
                            }
                            None
                        })
                        .collect();

                    serde_json::json!({
                        "target_fid": target_fid.value(),
                        "count": links.len(),
                        "links": links
                    })
                };

                // Convert to JSON string
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error formatting links to target FID {}", target_fid)
                })
            },
            Err(e) => format!("Error fetching links by target: {}", e),
        }
    }

    /// Get link compact state messages by FID
    pub async fn do_get_link_compact_state_by_fid(&self, fid: Fid) -> String {
        tracing::info!("MCP: Fetching link compact state for FID: {}", fid);

        // Use the data context to fetch compact state messages
        match self.data_context.get_link_compact_state_by_fid(fid).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No link compact state messages found for FID {}", fid);
                }

                // Process messages into link objects
                let links: Vec<serde_json::Value> = messages
                    .iter()
                    .filter_map(|message| {
                        if let Ok(data) = ProstMessage::decode(&*message.payload) {
                            let msg_data: crate::proto::MessageData = data;
                            if let Some(link_obj) =
                                super::utils::process_link_message(message, &msg_data)
                            {
                                return Some(serde_json::Value::Object(link_obj));
                            }
                        }
                        None
                    })
                    .collect();

                // Create result
                let result = serde_json::json!({
                    "fid": fid.value(),
                    "count": links.len(),
                    "compact_links": links
                });

                // Convert to JSON string
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error formatting link compact state for FID {}", fid)
                })
            },
            Err(e) => format!("Error fetching link compact state: {}", e),
        }
    }

    /// Get all links by FID with timestamp filtering
    pub async fn do_get_all_links_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> String {
        tracing::info!("MCP: Fetching all links for FID: {} with time filtering", fid);

        // Use the data context to fetch links with time filtering
        match self.data_context.get_all_links_by_fid(fid, limit, start_time, end_time).await {
            Ok(messages) => {
                // Format the basic response
                let base_response = super::utils::format_links_response(messages, Some(fid));

                // If there are no links, return a time-specific message
                if base_response.starts_with("No links found") {
                    let time_range = match (start_time, end_time) {
                        (Some(start), Some(end)) => {
                            format!(" between timestamps {} and {}", start, end)
                        },
                        (Some(start), None) => format!(" after timestamp {}", start),
                        (None, Some(end)) => format!(" before timestamp {}", end),
                        (None, None) => "".to_string(),
                    };

                    return format!("No links found for FID {}{}", fid, time_range);
                }

                base_response
            },
            Err(e) => format!("Error fetching links with time filtering: {}", e),
        }
    }
}
