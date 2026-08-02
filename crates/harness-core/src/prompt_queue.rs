//! Durable session-local prompt queue (ordering MVP).
//!
//! Persists an ordered FIFO of prompts under a session directory. Multi-client
//! lock coordination and send-now product UX are out of scope; this is durable
//! ordering storage only and does not mutate conversation events.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Relative path under a session dir for the durable prompt queue.
pub const PROMPT_QUEUE_RELATIVE_PATH: &str = "tui/prompt-queue.json";

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptQueueDocument {
    version: u32,
    entries: Vec<PromptQueueEntry>,
}

impl PromptQueueDocument {
    fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            entries: Vec::new(),
        }
    }
}

/// One durable queued prompt (FIFO order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptQueueEntry {
    pub id: String,
    pub text: String,
    pub enqueued_at_unix_ms: u64,
    #[serde(default)]
    pub is_interjection: bool,
}

/// Result of a mid-turn interjection insert (queue only; events untouched).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidTurnInterjection {
    pub entry: PromptQueueEntry,
    /// Zero-based position after insert (front = 0).
    pub position: usize,
    /// Whether a turn was reported running at insert time.
    pub turn_was_running: bool,
    /// Always false for this API — conversation events are never rewritten.
    pub mutates_conversation_events: bool,
}

/// Failures loading or updating the durable prompt queue.
#[derive(Debug, Error)]
pub enum PromptQueueError {
    #[error("prompt queue text must be non-empty after trim")]
    EmptyText,
    #[error("failed to create prompt queue parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to read prompt queue {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse prompt queue {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("unsupported prompt queue version {version} in {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("failed to write prompt queue {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace prompt queue {path}: {source}")]
    Replace {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("prompt queue index {index} out of bounds (len {len})")]
    OutOfBounds { index: usize, len: usize },
}

/// Session-scoped durable prompt queue store.
#[derive(Debug, Clone)]
pub struct DurablePromptQueue {
    path: PathBuf,
}

impl DurablePromptQueue {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path_for_session(session_dir: &Path) -> PathBuf {
        session_dir.join(PROMPT_QUEUE_RELATIVE_PATH)
    }

    pub fn for_session(session_dir: &Path) -> Self {
        Self::open(Self::default_path_for_session(session_dir))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        Ok(self.load()?.entries)
    }

