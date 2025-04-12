pub mod fid;

use clap::{ArgMatches, Command};
use color_eyre::eyre::Result;
use waypoint::config::Config;

/// Register all backfill-related subcommands
pub fn register_commands(app: Command) -> Command {
    app.about("Backfill data from Snapchain")
        .subcommand_required(true)
        .arg_required_else_help(true)
        // FID-based backfill commands
        .subcommand(fid::register_commands(Command::new("fid")
            .about("FID-based backfill operations")))
}

/// Handle backfill commands based on matches
pub async fn handle_command(matches: &ArgMatches, config: &Config) -> Result<()> {
    match matches.subcommand() {
        Some(("fid", submatches)) => fid::handle_command(submatches, config).await,
        // Just for clarity in error messages
        None => {
            println!("Please specify a backfill subcommand. Available command groups:");
            println!("  fid    - FID-based backfill operations");
            Ok(())
        },
        Some((cmd, _)) => {
            println!("Unknown command group: {}. Available command groups:", cmd);
            println!("  fid    - FID-based backfill operations");
            Ok(())
        }
    }
}