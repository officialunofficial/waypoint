//! MCP API handlers for Waypoint

mod common;
mod utils;

use std::sync::Arc;

use crate::core::types::Fid;
use crate::database::NullDb;
use crate::query::{QueryError, WaypointQuery, parse_hash_bytes};

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::Parameters,
    },
    model::CallToolResult,
    model::*,
    prompt, prompt_handler, prompt_router,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use tracing::info;

/// MCP protocol version used by this server
pub const MCP_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_03_26;

#[derive(Clone)]
pub struct WaypointMcpTools {
    query: Arc<WaypointQuery<NullDb, crate::hub::providers::FarcasterHubClient>>,
    tool_router: ToolRouter<WaypointMcpTools>,
    prompt_router: PromptRouter<WaypointMcpTools>,
}

impl WaypointMcpTools {
    pub fn new(query: WaypointQuery<NullDb, crate::hub::providers::FarcasterHubClient>) -> Self {
        Self {
            query: Arc::new(query),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

#[tool_router]
impl WaypointMcpTools {
    /// Create a text resource with the given URI and name
    fn create_resource_text(uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    /// Create a JSON resource template with the given URI template, name, and description
    fn create_resource_template_json(
        uri_template: &str,
        name: &str,
        description: &str,
    ) -> ResourceTemplate {
        RawResourceTemplate {
            uri_template: uri_template.to_string(),
            name: name.to_string(),
            title: None,
            description: Some(description.to_string()),
            mime_type: Some("application/json".to_string()),
            icons: None,
        }
        .no_annotation()
    }

    fn map_query_error(error: QueryError) -> McpError {
        match error {
            QueryError::InvalidInput(message) => McpError::invalid_params(message, None),
            QueryError::DataAccess(err) => McpError::internal_error(
                "Query data access error",
                Some(serde_json::json!({ "error": err.to_string() })),
            ),
            QueryError::Processing(message) => McpError::internal_error(
                "Query processing error",
                Some(serde_json::json!({ "error": message })),
            ),
        }
    }

    fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String, McpError> {
        serde_json::to_string_pretty(value).map_err(|err| {
            McpError::internal_error(
                "Failed to serialize JSON response",
                Some(serde_json::json!({ "error": err.to_string() })),
            )
        })
    }

    fn call_tool_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let json = Self::serialize_json(value)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Create a ReadResourceResult containing JSON content
    fn resource_json_contents(
        uri: &str,
        json: &impl serde::Serialize,
    ) -> Result<ReadResourceResult, McpError> {
        let text = Self::serialize_json(json)?;
        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: uri.to_string(),
                mime_type: Some("application/json".to_string()),
                text,
                meta: None,
            }],
        })
    }

