//! Global error types and error handling utilities

use error_stack::{Context, Report, ResultExt};
use std::fmt::{self, Display};

/// Main error context for the application
#[derive(Debug)]
pub struct WaypointError;

impl Display for WaypointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Waypoint operation failed")
    }
}

impl Context for WaypointError {}

/// Specific error contexts for different subsystems
#[derive(Debug)]
pub enum ErrorContext {
    Config,
    Database,
    Redis,
    Hub,
    Processing,
    IO,
    Serialization,
    Network,
    Unknown,
}

impl Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorContext::Config => write!(f, "Configuration error"),
            ErrorContext::Database => write!(f, "Database error"),
            ErrorContext::Redis => write!(f, "Redis error"),
            ErrorContext::Hub => write!(f, "Hub error"),
            ErrorContext::Processing => write!(f, "Processing error"),
            ErrorContext::IO => write!(f, "IO error"),
            ErrorContext::Serialization => write!(f, "Serialization error"),
            ErrorContext::Network => write!(f, "Network error"),
            ErrorContext::Unknown => write!(f, "Unknown error"),
        }
    }
}

impl Context for ErrorContext {}

/// Result type alias for Waypoint operations using error-stack
pub type Result<T> = error_stack::Result<T, WaypointError>;

/// Initialize error handling for the application
pub fn install_error_handlers() -> color_eyre::Result<()> {
    // Setup color-eyre for compatibility with existing code
    color_eyre::install()?;

    // Setup error-stack report configuration
    error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::Emphasis);

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
        if std::env::var_os("RUST_TEST").is_some() {
            return;
        }

        // Print the panic with error-stack formatting
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

/// Extension trait for converting standard errors into error-stack reports
pub trait IntoWaypointError<T> {
    /// Convert this error into a WaypointError report
    fn into_waypoint_err(self) -> Result<T>;

    /// Convert this error into a WaypointError report with additional context
    fn into_waypoint_err_with_context(self, context: ErrorContext) -> Result<T>;
}

impl<T, E> IntoWaypointError<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn into_waypoint_err(self) -> Result<T> {
        self.map_err(|e| Report::new(WaypointError).attach_printable(e.to_string()))
    }

    fn into_waypoint_err_with_context(self, context: ErrorContext) -> Result<T> {
        self.map_err(|e| {
            Report::new(WaypointError).attach_printable(context).attach_printable(e.to_string())
        })
    }
}

/// Helper functions for working with error-stack
pub mod helpers {
    use super::*;

    /// Create a new error report with context
    pub fn error_with_context<C: Context>(
        context: C,
        message: impl Display + fmt::Debug + Send + Sync + 'static,
    ) -> Report<C> {
        Report::new(context).attach_printable(message)
    }

    /// Attach additional context to an existing error
    pub fn attach_context<T, C: Context>(
        result: error_stack::Result<T, C>,
        message: impl Display + fmt::Debug + Send + Sync + 'static,
    ) -> error_stack::Result<T, C> {
        result.attach_printable(message)
    }
}

// Re-export commonly used items
pub use helpers::*;
