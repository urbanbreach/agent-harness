//! Session-local ordering storage. Queue operations never run prompts or change events.
use crate::store;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const PROMPT_QUEUE_RELATIVE_PATH: &str = "tui/prompt-queue.json";
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptQueueEntry {
    pub id: String,
    pub text: String,
    pub enqueued_at_unix_ms: u64,
    #[serde(default)]
    pub is_interjection: bool,
}
#[derive(Debug, Serialize)]
pub struct MidTurnInterjection {
    pub entry: PromptQueueEntry,
    pub position: usize,
    pub turn_was_running: bool,
    pub mutates_conversation_events: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    entries: Vec<PromptQueueEntry>,
}
#[derive(Debug, thiserror::Error)]
pub enum PromptQueueError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid prompt queue: {0}")]
    Invalid(&'static str),
    #[error("unsupported prompt queue version {0}")]
    UnsupportedVersion(u32),
}
#[derive(Debug, Clone)]
pub struct DurablePromptQueue {
    path: PathBuf,
}
impl DurablePromptQueue {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn for_session(session: &Path) -> Self {
        Self::open(session.join(PROMPT_QUEUE_RELATIVE_PATH))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn list(&self) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        self.load()
    }
    pub fn enqueue(
        &self,
        id: impl Into<String>,
        text: impl Into<String>,
        timestamp: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.insert(id.into(), text.into(), timestamp, false)
    }
    pub fn interject_mid_turn(
        &self,
        id: impl Into<String>,
        text: impl Into<String>,
        timestamp: u64,
        turn_running: bool,
    ) -> Result<MidTurnInterjection, PromptQueueError> {
        Ok(MidTurnInterjection {
            entry: self.insert(id.into(), text.into(), timestamp, true)?,
            position: 0,
            turn_was_running: turn_running,
            mutates_conversation_events: false,
        })
    }
    fn insert(
        &self,
        id: String,
        text: String,
        timestamp: u64,
        is_interjection: bool,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let mut entry = PromptQueueEntry {
            id,
            text,
            enqueued_at_unix_ms: timestamp,
            is_interjection,
        };
        validate(&mut entry)?;
        self.update(|entries| {
            if entries.len() >= 256 || entries.iter().any(|e| e.id == entry.id) {
                return Err(PromptQueueError::Invalid(
                    "queue is full or the ID already exists",
                ));
            }
            entries.insert(
                if is_interjection { 0 } else { entries.len() },
                entry.clone(),
            );
            Ok(entry)
        })
    }
    pub fn dequeue(&self) -> Result<Option<PromptQueueEntry>, PromptQueueError> {
        store::validate_private_path(&self.path)?;
        if !self.path.try_exists()? {
            return Ok(None);
        }
        self.update(|entries| Ok((!entries.is_empty()).then(|| entries.remove(0))))
    }
    fn update<T>(
        &self,
        change: impl FnOnce(&mut Vec<PromptQueueEntry>) -> Result<T, PromptQueueError>,
    ) -> Result<T, PromptQueueError> {
        let _lock = store::lock_private_parent(&self.path)?;
        let mut entries = self.load()?;
        let result = change(&mut entries)?;
        let bytes = serde_json::to_vec(&Document {
            version: 1,
            entries,
        })
        .map_err(|_| PromptQueueError::Invalid("cannot encode queue"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(PromptQueueError::Invalid("queue exceeds 4 MiB"));
        }
        store::write_private_atomic(&self.path, &bytes)?;
        Ok(result)
    }
    fn load(&self) -> Result<Vec<PromptQueueEntry>, PromptQueueError> {
        let Some(bytes) = store::read_private_bytes(&self.path, MAX_BYTES)? else {
            return Ok(Vec::new());
        };
        let mut document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| PromptQueueError::Invalid("malformed document"))?;
        if document.version != 1 {
            return Err(PromptQueueError::UnsupportedVersion(document.version));
        }
        if document.entries.len() > 256 {
            return Err(PromptQueueError::Invalid("queue exceeds 256 entries"));
        }
        let mut ids = BTreeSet::new();
        for entry in &mut document.entries {
            validate(entry)?;
            if !ids.insert(entry.id.as_str()) {
                return Err(PromptQueueError::Invalid("duplicate ID"));
            }
        }
        Ok(document.entries)
    }
}
fn validate(entry: &mut PromptQueueEntry) -> Result<(), PromptQueueError> {
    if entry.id.trim().is_empty()
        || entry.id.len() > 256
        || entry.id.chars().any(char::is_control)
        || crate::redact::redact_artifact_text(&entry.id) != entry.id
    {
        return Err(PromptQueueError::Invalid("invalid ID"));
    }
    if entry.text.trim().is_empty() || entry.text.len() > 64 * 1024 || entry.text.contains('\0') {
        return Err(PromptQueueError::Invalid(
            "text must contain 1 byte to 64 KiB",
        ));
    }
    entry.text = crate::redact::redact_artifact_text(entry.text.trim());
    Ok(())
}
