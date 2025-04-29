//! MCP handlers for Cast-related operations

use crate::core::types::{Fid, Message};
use crate::services::mcp::base::WaypointMcpService;
use crate::services::mcp::handlers::utils::format_casts_response;
use std::collections::HashSet;

use prost::Message as ProstMessage;

/// Helper function to format a message as JSON
fn format_message(message: &Message) -> serde_json::Map<String, serde_json::Value> {
    let mut json_obj = serde_json::Map::new();

    // Basic message metadata
    json_obj.insert("id".to_string(), serde_json::json!(message.id.value()));
    json_obj.insert("type".to_string(), serde_json::json!(message.message_type.to_string()));

    // Try to decode the protobuf message data from payload
    if let Ok(data) = ProstMessage::decode(&*message.payload) {
        let msg_data: crate::proto::MessageData = data;

        // Add common fields
        json_obj.insert("fid".to_string(), serde_json::json!(msg_data.fid));
        json_obj.insert("timestamp".to_string(), serde_json::json!(msg_data.timestamp));

        // Extract cast-specific data if it's a cast
        if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
            json_obj.insert("text".to_string(), serde_json::json!(cast.text));

            // Add mentions if present
            if !cast.mentions.is_empty() {
                json_obj.insert("mentions".to_string(), serde_json::json!(cast.mentions));
            }

            // Add parent information if present
            match &cast.parent {
                Some(crate::proto::cast_add_body::Parent::ParentCastId(parent)) => {
                    json_obj.insert(
                        "parent".to_string(),
                        serde_json::json!({
                            "fid": parent.fid,
                            "hash": hex::encode(&parent.hash)
                        }),
                    );
                },
                Some(crate::proto::cast_add_body::Parent::ParentUrl(url)) => {
                    json_obj.insert("parent_url".to_string(), serde_json::json!(url));
                },
                None => {},
            }

            // Add embeds information if present, including quote casts
            if !cast.embeds.is_empty() {
                let embeds: Vec<serde_json::Value> = cast
                    .embeds
                    .iter()
                    .filter_map(|embed| match &embed.embed {
                        Some(crate::proto::embed::Embed::Url(url)) => Some(serde_json::json!({
                            "type": "url",
                            "url": url
                        })),
                        Some(crate::proto::embed::Embed::CastId(cast_id)) => {
                            Some(serde_json::json!({
                                "type": "cast",
                                "fid": cast_id.fid,
                                "hash": hex::encode(&cast_id.hash)
                            }))
                        },
                        None => None,
                    })
                    .collect();

                if !embeds.is_empty() {
                    json_obj.insert("embeds".to_string(), serde_json::json!(embeds));
                }
            }
        }
    }

    json_obj
}

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
                    if let Some(cast_obj) = super::utils::process_cast_message(&message, &msg_data)
                    {
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
            parent_fid,
            parent_hash_hex
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

    /// Get conversation details for a cast
    pub async fn do_get_conversation_impl(
        &self,
        fid: Fid,
        cast_hash: &str,
        recursive: bool,
        max_depth: usize,
        limit: usize,
    ) -> String {
        tracing::info!("MCP: Fetching conversation for cast hash: {}", cast_hash);

        // Convert hex hash to bytes
        let hash_bytes = match hex::decode(cast_hash.trim_start_matches("0x")) {
            Ok(bytes) => bytes,
            Err(e) => return format!("Invalid cast hash: {}", e),
        };

        // Fetch the root cast
        let root_cast = match self.data_context.get_cast(fid, &hash_bytes).await {
            Ok(Some(cast)) => cast,
            Ok(None) => return format!("Cast not found with fid: {} and hash: {}", fid, cast_hash),
            Err(e) => return format!("Error fetching cast: {}", e),
        };

        // Participants will track all unique FIDs in the conversation
        let mut participants = HashSet::new();

        // Add the root cast author to participants by decoding the protobuf
        if let Ok(data) = ProstMessage::decode(&*root_cast.payload) {
            let msg_data: crate::proto::MessageData = data;
            participants.insert(Fid::from(msg_data.fid));
        }

        // Fetch and build the conversation tree recursively
        let conversation_tree = self
            .build_conversation_tree(
                &root_cast,
                &mut participants,
                recursive,
                max_depth,
                limit,
                0, // current depth starts at 0
            )
            .await;

        // Extract topic/summary from conversation content
        let topic = self.extract_conversation_topic(&root_cast, &conversation_tree);

        // Check if this cast has any quote cast embeds and fetch them
        let mut quoted_casts = Vec::new();

        if let Ok(data) = ProstMessage::decode(&*root_cast.payload) {
            let msg_data: crate::proto::MessageData = data;

            if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
                // Extract any cast embeds (quote casts)
                for embed in &cast.embeds {
                    if let Some(crate::proto::embed::Embed::CastId(cast_id)) = &embed.embed {
                        // Try to fetch the quoted cast
                        let quoted_fid = Fid::from(cast_id.fid);
                        if let Ok(Some(quoted_cast)) =
                            self.data_context.get_cast(quoted_fid, &cast_id.hash).await
                        {
                            quoted_casts.push(format_message(&quoted_cast));

                            // Add the author of the quoted cast to participants
                            if let Ok(q_data) = ProstMessage::decode(&*quoted_cast.payload) {
                                let q_msg_data: crate::proto::MessageData = q_data;
                                participants.insert(Fid::from(q_msg_data.fid));
                            }
                        }
                    }
                }
            }
        }

        // Fetch user data for all participants to hydrate the conversation
        let mut user_data_map = serde_json::Map::new();
        for &participant_fid in &participants {
            match self.data_context.get_user_data_by_fid(participant_fid, 20).await {
                Ok(messages) => {
                    if !messages.is_empty() {
                        // Create a structured user profile from the messages
                        let mut profile = serde_json::Map::new();

                        // Add the FID to the profile
                        profile.insert(
                            "fid".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(
                                participant_fid.value(),
                            )),
                        );

                        // Process each message to extract user data
                        for message in messages {
                            if message.message_type != crate::core::types::MessageType::UserData {
                                continue;
                            }

                            // Try to decode the message payload as MessageData
                            if let Ok(data) = ProstMessage::decode(&*message.payload) {
                                let msg_data: crate::proto::MessageData = data;

                                // Extract the user_data_body if present
                                if let Some(crate::proto::message_data::Body::UserDataBody(
                                    user_data,
                                )) = msg_data.body
                                {
                                    // Map user data type to field name
                                    let field_name = match user_data.r#type {
                                        1 => "pfp",          // USER_DATA_TYPE_PFP
                                        2 => "display_name", // USER_DATA_TYPE_DISPLAY
                                        3 => "bio",          // USER_DATA_TYPE_BIO
                                        5 => "url",          // USER_DATA_TYPE_URL
                                        6 => "username",     // USER_DATA_TYPE_USERNAME
                                        7 => "location",     // USER_DATA_TYPE_LOCATION
                                        8 => "twitter",      // USER_DATA_TYPE_TWITTER
                                        9 => "github",       // USER_DATA_TYPE_GITHUB
                                        _ => continue,       // Unknown type
                                    };

                                    // Add to the profile
                                    profile.insert(
                                        field_name.to_string(),
                                        serde_json::Value::String(user_data.value),
                                    );
                                }
                            }
                        }

                        // Add this profile to the user data map, keyed by FID
                        user_data_map.insert(
                            participant_fid.value().to_string(),
                            serde_json::Value::Object(profile),
                        );
                    }
                },
                Err(_) => {
                    // If there's an error fetching user data, just continue with other participants
                    continue;
                },
            }
        }

        // Generate the summary with user information
        let summary = self.generate_conversation_summary(&root_cast, &conversation_tree).await;

        // Format the response
        let response = if quoted_casts.is_empty() {
            serde_json::json!({
                "root_cast": format_message(&root_cast),
                "participants": {
                    "count": participants.len(),
                    "fids": participants.iter().collect::<Vec<_>>(),
                    "user_data": user_data_map
                },
                "topic": topic,
                "summary": summary,
                "conversation": conversation_tree
            })
        } else {
            serde_json::json!({
                "root_cast": format_message(&root_cast),
                "participants": {
                    "count": participants.len(),
                    "fids": participants.iter().collect::<Vec<_>>(),
                    "user_data": user_data_map
                },
                "topic": topic,
                "summary": summary,
                "quoted_casts": quoted_casts,
                "conversation": conversation_tree
            })
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error formatting response: {}", e))
    }

    // Helper function to recursively build the conversation tree
    async fn build_conversation_tree(
        &self,
        _parent_cast: &Message,
        participants: &mut HashSet<Fid>,
        recursive: bool,
        max_depth: usize,
        limit: usize,
        current_depth: usize,
    ) -> serde_json::Value {
        // If we've reached max depth, stop recursion
        if current_depth >= max_depth {
            return serde_json::json!({
                "replies": [],
                "has_more": false
            });
        }

        // Fetch direct replies to this cast by extracting FID and hash
        let mut parent_fid = Fid::from(0);

        // Use the actual message hash from the Message struct
        let parent_hash = _parent_cast.id.value().as_bytes();

        // Also get FID from protobuf data
        if let Ok(data) = ProstMessage::decode(&*_parent_cast.payload) {
            let msg_data: crate::proto::MessageData = data;
            parent_fid = Fid::from(msg_data.fid);
        }

        let replies =
            match self.data_context.get_casts_by_parent(parent_fid, parent_hash, limit).await {
                Ok(messages) => messages,
                Err(e) => {
                    tracing::error!("Error fetching replies: {}", e);
                    vec![]
                },
            };

        // Add reply authors to participants by decoding the protobuf
        for reply in &replies {
            if let Ok(data) = ProstMessage::decode(&*reply.payload) {
                let msg_data: crate::proto::MessageData = data;
                participants.insert(Fid::from(msg_data.fid));
            }
        }

        // Format and possibly recurse for each reply
        let mut formatted_replies = Vec::new();
        for reply in replies {
            let mut formatted_reply = format_message(&reply);

            // If recursive, fetch replies to this reply
            if recursive && current_depth < max_depth - 1 {
                // Use Box::pin for recursion in async functions to avoid infinite size issues
                let nested_replies_future = Box::pin(self.build_conversation_tree(
                    &reply,
                    participants,
                    recursive,
                    max_depth,
                    limit,
                    current_depth + 1,
                ));

                let nested_replies = nested_replies_future.await;
                formatted_reply["replies"] = nested_replies["replies"].clone();
                formatted_reply["has_more_replies"] = nested_replies["has_more"].clone();
            }

            formatted_replies.push(formatted_reply);
        }

        serde_json::json!({
            "replies": formatted_replies,
            "has_more": formatted_replies.len() >= limit
        })
    }

    // Extract the main topic of conversation based on the root cast
    fn extract_conversation_topic(
        &self,
        _root_cast: &Message,
        _conversation_tree: &serde_json::Value,
    ) -> String {
        // For now, we'll simply use the first few words of the root cast as the topic
        // A more sophisticated implementation would use NLP to identify common themes
        // Extract text from the message payload
        let mut text = "".to_string();

        if let Ok(data) = ProstMessage::decode(&*_root_cast.payload) {
            let msg_data: crate::proto::MessageData = data;

            if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
                text = cast.text.clone();
            }
        }

        if text.is_empty() {
            return "Untitled conversation".to_string();
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        let topic = words.iter().take(5).cloned().collect::<Vec<_>>().join(" ");

        if topic.is_empty() { "Untitled conversation".to_string() } else { format!("{}...", topic) }
    }

    // Generate a summary of the conversation
    async fn generate_conversation_summary(
        &self,
        _root_cast: &Message,
        conversation_tree: &serde_json::Value,
    ) -> String {
        // Count replies in the tree (including nested replies)
        let reply_count = Self::count_replies_recursive(conversation_tree);

        // Get the root cast author and text from the protobuf
        let mut root_author = Fid::from(0);
        let mut root_text = "".to_string();

        if let Ok(data) = ProstMessage::decode(&*_root_cast.payload) {
            let msg_data: crate::proto::MessageData = data;
            root_author = Fid::from(msg_data.fid);

            if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
                root_text = cast.text.clone();
            }
        }

        // Try to get the username for the author
        let author_display = match self.data_context.get_user_data_by_fid(root_author, 20).await {
            Ok(messages) => {
                let mut username = None;
                let mut display_name = None;

                for message in messages {
                    if message.message_type != crate::core::types::MessageType::UserData {
                        continue;
                    }

                    if let Ok(data) = ProstMessage::decode(&*message.payload) {
                        let msg_data: crate::proto::MessageData = data;

                        if let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                            msg_data.body
                        {
                            match user_data.r#type {
                                2 => display_name = Some(user_data.value), // USER_DATA_TYPE_DISPLAY
                                6 => username = Some(user_data.value), // USER_DATA_TYPE_USERNAME
                                _ => {},
                            }
                        }
                    }
                }

                // Prefer display name, fall back to username, then FID
                if let Some(name) = display_name {
                    format!(
                        "{} (@{})",
                        name,
                        username.unwrap_or_else(|| root_author.value().to_string())
                    )
                } else if let Some(name) = username {
                    format!("@{}", name)
                } else {
                    format!("FID {}", root_author)
                }
            },
            Err(_) => format!("FID {}", root_author),
        };

        // Format summary
        format!(
            "Conversation started by {} with: \"{}\". {} replies in the thread.",
            author_display,
            Self::truncate_text(&root_text, 50),
            reply_count
        )
    }

    // Helper to count total replies in the tree - static method to avoid clippy warning
    fn count_replies_recursive(tree: &serde_json::Value) -> usize {
        let replies = &tree["replies"];
        if replies.is_array() {
            let count = replies.as_array().unwrap().len();

            // Add counts from nested replies
            let nested_count: usize = replies
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|reply| {
                    if reply.get("replies").is_some() {
                        Some(Self::count_replies_recursive(reply))
                    } else {
                        None
                    }
                })
                .sum();

            count + nested_count
        } else {
            0
        }
    }

    // Helper to truncate text with ellipsis - static method
    fn truncate_text(text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            text.to_string()
        } else {
            format!("{}...", &text[0..max_length])
        }
    }
}
