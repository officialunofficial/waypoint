use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use waypoint::{
    backfill::reconciler::MessageReconciler,
    config::Config,
    processor::{AppResources, consumer::EventProcessor, database::DatabaseProcessor},
};

/// Register user_data command
pub fn register_command() -> Command {
    Command::new("user-data")
        .about("Refresh only user_data for FIDs")
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
        )
}

/// Update user_data for FIDs
pub async fn execute(config: &Config, args: &ArgMatches) -> Result<()> {
    // Initialize clients
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let hub = Arc::new(Mutex::new(waypoint::hub::client::Hub::new(config.hub.clone())?));
    let database = Arc::new(waypoint::database::client::Database::new(&config.database).await?);

    // Get parameters
    let max_fid_str = args.get_one::<String>("max_fid");
    let batch_size = args.get_one::<usize>("batch_size").copied().unwrap_or(50);
    let concurrency = args.get_one::<usize>("concurrency").copied().unwrap_or(25);

    // Initialize hub client
    let mut hub_guard = hub.lock().await;
    hub_guard.connect().await?;
    let hub_client =
        hub_guard.client().ok_or_else(|| color_eyre::eyre::eyre!("No hub client available"))?;

    // Get max FID from hub if not specified
    let hub_max_fid = if max_fid_str.is_none() {
        let fids_response = hub_client
            .clone()
            .get_fids(tonic::Request::new(waypoint::proto::FidsRequest {
                page_size: Some(1),
                page_token: None,
                reverse: Some(true),
                shard_id: 0,
            }))
            .await?;

        fids_response.into_inner().fids.first().copied().unwrap_or(1000)
    } else {
        max_fid_str.and_then(|s| s.parse().ok()).unwrap_or(1000)
    };

    info!("Starting user_data refresh for FIDs up to {}", hub_max_fid);

    // Create shared application resources
    let app_resources = Arc::new(AppResources::new(hub.clone(), redis.clone(), database.clone()));

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

                        info!("Retrieved {} user_data messages for FID {}", message_count, fid);

                        // Process each message
                        let mut success = true;
                        let mut processed = 0;

                        // Process all events with the database processor
                        for message in &messages {
                            let event = reconciler_clone.message_to_hub_event(message.clone());

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

    Ok(())
}