    pub fn enqueue(
        &self,
        id: impl Into<String>,
        text: impl Into<String>,
        enqueued_at_unix_ms: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let text = text.into();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(PromptQueueError::EmptyText);
        }
        let entry = PromptQueueEntry {
            id: id.into(),
            text: trimmed.to_string(),
            enqueued_at_unix_ms,
            is_interjection: false,
        };
        let mut doc = self.load()?;
        doc.entries.push(entry.clone());
        self.store(&doc)?;
        Ok(entry)
    }

    /// Insert a mid-turn user interjection at the front of the durable queue.
    ///
    /// Does **not** mutate conversation events or abort the active turn. When
    /// `turn_running` is true, the entry is marked as queued-while-running so
    /// the product layer can drain it after the current turn completes.
    pub fn interject_mid_turn(
        &self,
        id: impl Into<String>,
        text: impl Into<String>,
        enqueued_at_unix_ms: u64,
        turn_running: bool,
    ) -> Result<MidTurnInterjection, PromptQueueError> {
        let text = text.into();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(PromptQueueError::EmptyText);
        }
        let entry = PromptQueueEntry {
            id: id.into(),
            text: trimmed.to_string(),
            enqueued_at_unix_ms,
            is_interjection: true,
        };
        let mut doc = self.load()?;
        // Front-insert: interjections drain before ordinary FIFO tail entries.
        doc.entries.insert(0, entry.clone());
        self.store(&doc)?;
        Ok(MidTurnInterjection {
            entry,
            position: 0,
            turn_was_running: turn_running,
            mutates_conversation_events: false,
        })
    }

    /// Pop the front entry (FIFO). Returns `None` when empty.
    pub fn dequeue(&self) -> Result<Option<PromptQueueEntry>, PromptQueueError> {
        let mut doc = self.load()?;
        if doc.entries.is_empty() {
            return Ok(None);
        }
        let entry = doc.entries.remove(0);
        self.store(&doc)?;
        Ok(Some(entry))
    }

    pub fn len(&self) -> Result<usize, PromptQueueError> {
        Ok(self.load()?.entries.len())
    }

    pub fn is_empty(&self) -> Result<bool, PromptQueueError> {
        Ok(self.load()?.entries.is_empty())
    }

    /// Remove and return all entries (FIFO order).
    pub fn drain(&self) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        let mut doc = self.load()?;
        let entries = std::mem::take(&mut doc.entries);
        self.store(&doc)?;
        Ok(entries)
    }

    /// Remove all entries, returning the count removed.
    pub fn clear(&self) -> Result<usize, PromptQueueError> {
        let mut doc = self.load()?;
        let count = doc.entries.len();
        doc.entries.clear();
        self.store(&doc)?;
        Ok(count)
    }

    /// Remove and return only interjection entries (preserving order).
    pub fn drain_interjections(&self) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        let mut doc = self.load()?;
        let all = std::mem::take(&mut doc.entries);
        let (interjections, remaining): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|e| e.is_interjection);
        doc.entries = remaining;
        self.store(&doc)?;
        Ok(interjections)
    }

    /// Edit the text of the entry with the given `id`.
    pub fn edit(&self, id: &str, new_text: &str) -> Result<PromptQueueEntry, PromptQueueError> {
        let trimmed = new_text.trim();
        if trimmed.is_empty() {
            return Err(PromptQueueError::EmptyText);
        }
        let mut doc = self.load()?;
        let len = doc.entries.len();
        let pos = doc
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(PromptQueueError::OutOfBounds { index: 0, len })?;
        doc.entries[pos].text = trimmed.to_string();
        let result = doc.entries[pos].clone();
        self.store(&doc)?;
        Ok(result)
    }

    /// Remove the entry with the given `id` and return it.
    pub fn remove(&self, id: &str) -> Result<PromptQueueEntry, PromptQueueError> {
        let mut doc = self.load()?;
        let len = doc.entries.len();
        let pos = doc
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(PromptQueueError::OutOfBounds { index: 0, len })?;
        let entry = doc.entries.remove(pos);
        self.store(&doc)?;
        Ok(entry)
    }

    /// Move the entry with the given `id` to position `to` (0-based).
    pub fn reorder(&self, id: &str, to: usize) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        let mut doc = self.load()?;
        let len = doc.entries.len();
        let from = doc
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(PromptQueueError::OutOfBounds { index: 0, len })?;
        if to >= len {
            return Err(PromptQueueError::OutOfBounds { index: to, len });
        }
        let entry = doc.entries.remove(from);
        doc.entries.insert(to, entry);
        let result = doc.entries.clone();
        self.store(&doc)?;
        Ok(result)
    }

    fn load(&self) -> Result<PromptQueueDocument, PromptQueueError> {
        match fs::read_to_string(&self.path) {
            Ok(body) => {
                let doc: PromptQueueDocument =
                    serde_json::from_str(&body).map_err(|err| PromptQueueError::Parse {
                        path: self.path.display().to_string(),
                        detail: err.to_string(),
                    })?;
                if doc.version != STORE_VERSION {
                    return Err(PromptQueueError::UnsupportedVersion {
                        path: self.path.display().to_string(),
                        version: doc.version,
                    });
                }
                Ok(doc)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(PromptQueueDocument::empty()),
            Err(err) => Err(PromptQueueError::Read {
                path: self.path.display().to_string(),
                source: err,
            }),
        }
    }

    fn store(&self, doc: &PromptQueueDocument) -> Result<(), PromptQueueError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| PromptQueueError::CreateParent {
                path: parent.display().to_string(),
                source: err,
            })?;
        }
        let body = serde_json::to_vec_pretty(doc).map_err(|err| PromptQueueError::Write {
            path: self.path.display().to_string(),
            source: io::Error::other(err.to_string()),
        })?;
        let temp_path = self.path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .map_err(|err| PromptQueueError::Write {
                    path: temp_path.display().to_string(),
                    source: err,
                })?;
            file.write_all(&body)
                .map_err(|err| PromptQueueError::Write {
                    path: temp_path.display().to_string(),
                    source: err,
                })?;
            file.sync_all().map_err(|err| PromptQueueError::Write {
                path: temp_path.display().to_string(),
                source: err,
            })?;
        }
        fs::rename(&temp_path, &self.path).map_err(|err| PromptQueueError::Replace {
            path: self.path.display().to_string(),
            source: err,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn enqueue_dequeue_preserves_fifo_order_on_disk() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempdir().unwrap();
        let queue = DurablePromptQueue::for_session(dir.path());

        // When
        queue.enqueue("a", "first", 1).unwrap();
        queue.enqueue("b", "second", 2).unwrap();
        let listed = queue.list().unwrap();
        let first = queue.dequeue().unwrap().unwrap();
        let second = queue.dequeue().unwrap().unwrap();
        let empty = queue.dequeue().unwrap();

        // Then
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].text, "first");
        assert_eq!(listed[1].text, "second");
        assert_eq!(first.id, "a");
        assert_eq!(second.id, "b");
        assert!(empty.is_none());
        assert!(queue.path().exists());
        assert!(queue.is_empty().unwrap());
    }

    #[test]
    fn empty_text_rejected_and_missing_file_is_empty() {
        // arrange
        // act
        // assert
        let dir = tempdir().unwrap();
        let queue = DurablePromptQueue::for_session(dir.path());
        assert!(queue.is_empty().unwrap());
        assert!(matches!(
            queue.enqueue("x", "   ", 0),
            Err(PromptQueueError::EmptyText)
        ));
        assert!(!queue.path().exists());
    }

    #[test]
    fn reopened_queue_loads_prior_entries() {
        // arrange
        // act
        // assert
        let dir = tempdir().unwrap();
        let path = DurablePromptQueue::default_path_for_session(dir.path());
        {
            let queue = DurablePromptQueue::open(&path);
            queue.enqueue("persist", "hello durable", 9).unwrap();
        }
        let reopened = DurablePromptQueue::open(&path);
        let listed = reopened.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].text, "hello durable");
    }

    #[test]
    fn dequeue_persists_removal_across_queue_reopen() {
        // arrange
        let dir = tempdir().unwrap();
        let path = DurablePromptQueue::default_path_for_session(dir.path());
        let queue = DurablePromptQueue::open(&path);
        queue.enqueue("a", "first", 1).unwrap();
        queue.enqueue("b", "second", 2).unwrap();

        // act — pop on one instance, then reopen a fresh instance
        let popped = queue.dequeue().unwrap().unwrap();
        let reopened = DurablePromptQueue::open(&path);
        let surviving = reopened.list().unwrap();

        // assert — the removal is durable; the FIFO tail survives intact
        assert_eq!(popped.id, "a");
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].id, "b");
    }

    #[test]
    fn mid_turn_interjection_inserts_front_without_event_mutation() {
        // arrange
        // act
        // assert
        // Given: ordinary FIFO entry already queued while a turn is running
        let dir = tempdir().unwrap();
        let queue = DurablePromptQueue::for_session(dir.path());
        queue.enqueue("tail", "later", 1).unwrap();

        // When: interject mid-turn
        let interjection = queue.interject_mid_turn("inj", "urgent", 2, true).unwrap();

        // Then: front position, events flag false, durable order preserved
        assert_eq!(interjection.position, 0);
        assert!(interjection.turn_was_running);
        assert!(!interjection.mutates_conversation_events);
        assert_eq!(interjection.entry.text, "urgent");
        let listed = queue.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "inj");
        assert_eq!(listed[1].id, "tail");
        let first = queue.dequeue().unwrap().unwrap();
        assert_eq!(first.id, "inj");
    }

    #[test]
    fn interject_mid_turn_records_idle_state_when_turn_not_running() {
        // arrange
        let dir = tempdir().unwrap();
        let queue = DurablePromptQueue::for_session(dir.path());

        // act — interject while no turn is running
        let interjection = queue
            .interject_mid_turn("idle-inj", "queued while idle", 5, false)
            .unwrap();

        // assert — honest turn-state flag for post-turn drain; events untouched
        assert!(!interjection.turn_was_running);
        assert!(!interjection.mutates_conversation_events);
        assert_eq!(interjection.position, 0);
        assert_eq!(queue.dequeue().unwrap().unwrap().id, "idle-inj");
    }
}