    fn waypoint_resource_name(resource: &utils::WaypointResource) -> &'static str {
        match resource {
            utils::WaypointResource::UserByFid { .. } => "user_by_fid",
            utils::WaypointResource::UserByUsername { .. } => "user_by_username",
            utils::WaypointResource::VerificationsByFid { .. } => "verifications_by_fid",
            utils::WaypointResource::VerificationByAddress { .. } => "verification_by_address",
            utils::WaypointResource::AllVerificationMessagesByFid { .. } => {
                "all_verification_messages_by_fid"
            },
            utils::WaypointResource::Cast { .. } => "cast",
            utils::WaypointResource::Conversation { .. } => "conversation",
            utils::WaypointResource::CastsByFid { .. } => "casts_by_fid",
            utils::WaypointResource::CastsByMention { .. } => "casts_by_mention",
            utils::WaypointResource::CastsByParent { .. } => "casts_by_parent",
            utils::WaypointResource::CastsByParentUrl { .. } => "casts_by_parent_url",
            utils::WaypointResource::ReactionsByFid { .. } => "reactions_by_fid",
            utils::WaypointResource::ReactionsByTargetCast { .. } => "reactions_by_target_cast",
            utils::WaypointResource::ReactionsByTargetUrl { .. } => "reactions_by_target_url",
            utils::WaypointResource::LinksByFid { .. } => "links_by_fid",
            utils::WaypointResource::LinksByTarget { .. } => "links_by_target",
            utils::WaypointResource::LinkCompactStateByFid { .. } => "link_compact_state_by_fid",
            utils::WaypointResource::UsernameProofByName { .. } => "username_proof_by_name",
            utils::WaypointResource::UsernameProofsByFid { .. } => "username_proofs_by_fid",
        }
    }

    #[tool(description = "Get Farcaster user data by FID", annotations(read_only_hint = true))]
    async fn get_user_by_fid(
        &self,
        Parameters(common::UserByFidRequest { fid }): Parameters<common::UserByFidRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(transport = "mcp", kind = "tool", tool = "get_user_by_fid", fid, "MCP tool call");
        let fid = Fid::from(fid);
        let result = self.query.do_get_user_by_fid(fid).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get verified addresses for a Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_verifications_by_fid(
        &self,
        Parameters(common::GetVerificationsRequest { fid, limit }): Parameters<
            common::GetVerificationsRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_verifications_by_fid",
            fid,
            limit,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_verifications_by_fid(fid, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get a specific verification by FID and address",
        annotations(read_only_hint = true)
    )]
    async fn get_verification(
        &self,
        Parameters(common::GetVerificationRequest { fid, address }): Parameters<
            common::GetVerificationRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_verification",
            fid,
            has_address = !address.is_empty(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result =
            self.query.do_get_verification(fid, &address).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get all verification messages for a Farcaster user with optional timestamp filtering",
        annotations(read_only_hint = true)
    )]
    async fn get_all_verification_messages_by_fid(
        &self,
        Parameters(common::FidTimestampRequest { fid, limit, start_time, end_time }): Parameters<
            common::FidTimestampRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_all_verification_messages_by_fid",
            fid,
            limit,
            has_start_time = start_time.is_some(),
            has_end_time = end_time.is_some(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_all_verification_messages_by_fid(fid, limit, start_time, end_time)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Find a Farcaster user's FID by their username",
        annotations(read_only_hint = true)
    )]
    async fn get_fid_by_username(
        &self,
        Parameters(common::GetFidByUsernameRequest { username }): Parameters<
            common::GetFidByUsernameRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_fid_by_username",
            username_len = username.len(),
            "MCP tool call"
        );
        let result =
            self.query.do_get_fid_by_username(&username).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get a complete Farcaster user profile by username",
        annotations(read_only_hint = true)
    )]
    async fn get_user_by_username(
        &self,
        Parameters(common::GetFidByUsernameRequest { username }): Parameters<
            common::GetFidByUsernameRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_user_by_username",
            username_len = username.len(),
            "MCP tool call"
        );
        let result =
            self.query.do_get_user_by_username(&username).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(description = "Get a specific cast by FID and hash", annotations(read_only_hint = true))]
    async fn get_cast(
        &self,
        Parameters(common::GetCastRequest { fid, hash }): Parameters<common::GetCastRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_cast",
            fid,
            has_hash = !hash.is_empty(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self.query.do_get_cast(fid, &hash).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get casts by a specific Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_casts_by_fid(
        &self,
        Parameters(common::GetCastsRequest { fid, limit }): Parameters<common::GetCastsRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_casts_by_fid",
            fid,
            limit,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result =
            self.query.do_get_casts_by_fid(fid, limit).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get casts that mention a specific Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_casts_by_mention(
        &self,
        Parameters(common::GetCastMentionsRequest { fid, limit }): Parameters<
            common::GetCastMentionsRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_casts_by_mention",
            fid,
            limit,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result =
            self.query.do_get_casts_by_mention(fid, limit).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get cast replies to a specific parent cast",
        annotations(read_only_hint = true)
    )]
    async fn get_casts_by_parent(
        &self,
        Parameters(common::GetCastRepliesRequest { parent_fid, parent_hash, limit }): Parameters<
            common::GetCastRepliesRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_casts_by_parent",
            parent_fid,
            limit,
            has_parent_hash = !parent_hash.is_empty(),
            "MCP tool call"
        );
        let parent_fid = Fid::from(parent_fid);
        let result = self
            .query
            .do_get_casts_by_parent(parent_fid, &parent_hash, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(description = "Get cast replies to a specific URL", annotations(read_only_hint = true))]
    async fn get_casts_by_parent_url(
        &self,
        Parameters(common::GetCastRepliesByUrlRequest { parent_url, limit }): Parameters<
            common::GetCastRepliesByUrlRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_casts_by_parent_url",
            limit,
            has_parent_url = !parent_url.is_empty(),
            "MCP tool call"
        );
        let result = self
            .query
            .do_get_casts_by_parent_url(&parent_url, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get all casts from a user with optional timestamp filtering",
        annotations(read_only_hint = true)
    )]
    async fn get_all_casts_by_fid(
        &self,
        Parameters(common::GetAllCastsWithTimeRequest { fid, limit, start_time, end_time }): Parameters<common::GetAllCastsWithTimeRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_all_casts_by_fid",
            fid,
            limit,
            has_start_time = start_time.is_some(),
            has_end_time = end_time.is_some(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_all_casts_by_fid(fid, limit, start_time, end_time)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get conversation details for a cast, including participants and replies",
        annotations(read_only_hint = true)
    )]
    async fn get_conversation(
        &self,
        Parameters(common::GetConversationRequest {
            fid,
            cast_hash,
            recursive,
            max_depth,
            limit
        }): Parameters<common::GetConversationRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_conversation",
            fid,
            limit,
            recursive,
            max_depth,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_conversation(fid, &cast_hash, recursive, max_depth, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(description = "Get a specific reaction", annotations(read_only_hint = true))]
    async fn get_reaction(
        &self,
        Parameters(common::ReactionRequest {
            fid,
            reaction_type,
            target_cast_fid,
            target_cast_hash,
            target_url,
        }): Parameters<common::ReactionRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_reaction",
            fid,
            reaction_type,
            has_target_cast_fid = target_cast_fid.is_some(),
            has_target_cast_hash = target_cast_hash.as_ref().is_some_and(|hash| !hash.is_empty()),
            has_target_url = target_url.as_ref().is_some_and(|url| !url.is_empty()),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let target_cast_fid = target_cast_fid.map(Fid::from);

        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            Some(parse_hash_bytes(hash).map_err(|message| {
                McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
            })?)
        } else {
            None
        };

        let target_cast_hash_ref = target_cast_hash_bytes.as_deref();
        let target_url_ref = target_url.as_deref();

        let result = self
            .query
            .do_get_reaction(
                fid,
                reaction_type,
                target_cast_fid,
                target_cast_hash_ref,
                target_url_ref,
            )
            .await
            .map_err(Self::map_query_error)?;

        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get reactions by a specific Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_reactions_by_fid(
        &self,
        Parameters(common::ReactionsByFidRequest { fid, reaction_type, limit }): Parameters<
            common::ReactionsByFidRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_reactions_by_fid",
            fid,
            limit,
            has_reaction_type = reaction_type.is_some(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_reactions_by_fid(fid, reaction_type, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get reactions for a target (cast or URL)",
        annotations(read_only_hint = true)
    )]
    async fn get_reactions_by_target(
        &self,
        Parameters(common::ReactionsByTargetRequest {
            target_cast_fid,
            target_cast_hash,
            target_url,
            reaction_type,
            limit,
        }): Parameters<common::ReactionsByTargetRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_reactions_by_target",
            limit,
            has_target_cast_fid = target_cast_fid.is_some(),
            has_target_cast_hash = target_cast_hash.as_ref().is_some_and(|hash| !hash.is_empty()),
            has_target_url = target_url.as_ref().is_some_and(|url| !url.is_empty()),
            has_reaction_type = reaction_type.is_some(),
            "MCP tool call"
        );
        let target_cast_fid = target_cast_fid.map(Fid::from);

        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            Some(parse_hash_bytes(hash).map_err(|message| {
                McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
            })?)
        } else {
            None
        };

        let target_cast_hash_ref = target_cast_hash_bytes.as_deref();
        let target_url_ref = target_url.as_deref();

        let result = self
            .query
            .do_get_reactions_by_target(
                target_cast_fid,
                target_cast_hash_ref,
                target_url_ref,
                reaction_type,
                limit,
            )
            .await
            .map_err(Self::map_query_error)?;

        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get all reactions from a user with optional timestamp filtering",
        annotations(read_only_hint = true)
    )]
    async fn get_all_reactions_by_fid(
        &self,
        Parameters(common::FidTimestampRequest { fid, limit, start_time, end_time }): Parameters<
            common::FidTimestampRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_all_reactions_by_fid",
            fid,
            limit,
            has_start_time = start_time.is_some(),
            has_end_time = end_time.is_some(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_all_reactions_by_fid(fid, limit, start_time, end_time)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(description = "Get a specific link", annotations(read_only_hint = true))]
    async fn get_link(
        &self,
        Parameters(common::LinkRequest { fid, link_type, target_fid }): Parameters<
            common::LinkRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_link",
            fid,
            target_fid,
            has_link_type = !link_type.is_empty(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let target_fid = Fid::from(target_fid);
        let result = self
            .query
            .do_get_link(fid, &link_type, target_fid)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get links by a specific Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_links_by_fid(
        &self,
        Parameters(common::LinksByFidRequest { fid, link_type, limit }): Parameters<
            common::LinksByFidRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_links_by_fid",
            fid,
            limit,
            has_link_type = link_type.as_ref().is_some_and(|value| !value.is_empty()),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let link_type_ref = link_type.as_deref().filter(|value| !value.trim().is_empty());
        let result = self
            .query
            .do_get_links_by_fid(fid, link_type_ref, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get links to a target Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_links_by_target(
        &self,
        Parameters(common::LinksByTargetRequest { target_fid, link_type, limit }): Parameters<
            common::LinksByTargetRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_links_by_target",
            target_fid,
            limit,
            has_link_type = link_type.as_ref().is_some_and(|value| !value.is_empty()),
            "MCP tool call"
        );
        let target_fid = Fid::from(target_fid);
        let link_type_ref = link_type.as_deref().filter(|value| !value.trim().is_empty());
        let result = self
            .query
            .do_get_links_by_target(target_fid, link_type_ref, limit)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get link compact state messages for a Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_link_compact_state_by_fid(
        &self,
        Parameters(common::FidRequest { fid }): Parameters<common::FidRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_link_compact_state_by_fid",
            fid,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_link_compact_state_by_fid(fid)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get all links from a user with optional timestamp filtering",
        annotations(read_only_hint = true)
    )]
    async fn get_all_links_by_fid(
        &self,
        Parameters(common::FidTimestampRequest { fid, limit, start_time, end_time }): Parameters<
            common::FidTimestampRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_all_links_by_fid",
            fid,
            limit,
            has_start_time = start_time.is_some(),
            has_end_time = end_time.is_some(),
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result = self
            .query
            .do_get_all_links_by_fid(fid, limit, start_time, end_time)
            .await
            .map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(description = "Get a single username proof by name", annotations(read_only_hint = true))]
    async fn get_username_proof(
        &self,
        Parameters(common::GetUsernameProofRequest { name }): Parameters<
            common::GetUsernameProofRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_username_proof",
            name_len = name.len(),
            "MCP tool call"
        );
        let result =
            self.query.do_get_username_proof(&name).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }

    #[tool(
        description = "Get username proofs for a Farcaster user (fname and ENS registrations)",
        annotations(read_only_hint = true)
    )]
    async fn get_username_proofs_by_fid(
        &self,
        Parameters(common::FidRequest { fid }): Parameters<common::FidRequest>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            transport = "mcp",
            kind = "tool",
            tool = "get_username_proofs_by_fid",
            fid,
            "MCP tool call"
        );
        let fid = Fid::from(fid);
        let result =
            self.query.do_get_username_proofs_by_fid(fid).await.map_err(Self::map_query_error)?;
        Self::call_tool_json(&result)
    }
}

