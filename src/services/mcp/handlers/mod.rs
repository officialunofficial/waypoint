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
    Error as McpError, ServerHandler, const_string,
    model::CallToolResult,
    model::*,
    service::{RequestContext, RoleServer},
    tool,
};

// Helper trait for debugging JSON value types
trait JsonValueTypeInfo {
    fn type_name(&self) -> &'static str;
}

impl JsonValueTypeInfo for serde_json::Value {
    fn type_name(&self) -> &'static str {
        match self {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
    }
}

// Non-generic wrapper for WaypointMcpService to use with RMCP macros
#[derive(Clone)]
pub struct WaypointMcpTools {
    service: Arc<WaypointMcpService<NullDb, crate::hub::providers::FarcasterHubClient>>,
}

impl WaypointMcpTools {
    pub fn new(
        service: WaypointMcpService<NullDb, crate::hub::providers::FarcasterHubClient>,
    ) -> Self {
        Self { service: Arc::new(service) }
    }
}

// Tool implementations for standard APIs
#[tool(tool_box)]
impl WaypointMcpTools {
    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    // User data APIs
    #[tool(description = "Get Farcaster user data by FID")]
    async fn get_user_by_fid(
        &self,
        #[tool(aggr)] common::UserByFidRequest { fid }: common::UserByFidRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_user_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get verified addresses for a Farcaster user")]
    async fn get_verifications_by_fid(
        &self,
        #[tool(aggr)]
        common::GetVerificationsRequest { fid, limit }: common::GetVerificationsRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_verifications_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Cast APIs
    #[tool(description = "Get a specific cast by FID and hash")]
    async fn get_cast(
        &self,
        #[tool(aggr)] common::GetCastRequest { fid, hash }: common::GetCastRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_cast(fid, &hash).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get casts by a specific Farcaster user")]
    async fn get_casts_by_fid(
        &self,
        #[tool(aggr)] common::GetCastsRequest { fid, limit }: common::GetCastsRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_fid(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get casts that mention a specific Farcaster user")]
    async fn get_casts_by_mention(
        &self,
        #[tool(aggr)] common::GetCastMentionsRequest { fid, limit }: common::GetCastMentionsRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_casts_by_mention(fid, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get cast replies to a specific parent cast")]
    async fn get_casts_by_parent(
        &self,
        #[tool(aggr)]
        common::GetCastRepliesRequest { parent_fid, parent_hash, limit }: common::GetCastRepliesRequest,
    ) -> Result<CallToolResult, McpError> {
        let parent_fid = Fid::from(parent_fid);
        let result = self.service.do_get_casts_by_parent(parent_fid, &parent_hash, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get cast replies to a specific URL")]
    async fn get_casts_by_parent_url(
        &self,
        #[tool(aggr)] common::GetCastRepliesByUrlRequest { parent_url, limit }: common::GetCastRepliesByUrlRequest,
    ) -> Result<CallToolResult, McpError> {
        let result = self.service.do_get_casts_by_parent_url(&parent_url, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all casts from a user with optional timestamp filtering")]
    async fn get_all_casts_by_fid(
        &self,
        #[tool(aggr)] common::GetAllCastsWithTimeRequest { fid, limit, start_time, end_time }: common::GetAllCastsWithTimeRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_casts_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // Reaction APIs
    #[tool(description = "Get a specific reaction")]
    async fn get_reaction(
        &self,
        #[tool(aggr)] common::ReactionRequest {
            fid,
            reaction_type,
            target_cast_fid,
            target_cast_hash,
            target_url,
        }: common::ReactionRequest,
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
        #[tool(aggr)] common::ReactionsByFidRequest { fid, reaction_type, limit }: common::ReactionsByFidRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_reactions_by_fid(fid, reaction_type, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get reactions for a target (cast or URL)")]
    async fn get_reactions_by_target(
        &self,
        #[tool(aggr)] common::ReactionsByTargetRequest {
            target_cast_fid,
            target_cast_hash,
            target_url,
            reaction_type,
            limit,
        }: common::ReactionsByTargetRequest,
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
        #[tool(aggr)] common::FidTimestampRequest { fid, limit, start_time, end_time }: common::FidTimestampRequest,
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
        #[tool(aggr)] common::LinkRequest { fid, link_type, target_fid }: common::LinkRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let target_fid = Fid::from(target_fid);
        let result = self.service.do_get_link(fid, &link_type, target_fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get links by a specific Farcaster user")]
    async fn get_links_by_fid(
        &self,
        #[tool(aggr)]
        common::LinksByFidRequest { fid, link_type, limit }: common::LinksByFidRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let link_type_ref = link_type.as_deref();
        let result = self.service.do_get_links_by_fid(fid, link_type_ref, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get links to a target Farcaster user")]
    async fn get_links_by_target(
        &self,
        #[tool(aggr)] common::LinksByTargetRequest { target_fid, link_type, limit }: common::LinksByTargetRequest,
    ) -> Result<CallToolResult, McpError> {
        let target_fid = Fid::from(target_fid);
        let link_type_ref = link_type.as_deref();
        let result = self.service.do_get_links_by_target(target_fid, link_type_ref, limit).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get link compact state messages for a Farcaster user")]
    async fn get_link_compact_state_by_fid(
        &self,
        #[tool(aggr)] common::FidRequest { fid }: common::FidRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_link_compact_state_by_fid(fid).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get all links from a user with optional timestamp filtering")]
    async fn get_all_links_by_fid(
        &self,
        #[tool(aggr)] common::FidTimestampRequest { fid, limit, start_time, end_time }: common::FidTimestampRequest,
    ) -> Result<CallToolResult, McpError> {
        let fid = Fid::from(fid);
        let result = self.service.do_get_all_links_by_fid(fid, limit, start_time, end_time).await;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

const_string!(WaypointPrompt = "waypoint_prompt");

#[tool(tool_box)]
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
        _request: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                self._create_resource_text("str:///waypoint/docs", "waypoint-docs"),
                self._create_resource_text("memo://farcaster-info", "farcaster-info"),
            ],
            next_cursor: None,
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

    async fn list_prompts(
        &self,
        _request: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            prompts: vec![Prompt::new(
                WaypointPrompt::VALUE,
                Some("A prompt that helps with Farcaster data querying and takes an FID parameter"),
                Some(vec![PromptArgument {
                    name: "fid".to_string(),
                    description: Some("The Farcaster ID to focus on".to_string()),
                    required: Some(true),
                }]),
            )],
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        GetPromptRequestParam { name, arguments }: GetPromptRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        match name.as_str() {
            WaypointPrompt::VALUE => {
                // Ensure arguments is not None
                let arguments = arguments.ok_or_else(|| {
                    McpError::invalid_params("No arguments provided to waypoint_prompt", None)
                })?;

                // Extract FID from arguments
                let fid_value = arguments.get("fid").ok_or_else(|| {
                    McpError::invalid_params("No FID provided to waypoint_prompt", None)
                })?;

                // Log the type of the FID value for debugging at trace level
                tracing::trace!(
                    "FID value type: {:?}, value: {:?}",
                    fid_value.type_name(),
                    fid_value
                );

                // Try to parse FID as different types
                let fid = if let Some(num) = fid_value.as_u64() {
                    num.to_string()
                } else if let Some(num_str) = fid_value.as_str() {
                    // Try to parse string as number
                    match num_str.parse::<u64>() {
                        Ok(n) => n.to_string(),
                        Err(_) => {
                            return Err(McpError::invalid_params(
                                format!("FID must be a number, got string: '{}'", num_str),
                                Some(serde_json::json!({"fid_value": fid_value})),
                            ));
                        },
                    }
                } else {
                    // If it's neither u64 nor string, return detailed error
                    return Err(McpError::invalid_params(
                        format!("FID must be a number, got type: {}", fid_value.type_name()),
                        Some(serde_json::json!({"fid_value": fid_value})),
                    ));
                };

                let prompt = format!(
                    "You are a helpful assistant for exploring Farcaster data. You're currently focusing on FID {}. You can help fetch user data, verifications, casts, reactions, and links for this user using the appropriate tools.",
                    fid
                );

                Ok(GetPromptResult {
                    description: Some("A prompt for Farcaster data exploration".to_string()),
                    messages: vec![PromptMessage {
                        role: PromptMessageRole::Assistant,
                        content: PromptMessageContent::text(prompt),
                    }],
                })
            },
            _ => Err(McpError::invalid_params(
                format!("Prompt not found: {}", name),
                Some(serde_json::json!({
                    "name": name
                })),
            )),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult { resource_templates: vec![], next_cursor: None })
    }
}
