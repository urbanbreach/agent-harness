use crate::event::{EventActor, EventEnvelopeV1, EventV1, LiveEventEnvelope, RuntimeEvent};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Mutex, MutexGuard},
};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
mod lock;
mod observer;
mod private;
pub(crate) use lock::existing_writer_lock;
pub(crate) use private::{lock_private_parent, read_private_bytes, validate_private_path};
pub use private::{open_private_append, open_private_file};
mod reader;
mod recovery;
pub(crate) use observer::observer;
pub use reader::{read_events, JournalReader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeWithoutSeqV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub run_id: crate::ids::RunId,
    pub mono_ms: u64,
    pub ts: Option<String>,
    pub actor: EventActor,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
    pub payload: EventV1,
}

impl From<EventEnvelopeV1> for EventEnvelopeWithoutSeqV1 {
    fn from(e: EventEnvelopeV1) -> Self {
        Self {
            schema_version: e.schema_version,
            event_id: e.event_id,
            run_id: e.run_id,
            mono_ms: e.mono_ms,
            ts: e.ts,
            actor: e.actor,
            correlation_id: e.correlation_id,
            causation_id: e.causation_id,
            stream_key: e.stream_key,
            payload: e.payload,
        }
    }
}

impl EventEnvelopeWithoutSeqV1 {
    fn sequence(self, seq: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: self.schema_version,
            event_id: self.event_id,
            run_id: self.run_id,
            seq,
            mono_ms: self.mono_ms,
            ts: self.ts,
            actor: self.actor,
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
            stream_key: self.stream_key,
            payload: self.payload,
        }
    }
}
pub type EventStream = Pin<Box<dyn Stream<Item = Result<EventEnvelopeV1, EventStoreError>> + Send>>;
pub type RuntimeEventStream =
    Pin<Box<dyn Stream<Item = Result<RuntimeEvent, EventStoreError>> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("subscriber lagged by {0} events")]
    SubscriberLagged(u64),
    #[error("event store lock poisoned")]
    LockPoisoned,
}

