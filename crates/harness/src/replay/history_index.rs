use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::session::history_index::{
    acquire_history_index_lock, write_history_index, IndexedSessionHistoryEntry,
    JournalFingerprint, SessionHistoryEntry, SessionHistoryIndex,
    SESSION_HISTORY_INDEX_SCHEMA_VERSION,
};

pub use harness_core::session::history_index::SESSION_HISTORY_INDEX_FILE_NAME;

use super::{inspect_single_session, SessionInspectionEntry};
use crate::cli_io::EVENTS_FILE_NAME;

#[derive(Debug, Clone)]
pub struct SessionHistoryIndexReport {
    pub entries: Vec<SessionInspectionEntry>,
    pub journals_scanned: usize,
    pub journals_opened: usize,
    pub rebuilt: bool,
    pub recovery_reason: Option<String>,
    pub index_path: PathBuf,
}

trait JournalSource {
    fn open_session(&mut self, run_dir: &Path) -> SessionInspectionEntry;
    fn opened(&self) -> usize;
}

#[derive(Debug, Default)]
struct FileJournalSource {
    opened: usize,
}

impl JournalSource for FileJournalSource {
    fn open_session(&mut self, run_dir: &Path) -> SessionInspectionEntry {
        self.opened += 1;
        inspect_single_session(run_dir)
    }

    fn opened(&self) -> usize {
        self.opened
    }
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
    let mut source = FileJournalSource::default();
    update_index_with_source(session_dir, force_rebuild, &mut source)
}

fn update_index_with_source(
    session_dir: &Path,
    force_rebuild: bool,
    source: &mut dyn JournalSource,
) -> Result<SessionHistoryIndexReport, String> {
    fs::read_dir(session_dir).map_err(|error| {
        format!(
            "failed to read session directory {}: {error}",
            session_dir.display()
        )
    })?;
    let _lock = acquire_history_index_lock(session_dir)?;
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
            IndexedSessionHistoryEntry {
                fingerprint,
                entry: source.open_session(&run_dir).normalize_lineage().into(),
            },
        );
    }

    if !rebuilt && (removed || journals_scanned > 0) {
        recovery_reason = Some("stale".to_string());
    }
    if rebuilt || removed || journals_scanned > 0 {
        write_history_index(&index_path, &index)?;
    }
    let mut entries = index
        .entries
        .into_values()
        .map(|indexed| indexed.entry.into())
        .collect::<Vec<_>>();
    SessionInspectionEntry::sort_by_updated_desc(&mut entries);
    Ok(SessionHistoryIndexReport {
        entries,
        journals_scanned,
        journals_opened: source.opened(),
        rebuilt,
        recovery_reason,
        index_path,
    })
}

fn empty_index() -> SessionHistoryIndex {
    SessionHistoryIndex::empty()
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

impl From<SessionInspectionEntry> for SessionHistoryEntry {
    fn from(entry: SessionInspectionEntry) -> Self {
        Self {
            run_dir: entry.run_dir,
            catalog: entry.catalog,
            sort_unix_ms: entry.sort_unix_ms,
            artifact_count: entry.artifact_count,
            child_session_count: entry.child_session_count,
        }
    }
}

impl From<SessionHistoryEntry> for SessionInspectionEntry {
    fn from(entry: SessionHistoryEntry) -> Self {
        Self {
            run_dir: entry.run_dir,
            catalog: entry.catalog,
            sort_unix_ms: entry.sort_unix_ms,
            artifact_count: entry.artifact_count,
            child_session_count: entry.child_session_count,
        }
    }
}
