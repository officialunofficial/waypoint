use clap::{Arg, Command};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use waypoint::{
    backfill::{
        block_reconciler::BlockReconciler,
        block_worker::{BlockBackfillJob, BlockBackfillQueue, BlockWorker},
        reconciler::MessageReconciler,
        worker::{BackfillJob, BackfillQueue, Worker},
    },
    config::Config,
    database::client::Database,
    hub::client::Hub,
    processor::{AppResources, consumer::EventProcessor, database::DatabaseProcessor},
    redis::client::Redis,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging directly in this binary
    let config = Config::load().map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    })?;

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

    let matches = Command::new("Backfill queue and workers")
        .version("1.0")
        .about("Backfills data from the Hub")
        .subcommand(
            Command::new("queue")
                .about("Queue FIDs for backfill")
                .arg(
                    Arg::new("fids")
                        .long("fids")
                        .help("Comma-separated list of FIDs to backfill")
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    Arg::new("max_fid")
                        .long("max-fid")
                        .help("Maximum FID to backfill up to")
                        .value_parser(clap::value_parser!(String)),
                ),
        )
        .subcommand(Command::new("worker").about("Start backfill worker"))
        .subcommand(
            Command::new("user-data")
                .about("Refresh only user_data for all FIDs")
                .arg(
                    Arg::new("max_fid")
                        .long("max-fid")
                        .help("Maximum FID to update user data for")
                        .value_parser(clap::value_parser!(String)),
                )
                .arg(
                    Arg::new("batch_size")
                        .long("batch-size")
                        .help("Number of FIDs to process in each batch")
                        .value_parser(clap::value_parser!(usize))
                        .default_value("50"),
                )
                .arg(
                    Arg::new("concurrency")
                        .long("concurrency")
                        .help("Number of concurrent FIDs to process")
                        .value_parser(clap::value_parser!(usize))
                        .default_value("25"),
                ),
        )
        .subcommand(
            Command::new("block-queue")
                .about("Queue blocks for backfill")
                .arg(
                    Arg::new("shard_id")
                        .long("shard-id")
                        .help("Shard ID to backfill")
                        .value_parser(clap::value_parser!(u32))
                        .default_value("0"),
                )
                .arg(
                    Arg::new("start_block")
                        .long("start-block")
                        .help("Starting block number")
                        .value_parser(clap::value_parser!(u64))
                        .required(false),
                )
                .arg(
                    Arg::new("end_block")
                        .long("end-block")
                        .help("Ending block number")
                        .value_parser(clap::value_parser!(u64))
                        .required(false),
                )
                .arg(
                    Arg::new("batch_size")
                        .long("batch-size")
                        .help("Number of blocks per job")
                        .value_parser(clap::value_parser!(u64))
                        .default_value("100"),
                ),
        )
        .subcommand(Command::new("block-worker").about("Start block-based backfill worker"))
        .get_matches();

    // We already loaded config for logging, reuse it

    // Initialize clients
    let redis = Arc::new(Redis::new(&config.redis).await?);
    let hub = Arc::new(Mutex::new(Hub::new(config.hub.clone())?));
    let database = Arc::new(Database::new(&config.database).await?);

    // Initialize queues
    let fid_queue = Arc::new(BackfillQueue::new(redis.clone(), "backfill:fid:queue".to_string()));
    let block_queue = Arc::new(BlockBackfillQueue::new(redis.clone(), "backfill:block:queue".to_string()));

    match matches.subcommand() {
        Some(("queue", args)) => {
            let fids_str = args.get_one::<String>("fids");
            let max_fid_str = args.get_one::<String>("max_fid");

            let mut hub_guard = hub.lock().await;
            hub_guard.connect().await?;
            let hub_client = hub_guard.client().ok_or::<Box<dyn std::error::Error + Send + Sync>>(
                "No hub client available".into(),
            )?;

            // Get max FID from hub
            let fids_response = hub_client
                .clone()
                .get_fids(tonic::Request::new(waypoint::proto::FidsRequest {
                    page_size: Some(1),
                    page_token: None,
                    reverse: Some(true),
                }))
                .await?;

            let hub_max_fid = fids_response.into_inner().fids.first().copied().unwrap_or(1000);
            info!("Detected maximum FID from hub: {}", hub_max_fid);

            let fids = if let Some(fids) = fids_str {
                fids.split(',').filter_map(|f| f.trim().parse::<u64>().ok()).collect::<Vec<_>>()
            } else if let Some(max_fid) = max_fid_str {
                let max = max_fid.parse::<u64>().unwrap_or(hub_max_fid);
                info!("Using specified max FID: {}", max);
                // Increase batch size for better throughput
                let batch_size: u64 = 50; // Increased from 10 to 50 FIDs per job

                // Queue in batches
                for start in (1..=max).step_by(batch_size as usize) {
                    let end = std::cmp::min(start + batch_size - 1, max);
                    let batch_fids = (start..=end).collect::<Vec<_>>();

                    fid_queue
                        .add_job(BackfillJob {
                            fids: batch_fids,
                            priority: waypoint::backfill::worker::JobPriority::Normal,
                            state: waypoint::backfill::worker::JobState::Pending,
                            visibility_timeout: None,
                            attempts: 0,
                            created_at: chrono::Utc::now(),
                            id: String::new(),
                        })
                        .await?;
                    info!("Queued FIDs {}-{}", start, end);
                }

                Vec::new() // Return empty since we've already queued
            } else {
                // Use the max FID we found from hub
                info!("Using hub's max FID for backfill: {}", hub_max_fid);
                // Increase batch size for better throughput
                let batch_size: u64 = 50; // Increased from 10 to 50 FIDs per job

                for start in (1..=hub_max_fid).step_by(batch_size as usize) {
                    let end = std::cmp::min(start + batch_size - 1, hub_max_fid);
                    let batch_fids = (start..=end).collect::<Vec<_>>();

                    fid_queue
                        .add_job(BackfillJob {
                            fids: batch_fids,
                            priority: waypoint::backfill::worker::JobPriority::Normal,
                            state: waypoint::backfill::worker::JobState::Pending,
                            visibility_timeout: None,
                            attempts: 0,
                            created_at: chrono::Utc::now(),
                            id: String::new(),
                        })
                        .await?;
                    info!("Queued FIDs {}-{}", start, end);
                }

                Vec::new() // Return empty since we've already queued
            };

            // Add any directly specified FIDs
            if !fids.is_empty() {
                fid_queue
                    .add_job(BackfillJob {
                        fids,
                        priority: waypoint::backfill::worker::JobPriority::Normal,
                        state: waypoint::backfill::worker::JobState::Pending,
                        visibility_timeout: None,
                        attempts: 0,
                        created_at: chrono::Utc::now(),
                        id: String::new(),
                    })
                    .await?;
                info!("Queued specified FIDs");
            }

            info!("Backfill jobs queued successfully");

            hub_guard.disconnect().await?;
        },
        Some(("worker", _)) => {
            // Clone the hub client first
            let hub_client = {
                let mut hub_guard = hub.lock().await;
                hub_guard.connect().await?;
                hub_guard
                    .client()
                    .ok_or::<Box<dyn std::error::Error + Send + Sync>>(
                        "No hub client available".into(),
                    )?
                    .clone()
            };

            // Create shared application resources
            let app_resources =
                Arc::new(AppResources::new(hub.clone(), redis.clone(), database.clone()));

            // Create processors
            let db_processor = Arc::new(DatabaseProcessor::new(app_resources.clone()));

            // Create reconciler
            let reconciler = Arc::new(MessageReconciler::new(
                hub_client,
                database,
                std::time::Duration::from_secs(30),
                true,
            ));

            // Create and run worker with increased concurrency
            // Higher concurrency for more parallel job processing
            // Use config system for concurrency
            let concurrency = config.backfill.concurrency.unwrap_or(50);

            info!("Using worker concurrency: {}", concurrency);
            let mut worker = Worker::new(reconciler, fid_queue, concurrency);

            // Add processors to worker
            worker.add_processor(db_processor);

            info!("Starting backfill worker");
            worker.run().await?;

            // Disconnect hub client when done
            let mut hub_guard = hub.lock().await;
            hub_guard.disconnect().await?;
        },
        Some(("block-queue", args)) => {
            let shard_id = args.get_one::<u32>("shard_id").copied().unwrap_or(0);
            let batch_size = args.get_one::<u64>("batch_size").copied().unwrap_or(100);
            
            // Connect to hub to get block info
            let mut hub_guard = hub.lock().await;
            hub_guard.connect().await?;
            // Validate that hub client is available
            hub_guard.client().ok_or::<Box<dyn std::error::Error + Send + Sync>>(
                "No hub client available".into(),
            )?;
            
            // Get latest block from hub
            let hub_info = hub_guard.get_hub_info().await?;
            
            // Find the shard info
            let mut latest_block = 0;
            for shard_info in hub_info.shard_infos {
                if shard_info.shard_id == shard_id {
                    latest_block = shard_info.max_height;
                    break;
                }
            }
            
            if latest_block == 0 {
                return Err(format!("Shard {} not found or has no blocks", shard_id).into());
            }
            
            info!("Detected latest block from hub: {} for shard {}", latest_block, shard_id);
            
            // Get block range from args or use defaults
            let start_block = args.get_one::<u64>("start_block").copied().unwrap_or(1);
            let end_block = args.get_one::<u64>("end_block").copied().unwrap_or(latest_block);
            
            info!("Will queue blocks {} to {} for shard {}", start_block, end_block, shard_id);
            
            // Queue block jobs in batches
            for batch_start in (start_block..=end_block).step_by(batch_size as usize) {
                let batch_end = std::cmp::min(batch_start + batch_size - 1, end_block);
                
                block_queue
                    .add_job(BlockBackfillJob {
                        shard_id,
                        start_block: batch_start,
                        end_block: batch_end,
                        priority: waypoint::backfill::block_worker::JobPriority::Normal,
                        state: waypoint::backfill::block_worker::JobState::Pending,
                        visibility_timeout: None,
                        attempts: 0,
                        created_at: chrono::Utc::now(),
                        id: String::new(),
                    })
                    .await?;
                    
                info!("Queued blocks {}-{} for shard {}", batch_start, batch_end, shard_id);
            }
            
            info!("Block backfill jobs queued successfully");
            hub_guard.disconnect().await?;
        },
        Some(("block-worker", _)) => {
            // Clone the hub client first
            let hub_client = {
                let mut hub_guard = hub.lock().await;
                hub_guard.connect().await?;
                hub_guard
                    .client()
                    .ok_or::<Box<dyn std::error::Error + Send + Sync>>(
                        "No hub client available".into(),
                    )?
                    .clone()
            };

            // Create shared application resources
            let app_resources =
                Arc::new(AppResources::new(hub.clone(), redis.clone(), database.clone()));

            // Create processors
            let db_processor = Arc::new(DatabaseProcessor::new(app_resources.clone()));

            // Create reconciler for blocks
            let block_reconciler = Arc::new(BlockReconciler::new(
                hub_client,
                database.clone(),
                std::time::Duration::from_secs(30),
            ));

            // Create and run worker with block-based approach
            let concurrency = config.backfill.concurrency.unwrap_or(25);

            info!("Using block worker concurrency: {}", concurrency);
            let mut worker = BlockWorker::new(block_reconciler, block_queue, concurrency);

            // Add processors to worker
            worker.add_processor(db_processor);

            info!("Starting block-based backfill worker");
            worker.run().await?;

            // Disconnect hub client when done
            let mut hub_guard = hub.lock().await;
            hub_guard.disconnect().await?;
        },
        Some(("user-data", args)) => {
            // Get parameters
            let max_fid_str = args.get_one::<String>("max_fid");
            let batch_size = args.get_one::<usize>("batch_size").copied().unwrap_or(50);
            let concurrency = args.get_one::<usize>("concurrency").copied().unwrap_or(25);

            // Initialize hub client
            let mut hub_guard = hub.lock().await;
            hub_guard.connect().await?;
            let hub_client = hub_guard.client().ok_or::<Box<dyn std::error::Error + Send + Sync>>(
                "No hub client available".into(),
            )?;

            // Get max FID from hub if not specified
            let hub_max_fid = if max_fid_str.is_none() {
                let fids_response = hub_client
                    .clone()
                    .get_fids(tonic::Request::new(waypoint::proto::FidsRequest {
                        page_size: Some(1),
                        page_token: None,
                        reverse: Some(true),
                    }))
                    .await?;

                fids_response.into_inner().fids.first().copied().unwrap_or(1000)
            } else {
                max_fid_str.and_then(|s| s.parse().ok()).unwrap_or(1000)
            };

            info!("Starting user_data refresh for FIDs up to {}", hub_max_fid);

            // Create shared application resources
            let app_resources =
                Arc::new(AppResources::new(hub.clone(), redis.clone(), database.clone()));

            // Create processor for database
            let db_processor = Arc::new(DatabaseProcessor::new(app_resources.clone()));

            // Create reconciler for user_data operations
            let reconciler = Arc::new(MessageReconciler::new(
                hub_client.clone(),
                database.clone(),
                std::time::Duration::from_secs(30),
                true,
            ));

            let start_time = std::time::Instant::now();
            let mut total_processed = 0;
            let mut success_count = 0;
            let mut error_count = 0;

            // Process in batches
            for start_fid in (1..=hub_max_fid).step_by(batch_size) {
                let end_fid = std::cmp::min(start_fid + (batch_size as u64) - 1, hub_max_fid);
                info!("Processing user_data for FIDs {}-{}", start_fid, end_fid);

                // Create a semaphore to control concurrency
                let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
                let mut handles = Vec::new();

                for fid in start_fid..=end_fid {
                    let reconciler_clone = Arc::clone(&reconciler);
                    let processor_clone = Arc::clone(&db_processor);
                    let semaphore_clone = Arc::clone(&semaphore);

                    let handle = tokio::spawn(async move {
                        // Acquire a permit from the semaphore
                        let _permit = semaphore_clone.acquire().await.unwrap();

                        info!("Updating user_data for FID {}", fid);

                        // Get user_data messages for this FID
                        match reconciler_clone.get_all_user_data_messages(fid).await {
                            Ok(messages) => {
                                let message_count = messages.len();
                                if message_count == 0 {
                                    info!("No user_data found for FID {}", fid);
                                    return (fid, true, 0);
                                }

                                info!(
                                    "Retrieved {} user_data messages for FID {}",
                                    message_count, fid
                                );

                                // Process each message
                                let mut success = true;
                                let mut processed = 0;

                                // Process all events with the database processor
                                for message in &messages {
                                    let event =
                                        reconciler_clone.message_to_hub_event(message.clone());

                                    // Process with database processor
                                    match processor_clone.process_event(event).await {
                                        Err(e) => {
                                            error!(
                                                "Error processing user_data for FID {} with DB processor: {:?}",
                                                fid, e
                                            );
                                            success = false;
                                        },
                                        _ => {
                                            processed += 1;
                                        },
                                    }
                                }

                                info!(
                                    "Processed {}/{} user_data messages for FID {}",
                                    processed, message_count, fid
                                );

                                (fid, success, processed)
                            },
                            Err(e) => {
                                error!("Failed to retrieve user_data for FID {}: {:?}", fid, e);
                                (fid, false, 0)
                            },
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all FIDs in this batch to be processed
                for handle in handles {
                    match handle.await {
                        Ok((_fid, success, processed)) => {
                            total_processed += processed;
                            if success {
                                success_count += 1;
                            } else {
                                error_count += 1;
                            }
                        },
                        Err(e) => {
                            error!("Task failure: {:?}", e);
                            error_count += 1;
                        },
                    }
                }

                let elapsed = start_time.elapsed();
                info!(
                    "Processed batch {}-{}, total progress: {} user_data messages, {} FIDs successful, {} FIDs failed, elapsed: {:.2?}",
                    start_fid, end_fid, total_processed, success_count, error_count, elapsed
                );
            }

            let elapsed = start_time.elapsed();
            info!(
                "User_data refresh complete: {} user_data messages, {} FIDs successful, {} FIDs failed, elapsed: {:.2?}",
                total_processed, success_count, error_count, elapsed
            );

            // Disconnect hub client when done
            hub_guard.disconnect().await?;
        },
        _ => {
            println!("Please specify a subcommand. Use --help for more information.");
        },
    }

    Ok(())
}