use color_eyre::eyre::Result;
use std::time::Duration;
use tracing::info;
use waypoint::{
    app::App,
    config::Config,
    services::{
        mcp::McpService,
        streaming::{ProcessorType, StreamingService},
    },
};

/// Run the main streaming service
pub async fn run_service(config: &Config) -> Result<()> {
    // Validate configuration
    config.validate().map_err(|e| color_eyre::eyre::eyre!("Invalid configuration: {}", e))?;

    // Create the application
    let mut app = App::new(config.clone())
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create application: {}", e))?;

    // Configure health server
    let port = std::env::var_os("PORT")
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid PORT");

    app.with_health_server(port);

    // Configure streaming service with appropriate processors using the builder pattern
    let streaming_service = StreamingService::new().configure(|options| {
        options
            .with_batch_size(10)
            .with_concurrency(200)
            .with_timeout(Duration::from_secs(120))
            .with_retention(Duration::from_secs(24 * 60 * 60))
            .with_processors(vec![
                ProcessorType::Database, // Enable database by default
                ProcessorType::Print,    // Enable print processor by default for easier debugging
            ])
    });

    app.register_service(streaming_service);

    // Register MCP service if enabled
    if config.mcp.enabled {
        let mcp_service =
            McpService::new().configure(config.mcp.bind_address.clone(), config.mcp.port);
        app.register_service(mcp_service);
        info!(
            "MCP service registered with bind address {}:{}",
            config.mcp.bind_address, config.mcp.port
        );
    } else {
        info!("MCP service disabled in configuration");
    }

    // Run the application
    info!("Starting Waypoint service");
    app.run_until_shutdown()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Application error: {}", e))?;

    info!("Shutdown complete");
    Ok(())
}
