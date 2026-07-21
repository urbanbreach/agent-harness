//! Durable edit-attribution journal under `.agent-harness/edit-attribution.jsonl`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::journal_store::{
    append_record, apply_loaded_records, ensure_parent_for_write, hex_encode, load_records,
    normalize_workspace_relative, now_unix_ms, write_bytes, JournalKind, JournalRecord,
};
use super::{
    sha256_hex, AttributedEdit, EditAttributionSummary, EditAttributionTracker, EditSource,
};

/// Relative journal path under a workspace root.
pub const EDIT_ATTRIBUTION_JOURNAL_REL: &str = ".agent-harness/edit-attribution.jsonl";

pub(super) const JOURNAL_VERSION: u32 = 1;

/// Failures for durable attribution journal I/O and path safety.
#[derive(Debug, Error)]
pub enum EditAttributionError {
    #[error("failed to create edit-attribution parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read edit-attribution journal {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse edit-attribution journal line in {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("failed to write edit-attribution journal {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid attribution path `{path}` (empty or escapes workspace)")]
    InvalidPath { path: String },
    #[error("no attribution record for path `{path}`")]
    NotFound { path: String },
    #[error("no agent snapshot available to revert path `{path}`")]
    NoAgentSnapshot { path: String },
    #[error("failed to restore path `{path}`: {source}")]
    Restore {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Result of querying one path's latest attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAttributionQuery {
    pub path: String,
    pub source: EditSource,
    pub content_sha256: String,
    pub drifted: bool,
    pub one_line: String,
}

/// Result of reverting one path to the last agent snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertAttributionResult {
    pub path: String,
    pub restored_sha256: String,
    pub bytes_written: usize,
}

/// Durable multi-path edit attribution journal for one workspace.
#[derive(Debug, Clone)]
pub struct EditAttributionJournal {
    workspace_root: PathBuf,
    journal_path: PathBuf,
    tracker: EditAttributionTracker,
    agent_snapshots: BTreeMap<String, Vec<u8>>,
    next_seq: u64,
}

impl EditAttributionJournal {
    pub fn open(workspace_root: impl Into<PathBuf>) -> Result<Self, EditAttributionError> {
        let workspace_root = workspace_root.into();
        let journal_path = workspace_root.join(EDIT_ATTRIBUTION_JOURNAL_REL);
        let mut journal = Self {
            workspace_root,
            journal_path: journal_path.clone(),
            tracker: EditAttributionTracker::new(),
            agent_snapshots: BTreeMap::new(),
            next_seq: 1,
        };
        let records = load_records(&journal_path)?;
        apply_loaded_records(
            &mut journal.tracker,
            &mut journal.agent_snapshots,
            &mut journal.next_seq,
            &records,
        );
        Ok(journal)
    }