#[prompt_router]
impl WaypointMcpTools {
    /// A prompt that helps with Farcaster data querying and takes an FID parameter
    #[prompt(name = "waypoint_prompt")]
    async fn waypoint_prompt(
        &self,
        Parameters(common::WaypointPromptArgs { fid, username }): Parameters<
            common::WaypointPromptArgs,
        >,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let username_context = match username {
            Some(name) => format!(" (username: {})", name),
            None => String::new(),
        };

        let prompt = format!(
            "You are a helpful assistant for exploring Farcaster data. You're currently focusing on FID {}{}. You can help fetch user data, verifications, casts, reactions, and links for this user using the appropriate tools.",
            fid, username_context
        );

        Ok(GetPromptResult {
            description: Some("A prompt for Farcaster data exploration".to_string()),
            messages: vec![PromptMessage {
                role: PromptMessageRole::Assistant,
                content: PromptMessageContent::text(prompt),
            }],
        })
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for WaypointMcpTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: MCP_PROTOCOL_VERSION,
            capabilities: ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Waypoint MCP service with tools to query Farcaster data. Access user data, verifications, casts, reactions, and links with comprehensive APIs for exploring the Farcaster social graph.".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                Self::create_resource_text("str:///waypoint/docs", "waypoint-docs"),
                Self::create_resource_text("memo://farcaster-info", "farcaster-info"),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParams { uri, .. }: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if uri.starts_with("waypoint://") || uri.starts_with("waypoint:///") {
            let resource = utils::parse_waypoint_resource_uri(&uri).map_err(|message| {
                McpError::invalid_params(
                    message,
                    Some(serde_json::json!({
                        "uri": uri
                    })),
                )
            })?;

            info!(
                transport = "mcp",
                kind = "resource",
                resource = Self::waypoint_resource_name(&resource),
                "MCP resource read"
            );

            let limit = common::default_limit();

            let response = match resource {
                utils::WaypointResource::UserByFid { fid } => {
                    let result = self
                        .query
                        .do_get_user_by_fid(Fid::from(fid))
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::UserByUsername { username } => {
                    let result = self
                        .query
                        .do_get_user_by_username(&username)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::VerificationsByFid { fid } => {
                    let result = self
                        .query
                        .do_get_verifications_by_fid(Fid::from(fid), limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::VerificationByAddress { fid, address } => {
                    let result = self
                        .query
                        .do_get_verification(Fid::from(fid), &address)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::AllVerificationMessagesByFid { fid } => {
                    let result = self
                        .query
                        .do_get_all_verification_messages_by_fid(Fid::from(fid), limit, None, None)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::Cast { fid, hash } => {
                    let result = self
                        .query
                        .do_get_cast(Fid::from(fid), &hash)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::Conversation { fid, hash } => {
                    let result = self
                        .query
                        .do_get_conversation(Fid::from(fid), &hash, true, 5, limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::CastsByFid { fid } => {
                    let result = self
                        .query
                        .do_get_casts_by_fid(Fid::from(fid), limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::CastsByMention { fid } => {
                    let result = self
                        .query
                        .do_get_casts_by_mention(Fid::from(fid), limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::CastsByParent { fid, hash } => {
                    let result = self
                        .query
                        .do_get_casts_by_parent(Fid::from(fid), &hash, limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::CastsByParentUrl { url } => {
                    let result = self
                        .query
                        .do_get_casts_by_parent_url(&url, limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::ReactionsByFid { fid } => {
                    let result = self
                        .query
                        .do_get_reactions_by_fid(Fid::from(fid), None, limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::ReactionsByTargetCast { fid, hash } => {
                    let target_cast_hash = parse_hash_bytes(&hash).map_err(|message| {
                        McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
                    })?;

                    let result = self
                        .query
                        .do_get_reactions_by_target(
                            Some(Fid::from(fid)),
                            Some(target_cast_hash.as_slice()),
                            None,
                            None,
                            limit,
                        )
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::ReactionsByTargetUrl { url } => {
                    let result = self
                        .query
                        .do_get_reactions_by_target(None, None, Some(&url), None, limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::LinksByFid { fid } => {
                    let link_type = common::default_link_type();
                    let result = self
                        .query
                        .do_get_links_by_fid(Fid::from(fid), Some(link_type.as_str()), limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::LinksByTarget { fid } => {
                    let link_type = common::default_link_type();
                    let result = self
                        .query
                        .do_get_links_by_target(Fid::from(fid), Some(link_type.as_str()), limit)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::LinkCompactStateByFid { fid } => {
                    let result = self
                        .query
                        .do_get_link_compact_state_by_fid(Fid::from(fid))
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::UsernameProofByName { name } => {
                    let result = self
                        .query
                        .do_get_username_proof(&name)
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
                utils::WaypointResource::UsernameProofsByFid { fid } => {
                    let result = self
                        .query
                        .do_get_username_proofs_by_fid(Fid::from(fid))
                        .await
                        .map_err(Self::map_query_error)?;
                    Self::resource_json_contents(&uri, &result)?
                },
            };

            return Ok(response);
        }

        match uri.as_str() {
            "str:///waypoint/docs" => {
                let docs = "Waypoint is a Farcaster data indexer that streams data from the Farcaster network and allows querying by FID.";
                Ok(ReadResourceResult { contents: vec![ResourceContents::text(docs, uri)] })
            },
            "memo://farcaster-info" => {
                let info = "Farcaster is a decentralized social network built on Ethereum. Users have an FID (Farcaster ID) that identifies them on the network.";
                Ok(ReadResourceResult { contents: vec![ResourceContents::text(info, uri)] })
            },
            _ => Err(McpError::resource_not_found(
                format!("Resource not found: {}", uri),
                Some(serde_json::json!({
                    "uri": uri
                })),
            )),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        // URI templates follow RFC 6570 (URI Template)
        // - Level 1: Simple string expansion {var}
        // - Level 3: Query string expansion {?var} for complex values like URLs
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                // Users
                Self::create_resource_template_json(
                    "waypoint://users/{fid}",
                    "user-by-fid",
                    "Farcaster user profile by FID",
                ),
                Self::create_resource_template_json(
                    "waypoint://users/by-username/{username}",
                    "user-by-username",
                    "Farcaster user profile by username",
                ),
                // Verifications
                Self::create_resource_template_json(
                    "waypoint://verifications/{fid}",
                    "verifications-by-fid",
                    "Verified addresses for a FID",
                ),
                Self::create_resource_template_json(
                    "waypoint://verifications/{fid}/{address}",
                    "verification-by-fid-address",
                    "Specific verification by FID and address",
                ),
                Self::create_resource_template_json(
                    "waypoint://verifications/all-by-fid/{fid}",
                    "all-verification-messages-by-fid",
                    "All verification messages for a FID",
                ),
                // Casts
                Self::create_resource_template_json(
                    "waypoint://casts/{fid}/{hash}",
                    "cast-by-fid-hash",
                    "Specific cast by author FID and cast hash",
                ),
                Self::create_resource_template_json(
                    "waypoint://casts/by-fid/{fid}",
                    "casts-by-fid",
                    "Recent casts by FID",
                ),
                Self::create_resource_template_json(
                    "waypoint://casts/by-mention/{fid}",
                    "casts-by-mention",
                    "Casts mentioning a FID",
                ),
                Self::create_resource_template_json(
                    "waypoint://casts/by-parent/{fid}/{hash}",
                    "casts-by-parent",
                    "Replies to a parent cast",
                ),
                Self::create_resource_template_json(
                    "waypoint://casts/by-parent-url{?url}",
                    "casts-by-parent-url",
                    "Replies to a parent URL (RFC 6570 query expansion)",
                ),
                // Conversations
                Self::create_resource_template_json(
                    "waypoint://conversations/{fid}/{hash}",
                    "conversation",
                    "Conversation thread for a cast (includes replies, participants, context)",
                ),
                // Reactions
                Self::create_resource_template_json(
                    "waypoint://reactions/by-fid/{fid}",
                    "reactions-by-fid",
                    "Reactions by FID",
                ),
                Self::create_resource_template_json(
                    "waypoint://reactions/by-target-cast/{fid}/{hash}",
                    "reactions-by-target-cast",
                    "Reactions for a target cast",
                ),
                Self::create_resource_template_json(
                    "waypoint://reactions/by-target-url{?url}",
                    "reactions-by-target-url",
                    "Reactions for a target URL (RFC 6570 query expansion)",
                ),
                // Links
                Self::create_resource_template_json(
                    "waypoint://links/by-fid/{fid}",
                    "links-by-fid",
                    "Links by FID (default follow)",
                ),
                Self::create_resource_template_json(
                    "waypoint://links/by-target/{fid}",
                    "links-by-target",
                    "Links to a target FID (default follow)",
                ),
                Self::create_resource_template_json(
                    "waypoint://links/compact-state/{fid}",
                    "link-compact-state",
                    "Link compact state for a FID",
                ),
                // Username Proofs
                Self::create_resource_template_json(
                    "waypoint://username-proofs/by-name/{name}",
                    "username-proof-by-name",
                    "Username proof for a specific name",
                ),
                Self::create_resource_template_json(
                    "waypoint://username-proofs/{fid}",
                    "username-proofs-by-fid",
                    "Username proofs (fname and ENS registrations) for a FID",
                ),
            ],
            next_cursor: None,
            meta: None,
        })
    }
}
