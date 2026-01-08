use color_eyre::eyre::{self, Result};
use std::time::Duration;
use tracing::info;
use waypoint::{
    app::App,
    config::{Config, ServiceMode},
    services::{
        consumer::ConsumerService, mcp::McpService, producer::ProducerService,
        streaming::StreamingService,
    },
};

/// Run the main streaming service
///
/// The `mode` parameter determines which components to run:
/// - `Producer`: Only subscribe to Hub and write to Redis
/// - `Consumer`: Only read from Redis and write to PostgreSQL
/// - `Both`: Run both producer and consumer (default, backward compatible)
pub async fn run_service(config: &Config, mode: ServiceMode) -> Result<()> {
    // Validate configuration for the specific mode
    config
        .validate_for_mode(mode)
        .map_err(|e| eyre::eyre!("Invalid configuration for {} mode: {}", mode, e))?;

    info!("Starting Waypoint in {} mode", mode);

    // Create the application with mode-specific initialization
    let mut app = App::new_with_mode(config.clone(), mode)
        .await
        .map_err(|e| eyre::eyre!("Failed to create application: {}", e))?;

    // Configure health server
    let port = std::env::var_os("PORT")
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid PORT");

    app.with_health_server(port);

    // Register services based on mode
    match mode {
        ServiceMode::Producer => {
            info!("Registering producer service (Hub → Redis)");
            let producer_service = ProducerService::new().with_spam_filter(true);
            app.register_service(producer_service);
        },
        ServiceMode::Consumer => {
            info!("Registering consumer service (Redis → PostgreSQL)");
            let consumer_service = ConsumerService::new()
                .with_batch_size(10)
                .with_concurrency(200)
                .with_timeout(Duration::from_secs(120))
                .with_retention(Duration::from_secs(24 * 60 * 60))
                .with_print_processor(false);
            app.register_service(consumer_service);
        },
        ServiceMode::Both => {
            info!("Registering streaming service (Hub → Redis → PostgreSQL)");
            // Configure streaming service with appropriate processors using the builder pattern
            let streaming_service = StreamingService::new()
                .configure(|options| {
                    options
                        .with_batch_size(10)
                        .with_concurrency(200)
                        .with_timeout(Duration::from_secs(120))
                        .with_retention(Duration::from_secs(24 * 60 * 60))
                })
                .with_spam_filter(true)
                .with_print_processor(false);
            app.register_service(streaming_service);
        },
    }

    // Register MCP service if enabled (only for consumer or both modes, needs database)
    if config.mcp.enabled && matches!(mode, ServiceMode::Consumer | ServiceMode::Both) {
        let mcp_service =
            McpService::new().configure(config.mcp.bind_address.clone(), config.mcp.port);
        app.register_service(mcp_service);
        info!(
            "MCP service registered with bind address {}:{}",
            config.mcp.bind_address, config.mcp.port
        );
    } else if config.mcp.enabled && mode == ServiceMode::Producer {
        info!("MCP service disabled in producer mode (requires database)");
    } else {
        info!("MCP service disabled in configuration");
    }

    // Run the application
    info!("Starting Waypoint service in {} mode", mode);
    app.run_until_shutdown()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Application error: {}", e))?;

    info!("Shutdown complete");
    Ok(())
}
