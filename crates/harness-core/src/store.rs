// allow: SIZE_OK — event store (JSONL persistence + append sequencing)
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::{Stream, StreamExt};

use crate::event::{EventActor, EventEnvelopeV1, EventV1, LiveEventEnvelope, RuntimeEvent};
use crate::path_display::display_path;
use crate::session_paths::{
    ARTIFACTS_DIR_NAME, EVENTS_FILE_NAME, META_FILE_NAME, WRITER_LOCK_FILE_NAME,
};

const SUBSCRIBER_BUFFER: usize = 1024;
const WRITER_LOCK_RECOVERY_FILE_NAME: &str = ".writer.lock.recovering";
static NEXT_WRITER_LOCK_TOKEN: AtomicU64 = AtomicU64::new(1);

pub type EventStream = Pin<Box<dyn Stream<Item = Result<EventEnvelopeV1, EventStoreError>> + Send>>;
pub type RuntimeEventStream =
    Pin<Box<dyn Stream<Item = Result<RuntimeEvent, EventStoreError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeWithoutSeqV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub run_id: crate::ids::RunId,
    pub mono_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub actor: EventActor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,
    pub payload: EventV1,
}

impl EventEnvelopeWithoutSeqV1 {
    fn with_seq(self, seq: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: self.schema_version,
            event_id: self.event_id,
            seq,
            run_id: self.run_id,
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

impl From<EventEnvelopeV1> for EventEnvelopeWithoutSeqV1 {
    fn from(value: EventEnvelopeV1) -> Self {
        Self {
            schema_version: value.schema_version,
            event_id: value.event_id,
            run_id: value.run_id,
            mono_ms: value.mono_ms,
            ts: value.ts,
            actor: value.actor,
            correlation_id: value.correlation_id,
            causation_id: value.causation_id,
            stream_key: value.stream_key,
            payload: value.payload,
        }
    }
}

pub trait EventStore: Send + Sync {
    fn append(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError>;
    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError>;
    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError>;
    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError>;
    fn publish_live(&self, envelope: LiveEventEnvelope);
}

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("failed to create event directory {path}: {source}")]
    CreateDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open event log {path}: {source}")]
    OpenLog {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to acquire event writer lock {path}: {source}")]
    AcquireWriterLock {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("run directory does not exist: {path}")]
    RunDirectoryMissing { path: String },
    #[error("failed to read event log {path}: {source}")]
    ReadLog {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write event log {path}: {source}")]
    WriteLog {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize event envelope: {0}")]
    SerializeEnvelope(#[source] serde_json::Error),
    #[error("invalid JSONL event at line {line}: {source}")]
    InvalidJsonLine {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("non-monotonic event sequence at line {line}: expected {expected}, got {actual}")]
    NonMonotonicSequence {
        line: usize,
        expected: u64,
        actual: u64,
    },
    #[error("subscriber lagged by {0} messages")]
    SubscriberLagged(u64),
    #[error("event store lock poisoned")]
    LockPoisoned,
}

#[derive(Debug)]
pub struct InMemoryEventStore {
    state: Mutex<InMemoryState>,
    tx: broadcast::Sender<EventEnvelopeV1>,
    runtime_tx: broadcast::Sender<RuntimeEvent>,
}

#[derive(Debug)]
struct InMemoryState {
    next_seq: u64,
    events: Vec<EventEnvelopeV1>,
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(SUBSCRIBER_BUFFER);
        let (runtime_tx, _) = broadcast::channel(SUBSCRIBER_BUFFER);
        Self {
            state: Mutex::new(InMemoryState {
                next_seq: 1,
                events: Vec::new(),
            }),
            tx,
            runtime_tx,
        }
    }
}

impl EventStore for InMemoryEventStore {
    fn append(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        let mut state = lock_state(&self.state)?;
        let envelope = envelope.with_seq(state.next_seq);
        state.next_seq += 1;
        let event = envelope.clone();
        state.events.push(event.clone());
        let _ = self.tx.send(event.clone());
        let _ = self.runtime_tx.send(RuntimeEvent::Durable(Box::new(event)));
        Ok(envelope)
    }

    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let state = lock_state(&self.state)?;
        let replayed: Vec<_> = state
            .events
            .iter()
            .filter(|event| event.seq >= from_seq)
            .cloned()
            .collect();
        Ok(Box::pin(tokio_stream::iter(replayed.into_iter().map(Ok))))
    }

    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let (replayed, max_replayed_seq, rx) = {
            let state = lock_state(&self.state)?;
            let replayed: Vec<_> = state
                .events
                .iter()
                .filter(|event| event.seq >= from_seq)
                .cloned()
                .collect();
            let max_replayed_seq = replayed
                .last()
                .map(|event| event.seq)
                .unwrap_or_else(|| from_seq.saturating_sub(1));
            (replayed, max_replayed_seq, self.tx.subscribe())
        };

        let replay_stream = tokio_stream::iter(replayed.into_iter().map(Ok));
        let live_stream = broadcast_stream(rx, max_replayed_seq);
        Ok(Box::pin(replay_stream.chain(live_stream)))
    }

    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError> {
        let (replayed, max_replayed_seq, rx) = {
            let state = lock_state(&self.state)?;
            let replayed: Vec<_> = state
                .events
                .iter()
                .filter(|event| event.seq >= from_seq)
                .cloned()
                .collect();
            let max_replayed_seq = replayed
                .last()
                .map(|event| event.seq)
                .unwrap_or_else(|| from_seq.saturating_sub(1));
            (replayed, max_replayed_seq, self.runtime_tx.subscribe())
        };

        let replay_stream = tokio_stream::iter(
            replayed
                .into_iter()
                .map(Box::new)
                .map(RuntimeEvent::Durable)
                .map(Ok),
        );
        let live_stream = runtime_broadcast_stream(rx, max_replayed_seq);
        Ok(Box::pin(replay_stream.chain(live_stream)))
    }

    fn publish_live(&self, envelope: LiveEventEnvelope) {
        let _ = self.runtime_tx.send(RuntimeEvent::Live(Box::new(envelope)));
    }
}

#[derive(Debug)]
pub struct JsonlFileEventStore {
    _writer_lock: WriterLock,
    file_path: PathBuf,
    deterministic: bool,
    state: Mutex<JsonlState>,
    tx: broadcast::Sender<EventEnvelopeV1>,
    runtime_tx: broadcast::Sender<RuntimeEvent>,
}

pub trait EventStoreOpener: Send + Sync {
    fn open(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError>;

    fn open_existing(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError>;
}

#[derive(Debug, Default)]
pub struct JsonlEventStoreOpener;

impl EventStoreOpener for JsonlEventStoreOpener {
    fn open(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        JsonlFileEventStore::open(session_dir, run_id, deterministic)
    }

    fn open_existing(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        JsonlFileEventStore::open_existing(session_dir, run_id, deterministic)
    }
}

#[derive(Debug)]
struct WriterLock {
    path: PathBuf,
    contents: String,
    _file: File,
}

#[derive(Debug)]
struct WriterLockRecoveryGuard {
    path: PathBuf,
    contents: String,
    _file: File,
}

impl WriterLock {
    fn acquire(run_dir: &Path) -> Result<Self, EventStoreError> {
        let path = run_dir.join(WRITER_LOCK_FILE_NAME);
        match create_writer_lock(&path) {
            Ok((file, contents)) => Ok(Self {
                path,
                contents,
                _file: file,
            }),
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                let _recovery_guard = WriterLockRecoveryGuard::acquire(run_dir, &path)?;
                if stale_writer_lock(run_dir, &path) {
                    let _ = fs::remove_file(&path);
                    let (file, contents) = create_writer_lock(&path).map_err(|source| {
                        EventStoreError::AcquireWriterLock {
                            path: display_path(&path),
                            source,
                        }
                    })?;
                    Ok(Self {
                        path,
                        contents,
                        _file: file,
                    })
                } else {
                    Err(EventStoreError::AcquireWriterLock {
                        path: display_path(&path),
                        source,
                    })
                }
            }
            Err(source) => Err(EventStoreError::AcquireWriterLock {
                path: display_path(&path),
                source,
            }),
        }
    }
}

impl WriterLockRecoveryGuard {
    fn acquire(run_dir: &Path, writer_lock_path: &Path) -> Result<Self, EventStoreError> {
        let path = run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME);
        let (file, contents) =
            create_writer_lock(&path).map_err(|source| EventStoreError::AcquireWriterLock {
                path: display_path(writer_lock_path),
                source,
            })?;
        Ok(Self {
            path,
            contents,
            _file: file,
        })
    }
}

fn create_writer_lock(path: &Path) -> Result<(File, String), std::io::Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let token = NEXT_WRITER_LOCK_TOKEN.fetch_add(1, Ordering::Relaxed);
    let contents = format!("pid={}\ntoken={token}\n", std::process::id());
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok((file, contents))
}

fn stale_writer_lock(run_dir: &Path, path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    if contents.trim().is_empty() {
        return unborn_run_dir(run_dir);
    }
    let Some(pid) = contents.lines().find_map(|line| {
        line.strip_prefix("pid=")
            .and_then(|pid| pid.parse::<u32>().ok())
    }) else {
        return unborn_run_dir(run_dir);
    };

    !process_exists(pid)
}

pub(crate) fn unborn_run_dir(run_dir: &Path) -> bool {
    if run_dir.join(EVENTS_FILE_NAME).exists()
        || run_dir.join(META_FILE_NAME).exists()
        || run_dir.join(ARTIFACTS_DIR_NAME).exists()
    {
        return false;
    }

    let Ok(entries) = fs::read_dir(run_dir) else {
        return false;
    };
    entries.into_iter().all(|entry| {
        entry
            .ok()
            .and_then(|entry| entry.file_name().into_string().ok())
            .is_some_and(|name| {
                name == WRITER_LOCK_FILE_NAME || name == WRITER_LOCK_RECOVERY_FILE_NAME
            })
    })
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_exists(_pid: u32) -> bool {
    true
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.contents) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for WriterLockRecoveryGuard {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.contents) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug)]
struct JsonlState {
    file: File,
    next_seq: u64,
    replay_index: Vec<EventLogIndexEntry>,
    indexed_len: u64,
}

