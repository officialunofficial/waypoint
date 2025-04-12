pub mod queue;
pub mod user_data;
pub mod worker;

use clap::{ArgMatches, Command};
use color_eyre::eyre::Result;
use waypoint::config::Config;

/// Register FID-related commands
pub fn register_commands(app: Command) -> Command {
    app.subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(queue::register_command())
        .subcommand(worker::register_command())
        .subcommand(user_data::register_command())
}

/// Handle FID-related commands
pub async fn handle_command(matches: &ArgMatches, config: &Config) -> Result<()> {
    match matches.subcommand() {
        Some(("queue", args)) => queue::execute(config, args).await,
        Some(("worker", args)) => worker::execute(config, args).await,
        Some(("user-data", args)) => user_data::execute(config, args).await,
        // Just for clarity in error messages
        None => {
            println!("Please specify a valid FID command. Available commands:");
            println!("  queue      - Queue FIDs for backfill");
            println!("  worker     - Start FID-based backfill worker");
            println!("  user-data  - Update user_data for FIDs");
            Ok(())
        },
        Some((cmd, _)) => {
            println!("Unknown command: {}. Available commands:", cmd);
            println!("  queue      - Queue FIDs for backfill");
            println!("  worker     - Start FID-based backfill worker");
            println!("  user-data  - Update user_data for FIDs");
            Ok(())
        },
    }
}
