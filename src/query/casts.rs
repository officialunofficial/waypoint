//! Cast and conversation query operations.

use crate::core::types::{Fid, Message, MessageType};
use crate::query::responses::{
    CastLookupResponse, CastNotFoundResponse, CastReference, CastsByFidResponse,
    CastsByMentionResponse, CastsByParentResponse, CastsByParentUrlResponse, ConversationCast,
    ConversationFoundResponse, ConversationParentCast, ConversationParticipants,
    ConversationResponse, ConversationTree, UserProfile,
};
use crate::query::types::ConversationParams;
use crate::query::{QueryError, QueryResult, WaypointQuery};
use std::collections::BTreeMap;
use std::collections::HashSet;

use prost::Message as ProstMessage;

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

    if let Ok(msg_data) = <crate::proto::MessageData as ProstMessage>::decode(&*message.payload) {
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

            let embeds = super::utils::parse_embed_list(&cast.embeds);
            if !embeds.is_empty() {
                formatted.embeds = Some(embeds);
            }
        }
    }

    formatted
}

/// Decode a message payload and extract the FID, returning it if successful.
fn extract_fid(message: &Message) -> Option<Fid> {
    let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload).ok()?;
    Some(Fid::from(msg_data.fid))
}

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
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

    pub async fn do_get_conversation(
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

        let mut participants = HashSet::new();
        if let Some(author_fid) = extract_fid(&root_cast) {
            participants.insert(author_fid);
        }

        let mut parent_casts = Vec::new();
        let parent_info = Self::get_parent_cast_info(&root_cast);

        if let Some((parent_fid, parent_hash)) = parent_info {
            tracing::debug!("Found parent cast reference, fetching thread context");
            self.fetch_parent_casts(
                &mut parent_casts,
                &mut participants,
                parent_fid,
                &parent_hash,
                5,
            )
            .await;
        }

        let conversation_params = ConversationParams { recursive, max_depth, limit };
        let conversation_tree = self
            .build_conversation_tree(&root_cast, &mut participants, conversation_params, 0)
            .await;

        let topic = Self::extract_conversation_topic(&root_cast);

        let quoted_casts = self.fetch_quoted_casts(&root_cast, &mut participants).await;

        let user_data_map = self.fetch_participant_profiles(&participants).await;

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

    /// Extract cast embed references and fetch the quoted casts, adding authors to participants.
    async fn fetch_quoted_casts(
        &self,
        cast: &Message,
        participants: &mut HashSet<Fid>,
    ) -> Vec<ConversationCast> {
        let mut quoted_casts = Vec::new();

        let Ok(msg_data) = <crate::proto::MessageData as ProstMessage>::decode(&*cast.payload)
        else {
            return quoted_casts;
        };

        let Some(crate::proto::message_data::Body::CastAddBody(body)) = &msg_data.body else {
            return quoted_casts;
        };

        for embed in &body.embeds {
            if let Some(crate::proto::embed::Embed::CastId(cast_id)) = &embed.embed {
                let quoted_fid = Fid::from(cast_id.fid);
                if let Ok(Some(quoted_cast)) =
                    self.data_context.get_cast(quoted_fid, &cast_id.hash).await
                {
                    if let Some(author_fid) = extract_fid(&quoted_cast) {
                        participants.insert(author_fid);
                    }
                    quoted_casts.push(format_message(&quoted_cast));
                }
            }
        }

        quoted_casts
    }

    /// Fetch user profiles for all conversation participants.
    async fn fetch_participant_profiles(
        &self,
        participants: &HashSet<Fid>,
    ) -> BTreeMap<String, UserProfile> {
        let mut user_data_map = BTreeMap::new();

        for &participant_fid in participants {
            let Ok(messages) = self.data_context.get_user_data_by_fid(participant_fid, 20).await
            else {
                continue;
            };

            if messages.is_empty() {
                continue;
            }

            let mut profile =
                UserProfile { fid: participant_fid.value(), ..UserProfile::default() };

            for message in messages {
                if message.message_type != MessageType::UserData {
                    continue;
                }

                if let Ok(msg_data) =
                    <crate::proto::MessageData as ProstMessage>::decode(&*message.payload)
                    && let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                        msg_data.body
                {
                    super::users::set_user_data_field(
                        &mut profile,
                        user_data.r#type,
                        user_data.value,
                    );
                }
            }

            user_data_map.insert(participant_fid.value().to_string(), profile);
        }

        user_data_map
    }

    /// Extract parent cast reference (fid + hash) from a cast message, if present.
    fn get_parent_cast_info(cast: &Message) -> Option<(Fid, Vec<u8>)> {
        let msg_data: crate::proto::MessageData = ProstMessage::decode(&*cast.payload).ok()?;

        let Some(crate::proto::message_data::Body::CastAddBody(cast_body)) = &msg_data.body else {
            return None;
        };

        match &cast_body.parent {
            Some(crate::proto::cast_add_body::Parent::ParentCastId(parent)) => {
                Some((Fid::from(parent.fid), parent.hash.clone()))
            },
            _ => None,
        }
    }

    async fn fetch_parent_casts(
        &self,
        parent_casts: &mut Vec<ConversationCast>,
        participants: &mut HashSet<Fid>,
        parent_fid: Fid,
        parent_hash: &[u8],
        remaining_depth: usize,
    ) {
        if remaining_depth == 0 {
            return;
        }

        tracing::debug!(
            "Fetching specific cast with FID: {} and hash: {}",
            parent_fid,
            hex::encode(parent_hash)
        );

        match self.data_context.get_cast(parent_fid, parent_hash).await {
            Ok(Some(parent_cast)) => {
                if let Some(author_fid) = extract_fid(&parent_cast) {
                    participants.insert(author_fid);
                }

                parent_casts.push(format_message(&parent_cast));

                if let Some((grandparent_fid, grandparent_hash)) =
                    Self::get_parent_cast_info(&parent_cast)
                {
                    Box::pin(self.fetch_parent_casts(
                        parent_casts,
                        participants,
                        grandparent_fid,
                        &grandparent_hash,
                        remaining_depth - 1,
                    ))
                    .await;
                }
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

    async fn build_conversation_tree(
        &self,
        parent_cast: &Message,
        participants: &mut HashSet<Fid>,
        params: ConversationParams,
        current_depth: usize,
    ) -> ConversationTree {
        if current_depth >= params.max_depth {
            return ConversationTree { replies: vec![], has_more: false };
        }

        let parent_fid = extract_fid(parent_cast).unwrap_or(Fid::from(0));

        let parent_hash = match super::utils::parse_hash_bytes(parent_cast.id.value()) {
            Ok(hash_bytes) => hash_bytes,
            Err(message) => {
                tracing::error!(
                    "Failed to decode cast hash from ID: {} - {}",
                    parent_cast.id.value(),
                    message
                );
                return ConversationTree { replies: vec![], has_more: false };
            },
        };

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

        for reply in &replies {
            if let Some(reply_fid) = extract_fid(reply) {
                participants.insert(reply_fid);
            }
        }

        let mut formatted_replies = Vec::new();
        for reply in replies {
            let mut formatted_reply = format_message(&reply);

            let quoted = self.fetch_quoted_casts(&reply, participants).await;
            if !quoted.is_empty() {
                formatted_reply.quoted_casts = Some(quoted);
            }

            if params.recursive && current_depth < params.max_depth - 1 {
                let nested = Box::pin(self.build_conversation_tree(
                    &reply,
                    participants,
                    params,
                    current_depth + 1,
                ))
                .await;

                formatted_reply.replies = Some(nested.replies);
                formatted_reply.has_more_replies = Some(nested.has_more);
            }

            formatted_replies.push(formatted_reply);
        }

        let has_more = formatted_replies.len() >= params.limit;
        ConversationTree { replies: formatted_replies, has_more }
    }

    fn extract_conversation_topic(root_cast: &Message) -> String {
        let text = Self::extract_cast_text(root_cast).unwrap_or_default();

        if text.is_empty() {
            return "Untitled conversation".to_string();
        }

        let topic: String = text.split_whitespace().take(5).collect::<Vec<_>>().join(" ");

        if topic.is_empty() { "Untitled conversation".to_string() } else { format!("{}...", topic) }
    }

    fn extract_cast_text(message: &Message) -> Option<String> {
        let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload).ok()?;

        match &msg_data.body {
            Some(crate::proto::message_data::Body::CastAddBody(cast)) => Some(cast.text.clone()),
            _ => None,
        }
    }

    async fn generate_conversation_summary(
        &self,
        root_cast: &Message,
        conversation_tree: &ConversationTree,
    ) -> String {
        let reply_count = Self::count_replies_recursive(conversation_tree);

        let mut root_author = Fid::from(0);
        let mut root_text = String::new();

        if let Ok(msg_data) =
            <crate::proto::MessageData as ProstMessage>::decode(&*root_cast.payload)
        {
            root_author = Fid::from(msg_data.fid);
            if let Some(crate::proto::message_data::Body::CastAddBody(cast)) = &msg_data.body {
                root_text = cast.text.clone();
            }
        }

        let author_display = match self.data_context.get_user_data_by_fid(root_author, 20).await {
            Ok(messages) => {
                let mut username = None;
                let mut display_name = None;

                for message in messages {
                    if message.message_type != MessageType::UserData {
                        continue;
                    }

                    if let Ok(msg_data) =
                        <crate::proto::MessageData as ProstMessage>::decode(&*message.payload)
                        && let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                            msg_data.body
                    {
                        match user_data.r#type {
                            2 => display_name = Some(user_data.value),
                            6 => username = Some(user_data.value),
                            _ => {},
                        }
                    }
                }

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

        format!(
            "Conversation started by {} with: \"{}\". {} replies in the thread.",
            author_display,
            Self::truncate_text(&root_text, 50),
            reply_count
        )
    }

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
        assert_eq!(TestService::count_replies_recursive(&tree), 4);
    }

    #[test]
    fn test_count_replies_recursive_deeply_nested() {
        let tree = ConversationTree {
            replies: vec![make_reply(Some(vec![make_reply(Some(vec![make_reply(None)]))]))],
            has_more: false,
        };
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