#[derive(Debug, Clone, Copy)]
struct EventLogIndexEntry {
    seq: u64,
    line: usize,
    offset: u64,
}

fn serialize_jsonl_line(envelope: &EventEnvelopeV1) -> Result<Vec<u8>, EventStoreError> {
    let mut line = serde_json::to_vec(envelope).map_err(EventStoreError::SerializeEnvelope)?;
    line.push(b'\n');
    Ok(line)
}

impl JsonlFileEventStore {
    pub fn open(
        session_dir: impl AsRef<Path>,
        run_id: impl AsRef<str>,
        deterministic: bool,
    ) -> Result<Self, EventStoreError> {
        Self::open_internal(session_dir, run_id, deterministic, true)
    }

    pub fn open_existing(
        session_dir: impl AsRef<Path>,
        run_id: impl AsRef<str>,
        deterministic: bool,
    ) -> Result<Self, EventStoreError> {
        Self::open_internal(session_dir, run_id, deterministic, false)
    }

    fn open_internal(
        session_dir: impl AsRef<Path>,
        run_id: impl AsRef<str>,
        deterministic: bool,
        create_run_dir: bool,
    ) -> Result<Self, EventStoreError> {
        let run_dir = session_dir.as_ref().join(run_id.as_ref());
        if create_run_dir {
            fs::create_dir_all(&run_dir).map_err(|source| EventStoreError::CreateDirectory {
                path: display_path(&run_dir),
                source,
            })?;
        } else if !run_dir.is_dir() {
            return Err(EventStoreError::RunDirectoryMissing {
                path: display_path(&run_dir),
            });
        }

        let writer_lock = WriterLock::acquire(&run_dir)?;

        let file_path = run_dir.join(EVENTS_FILE_NAME);
        let mut options = OpenOptions::new();
        options.append(true);
        if create_run_dir {
            options.create(true);
        }
        let file = options
            .open(&file_path)
            .map_err(|source| EventStoreError::OpenLog {
                path: display_path(&file_path),
                source,
            })?;

        let scan = scan_events_from_file(&file_path)?;
        let (tx, _) = broadcast::channel(SUBSCRIBER_BUFFER);
        let (runtime_tx, _) = broadcast::channel(SUBSCRIBER_BUFFER);

        Ok(Self {
            _writer_lock: writer_lock,
            file_path,
            deterministic,
            state: Mutex::new(JsonlState {
                file,
                next_seq: scan.next_seq,
                replay_index: scan.index,
                indexed_len: scan.file_len,
            }),
            tx,
            runtime_tx,
        })
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub fn next_seq(&self) -> Result<u64, EventStoreError> {
        let state = lock_state(&self.state)?;
        Ok(state.next_seq)
    }
}

impl EventStore for JsonlFileEventStore {
    fn append(
        &self,
        envelope: EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        let mut state = lock_state(&self.state)?;
        let envelope = envelope.with_seq(state.next_seq);
        let record = serialize_jsonl_line(&envelope)?;
        let offset = state.indexed_len;

        state
            .file
            .write_all(&record)
            .map_err(|source| EventStoreError::WriteLog {
                path: display_path(&self.file_path),
                source,
            })?;

        if self.deterministic {
            state
                .file
                .flush()
                .and_then(|_| state.file.sync_data())
                .map_err(|source| EventStoreError::WriteLog {
                    path: display_path(&self.file_path),
                    source,
                })?;
        }

        state.next_seq += 1;
        let line = state.replay_index.len() + 1;
        state.replay_index.push(EventLogIndexEntry {
            seq: envelope.seq,
            line,
            offset,
        });
        state.indexed_len = state
            .indexed_len
            .saturating_add(u64::try_from(record.len()).unwrap_or(0));
        drop(state);

        let _ = self.tx.send(envelope.clone());
        let _ = self
            .runtime_tx
            .send(RuntimeEvent::Durable(Box::new(envelope.clone())));
        Ok(envelope)
    }

    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let replayed = {
            let mut state = lock_state(&self.state)?;
            replay_events_from_index(&self.file_path, &mut state, from_seq)?
        };

