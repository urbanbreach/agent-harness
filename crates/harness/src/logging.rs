use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

use harness_core::config::HarnessConfig;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn default_log_path(run_dir: &Path) -> PathBuf {
    run_dir.join("logs").join("harness.log")
}

pub fn init_logging(cfg: &HarnessConfig, run_dir: &Path) -> Result<PathBuf, String> {
    let log_path = cfg
        .logging
        .file
        .clone()
        .unwrap_or_else(|| default_log_path(run_dir));

    if let Some(existing) = LOG_PATH.get() {
        return Ok(existing.clone());
    }

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create log directory {}: {err}", parent.display()))?;
    }

    let level = LevelFilter::from_str(&cfg.logging.level)
        .map_err(|err| format!("invalid logging.level `{}`: {err}", cfg.logging.level))?;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("failed to open log file {}: {err}", log_path.display()))?;

    let (writer, guard) = tracing_appender::non_blocking(file);
    let _ = LOG_GUARD.set(guard);

    let subscriber = tracing_subscriber::registry().with(level).with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(writer),
    );

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;

    let _ = LOG_PATH.set(log_path.clone());
    tracing::info!(log_path = %log_path.display(), "initialized harness file logging");

    Ok(log_path)
}
