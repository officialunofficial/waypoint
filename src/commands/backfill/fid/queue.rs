use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use waypoint::{
    backfill::worker::{BackfillJob, BackfillQueue, JobPriority, JobState},
    config::Config,
    hub::client::Hub,
};

/// Helper function to get max FID from hub info
async fn get_max_fid_from_hub_info(hub: &mut Hub) -> u64 {
    match hub.get_hub_info().await {
        Ok(info) => {
            // Use the total number of FID registrations as an approximation
            // In practice, FIDs are usually assigned sequentially, so this is a good estimate
            let total_fids = info.db_stats.as_ref()
                .map(|stats| stats.num_fid_registrations)
                .unwrap_or(0);
            
            if total_fids > 0 {
                info!("Detected {} total FID registrations from hub info", total_fids);
                // Add some buffer to account for any recent registrations
                let max_fid_estimate = total_fids + 10000;
                info!("Using estimated max FID: {} (total registrations + 10k buffer)", max_fid_estimate);
                max_fid_estimate
            } else {
                let default_max = 10000;
                info!("No FID registrations found in hub info, using default max FID: {}", default_max);
                default_max
            }
        },
        Err(e) => {
            info!("Failed to get hub info: {}. Using default max FID.", e);
            let default_max = 10000;
            info!("Using default max FID: {}", default_max);
            default_max
        },
    }
}

/// Register FID queue command
pub fn register_command() -> Command {
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
        )
        .arg(
            Arg::new("batch_size")
                .long("batch-size")
                .help("Number of FIDs per job batch")
                .value_parser(clap::value_parser!(u64))
                .default_value("50"),
        )
}

/// Queue FIDs for backfill
pub async fn execute(config: &Config, args: &ArgMatches) -> Result<()> {
    // Initialize clients
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let hub = Arc::new(Mutex::new(waypoint::hub::client::Hub::new(config.hub.clone())?));
    let fid_queue = Arc::new(BackfillQueue::new(redis.clone(), "backfill:fid:queue".to_string()));

    let fids_str = args.get_one::<String>("fids");
    let max_fid_str = args.get_one::<String>("max_fid");
    let batch_size = args.get_one::<u64>("batch_size").copied().unwrap_or(50);

    let mut hub_guard = hub.lock().await;
    hub_guard.connect().await?;

    // Get the maximum FID - try GetFids first, fall back to hub info
    let hub_max_fid = match hub_guard.get_fids(Some(1), None, Some(true)).await {
        Ok(fids_response) => {
            if let Some(max_fid) = fids_response.fids.first() {
                info!("Detected maximum FID from hub: {}", max_fid);
                *max_fid
            } else {
                // No FIDs found, use hub info
                get_max_fid_from_hub_info(&mut hub_guard).await
            }
        },
        Err(e) => {
            info!("Failed to get FIDs from hub: {}. Falling back to hub info.", e);
            // For sharded hubs, GetFids might not work, so use hub info
            get_max_fid_from_hub_info(&mut hub_guard).await
        },
    };

    // If no options provided, use the maximum FID from hub
    if fids_str.is_none() && max_fid_str.is_none() {
        info!("No FIDs or max FID specified, using maximum FID from hub: {}", hub_max_fid);

        // Queue in batches up to the maximum FID
        let mut queued_batches = 0;
        for start in (1..=hub_max_fid).step_by(batch_size as usize) {
            let end = std::cmp::min(start + batch_size - 1, hub_max_fid);
            let batch_fids = (start..=end).collect::<Vec<_>>();

            fid_queue
                .add_job(BackfillJob {
                    fids: batch_fids,
                    priority: JobPriority::Normal,
                    state: JobState::Pending,
                    visibility_timeout: None,
                    attempts: 0,
                    created_at: chrono::Utc::now(),
                    id: String::new(),
                })
                .await?;

            queued_batches += 1;
            if queued_batches % 10 == 0 {
                info!("Queued {} batches of FIDs up to {}", queued_batches, end);
            }
        }

        info!("Queued all FIDs from 1 to {} in {} batches (batch size: {})", hub_max_fid, queued_batches, batch_size);
        
        // Log queue status
        let queue_len = fid_queue.get_queue_length().await.unwrap_or(0);
        info!("Total jobs in queue after queueing: {}", queue_len);
    }
    // Process specific FIDs if provided
    else if let Some(fids) = fids_str {
        let fid_list =
            fids.split(',').filter_map(|f| f.trim().parse::<u64>().ok()).collect::<Vec<_>>();

        if fid_list.is_empty() {
            return Err(color_eyre::eyre::eyre!("No valid FIDs found in the provided list"));
        }

        fid_queue
            .add_job(BackfillJob {
                fids: fid_list.clone(),
                priority: JobPriority::Normal,
                state: JobState::Pending,
                visibility_timeout: None,
                attempts: 0,
                created_at: chrono::Utc::now(),
                id: String::new(),
            })
            .await?;
        info!("Queued specific FIDs: {:?}", fid_list);
    }
    // Process FIDs up to max_fid or hub max
    else if let Some(max_fid) = max_fid_str {
        let max = max_fid.parse::<u64>().unwrap_or(hub_max_fid);
        info!("Using specified max FID: {}", max);

        // Queue in batches
        let mut queued_batches = 0;
        for start in (1..=max).step_by(batch_size as usize) {
            let end = std::cmp::min(start + batch_size - 1, max);
            let batch_fids = (start..=end).collect::<Vec<_>>();

            fid_queue
                .add_job(BackfillJob {
                    fids: batch_fids,
                    priority: JobPriority::Normal,
                    state: JobState::Pending,
                    visibility_timeout: None,
                    attempts: 0,
                    created_at: chrono::Utc::now(),
                    id: String::new(),
                })
                .await?;

            queued_batches += 1;
            if queued_batches % 10 == 0 {
                info!("Queued {} batches of FIDs up to {}", queued_batches, end);
            }
        }

        info!("Queued all FIDs from 1 to {} in {} batches (batch size: {})", max, queued_batches, batch_size);
        
        // Log queue status
        let queue_len = fid_queue.get_queue_length().await.unwrap_or(0);
        info!("Total jobs in queue after queueing: {}", queue_len);
    }

    info!("FID backfill jobs queued successfully");
    hub_guard.disconnect().await?;

    Ok(())
}