        Ok(Box::pin(tokio_stream::iter(replayed.into_iter().map(Ok))))
    }

    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let (replayed, max_replayed_seq, rx) = {
            let mut state = lock_state(&self.state)?;
            let replayed = replay_events_from_index(&self.file_path, &mut state, from_seq)?;
            let max_replayed_seq = replayed
                .last()
                .map(|event| event.seq)
                .unwrap_or_else(|| from_seq.saturating_sub(1));
            (replayed, max_replayed_seq, self.tx.subscribe())
        };

        let replay_stream = tokio_stream::iter(replayed.into_iter().map(Ok));
        let live_stream = broadcast_stream(rx, max_replayed_seq);
        Ok(Box::pin(replay_stream.chain(live_stream)))
    }

    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError> {
        let (replayed, max_replayed_seq, rx) = {
            let mut state = lock_state(&self.state)?;
            let replayed = replay_events_from_index(&self.file_path, &mut state, from_seq)?;
            let max_replayed_seq = replayed
                .last()
                .map(|event| event.seq)
                .unwrap_or_else(|| from_seq.saturating_sub(1));
            (replayed, max_replayed_seq, self.runtime_tx.subscribe())
        };

        let replay_stream = tokio_stream::iter(
            replayed
                .into_iter()
                .map(Box::new)
                .map(RuntimeEvent::Durable)
                .map(Ok),
        );
        let live_stream = runtime_broadcast_stream(rx, max_replayed_seq);
        Ok(Box::pin(replay_stream.chain(live_stream)))
    }

    fn publish_live(&self, envelope: LiveEventEnvelope) {
        let _ = self.runtime_tx.send(RuntimeEvent::Live(Box::new(envelope)));
    }
}

