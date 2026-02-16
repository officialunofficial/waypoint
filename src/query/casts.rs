//! Cast and conversation query operations.

use crate::core::types::{Fid, Message};
use crate::query::responses::{
    CastLookupResponse, CastNotFoundResponse, CastReference, CastsByFidResponse,
    CastsByMentionResponse, CastsByParentResponse, CastsByParentUrlResponse, ConversationCast,
    ConversationFoundResponse, ConversationParentCast, ConversationParticipants,
    ConversationResponse, ConversationTree,
};
use crate::query::types::ConversationParams;
use crate::query::{QueryError, QueryResult, WaypointQuery};
use std::collections::BTreeMap;
use std::collections::HashSet;

use prost::Message as ProstMessage;

/// Helper function to format a message in conversation-friendly shape.
fn format_message(message: &Message) -> ConversationCast {
    let mut formatted = ConversationCast {
        id: message.id.value().to_string(),
        message_type: message.message_type.to_string(),
        fid: None,
        timestamp: None,
        text: None,
        mentions: None,
        parent: None,
        parent_url: None,
        embeds: None,
        quoted_casts: None,
        replies: None,
        has_more_replies: None,
    };

    if let Ok(data) = ProstMessage::decode(&*message.payload) {
        let msg_data: crate::proto::MessageData = data;
        formatted.fid = Some(msg_data.fid);
        formatted.timestamp = Some(msg_data.timestamp);

        if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
            formatted.text = Some(cast.text.clone());

            if !cast.mentions.is_empty() {
                formatted.mentions = Some(cast.mentions.clone());
            }

            match &cast.parent {
                Some(crate::proto::cast_add_body::Parent::ParentCastId(parent)) => {
                    formatted.parent = Some(ConversationParentCast {
                        fid: parent.fid,
                        hash: hex::encode(&parent.hash),
                    });
                },
                Some(crate::proto::cast_add_body::Parent::ParentUrl(url)) => {
                    formatted.parent_url = Some(url.clone());
                },
                None => {},
            }

            if !cast.embeds.is_empty() {
                let embeds = cast
                    .embeds
                    .iter()
                    .filter_map(|embed| match &embed.embed {
                        Some(crate::proto::embed::Embed::Url(url)) => {
                            Some(crate::query::responses::CastEmbed::Url(
                                crate::query::responses::TypedUrlReference {
                                    target_type: "url".to_string(),
                                    url: url.clone(),
                                },
                            ))
                        },
                        Some(crate::proto::embed::Embed::CastId(cast_id)) => {
                            Some(crate::query::responses::CastEmbed::Cast(
                                crate::query::responses::TypedCastReference {
                                    target_type: "cast".to_string(),
                                    fid: cast_id.fid,
                                    hash: hex::encode(&cast_id.hash),
                                },
                            ))
                        },
                        None => None,
                    })
                    .collect::<Vec<_>>();

                if !embeds.is_empty() {
                    formatted.embeds = Some(embeds);
                }
            }
        }
    }

    formatted
}

