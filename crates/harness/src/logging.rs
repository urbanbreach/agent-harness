#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(
        clippy::cognitive_complexity,
        reason = "tracing macros expand into branches; this test covers one logging lifecycle"
    )]
    fn logs_filter_redact_and_switch_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let mut config = harness_core::config::load_config_from_str("{}")?;
        let first = init_logging(&config, temp.path())?;
        tracing::info!(api_key = "opaque-log-secret", "first message");
        tracing::debug!("hidden debug");
        tracing::info!(target: "rmcp::service", payload = "opaque-transport-payload", "received notification");
        let text = std::fs::read_to_string(&first)?;
        assert!(text.contains("first message") && text.contains("REDACTED"));
        assert!(!text.contains("opaque-log-secret") && !text.contains("hidden debug"));
        assert!(!text.contains("opaque-transport-payload"));
        config.logging.file = Some(temp.path().join("second.log"));
        config.logging.level = "debug".into();
        let second = init_logging(&config, temp.path())?;
        tracing::debug!("second message");
        tracing::info!("oversized sentinel {}", "x".repeat(17 * 1024));
        assert_eq!(std::fs::read_to_string(&first)?, text);
        let text = std::fs::read_to_string(&second)?;
        assert!(text.contains("second message") && text.contains("entry exceeded"));
        assert!(!text.contains("oversized sentinel"));
        #[cfg(unix)]
        {
            let link = temp.path().join("linked.log");
            std::os::unix::fs::symlink(&first, &link)?;
            config.logging.file = Some(link);
            assert!(init_logging(&config, temp.path()).is_err());
        }
        Ok(())
    }
}
use harness_core::{
    config::HarnessConfig,
    redact::{DefaultRedactor, Redactor},
};
use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{filter::filter_fn, prelude::*};

const MAX_ENTRY: usize = 16 * 1024;
const MAX_LOG: u64 = 16 * 1024 * 1024;
struct Log {
    path: Option<PathBuf>,
    file: Option<File>,
    explicit: bool,
    level: LevelFilter,
    bytes: u64,
}
static LOG: Mutex<Option<Log>> = Mutex::new(None);
static SUBSCRIBER: OnceLock<Result<(), String>> = OnceLock::new();

pub fn init_logging(config: &HarnessConfig, run_dir: &Path) -> Result<PathBuf, String> {
    let path = config
        .logging
        .file
        .clone()
        .unwrap_or_else(|| run_dir.join("harness.log"));
    let level = config
        .logging
        .level
        .parse::<LevelFilter>()
        .map_err(|e| e.to_string())?;
    configure(Some(path.clone()), level, false)?;
    tracing::info!("session logging started");
    Ok(path)
}

pub fn init_debug_logging(debug: bool, path: Option<&Path>) -> Result<(), String> {
    configure(
        path.map(Path::to_owned),
        if debug {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        },
        true,
    )?;
    tracing::debug!("debug logging enabled");
    Ok(())
}

fn configure(path: Option<PathBuf>, level: LevelFilter, explicit: bool) -> Result<(), String> {
    SUBSCRIBER
        .get_or_init(|| {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(Record::default)
                        .with_filter(filter_fn(|meta| {
                            // SDK traces can contain complete remote payloads. Tool failures are recorded by the coordinator.
                            if meta.target() == "rmcp" || meta.target().starts_with("rmcp::") {
                                return false;
                            }
                            LOG.lock().is_ok_and(|state| {
                                state.as_ref().is_some_and(|log| *meta.level() <= log.level)
                            })
                        })),
                )
                .try_init()
                .map_err(|e| e.to_string())
        })
        .clone()?;
    let mut state = LOG.lock().map_err(|_| "log lock poisoned")?;
    if !explicit && state.as_ref().is_some_and(|log| log.explicit) {
        return Ok(());
    }
    if let Some(log) = state.as_mut().filter(|log| log.path == path) {
        log.level = level;
        log.explicit = explicit;
    } else {
        let file = path
            .as_ref()
            .map(|path| {
                let file =
                    harness_core::store::open_private_append(path).map_err(|e| e.to_string())?;
                file.try_lock().map_err(|e| e.to_string())?;
                Ok::<_, String>(file)
            })
            .transpose()?;
        let bytes = file
            .as_ref()
            .map(|file| file.metadata().map(|m| m.len()))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        *state = Some(Log {
            path,
            file,
            explicit,
            level,
            bytes,
        });
    }
    Ok(())
}
#[derive(Default)]
struct Record {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl Write for Record {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.exceeded && self.bytes.len().saturating_add(bytes.len()) <= MAX_ENTRY {
            self.bytes.extend_from_slice(bytes);
        } else {
            self.exceeded = true;
            self.bytes.clear();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for Record {
    fn drop(&mut self) {
        let text = if self.exceeded {
            "log entry exceeded 16 KiB; omitted\n".into()
        } else {
            DefaultRedactor::default().redact_text(&String::from_utf8_lossy(&self.bytes))
        };
        if let Ok(mut state) = LOG.lock() {
            if let Some(log) = state.as_mut() {
                // ponytail: synchronous low-volume diagnostics; use a bounded worker if profiling shows log I/O stalls.
                if log.bytes.saturating_add(text.len() as u64) <= MAX_LOG
                    && match &mut log.file {
                        Some(file) => file.write_all(text.as_bytes()),
                        None => io::stderr().write_all(text.as_bytes()),
                    }
                    .is_ok()
                {
                    log.bytes += text.len() as u64;
                }
            }
        }
    }
}
