use std::sync::Arc;
use clap::Parser;
use tracing::info;
use color_eyre::eyre::Result;

use waypoint::{
    processor::AppResources,
    backfill::bench::run_benchmark,
    processor::database::DatabaseProcessor,
};

/// Run database batch insert performance benchmarks
#[derive(Parser, Debug)]
pub struct BenchmarkCommand {
    /// Number of messages to generate for the benchmark
    #[clap(long, default_value = "10000")]
    pub messages: usize,
}

impl BenchmarkCommand {
    pub async fn execute(self, resources: Arc<AppResources>) -> Result<()> {
        info!("Starting database batch insert benchmark");
        
        // Create a processor instance for testing
        let processor = Arc::new(DatabaseProcessor::new(resources.clone()));
        
        // Run the benchmark
        if let Err(e) = run_benchmark(resources.database.clone(), processor, self.messages).await {
            return Err(color_eyre::eyre::eyre!("Benchmark error: {}", e));
        }
        
        info!("Benchmark completed");
        
        Ok(())
    }
}