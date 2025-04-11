//! Waypoint - Snapchain synchronization tool
use clap::{Arg, ArgAction, Command};
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use waypoint::{
    app::App,
    config::Config,
    error,
    services::streaming::{ProcessorType, StreamingService},
};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
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

    // Apply noisy crates filter
    let noisy_crates = "h2=warn,tokio_util=warn,mio=warn,hyper=warn,rustls=warn,tonic=info";
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

    // Parse command line arguments
    let matches = Command::new("Waypoint")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Official Unofficial, Inc.")
        .about("Farcaster data synchronization service")
        .subcommand(Command::new("start").about("Start the service"))
        .subcommand(
            Command::new("backfill").about("Backfill data from the Hub").arg(
                Arg::new("fids")
                    .long("fids")
                    .help("Comma-separated list of FIDs to backfill")
                    .action(ArgAction::Set)
                    .value_name("FIDS"),
            ),
        )
        .subcommand(Command::new("worker").about("Start backfill worker"))
        .get_matches();

    match matches.subcommand() {
        Some(("start", _)) => {
            // Validate configuration
            config
                .validate()
                .map_err(|e| color_eyre::eyre::eyre!("Invalid configuration: {}", e))?;

            // Create the application
            let mut app = App::new(config)
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
                        ProcessorType::Print, // Enable print processor by default for easier debugging
                    ])
            });

            app.register_service(streaming_service);

            // Run the application
            info!("Starting Waypoint service");
            app.run_until_shutdown()
                .await
                .map_err(|e| color_eyre::eyre::eyre!("Application error: {}", e))?;

            info!("Shutdown complete");
        },
        Some(("backfill", args)) => {
            let fids_str = args.get_one::<String>("fids").cloned().unwrap_or_default();
            let command = format!("cargo run --bin backfill queue --fids {}", fids_str);

            let status =
                std::process::Command::new("sh").arg("-c").arg(command).status().map_err(|e| {
                    color_eyre::eyre::eyre!("Failed to execute backfill command: {}", e)
                })?;

            if !status.success() {
                return Err(color_eyre::eyre::eyre!(
                    "Backfill command failed with status: {}",
                    status
                ));
            }
        },
        Some(("worker", _)) => {
            let command = "cargo run --bin backfill worker";

            let status =
                std::process::Command::new("sh").arg("-c").arg(command).status().map_err(|e| {
                    color_eyre::eyre::eyre!("Failed to execute worker command: {}", e)
                })?;

            if !status.success() {
                return Err(color_eyre::eyre::eyre!(
                    "Worker command failed with status: {}",
                    status
                ));
            }
        },
        _ => {
            println!("Please specify a subcommand. Use --help for more information.");
        },
    }

    Ok(())
}
