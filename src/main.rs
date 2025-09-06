//! Waypoint - Snapchain synchronization tool
//!
//! Main application entry point with unified CLI command structure
//! for Snapchain synchronization and backfill operations.

mod commands;
mod service;

use clap::Command;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use waypoint::{config::Config, error, metrics};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    // Initialize Rustls crypto provider first
    rustls::crypto::aws_lc_rs::default_provider().install_default()
        .map_err(|_| color_eyre::eyre::eyre!("Failed to install default crypto provider"))?;
    
    // Initialize error handling
    error::install_error_handlers()?;

    // Load configuration from file and/or environment variables
    let config = Config::load()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to load configuration: {}", e))?;

    // Warn users about verbose logs if dependency filter not configured
    if let Some(ref filter) = config.logging.dependency_filter {
        println!("Using dependency log filter: {}", filter);
    } else if std::env::var_os("RUST_LOG").is_some() {
        println!("Note: You may see verbose logs from some dependencies.");
        println!(
            "To quiet them, set WAYPOINT_LOGGING__DEPENDENCY_FILTER in your config or .env file."
        );
    }

    // Initialize logging
    let mut env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.default_level));

    // Initialize metrics
    metrics::setup_metrics(&config);

    // Apply noisy crates filter including fred
    let noisy_crates =
        "h2=warn,tokio_util=warn,mio=warn,hyper=warn,rustls=warn,tonic=info,fred=info";
    let filter_string = format!("{},{}", env_filter, noisy_crates);
    env_filter = EnvFilter::try_new(&filter_string).unwrap_or(env_filter);

    // Apply dependency filters if configured
    if let Some(dep_filter) = &config.logging.dependency_filter {
        let filter_string = format!("{},{}", env_filter, dep_filter);
        env_filter = EnvFilter::try_new(&filter_string).unwrap_or(env_filter);
    }

    // Initialize the subscriber
    let format = fmt::format().with_thread_ids(true).with_target(false);
    tracing_subscriber::registry().with(env_filter).with(fmt::layer().event_format(format)).init();

    // Initialize Prometheus metrics endpoint
    if let Err(e) = metrics::init_prometheus_default().await {
        tracing::warn!("Failed to initialize Prometheus metrics: {}", e);
        // Continue running even if metrics fail to initialize
    }

    // Define base CLI structure
    let base_app = Command::new("Waypoint")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Official Unofficial, Inc.")
        .about("Farcaster data synchronization service");

    // Register all command modules
    let app = commands::register_commands(base_app);

    // Parse command line arguments
    let matches = app.get_matches();

    // Handle commands based on matches
    commands::handle_commands(matches, &config).await?;

    info!("Execution completed successfully");
    Ok(())
}
