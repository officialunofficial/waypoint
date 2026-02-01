//! Base MCP service implementation

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::Parameters,
    },
    model::*,
    prompt, prompt_handler, prompt_router, schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

// Import types from core
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::app::{Service, ServiceContext, ServiceHandle};
use crate::core::{
    data_context::DataContext,
    types::{Fid, Message as FarcasterMessage},
};

// NullDB implementation that satisfies the Database trait
#[derive(Debug, Clone)]
pub struct NullDb;

#[async_trait]
impl crate::core::data_context::Database for NullDb {
    async fn get_message(
        &self,
        _id: &crate::core::types::MessageId,
        _message_type: crate::core::types::MessageType,
    ) -> crate::core::data_context::Result<FarcasterMessage> {
        Err(crate::core::data_context::DataAccessError::NotFound(
            "NullDb does not store messages".to_string(),
        ))
    }

    async fn get_messages_by_fid(
        &self,
        _fid: Fid,
        _message_type: crate::core::types::MessageType,
        _limit: usize,
        _cursor: Option<crate::core::types::MessageId>,
    ) -> crate::core::data_context::Result<Vec<FarcasterMessage>> {
        Ok(Vec::new())
    }

    async fn store_message(
        &self,
        _message: FarcasterMessage,
    ) -> crate::core::data_context::Result<()> {
        Ok(())
    }

    async fn delete_message(
        &self,
        _id: &crate::core::types::MessageId,
        _message_type: crate::core::types::MessageType,
    ) -> crate::core::data_context::Result<()> {
        Ok(())
    }
}

// Simple MooCow service to demonstrate MCP functionality
#[derive(Clone)]
pub struct MooCow {
    moo_count: Arc<Mutex<i32>>,
    tool_router: ToolRouter<MooCow>,
    prompt_router: PromptRouter<MooCow>,
}

