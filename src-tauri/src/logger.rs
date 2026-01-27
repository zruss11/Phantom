//! Lightweight logging module with daily rotating log files.
//!
//! Logs are stored at `~/.config/phantom-harness/logs/` with 7-day retention.
//! Uses the `tracing` ecosystem for structured logging.

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Guard that keeps the non-blocking writer alive.
/// Must be held for the lifetime of the application.
pub struct LogGuard {
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Initialize the logging system with file and stdout output.
///
/// # Returns
/// A `LogGuard` that must be kept alive for logging to work.
///
/// # Errors
/// Returns an error if the log directory cannot be created or configured.
pub fn init_logging() -> Result<LogGuard, Box<dyn std::error::Error>> {
    let log_dir = dirs::config_dir()
        .ok_or("Could not find config directory")?
        .join("phantom-harness")
        .join("logs");

    std::fs::create_dir_all(&log_dir)?;

    // Create rolling file appender with daily rotation, keeping 7 days of logs
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("phantom-harness")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Use RUST_LOG env var if set, otherwise default to info level
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Initialize with both file and stdout output
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_target(true),
        )
        .init();

    tracing::info!(
        log_path = %log_dir.display(),
        "Logging initialized with daily rotation (7-day retention)"
    );

    Ok(LogGuard { _guard: guard })
}
