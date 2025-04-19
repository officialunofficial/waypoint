//! Base MCP service implementation

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{
    Error as McpError, ServerHandler, const_string,
    model::*,
    schemars,
    service::RequestContext,
    service::RoleServer,
    tool,
    transport::sse_server::{SseServer, SseServerConfig},
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

#[tool(tool_box)]
impl MooCow {
    pub fn new() -> Self {
        Self { moo_count: Arc::new(Mutex::new(0)) }
    }

    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
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
        #[tool(param)]
        #[schemars(description = "Number of times to moo")]
        times: i32,
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
        #[tool(aggr)] MooRequest { times }: MooRequest,
    ) -> Result<CallToolResult, McpError> {
        let moo_text = "Moo ".repeat(times as usize).trim().to_string();
        Ok(CallToolResult::success(vec![Content::text(moo_text)]))
    }
}

const_string!(MooPrompt = "moo_prompt");

#[tool(tool_box)]
impl ServerHandler for MooCow {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
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
        _request: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                self._create_resource_text("str:///cow/info", "cow-info"),
                self._create_resource_text("memo://moo-facts", "moo-facts"),
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

    async fn list_prompts(
        &self,
        _request: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            prompts: vec![Prompt::new(
                MooPrompt::VALUE,
                Some("A prompt about cows that takes an optional name parameter"),
                Some(vec![PromptArgument {
                    name: "name".to_string(),
                    description: Some("The name of the cow".to_string()),
                    required: Some(false),
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
            MooPrompt::VALUE => {
                let cow_name = arguments
                    .and_then(|json| json.get("name")?.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "Bessie".to_string());

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
            },
            _ => Err(McpError::invalid_params(
                "prompt not found",
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

        // Configure the server with a single endpoint
        let server_config = SseServerConfig {
            bind: socket_addr,
            sse_path: "/sse".to_string(),
            post_path: "/message".to_string(),
            ct: cancellation_token.clone(),
        };

        let ct_clone = cancellation_token.clone();

        // Launch the service
        let server_handle = tokio::spawn(async move {
            match SseServer::serve_with_config(server_config).await {
                Ok(sse_server) => {
                    // Create the waypoint service
                    let waypoint_service = WaypointMcpService::new(data_context);

                    // Create the tools wrapper
                    let tools = crate::services::mcp::handlers::WaypointMcpTools::new(waypoint_service);

                    // Initialize the tools with the SSE server
                    sse_server.with_service(move || tools.clone());

                    info!("MCP service started on {} and ready to accept connections", socket_addr);

                    // Wait for the service to be cancelled
                    ct_clone.cancelled().await;
                    info!("MCP service shutting down");
                },
                Err(e) => {
                    error!("Failed to start MCP service: {}", e);
                },
            }
        });

        // Create stop channel to allow service to be cleanly shutdown
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

        // Handle the stop signal
        let ct = cancellation_token.clone();
        tokio::spawn(async move {
            let _ = stop_rx.await;
            // Cancel the server when stop is received
            ct.cancel();
        });

        // Return the service handle
        Ok(ServiceHandle::new(stop_tx, server_handle))
    }
}