impl Default for MooCow {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct MooRequest {
    pub times: i32,
}

#[tool_router]
impl MooCow {
    pub fn new() -> Self {
        Self {
            moo_count: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    fn create_resource_text(uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    #[tool(description = "Moo like a cow")]
    async fn moo(&self) -> Result<CallToolResult, McpError> {
        let mut count = self.moo_count.lock().await;
        *count += 1;
        Ok(CallToolResult::success(vec![Content::text("Mooooo!")]))
    }

    #[tool(description = "Moo like a cow multiple times")]
    async fn moo_times(
        &self,
        Parameters(MooRequest { times }): Parameters<MooRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mut count = self.moo_count.lock().await;
        *count += times;

        let moo_text = "Mooooo! ".repeat(times as usize).trim().to_string();
        Ok(CallToolResult::success(vec![Content::text(moo_text)]))
    }

    #[tool(description = "Get the current moo count")]
    async fn get_moo_count(&self) -> Result<CallToolResult, McpError> {
        let count = self.moo_count.lock().await;
        let count_text = format!("This cow has moo'd {} times", count);
        Ok(CallToolResult::success(vec![Content::text(count_text)]))
    }

    #[tool(description = "Aggregate moo")]
    fn aggregate_moo(
        &self,
        Parameters(MooRequest { times }): Parameters<MooRequest>,
    ) -> Result<CallToolResult, McpError> {
        let moo_text = "Moo ".repeat(times as usize).trim().to_string();
        Ok(CallToolResult::success(vec![Content::text(moo_text)]))
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct MooPromptArgs {
    /// The name of the cow
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[prompt_router]
impl MooCow {
    /// A prompt about cows that takes an optional name parameter
    #[prompt(name = "moo_prompt")]
    async fn moo_prompt(
        &self,
        Parameters(MooPromptArgs { name }): Parameters<MooPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let cow_name = name.unwrap_or_else(|| "Bessie".to_string());

        let prompt = format!(
            "You are a helpful cow named {}. Answer questions with facts about cows and occasionally make 'moo' sounds.",
            cow_name
        );

        Ok(GetPromptResult {
            description: Some("A prompt about cows".to_string()),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::text(prompt),
            }],
        })
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for MooCow {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: super::handlers::MCP_PROTOCOL_VERSION,
            capabilities: ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("This is a MooCow server. Use the 'moo' tool to make the cow moo, and 'get_moo_count' to see how many times the cow has moo'd.".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                Self::create_resource_text("str:///cow/info", "cow-info"),
                Self::create_resource_text("memo://moo-facts", "moo-facts"),
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
        match uri.as_str() {
            "str:///cow/info" => {
                let info = "Cows are domesticated mammals that are raised for their meat, milk, and hides.";
                Ok(ReadResourceResult { contents: vec![ResourceContents::text(info, uri)] })
            },
            "memo://moo-facts" => {
                let facts = "Interesting Moo Facts:\n\n1. Cows can sleep standing up\n2. Cows have 32 teeth\n3. Cows can detect odors up to 5 miles away";
                Ok(ReadResourceResult { contents: vec![ResourceContents::text(facts, uri)] })
            },
            _ => Err(McpError::resource_not_found(
                "resource_not_found",
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
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
            meta: None,
        })
    }
}

// Waypoint MCP service with Snapchain data
#[derive(Clone)]
pub struct WaypointMcpService<DB, HC> {
    pub(crate) data_context: DataContext<DB, HC>,
}

impl<DB, HC> WaypointMcpService<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    pub fn new(data_context: DataContext<DB, HC>) -> Self {
        Self { data_context }
    }

    // Conversation API - new method
    pub async fn do_get_conversation(
        &self,
        fid: Fid,
        cast_hash: &str,
        recursive: bool,
        max_depth: usize,
        limit: usize,
    ) -> String {
        self.do_get_conversation_impl(fid, cast_hash, recursive, max_depth, limit).await
    }
}

/// MCP Service that integrates with the App's service lifecycle
pub struct McpService {
    bind_address: String,
    port: u16,
}

impl Default for McpService {
    fn default() -> Self {
        Self { bind_address: "127.0.0.1".to_string(), port: 8000 }
    }
}

impl McpService {
    /// Create a new MCP service with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure the service with custom settings
    pub fn configure(mut self, bind_address: String, port: u16) -> Self {
        self.bind_address = bind_address;
        self.port = port;
        self
    }
}

#[async_trait]
impl Service for McpService {
    fn name(&self) -> &str {
        "mcp"
    }

    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> crate::app::Result<ServiceHandle> {
        // Create socket address from configuration
        let socket_addr =
            format!("{}:{}", self.bind_address, self.port).parse::<SocketAddr>().map_err(|e| {
                crate::app::ServiceError::Initialization(format!("Invalid socket address: {}", e))
            })?;

        info!("Starting MCP service on {}", socket_addr);

        // Create the required clients for the DataContext
        let hub_config = context.config.hub.clone();

        // Create Hub client
        let mut hub = crate::hub::client::Hub::new(Arc::new(hub_config)).map_err(|e| {
            crate::app::ServiceError::Initialization(format!("Failed to create Hub client: {}", e))
        })?;

        // Connect to the Hub
        if let Err(err) = hub.connect().await {
            warn!("Failed to connect to Hub: {}. Will retry automatically when needed.", err);
        }

        // Create Hub client for data context
        let hub_client = crate::hub::providers::FarcasterHubClient::new(Arc::new(Mutex::new(hub)));

        // Create a data context with a NullDb and the hub client
        let data_context: DataContext<NullDb, _> =
            crate::core::data_context::DataContextBuilder::new()
                .with_database(NullDb)
                .with_hub_client(hub_client)
                .build();

        // Create a cancellation token for the service
        let cancellation_token = CancellationToken::new();
        let ct_for_shutdown = cancellation_token.clone();

        // Configure the Streamable HTTP server
        let server_config = StreamableHttpServerConfig {
            sse_keep_alive: Some(std::time::Duration::from_secs(15)),
            sse_retry: None,
            stateful_mode: true,
            cancellation_token: ct_for_shutdown.clone(),
        };

        // Launch the service
        let server_handle = tokio::spawn(async move {
            // Create the waypoint service
            let waypoint_service = WaypointMcpService::new(data_context);

            // Create the Streamable HTTP service with session management
            let service = StreamableHttpService::new(
                move || {
                    let tools = crate::services::mcp::handlers::WaypointMcpTools::new(
                        waypoint_service.clone(),
                    );
                    Ok(tools)
                },
                Arc::new(LocalSessionManager::default()),
                server_config,
            );

            // Create the router with the MCP endpoint
            let router = axum::Router::new().nest_service("/mcp", service);

            // Create TCP listener
            match tokio::net::TcpListener::bind(socket_addr).await {
                Ok(listener) => {
                    info!("MCP service started on {} and ready to accept connections", socket_addr);

                    let ct_shutdown = ct_for_shutdown.child_token();
                    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                        ct_shutdown.cancelled().await;
                        info!("MCP service shutting down");
                    });

                    if let Err(e) = server.await {
                        error!("MCP server shutdown with error: {}", e);
                    }
                },
                Err(e) => {
                    error!("Failed to bind MCP service to {}: {}", socket_addr, e);
                },
            }
        });

        // Create stop channel to allow service to be cleanly shutdown
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

        // Handle the stop signal
        tokio::spawn(async move {
            let _ = stop_rx.await;
            // Cancel the server when stop is received
            cancellation_token.cancel();
        });

        // Return the service handle
        Ok(ServiceHandle::new(stop_tx, server_handle))
    }
}
