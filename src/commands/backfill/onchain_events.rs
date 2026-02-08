use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use waypoint::{
    backfill::onchain_events::OnChainEventBackfiller,
    config::Config,
    hub::client::Hub,
    processor::{database::DatabaseProcessor, types::AppResources},
};

/// Register onchain events backfill commands
pub fn register_commands(app: Command) -> Command {
    app.about("Backfill onchain events for Farcaster FIDs")
        .arg_required_else_help(true)
        .arg(
            Arg::new("all")
                .long("all")
                .help("Backfill onchain events for all FIDs")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("fid")
                .long("fid")
                .value_name("FID")
                .help("Specific FID to backfill onchain events for")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("event-type")
                .long("event-type")
                .value_name("TYPE")
                .help("Specific event type to backfill missing events for")
                .value_parser([
                    "signer",
                    "signer_migrated",
                    "id_register",
                    "storage_rent",
                    "tier_purchase",
                ]),
        )
        .arg(
            Arg::new("batch-size")
                .long("batch-size")
                .value_name("SIZE")
                .help("Number of FIDs to process in each batch")
                .default_value("100")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Show what would be backfilled without actually doing it")
                .action(clap::ArgAction::SetTrue),
        )
}

/// Handle onchain events backfill command
pub async fn handle_command(matches: &ArgMatches, config: &Config) -> Result<()> {
    let all_fids = matches.get_flag("all");
    let specific_fid = matches.get_one::<u64>("fid").copied();
    let event_type = matches.get_one::<String>("event-type").map(|s| s.as_str());
    let batch_size = *matches.get_one::<usize>("batch-size").unwrap();
    let dry_run = matches.get_flag("dry-run");

    if !all_fids && specific_fid.is_none() && event_type.is_none() {
        println!("Please specify one of: --all, --fid <FID>, or --event-type <TYPE>");
        return Ok(());
    }

    info!("Initializing resources for onchain events backfill...");

    // Initialize individual clients
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let database = Arc::new(waypoint::database::client::Database::new(&config.database).await?);
    let mut hub = Hub::new(config.hub.clone())?;
    hub.connect().await.map_err(|e| color_eyre::eyre::eyre!("{}", e))?;
    let hub_client = Arc::new(hub);

    // Create shared application resources (wraps hub in Mutex for DatabaseProcessor compatibility)
    let hub_mutex = Arc::new(Mutex::new(hub_client.as_ref().clone()));
    let resources =
        Arc::new(AppResources::with_config(hub_mutex, redis, database.clone(), config.clone()));

    let processor = Arc::new(DatabaseProcessor::new(Arc::clone(&resources)));
    let backfiller = OnChainEventBackfiller::new(hub_client, database, processor);

    if dry_run {
        info!("DRY RUN MODE - No changes will be made");
    }

    match (all_fids, specific_fid, event_type) {
        (true, _, _) => {
            info!("Backfilling onchain events for all FIDs");
            if dry_run {
                let fids = backfiller.get_all_fids().await?;
                info!("Would process {} FIDs in batches of {}", fids.len(), batch_size);
                return Ok(());
            }

            let fids = backfiller.get_all_fids().await?;
            info!("Found {} FIDs to process", fids.len());

            // Process FIDs in batches
            let mut total_processed = 0;
            let mut total_errors = 0;
            let mut total_events = 0;

            for batch in fids.chunks(batch_size) {
                info!("Processing batch of {} FIDs", batch.len());

                match backfiller.backfill_fids(batch.to_vec()).await {
                    Ok(results) => {
                        for result in results {
                            total_processed += 1;
                            total_events += result.total_events_processed;
                            if !result.is_success() {
                                total_errors += 1;
                                warn!(
                                    "Backfill errors for FID {}: {:?}",
                                    result.fid, result.errors
                                );
                            }
                        }
                        info!(
                            "Batch completed. Total processed: {}, Total events: {}, Total errors: {}",
                            total_processed, total_events, total_errors
                        );
                    },
                    Err(e) => {
                        error!("Failed to process batch: {}", e);
                        total_errors += batch.len();
                    },
                }
            }

            info!(
                "Backfill completed! Processed: {}, Total events: {}, Errors: {}",
                total_processed, total_events, total_errors
            );

            if total_errors > 0 {
                warn!("{} FIDs had errors during backfill", total_errors);
                std::process::exit(1);
            }
        },
        (false, Some(fid), _) => {
            info!("Backfilling onchain events for specific FID: {}", fid);
            if dry_run {
                info!("Would backfill FID: {}", fid);
                return Ok(());
            }

            match backfiller.backfill_fid(fid).await {
                Ok(result) => {
                    info!("Backfill result: {}", result.summary());
                    if !result.is_success() {
                        warn!("Backfill completed with errors for FID {}", fid);
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    error!("Failed to backfill FID {}: {}", fid, e);
                    std::process::exit(1);
                },
            }
        },
        (false, None, Some(et)) => {
            let event_type_enum = match et {
                "signer" => waypoint::proto::OnChainEventType::EventTypeSigner,
                "signer_migrated" => waypoint::proto::OnChainEventType::EventTypeSignerMigrated,
                "id_register" => waypoint::proto::OnChainEventType::EventTypeIdRegister,
                "storage_rent" => waypoint::proto::OnChainEventType::EventTypeStorageRent,
                "tier_purchase" => waypoint::proto::OnChainEventType::EventTypeTierPurchase,
                _ => {
                    error!("Invalid event type: {}", et);
                    std::process::exit(1);
                },
            };

            info!("Getting FIDs missing event type: {:?}", event_type_enum);
            let fids = backfiller.get_fids_missing_event_type(event_type_enum).await?;
            info!("Found {} FIDs missing {} events", fids.len(), et);

            if dry_run {
                info!("Would process {} FIDs in batches of {}", fids.len(), batch_size);
                return Ok(());
            }

            // Process FIDs in batches
            let mut total_processed = 0;
            let mut total_errors = 0;
            let mut total_events = 0;

            for batch in fids.chunks(batch_size) {
                info!("Processing batch of {} FIDs for {} events", batch.len(), et);

                match backfiller.backfill_fids(batch.to_vec()).await {
                    Ok(results) => {
                        for result in results {
                            total_processed += 1;
                            total_events += result.total_events_processed;
                            if !result.is_success() {
                                total_errors += 1;
                                warn!(
                                    "Backfill errors for FID {}: {:?}",
                                    result.fid, result.errors
                                );
                            }
                        }
                        info!(
                            "Batch completed. Total processed: {}, Total events: {}, Total errors: {}",
                            total_processed, total_events, total_errors
                        );
                    },
                    Err(e) => {
                        error!("Failed to process batch: {}", e);
                        total_errors += batch.len();
                    },
                }
            }

            info!(
                "Backfill completed for {} events! Processed: {}, Total events: {}, Errors: {}",
                et, total_processed, total_events, total_errors
            );

            if total_errors > 0 {
                warn!("{} FIDs had errors during backfill", total_errors);
                std::process::exit(1);
            }
        },
        _ => {
            println!("Please specify one of: --all, --fid <FID>, or --event-type <TYPE>");
        },
    }

    info!("Onchain events backfill completed successfully!");
    Ok(())
}
