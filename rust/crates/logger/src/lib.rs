use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter,
};

/// Directory logs are written to: `$CLAW_LOG_DIR` when set, otherwise
/// `~/.claw/logs`. The override exists so test runs and `cargo run` do not
/// interleave with the installed agent's log file.
#[must_use]
pub fn log_dir() -> Option<std::path::PathBuf> {
    resolve_log_dir(
        std::env::var_os("CLAW_LOG_DIR").as_deref(),
        dirs::home_dir(),
    )
}

fn resolve_log_dir(
    override_dir: Option<&std::ffi::OsStr>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if let Some(dir) = override_dir {
        if !dir.is_empty() {
            return Some(std::path::PathBuf::from(dir));
        }
    }
    Some(home?.join(".claw").join("logs"))
}

/// Initialize a daily rotating file logger.
///
/// Logs will be written to `<log_dir>/<app_name>.log.YYYY-MM-DD`, where
/// `log_dir` is [`log_dir`].
/// Returns a `WorkerGuard`. This guard must be kept alive for the duration
/// of the application, otherwise background log writing may be aborted.
pub fn init_logger(app_name: &str) -> Option<WorkerGuard> {
    let log_dir = log_dir()?;

    // Ensure the directory exists
    let _ = std::fs::create_dir_all(&log_dir);

    // Create a daily rolling appender that keeps the last 5 days of logs
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("{}.log", app_name))
        .max_log_files(5) // Automatically delete logs older than 5 days
        .build(&log_dir)
        .expect("Failed to initialize rolling file appender");

    // Make it non-blocking
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // We only log to file by default, to avoid messing up CLI/TUI stdout,
    // but we can respect RUST_LOG for filtering (defaulting to info).
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

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

#[cfg(test)]
mod tests {
    use super::resolve_log_dir;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    #[test]
    fn override_wins_over_home() {
        let home = Some(PathBuf::from("/home/dev"));
        assert_eq!(
            resolve_log_dir(Some(OsStr::new("/tmp/claw-logs")), home),
            Some(PathBuf::from("/tmp/claw-logs"))
        );
    }

    #[test]
    fn empty_override_falls_back_to_home() {
        let home = Some(PathBuf::from("/home/dev"));
        assert_eq!(
            resolve_log_dir(Some(OsStr::new("")), home.clone()),
            Some(PathBuf::from("/home/dev/.claw/logs"))
        );
        assert_eq!(
            resolve_log_dir(None, home),
            Some(PathBuf::from("/home/dev/.claw/logs"))
        );
    }

    #[test]
    fn no_home_and_no_override_disables_file_logging() {
        assert_eq!(resolve_log_dir(None, None), None);
    }
}
