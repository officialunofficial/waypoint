use clap::{ArgAction, ArgMatches, Command, value_parser};
use color_eyre::eyre::Result;
use rmcp::transport::sse_server::{SseServer, SseServerConfig};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::info;
use waypoint::config::Config;
use waypoint::core::data_context::{DataContext, DataContextBuilder};
use waypoint::services::mcp::{NullDb, WaypointMcpService, WaypointMcpTools};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8000";

/// Register MCP commands
pub fn register_commands(app: Command) -> Command {
    app.about("MCP service commands").subcommand(
        Command::new("serve").about("Serve MCP service").arg(
            clap::Arg::new("bind")
                .long("bind")
                .short('b')
                .help("Bind address for the server")
                .default_value(DEFAULT_BIND_ADDRESS)
                .action(ArgAction::Set)
                .value_parser(value_parser!(std::net::SocketAddr)),
        ),
    )
}

/// Handle MCP commands
pub async fn handle_command(matches: &ArgMatches, _config: &Config) -> Result<()> {
    match matches.subcommand() {
        Some(("serve", serve_matches)) => {
            serve_mcp(serve_matches).await?;
        },
        _ => {
            let cmd = Command::new("mcp");
            register_commands(cmd.clone()).print_help()?;
        },
    }
    Ok(())
}

/// Serve MCP service with Farcaster data tools
async fn serve_mcp(matches: &ArgMatches) -> Result<()> {
    let bind_address = matches
        .get_one::<std::net::SocketAddr>("bind")
        .copied()
        .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.parse().unwrap());

    info!("Starting MCP service on {}", bind_address);

    // Create the required clients for the DataContext
    let hub_config = Arc::new(Config::default().hub);

    // Create Hub client (connection will happen automatically on first use)
    let hub = waypoint::hub::client::Hub::new(hub_config)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create Hub client: {}", e))?;

    // Create Hub client for data context
    let hub_client = waypoint::hub::providers::FarcasterHubClient::new(Arc::new(Mutex::new(hub)));

    // Create the data context with the Hub client and NullDb
    let data_context: DataContext<NullDb, _> =
        DataContextBuilder::new().with_database(NullDb).with_hub_client(hub_client).build();

    // Create a cancellation token for the service
    let cancellation_token = CancellationToken::new();

    // Configure the server
    let server_config = SseServerConfig {
        bind: bind_address,
        sse_path: "/sse".to_string(),
        post_path: "/message".to_string(),
        ct: cancellation_token.clone(),
        sse_keep_alive: None,
    };

    // Create the SSE server
    let (sse_server, router) = SseServer::new(server_config);

    // Initialize the WaypointMcpService with data context
    let waypoint_service = WaypointMcpService::new(data_context);

    // Create the tools wrapper
    let tools = WaypointMcpTools::new(waypoint_service);

    // Initialize the tools with the SSE server
    let ct = sse_server.with_service(move || tools.clone());

    // Create TCP listener and start server
    let listener = tokio::net::TcpListener::bind(bind_address).await?;

    info!("MCP service started on {}. Press Ctrl+C to stop", bind_address);

    // Spawn server task
    let ct_shutdown = ct.child_token();
    let server_handle = tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            ct_shutdown.cancelled().await;
            info!("MCP service shutting down");
        });

        if let Err(e) = server.await {
            tracing::error!("MCP server error: {}", e);
        }
    });

    // Wait for Ctrl+C and then cancel the server
    tokio::signal::ctrl_c().await?;
    ct.cancel();

    // Wait for server to finish
    let _ = server_handle.await;

    info!("MCP service stopped");

    Ok(())
}
