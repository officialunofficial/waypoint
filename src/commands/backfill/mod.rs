pub mod bench;
pub mod fid;

use clap::{ArgMatches, Command};
use color_eyre::eyre::Result;
use std::sync::Arc;
use waypoint::config::Config;

/// Register all backfill-related subcommands
pub fn register_commands(app: Command) -> Command {
    app.about("Backfill data from Snapchain")
        .subcommand_required(true)
        .arg_required_else_help(true)
        // FID-based backfill commands
        .subcommand(fid::register_commands(Command::new("fid")
            .about("FID-based backfill operations")))
        // Benchmark commands
        .subcommand(Command::new("bench")
            .about("Run database performance benchmarks")
            .arg_required_else_help(true)
            .arg(clap::Arg::new("messages")
                .long("messages")
                .value_name("COUNT")
                .help("Number of messages to generate for the benchmark")
                .default_value("10000")))
}

/// Handle backfill commands based on matches
pub async fn handle_command(matches: &ArgMatches, config: &Config) -> Result<()> {
    match matches.subcommand() {
        Some(("fid", submatches)) => fid::handle_command(submatches, config).await,
        Some(("bench", submatches)) => {
            // Get the message count parameter
            let messages = submatches.get_one::<String>("messages")
                .map(|s| s.parse::<usize>().unwrap_or(10000))
                .unwrap_or(10000);
            
            // Create benchmark command
            let command = bench::BenchmarkCommand {
                messages,
            };
            
            // Initialize clients
            let redis = Arc::new(waypoint::redis::client::Redis::new(&config.redis).await?);
            let hub = Arc::new(tokio::sync::Mutex::new(waypoint::hub::client::Hub::new(config.hub.clone())?));
            let database = Arc::new(waypoint::database::client::Database::new(&config.database).await?);
            
            // Create shared application resources
            let resources = Arc::new(waypoint::processor::AppResources::new(
                hub.clone(), 
                redis.clone(), 
                database.clone()
            ));
            
            // Execute benchmark command
            command.execute(resources).await?;
            
            Ok(())
        },
        // Just for clarity in error messages
        None => {
            println!("Please specify a backfill subcommand. Available command groups:");
            println!("  fid    - FID-based backfill operations");
            println!("  bench  - Database benchmark operations");
            Ok(())
        },
        Some((cmd, _)) => {
            println!("Unknown command group: {}. Available command groups:", cmd);
            println!("  fid    - FID-based backfill operations");
            println!("  bench  - Database benchmark operations");
            Ok(())
        },
    }
}
