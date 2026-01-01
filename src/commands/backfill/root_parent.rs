use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use waypoint::{
    backfill::root_parent::{RootParentBackfill, RootParentBackfillConfig, backfill_url_parents},
    config::Config,
    hub::client::Hub,
};

/// Register root_parent backfill commands
pub fn register_commands(app: Command) -> Command {
    app.about("Backfill root_parent columns for existing casts")
        .arg_required_else_help(true)
        .arg(
            Arg::new("hash-parents")
                .long("hash-parents")
                .help("Backfill casts with hash-based parents (requires Hub traversal)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("url-parents")
                .long("url-parents")
                .help("Backfill casts with URL parents (simple SQL update)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("all")
                .long("all")
                .help("Backfill both hash-based and URL parents")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("batch-size")
                .long("batch-size")
                .value_name("SIZE")
                .help("Number of casts to process per batch (hash-parents only)")
                .default_value("100")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("max-depth")
                .long("max-depth")
                .value_name("DEPTH")
                .help("Maximum parent chain depth to traverse (hash-parents only)")
                .default_value("100")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("batch-delay")
                .long("batch-delay")
                .value_name("MS")
                .help("Delay between batches in milliseconds (hash-parents only)")
                .default_value("100")
                .value_parser(clap::value_parser!(u64)),
        )
}

/// Handle root_parent backfill command
pub async fn handle_command(matches: &ArgMatches, config: &Config) -> Result<()> {
    let hash_parents = matches.get_flag("hash-parents");
    let url_parents = matches.get_flag("url-parents");
    let all = matches.get_flag("all");

    let batch_size = *matches.get_one::<usize>("batch-size").unwrap();
    let max_depth = *matches.get_one::<usize>("max-depth").unwrap();
    let batch_delay = *matches.get_one::<u64>("batch-delay").unwrap();

    if !hash_parents && !url_parents && !all {
        println!("Please specify one of: --all, --hash-parents, or --url-parents");
        return Ok(());
    }

    let do_hash = hash_parents || all;
    let do_url = url_parents || all;

    info!("Initializing resources for root_parent backfill...");

    // Initialize database
    let database = waypoint::database::client::Database::new(&config.database).await?;
    let pool = &database.pool;

    // Run URL parents backfill (simple SQL, no Hub needed)
    if do_url {
        info!("Backfilling URL parents...");
        match backfill_url_parents(pool).await {
            Ok(count) => {
                info!("URL parents backfill complete: {} casts updated", count);
            },
            Err(e) => {
                error!("URL parents backfill failed: {}", e);
                if !do_hash {
                    std::process::exit(1);
                }
            },
        }
    }

    // Run hash parents backfill (requires Hub traversal)
    if do_hash {
        info!("Backfilling hash-based parents (this may take a while)...");

        // Initialize Hub client
        let hub = Arc::new(Mutex::new(Hub::new(config.hub.clone())?));

        // Connect to Hub
        {
            let mut hub_guard = hub.lock().await;
            if !hub_guard.check_connection().await.unwrap_or(false) {
                error!("Failed to connect to Hub - required for hash-based backfill");
                std::process::exit(1);
            }
        }

        let backfill_config =
            RootParentBackfillConfig { batch_size, max_depth, batch_delay_ms: batch_delay };

        let backfiller = RootParentBackfill::new(pool.clone(), hub, backfill_config);

        match backfiller.run().await {
            Ok(stats) => {
                info!("Hash parents backfill complete:");
                info!("  Casts processed: {}", stats.casts_processed);
                info!("  Roots found: {}", stats.roots_found);
                info!("  Broken chains: {}", stats.chains_broken);
                info!("  Errors: {}", stats.errors);

                if stats.errors > 0 {
                    std::process::exit(1);
                }
            },
            Err(e) => {
                error!("Hash parents backfill failed: {}", e);
                std::process::exit(1);
            },
        }
    }

    info!("Root parent backfill completed successfully!");
    Ok(())
}
