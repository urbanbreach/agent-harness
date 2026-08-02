use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::event::{EventEnvelopeV1, EventV1, SCHEMA_VERSION};
use crate::ids::RunId;
use crate::proj::SessionModeSource;
use crate::session_paths::{EVENTS_FILE_NAME, META_FILE_NAME};

use super::discover::classify_candidate;
use super::{
    ForeignImportResult, ForeignSessionCandidate, ForeignSessionError, SUPPORTED_IMPORT_FORMAT,
    SUPPORTED_IMPORT_MARKER,
};

const IMPORT_RUN_ID_PREFIX: &str = "run_import";
static IMPORT_RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Import a foreign session directory as a **new** harness replay-only session.
///
/// Supported format only: `events.jsonl` with harness-compatible event envelopes.
/// Writes are append-only into a newly created run directory; the foreign path is
/// never mutated.
pub fn import_foreign_session_as_replay(
    foreign_path: &Path,
    dest_session_dir: &Path,
) -> Result<ForeignImportResult, ForeignSessionError> {
    if !foreign_path.is_dir() {
        return Err(ForeignSessionError::SourceNotDirectory {
            path: foreign_path.display().to_string(),
        });
    }
    if dest_session_dir.exists() && !dest_session_dir.is_dir() {
        return Err(ForeignSessionError::DestinationNotDirectory {
            path: dest_session_dir.display().to_string(),
        });
    }

    if paths_equal(foreign_path, dest_session_dir) {
        return Err(ForeignSessionError::ImportIntoActiveForbidden {
            active_session: dest_session_dir.display().to_string(),
        });
    }

    let candidate = classify_candidate(foreign_path);
    let marker = match &candidate {
        ForeignSessionCandidate::Discoverable { marker, .. } => marker.as_str(),
        ForeignSessionCandidate::Corrupt { reason, .. } => {
            return Err(ForeignSessionError::UnsupportedFormat {
                path: foreign_path.display().to_string(),
                reason: reason.clone(),
            });
        }
        ForeignSessionCandidate::Rejected { reason, .. } => {
            return Err(ForeignSessionError::UnsupportedFormat {
                path: foreign_path.display().to_string(),
                reason: reason.clone(),
            });
        }
    };
    if marker != SUPPORTED_IMPORT_MARKER {
        return Err(ForeignSessionError::UnsupportedFormat {
            path: foreign_path.display().to_string(),
            reason: format!(
                "marker `{marker}` is not importable; only `{SUPPORTED_IMPORT_MARKER}` is supported"
            ),
        });
    }

    let events_path = foreign_path.join(EVENTS_FILE_NAME);
    let source_events = parse_events_jsonl(&events_path)?;
    if source_events.is_empty() {
        return Err(ForeignSessionError::EmptySource {
            path: events_path.display().to_string(),
        });
    }

    fs::create_dir_all(dest_session_dir).map_err(|err| ForeignSessionError::DestinationWrite {
        path: dest_session_dir.display().to_string(),
        message: err.to_string(),
    })?;

    let run_id = next_import_run_id();
    let run_dir = dest_session_dir.join(&run_id);
    if run_dir.exists() {
        return Err(ForeignSessionError::DestinationWrite {
            path: run_dir.display().to_string(),
            message: "run directory already exists".to_string(),
        });
    }
    fs::create_dir_all(&run_dir).map_err(|err| ForeignSessionError::DestinationWrite {
        path: run_dir.display().to_string(),
        message: err.to_string(),
    })?;

    let source_run_id = source_events.first().map(|event| event.run_id.to_string());
    let rewritten = rewrite_import_events(&source_events, source_run_id.as_deref(), &run_id);
    write_events_jsonl(&run_dir, &rewritten)?;
    write_import_meta(
        &run_dir,
        &run_id,
        foreign_path,
        source_run_id.as_deref(),
        rewritten.len(),
    )?;

    Ok(ForeignImportResult {
        run_id,
        run_dir,
        event_count: rewritten.len(),
        source_path: foreign_path.to_path_buf(),
        format: SUPPORTED_IMPORT_FORMAT.to_string(),
        mode_source: SessionModeSource::ReplayOnly,
    })
}