fn runtime_broadcast_stream(
    rx: broadcast::Receiver<RuntimeEvent>,
    min_durable_seq_exclusive: u64,
) -> impl Stream<Item = Result<RuntimeEvent, EventStoreError>> {
    BroadcastStream::new(rx).filter_map(move |item| match item {
        Ok(RuntimeEvent::Durable(event)) if event.seq > min_durable_seq_exclusive => {
            Some(Ok(RuntimeEvent::Durable(event)))
        }
        Ok(RuntimeEvent::Durable(_)) => None,
        Ok(RuntimeEvent::Live(event)) => Some(Ok(RuntimeEvent::Live(event))),
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            Some(Err(EventStoreError::SubscriberLagged(skipped)))
        }
    })
}

fn broadcast_stream(
    rx: broadcast::Receiver<EventEnvelopeV1>,
    min_seq_exclusive: u64,
) -> impl Stream<Item = Result<EventEnvelopeV1, EventStoreError>> {
    BroadcastStream::new(rx).filter_map(move |item| match item {
        Ok(event) if event.seq > min_seq_exclusive => Some(Ok(event)),
        Ok(_) => None,
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            Some(Err(EventStoreError::SubscriberLagged(skipped)))
        }
    })
}

fn lock_state<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, EventStoreError> {
    mutex.lock().map_err(|_| EventStoreError::LockPoisoned)
}