    pub fn empty(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        let journal_path = workspace_root.join(EDIT_ATTRIBUTION_JOURNAL_REL);
        Self {
            workspace_root,
            journal_path,
            tracker: EditAttributionTracker::new(),
            agent_snapshots: BTreeMap::new(),
            next_seq: 1,
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub fn tracker(&self) -> &EditAttributionTracker {
        &self.tracker
    }

    pub fn summary(&self) -> EditAttributionSummary {
        self.tracker.summary()
    }

    pub fn list(&self) -> Vec<&AttributedEdit> {
        self.tracker.list()
    }

    pub fn query(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<EditAttributionQuery, EditAttributionError> {
        let path_key = normalize_workspace_relative(&self.workspace_root, path.as_ref())?;
        let entry = self
            .tracker
            .get(&path_key)
            .ok_or_else(|| EditAttributionError::NotFound {
                path: path_key.clone(),
            })?;
        Ok(EditAttributionQuery {
            path: entry.path.clone(),
            source: entry.source,
            content_sha256: entry.content_sha256.clone(),
            drifted: self.tracker.is_drifted(&entry.path),
            one_line: entry.one_line(),
        })
    }

    pub fn record_agent_tool_edit(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> Result<AttributedEdit, EditAttributionError> {
        let path_key = normalize_workspace_relative(&self.workspace_root, path.as_ref())?;
        let entry = self
            .tracker
            .record_agent_tool_edit(&path_key, content, mtime);
        self.agent_snapshots
            .insert(path_key.clone(), content.to_vec());
        let seq = self.alloc_seq();
        append_record(
            &self.journal_path,
            &JournalRecord {
                v: JOURNAL_VERSION,
                seq,
                path: path_key,
                source: EditSource::AgentTool,
                kind: JournalKind::AgentTool,
                content_sha256: entry.content_sha256.clone(),
                mtime_unix_ms: entry.mtime_unix_ms,
                agent_snapshot_hex: Some(hex_encode(content)),
                ts_unix_ms: now_unix_ms(),
            },
        )?;
        Ok(entry)
    }

    pub fn observe_external(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> Result<AttributedEdit, EditAttributionError> {
        let path_key = normalize_workspace_relative(&self.workspace_root, path.as_ref())?;
        let was_agent = matches!(
            self.tracker.get(&path_key).map(|e| e.source),
            Some(EditSource::AgentTool)
        );
        let prev_hash = self
            .tracker
            .get(&path_key)
            .map(|e| e.content_sha256.clone());
        let entry = self.tracker.observe_external(&path_key, content, mtime);
        let kind = if was_agent
            && entry.source == EditSource::External
            && prev_hash.as_deref() != Some(entry.content_sha256.as_str())
        {
            JournalKind::Drift
        } else if entry.source == EditSource::AgentTool {
            JournalKind::AgentTool
        } else {
            JournalKind::External
        };
        let seq = self.alloc_seq();
        append_record(
            &self.journal_path,
            &JournalRecord {
                v: JOURNAL_VERSION,
                seq,
                path: path_key,
                source: entry.source,
                kind,
                content_sha256: entry.content_sha256.clone(),
                mtime_unix_ms: entry.mtime_unix_ms,
                agent_snapshot_hex: None,
                ts_unix_ms: now_unix_ms(),
            },
        )?;
        Ok(entry)
    }

    pub fn revert_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<RevertAttributionResult, EditAttributionError> {
        let path_key = normalize_workspace_relative(&self.workspace_root, path.as_ref())?;
        let snapshot = self
            .agent_snapshots
            .get(&path_key)
            .cloned()
            .ok_or_else(|| EditAttributionError::NoAgentSnapshot {
                path: path_key.clone(),
            })?;
        let abs = self.workspace_root.join(&path_key);
        ensure_parent_for_write(&abs)?;
        write_bytes(&abs, &snapshot)?;
        let entry = self.record_agent_tool_edit(&path_key, &snapshot, None)?;
        Ok(RevertAttributionResult {
            path: path_key,
            restored_sha256: entry.content_sha256,
            bytes_written: snapshot.len(),
        })
    }

    fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn journal_records_agent_external_and_drift_with_durable_side_effects() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let mut journal = EditAttributionJournal::open(root).expect("open");

        journal
            .record_agent_tool_edit("src/agent.rs", b"agent-bytes", None)
            .expect("agent");
        journal
            .observe_external("src/external.rs", b"external-bytes", None)
            .expect("external");
        journal
            .record_agent_tool_edit("src/drift.rs", b"agent-v1", None)
            .expect("drift agent");
        journal
            .observe_external("src/drift.rs", b"external-v2", None)
            .expect("drift observe");

        assert!(journal.journal_path().is_file());
        let raw = fs::read_to_string(journal.journal_path()).expect("read journal");
        assert!(raw.lines().count() >= 4);
        assert!(raw.contains("\"kind\":\"agent_tool\""));
        assert!(raw.contains("\"kind\":\"external\""));
        assert!(raw.contains("\"kind\":\"drift\""));

        let summary = journal.summary();
        assert_eq!(summary.agent_tool, 1);
        assert_eq!(summary.external, 1);
        assert_eq!(summary.drift, 1);
        assert_eq!(summary.total, 3);

        let reloaded = EditAttributionJournal::open(root).expect("reload");
        let re_summary = reloaded.summary();
        assert_eq!(re_summary.total, 3);
        assert_eq!(re_summary.agent_tool, 1);
        assert_eq!(re_summary.external, 1);
        assert_eq!(re_summary.drift, 1);
        assert_eq!(
            reloaded.query("src/agent.rs").expect("query").source,
            EditSource::AgentTool
        );
        assert!(reloaded.query("src/drift.rs").expect("query").drifted);
    }

    #[test]
    fn query_fails_closed_for_unknown_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = EditAttributionJournal::open(dir.path()).expect("open");
        let err = journal.query("missing.rs").expect_err("unknown");
        assert!(matches!(err, EditAttributionError::NotFound { .. }));
    }

    #[test]
    fn revert_path_fails_closed_without_agent_snapshot() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let mut journal = EditAttributionJournal::open(dir.path()).expect("open");

        // act
        let err = journal
            .revert_path("src/never-edited.rs")
            .expect_err("no agent snapshot");

        // assert
        assert!(matches!(err, EditAttributionError::NoAgentSnapshot { .. }));
    }

    #[test]
    fn revert_path_restores_agent_snapshot_after_drift() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let mut journal = EditAttributionJournal::open(root).expect("open");
        let rel = "src/file.rs";
        let abs = root.join(rel);
        fs::create_dir_all(abs.parent().expect("parent")).expect("mkdir");
        fs::write(&abs, b"agent-content").expect("write agent");
        journal
            .record_agent_tool_edit(rel, b"agent-content", None)
            .expect("record");
        fs::write(&abs, b"human-edit").expect("external write");
        journal
            .observe_external(rel, b"human-edit", None)
            .expect("observe");
        assert_eq!(journal.query(rel).expect("q").source, EditSource::External);

        let reverted = journal.revert_path(rel).expect("revert");

        assert_eq!(reverted.path, rel);
        assert_eq!(fs::read(&abs).expect("read"), b"agent-content");
        assert_eq!(
            journal.query(rel).expect("q2").source,
            EditSource::AgentTool
        );
        assert_eq!(reverted.restored_sha256, sha256_hex(b"agent-content"));
    }

    #[test]
    fn invalid_path_escape_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut journal = EditAttributionJournal::open(dir.path()).expect("open");
        let err = journal
            .record_agent_tool_edit("../escape.rs", b"x", None)
            .expect_err("escape");
        assert!(matches!(err, EditAttributionError::InvalidPath { .. }));
    }
}
