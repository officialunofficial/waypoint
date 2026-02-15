//! MCP API handlers for Waypoint

mod casts;
mod common;
mod links;
mod reactions;
mod user_data;
mod utils;

use std::sync::Arc;

use crate::core::types::Fid;
use crate::services::mcp::base::{NullDb, WaypointMcpCore};

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

/// MCP protocol version used by this server
pub const MCP_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_03_26;

// Non-generic wrapper for WaypointMcpCore to use with RMCP macros
#[derive(Clone)]
pub struct WaypointMcpTools {
    service: Arc<WaypointMcpCore<NullDb, crate::hub::providers::FarcasterHubClient>>,
    tool_router: ToolRouter<WaypointMcpTools>,
    prompt_router: PromptRouter<WaypointMcpTools>,
}

impl WaypointMcpTools {
    pub fn new(
        service: WaypointMcpCore<NullDb, crate::hub::providers::FarcasterHubClient>,
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

    /// Create a ReadResourceResult containing JSON content
    fn resource_json_contents(uri: &str, json: String) -> ReadResourceResult {
        ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: uri.to_string(),
                mime_type: Some("application/json".to_string()),
                text: json,
                meta: None,
            }],
        }
    }

    // User data APIs
    #[tool(description = "Get Farcaster user data by FID", annotations(read_only_hint = true))]
    async fn get_user_by_fid(
        &self,
        Parameters(common::UserByFidRequest { fid }): Parameters<common::UserByFidRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_user_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = Fid::from(fid);
        let result = self.service.do_get_verifications_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = Fid::from(fid);
        let result = self.service.do_get_verification(fid, &address).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = Fid::from(fid);
        let result = self
            .service
            .do_get_all_verification_messages_by_fid(fid, limit, start_time, end_time)
            .await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let result = self.service.do_get_fid_by_username(&username).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let result = self.service.do_get_user_by_username(&username).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Cast APIs
    #[tool(description = "Get a specific cast by FID and hash", annotations(read_only_hint = true))]
    async fn get_cast(
        &self,
        Parameters(common::GetCastRequest { fid, hash }): Parameters<common::GetCastRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_cast(fid, &hash).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get casts by a specific Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_casts_by_fid(
        &self,
        Parameters(common::GetCastsRequest { fid, limit }): Parameters<common::GetCastsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_mention(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let parent_fid = Fid::from(parent_fid);
        let result = self.service.do_get_casts_by_parent(parent_fid, &parent_hash, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get cast replies to a specific URL", annotations(read_only_hint = true))]
    async fn get_casts_by_parent_url(
        &self,
        Parameters(common::GetCastRepliesByUrlRequest { parent_url, limit }): Parameters<
            common::GetCastRepliesByUrlRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_casts_by_parent_url(&parent_url, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get all casts from a user with optional timestamp filtering",
        annotations(read_only_hint = true)
    )]
    async fn get_all_casts_by_fid(
        &self,
        Parameters(common::GetAllCastsWithTimeRequest { fid, limit, start_time, end_time }): Parameters<common::GetAllCastsWithTimeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_casts_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = match fid.parse::<u64>() {
            Ok(fid) => Fid::from(fid),
            Err(_) => return Err(McpError::invalid_params("Invalid FID format", None)),
        };

        let result =
            self.service.do_get_conversation(fid, &cast_hash, recursive, max_depth, limit).await;

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Reaction APIs
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
        let fid = Fid::from(fid);
        let target_cast_fid = target_cast_fid.map(Fid::from);

        // Convert hex hash to bytes if provided
        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            Some(utils::parse_hash_bytes(hash).map_err(|message| {
                McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
            })?)
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
        let fid = Fid::from(fid);
        let result = self.service.do_get_reactions_by_fid(fid, reaction_type, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let target_cast_fid = target_cast_fid.map(Fid::from);

        // Convert hex hash to bytes if provided
        let target_cast_hash_bytes = if let Some(hash) = &target_cast_hash {
            Some(utils::parse_hash_bytes(hash).map_err(|message| {
                McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
            })?)
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
        let fid = Fid::from(fid);
        let result =
            self.service.do_get_all_reactions_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Link APIs
    #[tool(description = "Get a specific link", annotations(read_only_hint = true))]
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

    #[tool(
        description = "Get link compact state messages for a Farcaster user",
        annotations(read_only_hint = true)
    )]
    async fn get_link_compact_state_by_fid(
        &self,
        Parameters(common::FidRequest { fid }): Parameters<common::FidRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_link_compact_state_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
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
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_links_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Username Proofs API
    #[tool(description = "Get a single username proof by name", annotations(read_only_hint = true))]
    async fn get_username_proof(
        &self,
        Parameters(common::GetUsernameProofRequest { name }): Parameters<
            common::GetUsernameProofRequest,
        >,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_username_proof(&name).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Get username proofs for a Farcaster user (fname and ENS registrations)",
        annotations(read_only_hint = true)
    )]
    async fn get_username_proofs_by_fid(
        &self,
        Parameters(common::FidRequest { fid }): Parameters<common::FidRequest>,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_username_proofs_by_fid(fid).await;
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

            let limit = common::default_limit();

            let result = match resource {
                utils::WaypointResource::UserByFid { fid } => {
                    self.service.do_get_user_by_fid(Fid::from(fid)).await
                },
                utils::WaypointResource::UserByUsername { username } => {
                    self.service.do_get_user_by_username(&username).await
                },
                utils::WaypointResource::VerificationsByFid { fid } => {
                    self.service.do_get_verifications_by_fid(Fid::from(fid), limit).await
                },
                utils::WaypointResource::VerificationByAddress { fid, address } => {
                    self.service.do_get_verification(Fid::from(fid), &address).await
                },
                utils::WaypointResource::AllVerificationMessagesByFid { fid } => {
                    self.service
                        .do_get_all_verification_messages_by_fid(Fid::from(fid), limit, None, None)
                        .await
                },
                utils::WaypointResource::Cast { fid, hash } => {
                    self.service.do_get_cast(Fid::from(fid), &hash).await
                },
                utils::WaypointResource::Conversation { fid, hash } => {
                    // Use defaults: recursive=true, max_depth=5, limit=10
                    self.service.do_get_conversation(Fid::from(fid), &hash, true, 5, limit).await
                },
                utils::WaypointResource::CastsByFid { fid } => {
                    self.service.do_get_casts_by_fid(Fid::from(fid), limit).await
                },
                utils::WaypointResource::CastsByMention { fid } => {
                    self.service.do_get_casts_by_mention(Fid::from(fid), limit).await
                },
                utils::WaypointResource::CastsByParent { fid, hash } => {
                    self.service.do_get_casts_by_parent(Fid::from(fid), &hash, limit).await
                },
                utils::WaypointResource::CastsByParentUrl { url } => {
                    self.service.do_get_casts_by_parent_url(&url, limit).await
                },
                utils::WaypointResource::ReactionsByFid { fid } => {
                    self.service.do_get_reactions_by_fid(Fid::from(fid), None, limit).await
                },
                utils::WaypointResource::ReactionsByTargetCast { fid, hash } => {
                    let target_cast_hash = utils::parse_hash_bytes(&hash).map_err(|message| {
                        McpError::invalid_params(message, Some(serde_json::json!({ "hash": hash })))
                    })?;

                    self.service
                        .do_get_reactions_by_target(
                            Some(Fid::from(fid)),
                            Some(target_cast_hash.as_slice()),
                            None,
                            None,
                            limit,
                        )
                        .await
                },
                utils::WaypointResource::ReactionsByTargetUrl { url } => {
                    self.service
                        .do_get_reactions_by_target(None, None, Some(&url), None, limit)
                        .await
                },
                utils::WaypointResource::LinksByFid { fid } => {
                    let link_type = common::default_link_type();
                    self.service
                        .do_get_links_by_fid(Fid::from(fid), Some(link_type.as_str()), limit)
                        .await
                },
                utils::WaypointResource::LinksByTarget { fid } => {
                    let link_type = common::default_link_type();
                    self.service
                        .do_get_links_by_target(Fid::from(fid), Some(link_type.as_str()), limit)
                        .await
                },
                utils::WaypointResource::LinkCompactStateByFid { fid } => {
                    self.service.do_get_link_compact_state_by_fid(Fid::from(fid)).await
                },
                utils::WaypointResource::UsernameProofByName { name } => {
                    self.service.do_get_username_proof(&name).await
                },
                utils::WaypointResource::UsernameProofsByFid { fid } => {
                    self.service.do_get_username_proofs_by_fid(Fid::from(fid)).await
                },
            };

            return Ok(Self::resource_json_contents(&uri, result));
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
