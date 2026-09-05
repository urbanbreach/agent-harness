//! Agent vs external edit attribution (durable product).
//!
//! Tracks paths touched by agent tool edits versus external mtime/hash changes.
//! Persists an append-only journal under `.agent-harness/edit-attribution.jsonl`
//! and exposes query + path-level revert APIs. Not full VCS blame/diff.

pub mod diff_blame;
mod journal;
mod journal_store;
mod product;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use diff_blame::{compute_blame, compute_diff, BlameLine, BlameResult, DiffResult};
pub use journal::{
    EditAttributionError, EditAttributionJournal, EditAttributionQuery, RevertAttributionResult,
    EDIT_ATTRIBUTION_JOURNAL_REL,
};
pub use product::{run_multi_path_edit_attribution_product, MultiPathEditAttribution};

/// Who last attributed a path change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditSource {
    /// Change recorded from an agent tool edit path.
    AgentTool,
    /// Change observed outside agent tool records (mtime/hash drift).
    External,
}

impl EditSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentTool => "agent_tool",
            Self::External => "external",
        }
    }
}

/// One attributed path snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributedEdit {
    pub path: String,
    pub source: EditSource,
    pub content_sha256: String,
    pub mtime_unix_ms: Option<u64>,
}

impl AttributedEdit {
    /// Operator-facing one-line path attribution (not VCS blame).
    pub fn one_line(&self) -> String {
        format!(
            "edit attribution: `{}` source={}",
            self.path,
            self.source.as_str()
        )
    }
}

/// In-memory attribution helper (session-local projection of the durable journal).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditAttributionTracker {
    by_path: BTreeMap<String, AttributedEdit>,
    /// Paths that transitioned agent_tool → external (content drift).
    drifted_paths: BTreeSet<String>,
}

