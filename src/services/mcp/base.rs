//! Base MCP service implementation

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
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

// Waypoint MCP core with Snapchain data
#[derive(Clone)]
pub struct WaypointMcpCore<DB, HC> {
    pub(crate) data_context: DataContext<DB, HC>,
}

impl<DB, HC> WaypointMcpCore<DB, HC>
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
            // Create MCP core for tool handlers
            let mcp_core = WaypointMcpCore::new(data_context);

            // Create the Streamable HTTP service with session management
            let service = StreamableHttpService::new(
                move || {
                    let tools =
                        crate::services::mcp::handlers::WaypointMcpTools::new(mcp_core.clone());
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
