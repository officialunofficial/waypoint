//! Reaction query operations.

use crate::core::types::Fid;
use crate::query::responses::{
    CastReference, ReactionLookupResponse, ReactionNotFoundResponse, ReactionsByFidResponse,
    ReactionsByTargetResponse,
};
use crate::query::{QueryError, QueryResult, WaypointQuery};

use prost::Message as ProstMessage;

// Common types are used in the handler implementations

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    fn validate_reaction_type(reaction_type: u8) -> QueryResult<()> {
        if !(1..=2).contains(&reaction_type) {
            return Err(QueryError::InvalidInput(format!(
                "Invalid reaction type: {} (expected 1=Like or 2=Recast)",
                reaction_type
            )));
        }

        Ok(())
    }

    fn validate_optional_reaction_type(reaction_type: Option<u8>) -> QueryResult<()> {
        if let Some(value) = reaction_type {
            Self::validate_reaction_type(value)?;
        }

        Ok(())
    }

    fn validate_reaction_target(
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
    ) -> QueryResult<()> {
        if let Some(url) = target_url {
            if url.trim().is_empty() {
                return Err(QueryError::InvalidInput("target_url cannot be empty".to_string()));
            }

            return Ok(());
        }

        match (target_cast_fid, target_cast_hash) {
            (Some(_), Some(hash)) => {
                if hash.is_empty() {
                    return Err(QueryError::InvalidInput(
                        "target_cast_hash cannot be empty".to_string(),
                    ));
                }
                Ok(())
            },
            (None, None) => Err(QueryError::InvalidInput(
                "Either target_url or both target_cast_fid and target_cast_hash are required"
                    .to_string(),
            )),
            _ => Err(QueryError::InvalidInput(
                "target_cast_fid and target_cast_hash must be provided together".to_string(),
            )),
        }
    }

    /// Get a specific reaction
    pub async fn do_get_reaction(
        &self,
        fid: Fid,
        reaction_type: u8,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
    ) -> QueryResult<ReactionLookupResponse> {
        tracing::debug!("Query: Fetching reaction with FID: {} and type: {}", fid, reaction_type);

        Self::validate_reaction_type(reaction_type)?;
        Self::validate_reaction_target(target_cast_fid, target_cast_hash, target_url)?;

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

                let reaction = super::utils::process_reaction_message(&message, &msg_data)
                    .ok_or_else(|| {
                        QueryError::Processing(format!(
                            "reaction payload could not be processed for FID {}",
                            fid
                        ))
                    })?;
                Ok(ReactionLookupResponse::Found(reaction))
            },
            Ok(None) => {
                let (target_cast, target_url) = if let Some(url) = target_url {
                    (None, Some(url.to_string()))
                } else {
                    (
                        match (target_cast_fid, target_cast_hash) {
                            (Some(target_fid), Some(target_hash)) => Some(CastReference {
                                fid: target_fid.value(),
                                hash: hex::encode(target_hash),
                            }),
                            _ => None,
                        },
                        None,
                    )
                };

                Ok(ReactionLookupResponse::NotFound(ReactionNotFoundResponse {
                    fid: fid.value(),
                    reaction_type_id: i32::from(reaction_type),
                    found: false,
                    error: "Reaction not found".to_string(),
                    target_cast,
                    target_url,
                }))
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
    ) -> QueryResult<ReactionsByFidResponse> {
        tracing::debug!("Query: Fetching reactions for FID: {}", fid);

        Self::validate_optional_reaction_type(reaction_type)?;

        let messages = self.data_context.get_reactions_by_fid(fid, reaction_type, limit).await?;
        let reactions = super::utils::parse_reaction_messages(&messages);
        Ok(ReactionsByFidResponse { fid: fid.value(), count: reactions.len(), reactions })
    }

    /// Get reactions by target (cast or URL)
    pub async fn do_get_reactions_by_target(
        &self,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> QueryResult<ReactionsByTargetResponse> {
        tracing::debug!("Query: Fetching reactions by target");

        Self::validate_optional_reaction_type(reaction_type)?;
        Self::validate_reaction_target(target_cast_fid, target_cast_hash, target_url)?;

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

        let reactions = super::utils::parse_reaction_messages(&messages);

        let (target_cast, target_url) = if let Some(url) = target_url {
            (None, Some(url.to_string()))
        } else {
            (
                match (target_cast_fid, target_cast_hash) {
                    (Some(tfid), Some(thash)) => {
                        Some(CastReference { fid: tfid.value(), hash: hex::encode(thash) })
                    },
                    _ => None,
                },
                None,
            )
        };

        Ok(ReactionsByTargetResponse { count: reactions.len(), reactions, target_cast, target_url })
    }

    /// Get all reactions by FID with timestamp filtering
    pub async fn do_get_all_reactions_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> QueryResult<ReactionsByFidResponse> {
        tracing::debug!("Query: Fetching all reactions for FID: {} with time filtering", fid);

        let messages =
            self.data_context.get_all_reactions_by_fid(fid, limit, _start_time, _end_time).await?;
        let reactions = super::utils::parse_reaction_messages(&messages);
        Ok(ReactionsByFidResponse { fid: fid.value(), count: reactions.len(), reactions })
    }
}