impl EditAttributionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an agent tool edit with content hash (and optional mtime).
    pub fn record_agent_tool_edit(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> AttributedEdit {
        let path_key = normalize_path(path.as_ref());
        self.drifted_paths.remove(&path_key);
        let entry = AttributedEdit {
            path: path_key.clone(),
            source: EditSource::AgentTool,
            content_sha256: sha256_hex(content),
            mtime_unix_ms: mtime.and_then(system_time_to_unix_ms),
        };
        self.by_path.insert(path_key, entry.clone());
        entry
    }

    /// Observe on-disk bytes/mtime and attribute external when they diverge from
    /// the last agent-tool record (or when the path was never agent-touched).
    pub fn observe_external(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> AttributedEdit {
        let path_key = normalize_path(path.as_ref());
        let hash = sha256_hex(content);
        let mtime_unix_ms = mtime.and_then(system_time_to_unix_ms);

        let source = match self.by_path.get(&path_key) {
            Some(prev)
                if prev.source == EditSource::AgentTool
                    && prev.content_sha256 == hash
                    && prev.mtime_unix_ms == mtime_unix_ms =>
            {
                EditSource::AgentTool
            }
            Some(prev) if prev.source == EditSource::AgentTool && prev.content_sha256 == hash => {
                // Content still matches agent edit; keep agent attribution even if
                // mtime is missing/unstable on the host.
                EditSource::AgentTool
            }
            Some(prev) if prev.source == EditSource::AgentTool && prev.content_sha256 != hash => {
                self.drifted_paths.insert(path_key.clone());
                EditSource::External
            }
            Some(prev) => prev.source,
            None => EditSource::External,
        };

        let entry = AttributedEdit {
            path: path_key.clone(),
            source,
            content_sha256: hash,
            mtime_unix_ms,
        };
        self.by_path.insert(path_key, entry.clone());
        entry
    }

    pub fn get(&self, path: impl AsRef<Path>) -> Option<&AttributedEdit> {
        self.by_path.get(&normalize_path(path.as_ref()))
    }

    pub fn list(&self) -> Vec<&AttributedEdit> {
        self.by_path.values().collect()
    }

    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    pub fn is_drifted(&self, path: impl AsRef<Path>) -> bool {
        self.drifted_paths.contains(&normalize_path(path.as_ref()))
    }

    pub(crate) fn apply_loaded_entry(&mut self, entry: AttributedEdit, drifted: bool) {
        let path_key = entry.path.clone();
        if drifted {
            self.drifted_paths.insert(path_key.clone());
        } else {
            self.drifted_paths.remove(&path_key);
        }
        self.by_path.insert(path_key, entry);
    }

    pub fn summary(&self) -> EditAttributionSummary {
        let mut agent_tool = 0usize;
        let mut external = 0usize;
        let mut drift = 0usize;
        for entry in self.by_path.values() {
            match entry.source {
                EditSource::AgentTool => agent_tool += 1,
                EditSource::External if self.drifted_paths.contains(&entry.path) => {
                    drift += 1;
                }
                EditSource::External => external += 1,
            }
        }
        EditAttributionSummary {
            agent_tool,
            external,
            drift,
            total: self.by_path.len(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EditAttributionSummary {
    pub agent_tool: usize,
    pub external: usize,
    #[serde(default)]
    pub drift: usize,
    pub total: usize,
}

impl EditAttributionSummary {
    /// Operator-facing one-line counts (not VCS blame).
    pub fn one_line(&self) -> String {
        format!(
            "edit attribution: {} agent-tool, {} external, {} drift ({} total)",
            self.agent_tool, self.external, self.drift, self.total
        )
    }

    pub const fn has_external(&self) -> bool {
        self.external > 0 || self.drift > 0
    }

    pub const fn has_agent_tool(&self) -> bool {
        self.agent_tool > 0
    }

    pub const fn has_drift(&self) -> bool {
        self.drift > 0
    }
}

pub(crate) fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn sha256_hex(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn system_time_to_unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Convenience: hash file bytes from a path for external observation.
pub fn hash_path_contents(path: &Path) -> std::io::Result<(Vec<u8>, String)> {
    let bytes = std::fs::read(path)?;
    let hash = sha256_hex(&bytes);
    Ok((bytes, hash))
}

/// Content digest matching `EditAppliedEvent::new_file_digest` (blake3 hex, 12 chars).
///
/// Use this when comparing on-disk bytes to applied-edit digests from the event log.
/// Distinct from the tracker-local sha256 hashes used by [`EditAttributionTracker`].
pub fn content_digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().chars().take(12).collect()
}

/// Read path bytes and return the EditApplied-compatible digest12.
pub fn path_content_digest12(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(content_digest12(&bytes))
}

/// Workspace-relative path key helper (no filesystem I/O).
pub fn relative_path_key(workspace_root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_tool_edit_is_attributed_as_agent() {
        // Given
        let mut tracker = EditAttributionTracker::new();

        // When
        let entry = tracker.record_agent_tool_edit("src/a.rs", b"fn main() {}", None);

        // Then
        assert_eq!(entry.source, EditSource::AgentTool);
        assert_eq!(entry.path, "src/a.rs");
        assert!(!entry.content_sha256.is_empty());
        assert_eq!(
            tracker.get("src/a.rs").map(|e| e.source),
            Some(EditSource::AgentTool)
        );
    }

    #[test]
    fn external_mtime_hash_drift_marks_external() {
        // Given: agent wrote content A
        let mut tracker = EditAttributionTracker::new();
        tracker.record_agent_tool_edit("notes.txt", b"agent", None);

        // When: external observation sees different bytes
        let observed = tracker.observe_external("notes.txt", b"human edit", None);

        // Then
        assert_eq!(observed.source, EditSource::External);
        assert!(tracker.is_drifted("notes.txt"));
        assert_eq!(
            tracker.get("notes.txt").map(|e| e.source),
            Some(EditSource::External)
        );
    }

    #[test]
    fn unchanged_agent_content_keeps_agent_attribution() {
        let mut tracker = EditAttributionTracker::new();
        tracker.record_agent_tool_edit("keep.rs", b"stable", None);
        let observed = tracker.observe_external("keep.rs", b"stable", None);
        assert_eq!(observed.source, EditSource::AgentTool);
        assert!(!tracker.is_drifted("keep.rs"));
    }

    #[test]
    fn path_never_seen_by_agent_is_external() {
        let mut tracker = EditAttributionTracker::new();
        let observed = tracker.observe_external("only-external.md", b"x", None);
        assert_eq!(observed.source, EditSource::External);
        assert!(!tracker.is_drifted("only-external.md"));
        assert_eq!(tracker.list().len(), 1);
    }

    #[test]
    fn summary_counts_agent_external_and_drift_paths() {
        // Given
        let mut tracker = EditAttributionTracker::new();
        tracker.record_agent_tool_edit("a.rs", b"a", None);
        tracker.record_agent_tool_edit("b.rs", b"b", None);
        tracker.observe_external("c.rs", b"c", None);
        tracker.record_agent_tool_edit("d.rs", b"d1", None);
        tracker.observe_external("d.rs", b"d2", None);

        // When
        let summary = tracker.summary();

        // Then
        assert_eq!(summary.agent_tool, 2);
        assert_eq!(summary.external, 1);
        assert_eq!(summary.drift, 1);
        assert_eq!(summary.total, 4);
        assert!(summary.has_agent_tool());
        assert!(summary.has_external());
        assert!(summary.has_drift());
        assert!(summary.one_line().contains("2 agent-tool"));
        assert!(summary.one_line().contains("1 external"));
        assert!(summary.one_line().contains("1 drift"));
        assert!(summary.one_line().contains("4 total"));
        let agent_line = tracker.get("a.rs").expect("a.rs").one_line();
        assert!(agent_line.contains("source=agent_tool"));
        assert!(agent_line.contains("`a.rs`"));
        let external_line = tracker.get("c.rs").expect("c.rs").one_line();
        assert!(external_line.contains("source=external"));
        assert!(external_line.contains("`c.rs`"));
    }

    #[test]
    fn content_digest12_is_stable_twelve_hex_chars() {
        // Given / When
        let digest = content_digest12(b"agent-applied-bytes");

        // Then
        assert_eq!(digest.len(), 12);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(digest, content_digest12(b"agent-applied-bytes"));
        assert_ne!(digest, content_digest12(b"drifted-bytes"));
    }

    #[test]
    fn path_content_digest12_reads_file_bytes() {
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.txt");
        std::fs::write(&path, b"on-disk").expect("write");

        // When
        let digest = path_content_digest12(&path).expect("digest");

        // Then
        assert_eq!(digest, content_digest12(b"on-disk"));
    }
}