pub trait EventStore: Send + Sync {
    fn append(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError>;
    /// Apply accepted coordinator state after durable append and before delivery.
    fn append_applied(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
        apply: &mut dyn FnMut(&EventEnvelopeV1),
    ) -> Result<EventEnvelopeV1, EventStoreError>;
    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError>;
    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError>;
    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError>;
    fn publish_live(&self, envelope: LiveEventEnvelope);
    fn close_writer(&self) -> Result<(), EventStoreError>;
}

pub struct Journal {
    state: Mutex<State>,
    tx: broadcast::Sender<RuntimeEvent>,
    file_path: PathBuf,
}
struct State {
    file: Option<File>,
    writer_lock: Option<File>,
    memory: Vec<EventEnvelopeV1>,
    next_seq: u64,
    run_id: Option<String>,
    length: u64,
    needs_newline: bool,
    failed: bool,
    closed: bool,
}
pub type JsonlFileEventStore = Journal;
pub type InMemoryEventStore = Journal;

impl Journal {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                file: None,
                writer_lock: None,
                memory: Vec::new(),
                next_seq: 1,
                run_id: None,
                length: 0,
                needs_newline: false,
                failed: false,
                closed: false,
            }),
            tx: broadcast::channel(256).0,
            file_path: PathBuf::new(),
        }
    }
    pub fn open(
        root: impl AsRef<Path>,
        id: impl AsRef<str>,
        _deterministic: bool,
    ) -> Result<Self, EventStoreError> {
        Self::open_file(root.as_ref(), id.as_ref(), true, false, false)
    }
    pub fn open_existing(
        root: impl AsRef<Path>,
        id: impl AsRef<str>,
        _deterministic: bool,
    ) -> Result<Self, EventStoreError> {
        Self::open_file(root.as_ref(), id.as_ref(), false, false, false)
    }
    pub(crate) fn open_for_recovery(root: &Path, id: &str) -> Result<Self, EventStoreError> {
        Self::open_file(root, id, false, true, false)
    }
    pub(crate) fn open_child_projection(root: &Path, id: &str) -> Result<Self, EventStoreError> {
        let mut journal = Self::open_file(root, id, true, true, true)?;
        journal.tx = broadcast::channel(1).0;
        Ok(journal)
    }
    fn open_file(
        root: &Path,
        id: &str,
        create: bool,
        recover: bool,
        child_projection: bool,
    ) -> Result<Self, EventStoreError> {
        validate_session_id(id)?;
        let dir = root.join(id);
        reject_symlink(&dir)?;
        if create {
            create_private_dir(&dir)?;
        }
        if !dir.is_dir() {
            return Err(EventStoreError::Invalid("session directory does not exist"));
        }
        let lock = lock::acquire(&dir)?;
        if !child_projection {
            lock::check_parent(root, &dir)?;
        }
        let file_path = dir.join("events.jsonl");
        reject_symlink(&file_path)?;
        let file = private_options()
            .read(true)
            .append(true)
            .create(create)
            .open(&file_path)?;
        if !file.metadata()?.is_file() {
            return Err(EventStoreError::Invalid("journal is not a regular file"));
        }
        let mut length = file.metadata()?.len();
        let mut reader = JournalReader::open(&file_path, length)?;
        let mut next_seq = 1;
        let mut repaired = false;
        while let Some(event) = reader.next() {
            let event = match event {
                Ok(event) => event,
                Err(error)
                    if recover
                        && reader.needs_newline()
                        && matches!(&error,EventStoreError::Json(e) if e.is_eof()) =>
                {
                    length = reader.validated_length();
                    recovery::preserve_tail(&file_path, &file, length)?;
                    repaired = true;
                    break;
                }
                Err(error) => return Err(error),
            };
            if event.run_id.as_str() != id {
                return Err(EventStoreError::Invalid("journal belongs to another run"));
            }
            next_seq = event
                .seq
                .checked_add(1)
                .ok_or(EventStoreError::Invalid("event sequence exhausted"))?;
        }
        let needs_newline = !repaired && reader.needs_newline();
        Ok(Self {
            state: Mutex::new(State {
                file: Some(file),
                writer_lock: Some(lock),
                memory: Vec::new(),
                next_seq,
                run_id: Some(id.into()),
                length,
                needs_newline,
                failed: false,
                closed: false,
            }),
            tx: broadcast::channel(256).0,
            file_path,
        })
    }
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
    pub fn next_seq(&self) -> Result<u64, EventStoreError> {
        Ok(self.state()?.next_seq)
    }
    fn state(&self) -> Result<MutexGuard<'_, State>, EventStoreError> {
        self.state.lock().map_err(|_| EventStoreError::LockPoisoned)
    }
    fn history(&self, state: &State, from: u64) -> Result<EventStream, EventStoreError> {
        if from >= state.next_seq {
            return Ok(Box::pin(tokio_stream::empty()));
        }
        if !self.file_path.as_os_str().is_empty() {
            Ok(Box::pin(
                tokio_stream::iter(JournalReader::open(&self.file_path, state.length)?)
                    .filter(move |item| item.as_ref().map_or(true, |e| e.seq >= from)),
            ))
        } else {
            let events: Vec<_> = state
                .memory
                .iter()
                .filter(|e| e.seq >= from)
                .cloned()
                .map(Ok)
                .collect();
            Ok(Box::pin(tokio_stream::iter(events)))
        }
    }
}

