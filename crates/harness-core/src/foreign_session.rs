//! Import recognized journals into a separate replay-only session. No tools are replayed.
use crate::{event::EventEnvelopeV1, proj::SessionModeSource};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
mod discover;
mod import;
pub use discover::*;
pub use import::*;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 65_536;
const MARKERS: &[&str] = &[
    "events.jsonl",
    "session.json",
    "rollout.jsonl",
    "conversation.json",
    "transcript.jsonl",
];
const FORMAT: &str = "events_jsonl_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignAgentKind {
    Codex,
    Claude,
    OpenCode,
    Unknown,
}
impl ForeignAgentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
            Self::Unknown => "unknown",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForeignSessionCandidate {
    Discoverable {
        kind: ForeignAgentKind,
        path: PathBuf,
        marker: String,
    },
    Corrupt {
        kind: ForeignAgentKind,
        path: PathBuf,
        reason: String,
    },
    Rejected {
        path: PathBuf,
        reason: String,
    },
}
impl ForeignSessionCandidate {
    pub const fn is_discoverable(&self) -> bool {
        matches!(self, Self::Discoverable { .. })
    }
    pub const fn is_corrupt(&self) -> bool {
        matches!(self, Self::Corrupt { .. })
    }
    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
    pub fn is_importable(&self) -> bool {
        matches!(self, Self::Discoverable { marker, .. } if marker == "events.jsonl")
    }
    pub fn path(&self) -> &Path {
        match self {
            Self::Discoverable { path, .. }
            | Self::Corrupt { path, .. }
            | Self::Rejected { path, .. } => path,
        }
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Discoverable { kind, marker, .. } => format!(
                "foreign session: discoverable kind={} marker={marker} importable={}",
                kind.as_str(),
                self.is_importable()
            ),
            Self::Corrupt { reason, .. } => format!("foreign session: corrupt {reason}"),
            Self::Rejected { reason, .. } => format!("foreign session: rejected {reason}"),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignDiscoverSummary {
    pub discoverable: usize,
    pub importable: usize,
    pub discoverable_not_importable: usize,
    pub corrupt: usize,
    pub rejected: usize,
    pub total: usize,
}
impl ForeignDiscoverSummary {
    pub const fn has_importable(&self) -> bool {
        self.importable > 0
    }
    pub fn one_line(&self) -> String {
        format!("foreign sessions: total={} discoverable={} importable={} unsupported={} corrupt={} rejected={}",
        self.total, self.discoverable, self.importable, self.discoverable_not_importable, self.corrupt, self.rejected)
    }
}
pub fn summarize_discover_candidates(
    candidates: &[ForeignSessionCandidate],
) -> ForeignDiscoverSummary {
    let discoverable = candidates.iter().filter(|c| c.is_discoverable()).count();
    let importable = candidates.iter().filter(|c| c.is_importable()).count();
    ForeignDiscoverSummary {
        total: candidates.len(),
        discoverable,
        importable,
        discoverable_not_importable: discoverable - importable,
        corrupt: candidates.iter().filter(|c| c.is_corrupt()).count(),
        rejected: candidates.iter().filter(|c| c.is_rejected()).count(),
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignImportResult {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub event_count: usize,
    pub source_path: PathBuf,
    pub format: String,
    pub mode_source: SessionModeSource,
}
impl ForeignImportResult {
    pub fn one_line(&self) -> String {
        format!(
            "foreign import: imported run={} events={} format={} replay_only=true",
            self.run_id, self.event_count, self.format
        )
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ForeignSessionError {
    #[error("foreign scan root is not a directory: {path}")]
    ScanRootNotDirectory { path: String },
    #[error("foreign scan failed at {path}: {message}")]
    ScanRootRead { path: String, message: String },
    #[error("foreign import cannot change an active session: {active_session}")]
    ImportIntoActiveForbidden { active_session: String },
    #[error("foreign source is not a directory: {path}")]
    SourceNotDirectory { path: String },
    #[error("unsupported foreign format at {path}: {reason}")]
    UnsupportedFormat { path: String, reason: String },
    #[error("foreign journal is unreadable at {path}: {message}")]
    SourceRead { path: String, message: String },
    #[error("invalid foreign journal at {path}, record {line}: {message}")]
    SourceParse {
        path: String,
        line: usize,
        message: String,
    },
    #[error("foreign journal has no events: {path}")]
    EmptySource { path: String },
    #[error("foreign import destination is not a directory: {path}")]
    DestinationNotDirectory { path: String },
    #[error("cannot publish foreign import at {path}: {message}")]
    DestinationWrite { path: String, message: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ForeignImportOutcome {
    Imported {
        run_id: String,
        event_count: usize,
        source_path: String,
        format: String,
    },
    Failed {
        source_path: String,
        reason: String,
    },
}
impl ForeignImportOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Imported {
                run_id,
                event_count,
                ..
            } => format!("foreign import: imported run={run_id} events={event_count}"),
            Self::Failed { reason, .. } => format!("foreign import: failed {reason}"),
        }
    }
    pub fn from_result(result: &ForeignImportResult) -> Self {
        Self::Imported {
            run_id: result.run_id.clone(),
            event_count: result.event_count,
            source_path: safe_path(&result.source_path),
            format: result.format.clone(),
        }
    }
    pub fn from_error(source_path: impl Into<String>, error: &ForeignSessionError) -> Self {
        use crate::redact::Redactor;
        let redactor = crate::redact::DefaultRedactor::default();
        Self::Failed {
            source_path: redactor.redact_text(&source_path.into()),
            reason: redactor.redact_text(&error.to_string()),
        }
    }
}
pub fn import_foreign_session_outcome(source: &Path, destination: &Path) -> ForeignImportOutcome {
    match import_foreign_session_as_replay(source, destination) {
        Ok(result) => ForeignImportOutcome::from_result(&result),
        Err(error) => ForeignImportOutcome::from_error(safe_path(source), &error),
    }
}
pub fn refuse_import_into_active_session(
    _: &Path,
    active: &Path,
) -> Result<(), ForeignSessionError> {
    Err(ForeignSessionError::ImportIntoActiveForbidden {
        active_session: safe_path(active),
    })
}
fn safe_path(path: &Path) -> String {
    use crate::redact::Redactor;
    crate::redact::DefaultRedactor::default().redact_text(&path.display().to_string())
}
fn source_error(path: &Path, message: &str) -> ForeignSessionError {
    ForeignSessionError::SourceRead {
        path: safe_path(path),
        message: message.into(),
    }
}
fn load_events(source: &Path) -> Result<Vec<EventEnvelopeV1>, ForeignSessionError> {
    crate::store::validate_private_path(source)
        .map_err(|_| source_error(source, "unsafe source path"))?;
    if !source.is_dir() {
        return Err(ForeignSessionError::SourceNotDirectory {
            path: safe_path(source),
        });
    }
    let path = source.join("events.jsonl");
    crate::store::validate_private_path(&path)
        .map_err(|_| source_error(source, "unsafe journal path"))?;
    let meta =
        std::fs::symlink_metadata(&path).map_err(|_| source_error(source, "missing journal"))?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(source_error(
            source,
            "journal must be a regular file of at most 64 MiB",
        ));
    }
    let reader = crate::store::JournalReader::open(&path, meta.len())
        .map_err(|_| source_error(source, "cannot open journal"))?;
    let mut events = Vec::new();
    for (index, event) in reader.enumerate() {
        if index == MAX_EVENTS {
            return Err(source_error(source, "journal exceeds 65536 events"));
        }
        events.push(event.map_err(|_| ForeignSessionError::SourceParse {
            path: safe_path(&path),
            line: index + 1,
            message: "invalid event, sequence, or schema".into(),
        })?);
    }
    if events.is_empty() {
        return Err(ForeignSessionError::EmptySource {
            path: safe_path(source),
        });
    }
    crate::proj::checked_history(&events)
        .map_err(|_| source_error(source, "inconsistent event identities"))?;
    Ok(events)
}
