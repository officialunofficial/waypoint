//! Global error types and error handling utilities

use thiserror::Error;

/// Main error type for the application
#[derive(Error, Debug)]
pub enum WaypointError {
    #[error("Configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    #[error("Database error: {0}")]
    Database(#[from] crate::database::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] crate::redis::error::Error),

    #[error("Hub error: {0}")]
    Hub(#[from] crate::hub::error::Error),

    #[error("Processing error: {0}")]
    Processing(String),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Result type alias for Waypoint operations
pub type Result<T> = std::result::Result<T, WaypointError>;

/// Initialize error handling for the application
pub fn install_error_handlers() -> color_eyre::Result<()> {
    // Setup color-eyre for pretty error reporting
    color_eyre::install()?;

    // Set custom panic hook
    std::panic::set_hook(Box::new(|panic_info| {
        // Log the panic with tracing
        if let Some(location) = panic_info.location() {
            tracing::error!(
                message = %panic_info,
                panic.file = location.file(),
                panic.line = location.line(),
                panic.column = location.column(),
                "Application panic"
            );
        } else {
            tracing::error!(message = %panic_info, "Application panic");
        }

        // If in test environment, don't print the panic (test frameworks handle this)
        // Use var_os to avoid the clippy lint for std::env::var
        if std::env::var_os("RUST_TEST").is_some() {
            return;
        }

        // Print the panic with color-eyre
        eprintln!("💥 The application panicked! This is a bug and should be reported.");

        if let Some(location) = panic_info.location() {
            eprintln!(
                "Panic occurred at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("Panic message: {}", s);
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            eprintln!("Panic message: {}", s);
        }

        eprintln!("Stack trace:");
        let backtrace = std::backtrace::Backtrace::force_capture();
        eprintln!("{}", backtrace);
    }));

    Ok(())
}

/// Convert any error into a WaypointError
pub trait IntoWaypointError<T> {
    /// Convert this error into a WaypointError
    fn into_waypoint_err(self, context: &str) -> Result<T>;
}

impl<T, E: std::fmt::Display> IntoWaypointError<T> for std::result::Result<T, E> {
    fn into_waypoint_err(self, context: &str) -> Result<T> {
        self.map_err(|e| WaypointError::Unknown(format!("{}: {}", context, e)))
    }
}