struct ScanResult {
    index: Vec<EventLogIndexEntry>,
    next_seq: u64,
    file_len: u64,
}

fn scan_events_from_file(file_path: &Path) -> Result<ScanResult, EventStoreError> {
    scan_events_from_cursor(
        file_path,
        ScanCursor {
            offset: 0,
            next_seq: 1,
            line: 0,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct ScanCursor {
    offset: u64,
    next_seq: u64,
    line: usize,
}

fn scan_events_from_cursor(
    file_path: &Path,
    cursor: ScanCursor,
) -> Result<ScanResult, EventStoreError> {
    let mut file = File::open(file_path).map_err(|source| EventStoreError::ReadLog {
        path: display_path(file_path),
        source,
    })?;
    let mut file_len = file
        .metadata()
        .map(|metadata| metadata.len())
        .map_err(|source| EventStoreError::ReadLog {
            path: display_path(file_path),
            source,
        })?;

    file.seek(SeekFrom::Start(cursor.offset))
        .map_err(|source| EventStoreError::ReadLog {
            path: display_path(file_path),
            source,
        })?;

    let mut index = Vec::new();
    let mut expected_seq = cursor.next_seq;
    let mut offset = cursor.offset;

    let mut reader = BufReader::new(file);
    let mut raw_line = Vec::new();
    let mut line_number = cursor.line;
    loop {
        raw_line.clear();
        let bytes_read =
            reader
                .read_until(b'\n', &mut raw_line)
                .map_err(|source| EventStoreError::ReadLog {
                    path: display_path(file_path),
                    source,
                })?;
        if bytes_read == 0 {
            break;
        }

        line_number += 1;
        let line_offset = offset;
        offset = offset.saturating_add(u64::try_from(bytes_read).unwrap_or(0));
        let terminated = raw_line.ends_with(b"\n");
        let line = match decode_jsonl_line(&raw_line, file_path) {
            Ok(line) => line,
            Err(_) if !terminated => {
                repair_truncated_jsonl_tail(file_path, line_offset)?;
                file_len = line_offset;
                break;
            }
            Err(err) => return Err(err),
        };

        let event: EventEnvelopeV1 = match serde_json::from_str(&line) {
            Ok(event) => event,
            Err(_) if !terminated => {
                repair_truncated_jsonl_tail(file_path, line_offset)?;
                file_len = line_offset;
                break;
            }
            Err(source) => {
                return Err(EventStoreError::InvalidJsonLine {
                    line: line_number,
                    source,
                })
            }
        };

        if event.seq != expected_seq {
            return Err(EventStoreError::NonMonotonicSequence {
                line: line_number,
                expected: expected_seq,
                actual: event.seq,
            });
        }

        index.push(EventLogIndexEntry {
            seq: event.seq,
            line: line_number,
            offset: line_offset,
        });

        expected_seq += 1;

        if !terminated {
            append_missing_jsonl_newline(file_path)?;
            file_len = file_len.saturating_add(1);
            break;
        }
    }

    Ok(ScanResult {
        index,
        next_seq: expected_seq,
        file_len,
    })
}

fn repair_truncated_jsonl_tail(file_path: &Path, len: u64) -> Result<(), EventStoreError> {
    let file = OpenOptions::new()
        .write(true)
        .open(file_path)
        .map_err(|source| EventStoreError::WriteLog {
            path: display_path(file_path),
            source,
        })?;
    file.set_len(len)
        .and_then(|_| file.sync_data())
        .map_err(|source| EventStoreError::WriteLog {
            path: display_path(file_path),
            source,
        })
}

fn append_missing_jsonl_newline(file_path: &Path) -> Result<(), EventStoreError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(file_path)
        .map_err(|source| EventStoreError::WriteLog {
            path: display_path(file_path),
            source,
        })?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_data())
        .map_err(|source| EventStoreError::WriteLog {
            path: display_path(file_path),
            source,
        })
}

fn decode_jsonl_line<'line>(
    raw_line: &'line [u8],
    file_path: &Path,
) -> Result<&'line str, EventStoreError> {
    let mut line = raw_line;
    if let Some(stripped) = line.strip_suffix(b"\n") {
        line = stripped;
        if let Some(stripped) = line.strip_suffix(b"\r") {
            line = stripped;
        }
    }

    std::str::from_utf8(line).map_err(|source| EventStoreError::ReadLog {
        path: display_path(file_path),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
    })
}

