use clap::{ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tracing::info;
use waypoint::{backfill::worker::BackfillQueue, config::Config};

/// Register inspect command
pub fn register_command() -> Command {
    Command::new("inspect").about("Inspect the current state of the backfill queue")
}

/// Inspect the backfill queue state
pub async fn execute(config: &Config, _args: &ArgMatches) -> Result<()> {
    // Initialize Redis client
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let fid_queue = Arc::new(BackfillQueue::new(redis.clone(), "backfill:fid:queue".to_string()));

    info!("Inspecting backfill queue state...");

    // Get queue metrics
    let metrics = fid_queue.get_metrics().await;

    info!("Queue Metrics:");
    info!("  Jobs queued: {}", metrics.jobs_queued);
    info!("  Jobs processed: {}", metrics.jobs_processed);
    info!("  Jobs failed: {}", metrics.jobs_failed);
    info!("  FIDs processed: {}", metrics.fids_processed);
    info!("  Average job time: {:.2}ms", metrics.avg_job_time_ms);
    info!("");
    info!("Queue Sizes:");
    info!("  Normal queue: {} jobs", metrics.normal_priority_queue_size);
    info!("  In-progress queue: {} jobs", metrics.in_progress_queue_size);

    let total_pending = metrics.normal_priority_queue_size;
    info!("");
    info!("Total pending jobs: {}", total_pending);

    // Check for potential issues
    if metrics.in_progress_queue_size > 100 {
        info!("");
        info!("⚠️  WARNING: Large number of jobs in progress ({})", metrics.in_progress_queue_size);
        info!("   This might indicate jobs are not being properly completed.");
        info!("   Consider running cleanup or restarting workers.");
    }

    if total_pending == 0 && metrics.in_progress_queue_size > 0 {
        info!("");
        info!(
            "⚠️  WARNING: No pending jobs but {} jobs still in progress",
            metrics.in_progress_queue_size
        );
        info!("   These jobs may be stuck. Consider running cleanup.");
    }

    Ok(())
}
