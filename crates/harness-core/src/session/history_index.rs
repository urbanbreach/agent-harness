use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::event::EventEnvelopeV1;
use crate::fslock::FileLock;
use crate::proj::SessionCatalogEntry;

mod reducer;

pub use reducer::SessionHistoryRowReducer;

pub const SESSION_HISTORY_INDEX_FILE_NAME: &str = ".session-history-index-v1.json";
pub const SESSION_HISTORY_INDEX_SCHEMA_VERSION: u16 = 1;
const LOCK_FILE_NAME: &str = ".session-history-index-v1.lock";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryIndex {
    pub schema_version: u16,
    pub entries: BTreeMap<PathBuf, IndexedSessionHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedSessionHistoryEntry {
    pub fingerprint: JournalFingerprint,
    pub entry: SessionHistoryEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalFingerprint {
    pub bytes: u64,
    pub modified_unix_nanos: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHistoryEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
    pub sort_unix_ms: u128,
    pub artifact_count: usize,
    pub child_session_count: usize,
}

impl SessionHistoryIndex {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: SESSION_HISTORY_INDEX_SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

pub fn persist_committed_history_row(
    session_dir: &Path,
    events_path: &Path,
    reducer: &mut SessionHistoryRowReducer,
    appended: &EventEnvelopeV1,
) -> Result<(), String> {
    reducer.apply_event(appended);
    let fingerprint = journal_fingerprint(events_path)?;
    let sort_unix_ms = fingerprint.modified_unix_nanos / 1_000_000;
    reducer.entry.sort_unix_ms = sort_unix_ms;
    reducer.entry.catalog.last_updated_at = appended.ts.clone();
    let _lock = acquire_history_index_lock(session_dir)?;
    let index_path = session_dir.join(SESSION_HISTORY_INDEX_FILE_NAME);
    let mut index = read_valid_index(&index_path).unwrap_or_else(SessionHistoryIndex::empty);
    index.entries.insert(
        reducer.entry.run_dir.clone(),
        IndexedSessionHistoryEntry {
            fingerprint,
            entry: reducer.entry.clone(),
        },
    );
    write_history_index(&index_path, &index)
}

pub fn acquire_history_index_lock(session_dir: &Path) -> Result<impl Drop, String> {
    FileLock::acquire(session_dir.join(LOCK_FILE_NAME)).map_err(|error| {
        format!(
            "failed to lock session history index {}: {error}",
            session_dir.display()
        )
    })
}

#[must_use]
pub fn read_valid_index(path: &Path) -> Option<SessionHistoryIndex> {
    let body = fs::read_to_string(path).ok()?;
    let index = serde_json::from_str::<SessionHistoryIndex>(&body).ok()?;
    (index.schema_version == SESSION_HISTORY_INDEX_SCHEMA_VERSION).then_some(index)
}

pub fn write_history_index(path: &Path, index: &SessionHistoryIndex) -> Result<(), String> {
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
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                format!(
                    "failed to sync history index directory {}: {error}",
                    parent.display()
                )
            })?;
    }
    Ok(())
}

fn journal_fingerprint(path: &Path) -> Result<JournalFingerprint, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("failed to inspect journal {}: {error}", path.display()))?;
    Ok(JournalFingerprint {
        bytes: metadata.len(),
        modified_unix_nanos: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos()),
    })
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        ".{SESSION_HISTORY_INDEX_FILE_NAME}.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}
