use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use waypoint::{
    backfill::{
        reconciler::MessageReconciler,
        worker::{BackfillQueue, Worker},
    },
    config::Config,
    processor::{AppResources, database::DatabaseProcessor},
};

/// Register worker command
pub fn register_command() -> Command {
    Command::new("worker")
        .about("Start FID-based backfill worker")
        .arg(
            Arg::new("exit_on_complete")
                .long("exit-on-complete")
                .help("Exit when backfill queue is empty (instead of running forever)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("idle_timeout")
                .long("idle-timeout")
                .help("Seconds to wait with empty queue before considering backfill complete (default: 30)")
                .value_parser(clap::value_parser!(u64))
                .default_value("30"),
        )
}

/// Run a backfill worker for FIDs
pub async fn execute(config: &Config, args: &ArgMatches) -> Result<()> {
    // Parse arguments
    let exit_on_complete = args.get_flag("exit_on_complete");
    let idle_timeout = args.get_one::<u64>("idle_timeout").copied().unwrap_or(30);

    // Initialize clients
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let mut hub = waypoint::hub::client::Hub::new(config.hub.clone())?;
    hub.connect().await?;
    let hub = Arc::new(hub);
    let database = Arc::new(waypoint::database::client::Database::new(&config.database).await?);
    let fid_queue = Arc::new(BackfillQueue::new(redis.clone(), "backfill:fid:queue".to_string()));

    // Create shared application resources (wraps hub in Mutex for DatabaseProcessor compatibility)
    let hub_mutex = Arc::new(Mutex::new(hub.as_ref().clone()));
    let app_resources = Arc::new(AppResources::new(hub_mutex, redis.clone(), database.clone()));

    // Create processors
    let db_processor = Arc::new(DatabaseProcessor::new(app_resources.clone()));

    // Create reconciler
    let reconciler = Arc::new(MessageReconciler::new(
        hub.clone(),
        database,
        std::time::Duration::from_secs(30),
        true,
    ));

    // Set concurrency to match database connection pool capacity
    // Use 40% of max database connections to avoid saturating the pool
    // This is a more conservative value to prevent database timeouts
    let max_concurrency = (config.database.max_connections as f32 * 0.4) as usize;

    // Ensure there's a reasonable upper limit regardless of pool size
    let absolute_max = 8;

    let requested_concurrency = config.backfill.concurrency.unwrap_or(4);
    let concurrency =
        std::cmp::min(std::cmp::min(requested_concurrency, max_concurrency), absolute_max);

    info!(
        "Using worker concurrency: {} (requested: {}, max based on DB connections: {})",
        concurrency, requested_concurrency, max_concurrency
    );
    let mut worker = Worker::new(reconciler, fid_queue, concurrency);

    // Configure exit behavior
    if exit_on_complete {
        worker.set_exit_on_complete(idle_timeout);
        info!("Worker will exit when queue is empty for {} seconds", idle_timeout);
    }

    // Add processors to worker
    worker.add_processor(db_processor);

    info!("Starting FID-based backfill worker");
    worker.run().await.map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    // Hub will be dropped when Arc refcount reaches 0
    Ok(())
}
