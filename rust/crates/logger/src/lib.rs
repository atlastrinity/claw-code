
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter,
};

/// Initialize a daily rotating file logger.
///
/// Logs will be written to `~/.claw/logs/<app_name>.log.YYYY-MM-DD`.
/// Returns a `WorkerGuard`. This guard must be kept alive for the duration
/// of the application, otherwise background log writing may be aborted.
pub fn init_logger(app_name: &str) -> Option<WorkerGuard> {
    let home = dirs::home_dir()?;
    let log_dir = home.join(".claw").join("logs");

    // Ensure the directory exists
    let _ = std::fs::create_dir_all(&log_dir);

    // Create a daily rolling appender
    let file_appender = tracing_appender::rolling::daily(log_dir, format!("{}.log", app_name));

    // Make it non-blocking
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // We only log to file by default, to avoid messing up CLI/TUI stdout, 
    // but we can respect RUST_LOG for filtering (defaulting to info).
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // We'll use a JSON format or a clean text format. A clean text format without ANSI
    // is easiest for humans, but let's use a standard format with targets and spans.
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .with_thread_ids(true);

    // Try to set the global default subscriber. We ignore the error if it was already set.
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .try_init();

    Some(guard)
}