impl Default for Journal {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for Journal {
    fn append(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        self.append_applied(envelope, &mut |_| {})
    }
    fn append_applied(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
        apply: &mut dyn FnMut(&EventEnvelopeV1),
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        if matches!(
            envelope.payload,
            EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
        ) || matches!(&envelope.payload, EventV1::AssistantMessageFinished(e)
            if e.parts.iter().any(|part| matches!(part, crate::session::AssistantPart::Reasoning { .. })))
            || matches!(&envelope.payload, EventV1::ProviderRequestFinished(e)
                if e.metadata.as_ref().and_then(|m| m.thinking.as_ref()).is_some_and(|t| t.summary.is_some() || t.signature.is_some()))
        {
            return Err(EventStoreError::Invalid(
                "provider fragments and reasoning are live data and cannot be persisted",
            ));
        }
        let mut state = self.state()?;
        if state.closed {
            return Err(EventStoreError::Invalid("journal writer is closed"));
        }
        if state.failed {
            return Err(EventStoreError::Invalid(
                "journal write failed; reopen before writing",
            ));
        }
        if envelope.schema_version != crate::event::SCHEMA_VERSION
            || envelope.event_id.is_empty()
            || envelope.run_id.as_str().is_empty()
            || state
                .run_id
                .as_ref()
                .is_some_and(|id| id != envelope.run_id.as_str())
        {
            return Err(EventStoreError::Invalid("invalid event identity or schema"));
        }
        let next = state
            .next_seq
            .checked_add(1)
            .ok_or(EventStoreError::Invalid("event sequence exhausted"))?;
        let event = envelope.sequence(state.next_seq);
        let mut bytes = Vec::new();
        if state.needs_newline {
            bytes.push(b'\n');
        }
        serde_json::to_writer(&mut bytes, &event)?;
        bytes.push(b'\n');
        if bytes.len() > reader::MAX_RECORD_BYTES {
            return Err(EventStoreError::Invalid(
                "event exceeds journal record limit",
            ));
        }
        if let Some(file) = &mut state.file {
            if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_data()) {
                state.failed = true;
                return Err(error.into());
            }
        } else {
            state.memory.push(event.clone());
        }
        state.length +=
            u64::try_from(bytes.len()).map_err(|_| EventStoreError::Invalid("event too large"))?;
        state.needs_newline = false;
        state.next_seq = next;
        state.run_id.get_or_insert_with(|| event.run_id.to_string());
        apply(&event);
        let _ = self.tx.send(RuntimeEvent::Durable(Box::new(event.clone())));
        Ok(event)
    }
    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        self.history(&*self.state()?, from_seq)
    }
    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        Ok(Box::pin(self.subscribe_runtime(from_seq)?.filter_map(
            |event| match event {
                Ok(RuntimeEvent::Durable(e)) => Some(Ok(*e)),
                Ok(RuntimeEvent::Live(_)) => None,
                Err(e) => Some(Err(e)),
            },
        )))
    }
    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError> {
        let state = self.state()?;
        let rx = self.tx.subscribe();
        let history = self
            .history(&state, from_seq)?
            .map(|event| event.map(|e| RuntimeEvent::Durable(Box::new(e))));
        let live = BroadcastStream::new(rx)
            .map(|event| {
                event.map_err(|e| match e {
                    tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n) => {
                        EventStoreError::SubscriberLagged(n)
                    }
                })
            })
            .filter(
                move |event| !matches!(event, Ok(RuntimeEvent::Durable(e)) if e.seq < from_seq),
            );
        Ok(Box::pin(history.chain(live)))
    }
    fn publish_live(&self, event: LiveEventEnvelope) {
        let _ = self.tx.send(RuntimeEvent::Live(Box::new(event)));
    }
    fn close_writer(&self) -> Result<(), EventStoreError> {
        let mut state = self.state()?;
        state.closed = true;
        state.file.take();
        state.writer_lock.take();
        Ok(())
    }
}

fn reject_symlink(path: &Path) -> Result<(), EventStoreError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(EventStoreError::Invalid("session paths cannot be symlinks"))
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub trait EventStoreOpener: Send + Sync {
    fn open(&self, root: &Path, id: &str, deterministic: bool) -> Result<Journal, EventStoreError>;
    fn open_existing(
        &self,
        root: &Path,
        id: &str,
        deterministic: bool,
    ) -> Result<Journal, EventStoreError>;
}
#[derive(Debug, Default)]
pub struct JsonlEventStoreOpener;
impl EventStoreOpener for JsonlEventStoreOpener {
    fn open(&self, root: &Path, id: &str, deterministic: bool) -> Result<Journal, EventStoreError> {
        Journal::open(root, id, deterministic)
    }
    fn open_existing(
        &self,
        root: &Path,
        id: &str,
        deterministic: bool,
    ) -> Result<Journal, EventStoreError> {
        Journal::open_existing(root, id, deterministic)
    }
}

#[cfg(test)]
mod tests;

/// Atomically replaces a private derived file; the journal remains authoritative.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn create_private_dir(path: &Path) -> Result<(), std::io::Error> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(std::io::Error::other(
            "private directory must not be a symlink",
        ));
    }
    Ok(())
}

pub fn validate_session_id(id: &str) -> Result<(), EventStoreError> {
    if id.is_empty()
        || id.len() > 128
        || matches!(id, "." | "..")
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(EventStoreError::Invalid("invalid session directory name"));
    }
    Ok(())
}
