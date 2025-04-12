use clap::{ArgMatches, Command};
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
    Command::new("worker").about("Start FID-based backfill worker")
}

/// Run a backfill worker for FIDs
pub async fn execute(config: &Config, _args: &ArgMatches) -> Result<()> {
    // Initialize clients
    let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
    let hub = Arc::new(Mutex::new(waypoint::hub::client::Hub::new(config.hub.clone())?));
    let database = Arc::new(waypoint::database::client::Database::new(&config.database).await?);
    let fid_queue = Arc::new(BackfillQueue::new(redis.clone(), "backfill:fid:queue".to_string()));

    // Clone the hub client first
    let hub_client = {
        let mut hub_guard = hub.lock().await;
        hub_guard.connect().await?;
        hub_guard
            .client()
            .ok_or_else(|| color_eyre::eyre::eyre!("No hub client available"))?
            .clone()
    };

    // Create shared application resources
    let app_resources = Arc::new(AppResources::new(hub.clone(), redis.clone(), database.clone()));

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
    let concurrency = config.backfill.concurrency.unwrap_or(50);

    info!("Using worker concurrency: {}", concurrency);
    let mut worker = Worker::new(reconciler, fid_queue, concurrency);

    // Add processors to worker
    worker.add_processor(db_processor);

    info!("Starting FID-based backfill worker");
    worker.run().await.map_err(|e| color_eyre::eyre::eyre!("{}", e))?;

    // Disconnect hub client when done
    let mut hub_guard = hub.lock().await;
    hub_guard.disconnect().await?;

    Ok(())
}
