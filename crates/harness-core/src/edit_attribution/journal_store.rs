//! Append/load helpers for the edit-attribution JSONL journal.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::journal::EditAttributionError;
use super::{normalize_path, AttributedEdit, EditAttributionTracker, EditSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum JournalKind {
    AgentTool,
    External,
    Drift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct JournalRecord {
    pub v: u32,
    pub seq: u64,
    pub path: String,
    pub source: EditSource,
    pub kind: JournalKind,
    pub content_sha256: String,
    pub mtime_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_snapshot_hex: Option<String>,
    pub ts_unix_ms: u64,
}

pub(super) fn normalize_workspace_relative(
    workspace_root: &Path,
    path: &Path,
) -> Result<String, EditAttributionError> {
    let key = if path.is_absolute() {
        path.strip_prefix(workspace_root)
            .map(normalize_path)
            .unwrap_or_else(|_| normalize_path(path))
    } else {
        normalize_path(path)
    };
    if key.is_empty() || key.split('/').any(|seg| seg == "..") {
        return Err(EditAttributionError::InvalidPath { path: key });
    }
    Ok(key)
}

pub(super) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(super) fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex(bytes[i])?;
        let lo = from_hex(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(super) fn append_record(
    journal_path: &Path,
    record: &JournalRecord,
) -> Result<(), EditAttributionError> {
    if let Some(parent) = journal_path.parent() {
        fs::create_dir_all(parent).map_err(|source| EditAttributionError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let line = serde_json::to_string(record).map_err(|err| EditAttributionError::Write {
        path: journal_path.display().to_string(),
        source: io::Error::other(err),
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path)
        .map_err(|source| EditAttributionError::Write {
            path: journal_path.display().to_string(),
            source,
        })?;
    writeln!(file, "{line}").map_err(|source| EditAttributionError::Write {
        path: journal_path.display().to_string(),
        source,
    })?;
    file.sync_all()
        .map_err(|source| EditAttributionError::Write {
            path: journal_path.display().to_string(),
            source,
        })?;
    Ok(())
}

pub(super) fn load_records(
    journal_path: &Path,
) -> Result<Vec<JournalRecord>, EditAttributionError> {
    let raw = match fs::read_to_string(journal_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(EditAttributionError::Read {
                path: journal_path.display().to_string(),
                source: err,
            });
        }
    };
    let mut out = Vec::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record: JournalRecord =
            serde_json::from_str(line).map_err(|err| EditAttributionError::Parse {
                path: journal_path.display().to_string(),
                detail: format!("line {}: {err}", line_no + 1),
            })?;
        out.push(record);
    }
    Ok(out)
}

pub(super) fn apply_loaded_records(
    tracker: &mut EditAttributionTracker,
    agent_snapshots: &mut std::collections::BTreeMap<String, Vec<u8>>,
    next_seq: &mut u64,
    records: &[JournalRecord],
) {
    for record in records {
        *next_seq = (*next_seq).max(record.seq.saturating_add(1));
        if let Some(hex) = record.agent_snapshot_hex.as_deref() {
            if let Some(bytes) = hex_decode(hex) {
                agent_snapshots.insert(record.path.clone(), bytes);
            }
        }
        let drifted = match record.kind {
            JournalKind::AgentTool => false,
            JournalKind::Drift => true,
            JournalKind::External => tracker.is_drifted(&record.path),
        };
        tracker.apply_loaded_entry(
            AttributedEdit {
                path: record.path.clone(),
                source: record.source,
                content_sha256: record.content_sha256.clone(),
                mtime_unix_ms: record.mtime_unix_ms,
            },
            drifted,
        );
    }
}

pub(super) fn ensure_parent_for_write(path: &Path) -> Result<(), EditAttributionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EditAttributionError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    Ok(())
}

pub(super) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), EditAttributionError> {
    fs::write(path, bytes).map_err(|source| EditAttributionError::Restore {
        path: path.display().to_string(),
        source,
    })
}