fn parse_events_jsonl(path: &Path) -> Result<Vec<EventEnvelopeV1>, ForeignSessionError> {
    let text = fs::read_to_string(path).map_err(|err| ForeignSessionError::SourceRead {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    let mut events = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: EventEnvelopeV1 =
            serde_json::from_str(trimmed).map_err(|err| ForeignSessionError::SourceParse {
                path: path.display().to_string(),
                line: idx + 1,
                message: format!(
                    "{err}; expected harness event envelope (schema_version/event_id/seq/run_id/payload)"
                ),
            })?;
        if event.schema_version != SCHEMA_VERSION {
            return Err(ForeignSessionError::SourceParse {
                path: path.display().to_string(),
                line: idx + 1,
                message: format!(
                    "unsupported schema_version {}; only {SCHEMA_VERSION} is importable",
                    event.schema_version
                ),
            });
        }
        events.push(event);
    }
    Ok(events)
}

fn rewrite_import_events(
    source: &[EventEnvelopeV1],
    source_run_id: Option<&str>,
    dest_run_id: &str,
) -> Vec<EventEnvelopeV1> {
    source
        .iter()
        .enumerate()
        .map(|(idx, event)| {
            let seq = (idx as u64).saturating_add(1);
            let mut rewritten = event.clone();
            rewritten.event_id = format!("evt-import-{dest_run_id}-{seq:020}");
            rewritten.seq = seq;
            rewritten.run_id = RunId::from(dest_run_id);
            rewritten.correlation_id = None;
            rewritten.causation_id = None;
            rewritten.stream_key =
                rewrite_stream_key(event.stream_key.as_deref(), source_run_id, dest_run_id);
            if let EventV1::RunStarted(data) = &mut rewritten.payload {
                data.run_name = "replay".into();
            }
            rewritten
        })
        .collect()
}

fn rewrite_stream_key(
    stream_key: Option<&str>,
    source_run_id: Option<&str>,
    dest_run_id: &str,
) -> Option<String> {
    let key = stream_key?;
    let Some(source_run_id) = source_run_id else {
        return Some(key.to_string());
    };
    Some(key.replace(source_run_id, dest_run_id))
}

fn write_events_jsonl(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
) -> Result<(), ForeignSessionError> {
    let path = run_dir.join(EVENTS_FILE_NAME);
    let mut body = String::new();
    for event in events {
        let line =
            serde_json::to_string(event).map_err(|err| ForeignSessionError::DestinationWrite {
                path: path.display().to_string(),
                message: err.to_string(),
            })?;
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(&path, body).map_err(|err| ForeignSessionError::DestinationWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn write_import_meta(
    run_dir: &Path,
    run_id: &str,
    source_path: &Path,
    source_run_id: Option<&str>,
    event_count: usize,
) -> Result<(), ForeignSessionError> {
    let path = run_dir.join(META_FILE_NAME);
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let meta = serde_json::json!({
        "run_id": run_id,
        "run_name": "replay",
        "workspace_root": "",
        "created_at": created_at,
        "config_digest": "foreign-import-events-jsonl-v1",
        "harness_version": env!("CARGO_PKG_VERSION"),
        "mode_source": "replay_only",
        "foreign_import": {
            "format": SUPPORTED_IMPORT_FORMAT,
            "source_path": source_path.display().to_string(),
            "source_run_id": source_run_id,
            "event_count": event_count,
            "policy": "read-only replay import; append-only new events.jsonl; source path never mutated"
        }
    });
    let body = serde_json::to_string_pretty(&meta).map_err(|err| {
        ForeignSessionError::DestinationWrite {
            path: path.display().to_string(),
            message: err.to_string(),
        }
    })?;
    fs::write(&path, format!("{body}\n")).map_err(|err| ForeignSessionError::DestinationWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn next_import_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = IMPORT_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{IMPORT_RUN_ID_PREFIX}_{nanos:x}_{counter:04}")
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}
