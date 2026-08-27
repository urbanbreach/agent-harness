use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::{inspect_single_session, SessionInspectionEntry};
use crate::cli_io::EVENTS_FILE_NAME;

pub const SESSION_HISTORY_INDEX_FILE_NAME: &str = ".session-history-index-v1.json";
const SESSION_HISTORY_INDEX_SCHEMA_VERSION: u16 = 1;
static INDEX_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct SessionHistoryIndexReport {
    pub entries: Vec<SessionInspectionEntry>,
    pub journals_scanned: usize,
    pub rebuilt: bool,
    pub recovery_reason: Option<String>,
    pub index_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionHistoryIndex {
    schema_version: u16,
    entries: BTreeMap<PathBuf, IndexedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexedSession {
    fingerprint: JournalFingerprint,
    entry: SessionInspectionEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct JournalFingerprint {
    bytes: u64,
    modified_unix_nanos: u128,
}

pub fn inspect_session_catalog_indexed(
    session_dir: &Path,
) -> Result<SessionHistoryIndexReport, String> {
    update_index(session_dir, false)
}

pub fn rebuild_session_catalog_index(
    session_dir: &Path,
) -> Result<SessionHistoryIndexReport, String> {
    update_index(session_dir, true)
}

fn update_index(
    session_dir: &Path,
    force_rebuild: bool,
) -> Result<SessionHistoryIndexReport, String> {
    let index_path = session_dir.join(SESSION_HISTORY_INDEX_FILE_NAME);
    let (loaded, load_reason) = load_index(&index_path)?;
    let rebuilt = force_rebuild || loaded.is_none();
    let mut recovery_reason = if force_rebuild {
        load_reason.or_else(|| Some("explicit".to_string()))
    } else {
        load_reason
    };
    let mut index = if force_rebuild {
        empty_index()
    } else {
        loaded.unwrap_or_else(empty_index)
    };
    let journals = journal_fingerprints(session_dir)?;
    let retained_count = index.entries.len();
    index
        .entries
        .retain(|run_dir, _| journals.contains_key(run_dir));
    let removed = retained_count != index.entries.len();

    let mut journals_scanned = 0;
    for (run_dir, fingerprint) in journals {
        let unchanged = index.entries.get(&run_dir).is_some_and(|indexed| {
            indexed.fingerprint == fingerprint && indexed.entry.run_dir == run_dir
        });
        if unchanged {
            continue;
        }
        journals_scanned += 1;
        index.entries.insert(
            run_dir.clone(),
            IndexedSession {
                fingerprint,
                entry: inspect_single_session(&run_dir).normalize_lineage(),
            },
        );
    }

    if !rebuilt && (removed || journals_scanned > 0) {
        recovery_reason = Some("stale".to_string());
    }
    if rebuilt || removed || journals_scanned > 0 {
        write_index(&index_path, &index)?;
    }
    let mut entries = index
        .entries
        .into_values()
        .map(|indexed| indexed.entry)
        .collect::<Vec<_>>();
    SessionInspectionEntry::sort_by_updated_desc(&mut entries);
    Ok(SessionHistoryIndexReport {
        entries,
        journals_scanned,
        rebuilt,
        recovery_reason,
        index_path,
    })
}

fn empty_index() -> SessionHistoryIndex {
    SessionHistoryIndex {
        schema_version: SESSION_HISTORY_INDEX_SCHEMA_VERSION,
        entries: BTreeMap::new(),
    }
}

fn load_index(path: &Path) -> Result<(Option<SessionHistoryIndex>, Option<String>), String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((None, Some("missing".to_string())))
        }
        Err(error) => {
            return Err(format!(
                "failed to read history index {}: {error}",
                path.display()
            ))
        }
    };
    match serde_json::from_str::<SessionHistoryIndex>(&body) {
        Ok(index) if index.schema_version == SESSION_HISTORY_INDEX_SCHEMA_VERSION => {
            Ok((Some(index), None))
        }
        Ok(_) => Ok((None, Some("unsupported_version".to_string()))),
        Err(error) if error.is_eof() => Ok((None, Some("truncated".to_string()))),
        Err(_) => Ok((None, Some("corrupt".to_string()))),
    }
}

fn journal_fingerprints(
    session_dir: &Path,
) -> Result<BTreeMap<PathBuf, JournalFingerprint>, String> {
    let read_dir = fs::read_dir(session_dir).map_err(|error| {
        format!(
            "failed to read session directory {}: {error}",
            session_dir.display()
        )
    })?;
    let mut journals = BTreeMap::new();
    for entry in read_dir.flatten() {
        let run_dir = entry.path();
        let events_path = run_dir.join(EVENTS_FILE_NAME);
        if !run_dir.is_dir() || !events_path.is_file() {
            continue;
        }
        let metadata = events_path.metadata().map_err(|error| {
            format!(
                "failed to inspect journal {}: {error}",
                events_path.display()
            )
        })?;
        journals.insert(
            run_dir,
            JournalFingerprint {
                bytes: metadata.len(),
                modified_unix_nanos: modified_unix_nanos(metadata.modified().ok()),
            },
        );
    }
    Ok(journals)
}

fn modified_unix_nanos(value: Option<SystemTime>) -> u128 {
    value
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn write_index(path: &Path, index: &SessionHistoryIndex) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(index)
        .map_err(|error| format!("failed to serialize history index: {error}"))?;
    let temp_path = unique_temp_path(path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut temp_file = options.open(&temp_path).map_err(|error| {
        format!(
            "failed to create history index {}: {error}",
            temp_path.display()
        )
    })?;
    temp_file.write_all(&body).map_err(|error| {
        format!(
            "failed to write history index {}: {error}",
            temp_path.display()
        )
    })?;
    temp_file.sync_all().map_err(|error| {
        format!(
            "failed to sync history index {}: {error}",
            temp_path.display()
        )
    })?;
    drop(temp_file);
    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "failed to install history index {}: {error}",
            path.display()
        )
    })?;
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            format!(
                "failed to sync history index directory {}: {error}",
                parent.display()
            )
        })
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let counter = INDEX_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(SESSION_HISTORY_INDEX_FILE_NAME);
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}
