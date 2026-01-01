//! MCP API handlers for Waypoint

mod casts;
mod common;
mod links;
mod reactions;
mod user_data;
mod utils;

use std::sync::Arc;

use crate::core::types::Fid;
use crate::services::mcp::base::{NullDb, WaypointMcpService};

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

// Non-generic wrapper for WaypointMcpService to use with RMCP macros
#[derive(Clone)]
pub struct WaypointMcpTools {
    service: Arc<WaypointMcpService<NullDb, crate::hub::providers::FarcasterHubClient>>,
    tool_router: ToolRouter<WaypointMcpTools>,
    prompt_router: PromptRouter<WaypointMcpTools>,
}

impl WaypointMcpTools {
    pub fn new(
        service: WaypointMcpService<NullDb, crate::hub::providers::FarcasterHubClient>,
    ) -> Self {
        Self {
            service: Arc::new(service),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

// Tool implementations for standard APIs
#[tool_router]
impl WaypointMcpTools {
    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    // User data APIs
    #[tool(description = "Get Farcaster user data by FID")]
    async fn get_user_by_fid(
        &self,
        Parameters(common::UserByFidRequest { fid }): Parameters<common::UserByFidRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_user_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get verified addresses for a Farcaster user")]
    async fn get_verifications_by_fid(
        &self,
        Parameters(common::GetVerificationsRequest { fid, limit }): Parameters<
            common::GetVerificationsRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_verifications_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Find a Farcaster user's FID by their username")]
    async fn get_fid_by_username(
        &self,
        Parameters(common::GetFidByUsernameRequest { username }): Parameters<
            common::GetFidByUsernameRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_fid_by_username(&username).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get a complete Farcaster user profile by username")]
    async fn get_user_by_username(
        &self,
        Parameters(common::GetFidByUsernameRequest { username }): Parameters<
            common::GetFidByUsernameRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_user_by_username(&username).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Cast APIs
    #[tool(description = "Get a specific cast by FID and hash")]
    async fn get_cast(
        &self,
        Parameters(common::GetCastRequest { fid, hash }): Parameters<common::GetCastRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_cast(fid, &hash).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get casts by a specific Farcaster user")]
    async fn get_casts_by_fid(
        &self,
        Parameters(common::GetCastsRequest { fid, limit }): Parameters<common::GetCastsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get casts that mention a specific Farcaster user")]
    async fn get_casts_by_mention(
        &self,
        Parameters(common::GetCastMentionsRequest { fid, limit }): Parameters<
            common::GetCastMentionsRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_mention(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get cast replies to a specific parent cast")]
    async fn get_casts_by_parent(
        &self,
        Parameters(common::GetCastRepliesRequest { parent_fid, parent_hash, limit }): Parameters<
            common::GetCastRepliesRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let parent_fid = Fid::from(parent_fid);
        let result = self.service.do_get_casts_by_parent(parent_fid, &parent_hash, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get cast replies to a specific URL")]
    async fn get_casts_by_parent_url(
        &self,
        Parameters(common::GetCastRepliesByUrlRequest { parent_url, limit }): Parameters<
            common::GetCastRepliesByUrlRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_casts_by_parent_url(&parent_url, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all casts from a user with optional timestamp filtering")]
    async fn get_all_casts_by_fid(
        &self,
        Parameters(common::GetAllCastsWithTimeRequest { fid, limit, start_time, end_time }): Parameters<common::GetAllCastsWithTimeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_casts_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get conversation details for a cast, including participants and replies")]
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
        let fid = match fid.parse::<u64>() {
            Ok(fid) => Fid::from(fid),
            Err(_) => return Err(McpError::invalid_params("Invalid FID format", None)),
        };

        let result =
            self.service.do_get_conversation(fid, &cast_hash, recursive, max_depth, limit).await;

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Reaction APIs
    #[tool(description = "Get a specific reaction")]
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
        let fid = Fid::from(fid);
        let target_cast_fid = target_cast_fid.map(Fid::from);

        // Convert hex hash to bytes if provided
        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            match hex::decode(hash.trim_start_matches("0x")) {
                Ok(bytes) => Some(bytes),
                Err(_) => {
                    return Err(McpError::invalid_params(
                        "Invalid hash format",
                        Some(serde_json::json!({ "hash": hash })),
                    ));
                },
            }
        } else {
            None
        };

        let target_cast_hash_ref = target_cast_hash_bytes.as_deref();
        let target_url_ref = target_url.as_deref();

        let result = self
            .service
            .do_get_reaction(
                fid,
                reaction_type,
                target_cast_fid,
                target_cast_hash_ref,
                target_url_ref,
            )
            .await;

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get reactions by a specific Farcaster user")]
    async fn get_reactions_by_fid(
        &self,
        Parameters(common::ReactionsByFidRequest { fid, reaction_type, limit }): Parameters<
            common::ReactionsByFidRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_reactions_by_fid(fid, reaction_type, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get reactions for a target (cast or URL)")]
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
        let target_cast_fid = target_cast_fid.map(Fid::from);

        // Convert hex hash to bytes if provided
        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            match hex::decode(hash.trim_start_matches("0x")) {
                Ok(bytes) => Some(bytes),
                Err(_) => {
                    return Err(McpError::invalid_params(
                        "Invalid hash format",
                        Some(serde_json::json!({ "hash": hash })),
                    ));
                },
            }
        } else {
            None
        };

        let target_cast_hash_ref = target_cast_hash_bytes.as_deref();
        let target_url_ref = target_url.as_deref();

        let result = self
            .service
            .do_get_reactions_by_target(
                target_cast_fid,
                target_cast_hash_ref,
                target_url_ref,
                reaction_type,
                limit,
            )
            .await;

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all reactions from a user with optional timestamp filtering")]
    async fn get_all_reactions_by_fid(
        &self,
        Parameters(common::FidTimestampRequest { fid, limit, start_time, end_time }): Parameters<
            common::FidTimestampRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result =
            self.service.do_get_all_reactions_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Link APIs
    #[tool(description = "Get a specific link")]
    async fn get_link(
        &self,
        Parameters(common::LinkRequest { fid, link_type, target_fid }): Parameters<
            common::LinkRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let target_fid = Fid::from(target_fid);
        let result = self.service.do_get_link(fid, &link_type, target_fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get links by a specific Farcaster user")]
    async fn get_links_by_fid(
        &self,
        Parameters(common::LinksByFidRequest { fid, link_type, limit }): Parameters<
            common::LinksByFidRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        // Use "follow" as default when None is explicitly provided
        let link_type_ref = match link_type {
            Some(ref lt) if lt.is_empty() => Some("follow"),
            Some(ref lt) => Some(lt.as_str()),
            None => Some("follow"),
        };
        let result = self.service.do_get_links_by_fid(fid, link_type_ref, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get links to a target Farcaster user")]
    async fn get_links_by_target(
        &self,
        Parameters(common::LinksByTargetRequest { target_fid, link_type, limit }): Parameters<
            common::LinksByTargetRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let target_fid = Fid::from(target_fid);
        // Use "follow" as default when None is explicitly provided
        let link_type_ref = match link_type {
            Some(ref lt) if lt.is_empty() => Some("follow"),
            Some(ref lt) => Some(lt.as_str()),
            None => Some("follow"),
        };
        let result = self.service.do_get_links_by_target(target_fid, link_type_ref, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get link compact state messages for a Farcaster user")]
    async fn get_link_compact_state_by_fid(
        &self,
        Parameters(common::FidRequest { fid }): Parameters<common::FidRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_link_compact_state_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all links from a user with optional timestamp filtering")]
    async fn get_all_links_by_fid(
        &self,
        Parameters(common::FidTimestampRequest { fid, limit, start_time, end_time }): Parameters<
            common::FidTimestampRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_links_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        // Check if username is provided, ensuring we preserve any .eth suffix
        let username_context = if let Some(username) = username {
            // Make sure to use the full username including .eth if present
            format!(" (username: {})", username)
        } else {
            "".to_string()
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
            protocol_version: ProtocolVersion::V_2024_11_05,
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
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                self._create_resource_text("str:///waypoint/docs", "waypoint-docs"),
                self._create_resource_text("memo://farcaster-info", "farcaster-info"),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
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
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
            meta: None,
        })
    }
}