// Common types are used in the handler implementations

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get user data by FID using Hub client
    pub async fn do_get_cast(&self, fid: Fid, hash_hex: &str) -> QueryResult<CastLookupResponse> {
        tracing::debug!("Query: Fetching cast with FID: {} and hash: {}", fid, hash_hex);

        let hash_bytes =
            super::utils::parse_hash_bytes(hash_hex).map_err(QueryError::InvalidInput)?;
        let normalized_hash = hex::encode(&hash_bytes);

        match self.data_context.get_cast(fid, &hash_bytes).await {
            Ok(Some(message)) => {
                let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload)
                    .map_err(|err| {
                    QueryError::Processing(format!("failed to decode cast payload: {}", err))
                })?;

                let cast =
                    super::utils::process_cast_message(&message, &msg_data).ok_or_else(|| {
                        QueryError::Processing(format!(
                            "cast payload could not be processed for FID {} and hash {}",
                            fid, hash_hex
                        ))
                    })?;
                Ok(CastLookupResponse::Found(cast))
            },
            Ok(None) => Ok(CastLookupResponse::NotFound(CastNotFoundResponse {
                fid: fid.value(),
                hash: normalized_hash,
                found: false,
                error: "Cast not found".to_string(),
            })),
            Err(e) => Err(e.into()),
        }
    }

    /// Get casts by FID
    pub async fn do_get_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
    ) -> QueryResult<CastsByFidResponse> {
        tracing::debug!("Query: Fetching casts for FID: {}", fid);

        let messages = self.data_context.get_casts_by_fid(fid, limit).await?;
        let casts = super::utils::parse_cast_messages(&messages);
        Ok(CastsByFidResponse { fid: fid.value(), count: casts.len(), casts })
    }

    /// Get casts mentioning a user
    pub async fn do_get_casts_by_mention(
        &self,
        fid: Fid,
        limit: usize,
    ) -> QueryResult<CastsByMentionResponse> {
        tracing::debug!("Query: Fetching casts mentioning FID: {}", fid);

        let messages = self.data_context.get_casts_by_mention(fid, limit).await?;
        let casts = super::utils::parse_cast_messages(&messages);

        Ok(CastsByMentionResponse { mention_fid: fid.value(), count: casts.len(), casts })
    }

    /// Get replies to a cast
    pub async fn do_get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash_hex: &str,
        limit: usize,
    ) -> QueryResult<CastsByParentResponse> {
        tracing::debug!(
            "Query: Fetching replies to cast with FID: {} and hash: {}",
            parent_fid,
            parent_hash_hex
        );

        let parent_hash_bytes =
            super::utils::parse_hash_bytes(parent_hash_hex).map_err(QueryError::InvalidInput)?;
        let normalized_parent_hash = hex::encode(&parent_hash_bytes);

        let messages =
            self.data_context.get_casts_by_parent(parent_fid, &parent_hash_bytes, limit).await?;

        let replies = super::utils::parse_cast_messages(&messages);

        Ok(CastsByParentResponse {
            parent: CastReference { fid: parent_fid.value(), hash: normalized_parent_hash },
            count: replies.len(),
            replies,
        })
    }

    /// Get replies to a URL
    pub async fn do_get_casts_by_parent_url(
        &self,
        parent_url: &str,
        limit: usize,
    ) -> QueryResult<CastsByParentUrlResponse> {
        tracing::debug!("Query: Fetching replies to URL: {}", parent_url);

        let messages = self.data_context.get_casts_by_parent_url(parent_url, limit).await?;
        let replies = super::utils::parse_cast_messages(&messages);

        Ok(CastsByParentUrlResponse {
            parent_url: parent_url.to_string(),
            count: replies.len(),
            replies,
        })
    }

    /// Get all casts by FID with timestamp filtering
    pub async fn do_get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> QueryResult<CastsByFidResponse> {
        tracing::debug!("Query: Fetching all casts for FID: {} with time filtering", fid);

        let messages =
            self.data_context.get_all_casts_by_fid(fid, limit, start_time, end_time).await?;
        let casts = super::utils::parse_cast_messages(&messages);
        Ok(CastsByFidResponse { fid: fid.value(), count: casts.len(), casts })
    }

    /// Get conversation details for a cast, including parent context
    pub async fn do_get_conversation_impl(
        &self,
        fid: Fid,
        cast_hash: &str,
        recursive: bool,
        max_depth: usize,
        limit: usize,
    ) -> QueryResult<ConversationResponse> {
        tracing::debug!("Query: Fetching conversation for cast hash: {}", cast_hash);

        let hash_bytes = super::utils::parse_hash_bytes(cast_hash).map_err(|message| {
            QueryError::InvalidInput(format!("Invalid cast hash: {}", message))
        })?;
        let normalized_hash = hex::encode(&hash_bytes);

        let root_cast = match self.data_context.get_cast(fid, &hash_bytes).await? {
            Some(cast) => cast,
            None => {
                return Ok(ConversationResponse::NotFound(CastNotFoundResponse {
                    fid: fid.value(),
                    hash: normalized_hash,
                    found: false,
                    error: "Cast not found".to_string(),
                }));
            },
        };

        // Participants will track all unique FIDs in the conversation
        let mut participants = HashSet::new();

        // Add the root cast author to participants by decoding the protobuf
        if let Ok(data) = ProstMessage::decode(&*root_cast.payload) {
            let msg_data: crate::proto::MessageData = data;
            participants.insert(Fid::from(msg_data.fid));
        }

        // NEW: First fetch parent casts to build the conversation thread above this cast
        let mut parent_casts = Vec::new();
        let parent_info = self.get_parent_cast_info(&root_cast).await;

        // Recursively fetch parent casts to build the context thread (up to 5 levels)
        if parent_info.0.is_some() && parent_info.1.is_some() {
            tracing::debug!("Found parent cast reference, fetching thread context");
            self.fetch_parent_casts(&mut parent_casts, &mut participants, &parent_info, 0, 5).await;
        } else {
            tracing::debug!("No parent cast reference found, this may be a root conversation");
        }

        // Create parameters struct for conversation tree building
        let conversation_params = ConversationParams { recursive, max_depth, limit };

        // Fetch and build the conversation tree recursively (replies)
        let conversation_tree = self
            .build_conversation_tree(
                &root_cast,
                &mut participants,
                conversation_params,
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
        let mut user_data_map = BTreeMap::new();
        for &participant_fid in &participants {
            match self.data_context.get_user_data_by_fid(participant_fid, 20).await {
                Ok(messages) => {
                    if !messages.is_empty() {
                        let mut profile = crate::query::responses::UserProfile {
                            fid: participant_fid.value(),
                            ..crate::query::responses::UserProfile::default()
                        };

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
                                    Self::set_participant_user_field(
                                        &mut profile,
                                        user_data.r#type,
                                        user_data.value,
                                    );
                                }
                            }
                        }

                        // Add this profile to the user data map, keyed by FID
                        user_data_map.insert(participant_fid.value().to_string(), profile);
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

        let mut participant_fids = participants.iter().map(|fid| fid.value()).collect::<Vec<_>>();
        participant_fids.sort_unstable();

        Ok(ConversationResponse::Found(Box::new(ConversationFoundResponse {
            root_cast: format_message(&root_cast),
            participants: ConversationParticipants {
                count: participants.len(),
                fids: participant_fids.into_iter().map(|fid| fid.to_string()).collect(),
                user_data: user_data_map,
            },
            topic,
            summary,
            parent_casts: if parent_casts.is_empty() { None } else { Some(parent_casts) },
            quoted_casts: if quoted_casts.is_empty() { None } else { Some(quoted_casts) },
            conversation: conversation_tree,
        })))
    }

    fn set_participant_user_field(
        profile: &mut crate::query::responses::UserProfile,
        data_type: i32,
        value: String,
    ) {
        match data_type {
            1 => profile.pfp = Some(value),
            2 => profile.display_name = Some(value),
            3 => profile.bio = Some(value),
            5 => profile.url = Some(value),
            6 => profile.username = Some(value),
            7 => profile.location = Some(value),
            8 => profile.twitter = Some(value),
            9 => profile.github = Some(value),
            _ => {},
        }
    }

    // Get parent cast information from a cast
    async fn get_parent_cast_info(&self, cast: &Message) -> (Option<Fid>, Option<Vec<u8>>) {
        let mut parent_fid = None;
        let mut parent_hash = None;

        // Try to decode the message payload
        if let Ok(data) = ProstMessage::decode(&*cast.payload) {
            let msg_data: crate::proto::MessageData = data;

            // Check if this is a cast that has a parent
            if let Some(crate::proto::message_data::Body::CastAddBody(cast_body)) = &msg_data.body {
                // Check if the cast has a parent cast reference
                if let Some(crate::proto::cast_add_body::Parent::ParentCastId(parent)) =
                    &cast_body.parent
                {
                    parent_fid = Some(Fid::from(parent.fid));
                    parent_hash = Some(parent.hash.clone());
                }
                // Note: We're ignoring ParentUrl parents for now as they reference external content
            }
        }

        (parent_fid, parent_hash)
    }

    // Recursively fetch parent casts
    async fn fetch_parent_casts(
        &self,
        parent_casts: &mut Vec<ConversationCast>,
        participants: &mut HashSet<Fid>,
        parent_info: &(Option<Fid>, Option<Vec<u8>>),
        current_depth: usize,
        max_depth: usize,
    ) {
        // Stop recursion if we've reached max depth
        if current_depth >= max_depth {
            return;
        }

        // We need both FID and hash to fetch the parent cast
        let (Some(parent_fid), Some(parent_hash)) = (parent_info.0, parent_info.1.as_ref()) else {
            return;
        };

        tracing::debug!(
            "Fetching specific cast with FID: {} and hash: {}",
            parent_fid,
            hex::encode(parent_hash)
        );

        // Fetch the parent cast
        match self.data_context.get_cast(parent_fid, parent_hash).await {
            Ok(Some(parent_cast)) => {
                // Add the parent cast author to participants
                if let Ok(data) = ProstMessage::decode(&*parent_cast.payload) {
                    let msg_data: crate::proto::MessageData = data;
                    participants.insert(Fid::from(msg_data.fid));
                }

                // Format and add this parent cast to the list
                let formatted_cast = format_message(&parent_cast);
                parent_casts.push(formatted_cast);

                // Get the grandparent info and recurse
                let grandparent_info = self.get_parent_cast_info(&parent_cast).await;
                // Use Box::pin for recursion in async functions
                let future = Box::pin(self.fetch_parent_casts(
                    parent_casts,
                    participants,
                    &grandparent_info,
                    current_depth + 1,
                    max_depth,
                ));
                future.await;
            },
            Ok(None) => {
                tracing::warn!(
                    "Parent cast not found: FID {} hash {}",
                    parent_fid,
                    hex::encode(parent_hash)
                );
            },
            Err(e) => {
                tracing::error!("Error fetching parent cast: {}", e);
            },
        }
    }

    // Helper function to recursively build the conversation tree
    // We're using a struct to reduce the number of parameters and address the clippy warning
    async fn build_conversation_tree(
        &self,
        _parent_cast: &Message,
        participants: &mut HashSet<Fid>,
        params: ConversationParams,
        current_depth: usize,
    ) -> ConversationTree {
        // If we've reached max depth, stop recursion
        if current_depth >= params.max_depth {
            return ConversationTree { replies: vec![], has_more: false };
        }

        // For a cast with hash X, we want to find all casts that have X as their parent
        // To find replies to the current cast, we need:
        // 1. The FID of the cast author (parent_fid)
        // 2. The hash of the cast itself (parent_hash)
        let mut parent_fid = Fid::from(0);

        // Extract the FID from the MessageData
        if let Ok(msg_data) = ProstMessage::decode(&*_parent_cast.payload) {
            let msg_data: crate::proto::MessageData = msg_data;
            parent_fid = Fid::from(msg_data.fid);
        }

        // The hash is stored in the Message's id field
        // This is from the Message.hash property in the protobuf
        let parent_hash = match super::utils::parse_hash_bytes(_parent_cast.id.value()) {
            Ok(hash_bytes) => hash_bytes,
            Err(message) => {
                tracing::error!(
                    "Failed to decode cast hash from ID: {} - {}",
                    _parent_cast.id.value(),
                    message
                );
                return ConversationTree { replies: vec![], has_more: false };
            },
        };

        // Fetch direct replies to this cast using the Hub's GetCastsByParent API
        // This returns all casts that have the current cast as their parent
        let replies = match self
            .data_context
            .get_casts_by_parent(parent_fid, &parent_hash, params.limit)
            .await
        {
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

            // Process embedded casts (quotes) in the reply just like we do for the root cast
            if let Ok(data) = ProstMessage::decode(&*reply.payload) {
                let msg_data: crate::proto::MessageData = data;

                if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
                    // If there are embeds, check for cast embeds (quotes)
                    if !cast.embeds.is_empty() {
                        let mut quoted_casts = Vec::new();

                        for embed in &cast.embeds {
                            if let Some(crate::proto::embed::Embed::CastId(cast_id)) = &embed.embed
                            {
                                // Try to fetch the quoted cast
                                let quoted_fid = Fid::from(cast_id.fid);
                                if let Ok(Some(quoted_cast)) =
                                    self.data_context.get_cast(quoted_fid, &cast_id.hash).await
                                {
                                    quoted_casts.push(format_message(&quoted_cast));

                                    // Add the author of the quoted cast to participants
                                    if let Ok(q_data) = ProstMessage::decode(&*quoted_cast.payload)
                                    {
                                        let q_msg_data: crate::proto::MessageData = q_data;
                                        participants.insert(Fid::from(q_msg_data.fid));
                                    }
                                }
                            }
                        }

                        // Add quoted casts to the formatted reply if any were found
                        if !quoted_casts.is_empty() {
                            formatted_reply.quoted_casts = Some(quoted_casts);
                        }
                    }
                }
            }

            // If recursive, fetch replies to this reply
            if params.recursive && current_depth < params.max_depth - 1 {
                // Use Box::pin for recursion in async functions to avoid infinite size issues
                let nested_replies_future = Box::pin(self.build_conversation_tree(
                    &reply,
                    participants,
                    params,
                    current_depth + 1,
                ));

                let nested_replies = nested_replies_future.await;
                formatted_reply.replies = Some(nested_replies.replies);
                formatted_reply.has_more_replies = Some(nested_replies.has_more);
            }

            formatted_replies.push(formatted_reply);
        }

        let has_more = formatted_replies.len() >= params.limit;
        ConversationTree { replies: formatted_replies, has_more }
    }

    // Extract the main topic of conversation based on the root cast
    fn extract_conversation_topic(
        &self,
        _root_cast: &Message,
        _conversation_tree: &ConversationTree,
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
        conversation_tree: &ConversationTree,
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
    fn count_replies_recursive(tree: &ConversationTree) -> usize {
        tree.replies
            .iter()
            .map(|reply| {
                1 + reply.replies.as_ref().map_or(0, |nested| Self::count_nested_replies(nested))
            })
            .sum()
    }

    fn count_nested_replies(replies: &[ConversationCast]) -> usize {
        replies
            .iter()
            .map(|reply| {
                1 + reply.replies.as_ref().map_or(0, |nested| Self::count_nested_replies(nested))
            })
            .sum()
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

#[cfg(test)]
mod tests {
    use crate::core::data_context::DataAccessError;
    use crate::core::types::{Fid, Message, MessageId, MessageType};
    use crate::query::WaypointQuery;
    use crate::query::responses::{ConversationCast, ConversationTree};
    use async_trait::async_trait;

    // Minimal mock types to access associated functions on WaypointQuery<DB, HC>
    #[derive(Clone, Debug)]
    struct MockDb;

    #[async_trait]
    impl crate::core::data_context::Database for MockDb {
        async fn get_message(
            &self,
            _id: &MessageId,
            _message_type: MessageType,
        ) -> crate::core::data_context::Result<Message> {
            Err(DataAccessError::Other("mock".into()))
        }
        async fn get_messages_by_fid(
            &self,
            _fid: Fid,
            _message_type: MessageType,
            _limit: usize,
            _cursor: Option<MessageId>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn store_message(&self, _message: Message) -> crate::core::data_context::Result<()> {
            Ok(())
        }
        async fn delete_message(
            &self,
            _id: &MessageId,
            _message_type: MessageType,
        ) -> crate::core::data_context::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    struct MockHub;

    #[async_trait]
    impl crate::core::data_context::HubClient for MockHub {
        async fn get_user_data_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_user_data(
            &self,
            _fid: Fid,
            _data_type: &str,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }
        async fn get_username_proofs_by_fid(
            &self,
            _fid: Fid,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_username_proof_by_name(
            &self,
            _name: &str,
        ) -> crate::core::data_context::Result<Option<crate::proto::UserNameProof>> {
            Ok(None)
        }
        async fn get_verifications_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_verification(
            &self,
            _fid: Fid,
            _address: &[u8],
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }
        async fn get_casts_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_cast(
            &self,
            _fid: Fid,
            _hash: &[u8],
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }
        async fn get_casts_by_mention(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_casts_by_parent(
            &self,
            _parent_fid: Fid,
            _parent_hash: &[u8],
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_casts_by_parent_url(
            &self,
            _parent_url: &str,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_all_casts_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_reaction(
            &self,
            _fid: Fid,
            _reaction_type: u8,
            _target_cast_fid: Option<Fid>,
            _target_cast_hash: Option<&[u8]>,
            _target_url: Option<&str>,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }
        async fn get_reactions_by_fid(
            &self,
            _fid: Fid,
            _reaction_type: Option<u8>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_reactions_by_target(
            &self,
            _target_cast_fid: Option<Fid>,
            _target_cast_hash: Option<&[u8]>,
            _target_url: Option<&str>,
            _reaction_type: Option<u8>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_all_reactions_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_all_verification_messages_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_link(
            &self,
            _fid: Fid,
            _link_type: &str,
            _target_fid: Fid,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }
        async fn get_links_by_fid(
            &self,
            _fid: Fid,
            _link_type: Option<&str>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_links_by_target(
            &self,
            _target_fid: Fid,
            _link_type: Option<&str>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_link_compact_state_by_fid(
            &self,
            _fid: Fid,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
        async fn get_all_links_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
    }

    type TestService = WaypointQuery<MockDb, MockHub>;

    fn make_reply(replies: Option<Vec<ConversationCast>>) -> ConversationCast {
        ConversationCast {
            id: "id".to_string(),
            message_type: "cast".to_string(),
            fid: None,
            timestamp: None,
            text: None,
            mentions: None,
            parent: None,
            parent_url: None,
            embeds: None,
            quoted_casts: None,
            replies,
            has_more_replies: None,
        }
    }

    #[test]
    fn test_count_replies_recursive_no_replies() {
        let tree = ConversationTree { replies: vec![], has_more: false };
        assert_eq!(TestService::count_replies_recursive(&tree), 0);
    }

    #[test]
    fn test_count_replies_recursive_empty_replies() {
        let tree = ConversationTree { replies: vec![], has_more: false };
        assert_eq!(TestService::count_replies_recursive(&tree), 0);
    }

    #[test]
    fn test_count_replies_recursive_flat_replies() {
        let tree = ConversationTree {
            replies: vec![make_reply(None), make_reply(None), make_reply(None)],
            has_more: false,
        };
        assert_eq!(TestService::count_replies_recursive(&tree), 3);
    }

    #[test]
    fn test_count_replies_recursive_nested_replies() {
        let tree = ConversationTree {
            replies: vec![
                make_reply(Some(vec![make_reply(None), make_reply(None)])),
                make_reply(None),
            ],
            has_more: false,
        };
        // 2 top-level + 2 nested = 4
        assert_eq!(TestService::count_replies_recursive(&tree), 4);
    }

    #[test]
    fn test_count_replies_recursive_deeply_nested() {
        let tree = ConversationTree {
            replies: vec![make_reply(Some(vec![make_reply(Some(vec![make_reply(None)]))]))],
            has_more: false,
        };
        // 1 at each level = 3
        assert_eq!(TestService::count_replies_recursive(&tree), 3);
    }

    #[test]
    fn test_count_replies_recursive_replies_not_array() {
        let tree = ConversationTree { replies: vec![], has_more: false };
        assert_eq!(TestService::count_replies_recursive(&tree), 0);
    }

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(TestService::truncate_text("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_text_exact_length() {
        assert_eq!(TestService::truncate_text("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_text_long() {
        assert_eq!(TestService::truncate_text("hello world", 5), "hello...");
    }

    #[test]
    fn test_truncate_text_empty() {
        assert_eq!(TestService::truncate_text("", 5), "");
    }
}
