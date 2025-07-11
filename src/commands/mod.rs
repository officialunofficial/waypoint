pub mod backfill;
pub mod mcp;

use clap::Command;
use color_eyre::eyre::Result;
use waypoint::config::Config;

/// Register all application commands
pub fn register_commands(app: Command) -> Command {
    // Register service and backfill commands
    app.subcommand(Command::new("start").about("Start the service"))
        .subcommand(backfill::register_commands(Command::new("backfill")))
        .subcommand(mcp::register_commands(Command::new("mcp")))
}

/// Handle all application commands
pub async fn handle_commands(matches: clap::ArgMatches, config: &Config) -> Result<()> {
    match matches.subcommand() {
        Some(("start", _)) => crate::service::run_service(config).await,
        Some(("backfill", backfill_matches)) => {
            backfill::handle_command(backfill_matches, config).await
        },
        Some(("mcp", mcp_matches)) => mcp::handle_command(mcp_matches, config).await,
        _ => {
            println!("Please specify a subcommand. Use --help for more information.");
            Ok(())
        },
    }
}
