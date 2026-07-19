//! Foreign coding-agent session discovery and one supported replay-import path.
//!
//! Discovery scans operator-supplied roots for foreign session-like directories,
//! classifies them as discoverable/corrupt/rejected, and never mutates an active
//! harness session.
//!
//! Import supports exactly one format: a directory whose primary marker is
//! `events.jsonl` containing harness-compatible event envelopes (events-like
//! JSONL). Import materializes a **new** read-only replay session under the
//! destination store via append-only event writes. Unknown formats fail closed.

mod discover;
mod import;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::proj::SessionModeSource;

pub use discover::{discover_foreign_sessions, refuse_import_into_active_session};
pub use import::import_foreign_session_as_replay;

/// Known foreign coding-agent families we can label (best-effort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignAgentKind {
    /// OpenAI Codex / codex-cli style session trees.
    Codex,
    /// Claude Code / Claude Desktop style session trees.
    Claude,
    /// OpenCode-style session trees.
    OpenCode,
    /// Session-like but family not identified.
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

/// Outcome of inspecting one candidate directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForeignSessionCandidate {
    /// Looks like a foreign session and basic markers parse.
    Discoverable {
        kind: ForeignAgentKind,
        path: PathBuf,
        marker: String,
    },
    /// Looks like a foreign session but required content is corrupt/unreadable.
    Corrupt {
        kind: ForeignAgentKind,
        path: PathBuf,
        reason: String,
    },
    /// Directory exists but is not treated as a foreign session.
    Rejected { path: PathBuf, reason: String },
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

    /// True only when the discoverable marker is the supported import path.
    pub fn is_importable(&self) -> bool {
        match self {
            Self::Discoverable { marker, .. } => marker == SUPPORTED_IMPORT_MARKER,
            Self::Corrupt { .. } | Self::Rejected { .. } => false,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Discoverable { path, .. }
            | Self::Corrupt { path, .. }
            | Self::Rejected { path, .. } => path,
        }
    }
}

/// Operator-facing counts for a discover scan (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ForeignDiscoverSummary {
    pub discoverable: usize,
    /// Discoverable candidates whose marker is the supported import path.
    pub importable: usize,
    /// Discoverable candidates that are not importable yet (other markers).
    pub discoverable_not_importable: usize,
    pub corrupt: usize,
    pub rejected: usize,
    pub total: usize,
}

impl ForeignDiscoverSummary {
    pub fn one_line(&self) -> String {
        format!(
            "foreign discover: {} discoverable ({} importable, {} not yet), {} corrupt, {} rejected ({} total)",
            self.discoverable,
            self.importable,
            self.discoverable_not_importable,
            self.corrupt,
            self.rejected,
            self.total
        )
    }

    pub const fn has_importable(&self) -> bool {
        self.importable > 0
    }
}

/// Summarize discover candidates for CLI/operator surfaces.
pub fn summarize_discover_candidates(
    candidates: &[ForeignSessionCandidate],
) -> ForeignDiscoverSummary {
    let mut summary = ForeignDiscoverSummary {
        total: candidates.len(),
        ..ForeignDiscoverSummary::default()
    };
    for candidate in candidates {
        match candidate {
            ForeignSessionCandidate::Discoverable { .. } => {
                summary.discoverable = summary.discoverable.saturating_add(1);
                if candidate.is_importable() {
                    summary.importable = summary.importable.saturating_add(1);
                } else {
                    summary.discoverable_not_importable =
                        summary.discoverable_not_importable.saturating_add(1);
                }
            }
            ForeignSessionCandidate::Corrupt { .. } => {
                summary.corrupt = summary.corrupt.saturating_add(1);
            }
            ForeignSessionCandidate::Rejected { .. } => {
                summary.rejected = summary.rejected.saturating_add(1);
            }
        }
    }
    summary
}

/// Successful read-only replay import into the harness session store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignImportResult {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub event_count: usize,
    pub source_path: PathBuf,
    /// Stable format id for the one supported foreign marker.
    pub format: String,
    pub mode_source: SessionModeSource,
}

impl ForeignImportResult {
    /// Operator-facing one-line diagnostics for a successful replay import.
    pub fn one_line(&self) -> String {
        format!(
            "foreign import: ok run=`{}` events={} format=`{}` source=`{}`",
            self.run_id,
            self.event_count,
            self.format,
            self.source_path.display()
        )
    }
}

/// Errors for foreign-session operations that must fail closed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ForeignSessionError {
    #[error("foreign session scan root is not a directory: {path}")]
    ScanRootNotDirectory { path: String },
    #[error("failed to read foreign session scan root {path}: {message}")]
    ScanRootRead { path: String, message: String },
    #[error(
        "refusing to import foreign session into active harness session \
         (import creates a new replay session only; active_session={active_session})"
    )]
    ImportIntoActiveForbidden { active_session: String },
    #[error("foreign session path is not a directory: {path}")]
    SourceNotDirectory { path: String },
    #[error(
        "unsupported foreign session format at {path}: {reason} \
         (supported: directory with events.jsonl harness-compatible event envelopes)"
    )]
    UnsupportedFormat { path: String, reason: String },
    #[error("foreign events.jsonl is unreadable at {path}: {message}")]
    SourceRead { path: String, message: String },
    #[error("foreign events.jsonl parse failed at {path} line {line}: {message}")]
    SourceParse {
        path: String,
        line: usize,
        message: String,
    },
    #[error("foreign events.jsonl has no importable event lines: {path}")]
    EmptySource { path: String },
    #[error("destination session directory is not a directory: {path}")]
    DestinationNotDirectory { path: String },
    #[error("failed to create imported session at {path}: {message}")]
    DestinationWrite { path: String, message: String },
}

/// Result of a single foreign-session import attempt (diagnostics only).
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
                source_path,
                format,
            } => format!(
                "foreign import: ok run=`{run_id}` events={event_count} format=`{format}` source=`{source_path}`"
            ),
            Self::Failed {
                source_path,
                reason,
            } => format!("foreign import: failed source=`{source_path}` ({reason})"),
        }
    }

    pub fn from_result(result: &ForeignImportResult) -> Self {
        Self::Imported {
            run_id: result.run_id.clone(),
            event_count: result.event_count,
            source_path: result.source_path.display().to_string(),
            format: result.format.clone(),
        }
    }

    pub fn from_error(source_path: impl Into<String>, err: &ForeignSessionError) -> Self {
        Self::Failed {
            source_path: source_path.into(),
            reason: err.to_string(),
        }
    }
}

/// Attempt a foreign-session replay import and return a structured operator-facing outcome.
pub fn import_foreign_session_outcome(
    foreign_path: &std::path::Path,
    dest_session_dir: &std::path::Path,
) -> ForeignImportOutcome {
    let source = foreign_path.display().to_string();
    match import_foreign_session_as_replay(foreign_path, dest_session_dir) {
        Ok(result) => ForeignImportOutcome::from_result(&result),
        Err(err) => ForeignImportOutcome::from_error(source, &err),
    }
}

/// Marker filenames used to detect foreign session-like directories.
pub(super) const MARKERS: &[&str] = &[
    "events.jsonl",
    "session.json",
    "rollout.jsonl",
    "conversation.json",
    "transcript.jsonl",
];

pub(super) const SUPPORTED_IMPORT_MARKER: &str = "events.jsonl";
pub(super) const SUPPORTED_IMPORT_FORMAT: &str = "events_jsonl_v1";