fn replay_events_from_index(
    file_path: &Path,
    state: &mut JsonlState,
    from_seq: u64,
) -> Result<Vec<EventEnvelopeV1>, EventStoreError> {
    let current_len = state
        .file
        .metadata()
        .map(|metadata| metadata.len())
        .map_err(|source| EventStoreError::ReadLog {
            path: display_path(file_path),
            source,
        })?;

    if current_len < state.indexed_len {
        let scan = scan_events_from_file(file_path)?;
        state.next_seq = scan.next_seq;
        state.replay_index = scan.index;
        state.indexed_len = scan.file_len;
    } else if current_len > state.indexed_len {
        let scan = scan_events_from_cursor(
            file_path,
            ScanCursor {
                offset: state.indexed_len,
                next_seq: state.next_seq,
                line: state.replay_index.len(),
            },
        )?;
        state.next_seq = scan.next_seq;
        state.replay_index.extend(scan.index);
        state.indexed_len = scan.file_len;
    }

    let start_index = state
        .replay_index
        .partition_point(|entry| entry.seq < from_seq);

    let Some(first_entry) = state.replay_index.get(start_index).copied() else {
        return Ok(Vec::new());
    };

    let mut file = File::open(file_path).map_err(|source| EventStoreError::ReadLog {
        path: display_path(file_path),
        source,
    })?;
    file.seek(SeekFrom::Start(first_entry.offset))
        .map_err(|source| EventStoreError::ReadLog {
            path: display_path(file_path),
            source,
        })?;

    let mut events = Vec::new();
    let mut expected_seq = first_entry.seq;
    let mut line_number = first_entry.line.saturating_sub(1);
    let mut reader = BufReader::new(file);
    let mut raw_line = Vec::new();
    loop {
        raw_line.clear();
        let bytes_read =
            reader
                .read_until(b'\n', &mut raw_line)
                .map_err(|source| EventStoreError::ReadLog {
                    path: display_path(file_path),
                    source,
                })?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;
        let line = decode_jsonl_line(&raw_line, file_path)?;
        let event: EventEnvelopeV1 =
            serde_json::from_str(line).map_err(|source| EventStoreError::InvalidJsonLine {
                line: line_number,
                source,
            })?;
        if event.seq != expected_seq {
            return Err(EventStoreError::NonMonotonicSequence {
                line: line_number,
                expected: expected_seq,
                actual: event.seq,
            });
        }
        events.push(event);
        expected_seq += 1;
    }

    Ok(events)
}

#[cfg(test)]
mod tests;
