use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::{Stream, StreamExt};

use crate::event::{EventActor, EventEnvelopeV1, EventV1};

const EVENTS_FILE_NAME: &str = "events.jsonl";
const WRITER_LOCK_FILE_NAME: &str = ".writer.lock";
const SUBSCRIBER_BUFFER: usize = 1024;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<EventEnvelopeV1, EventStoreError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeWithoutSeqV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub run_id: String,
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
        Self {
            state: Mutex::new(InMemoryState {
                next_seq: 1,
                events: Vec::new(),
            }),
            tx,
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
        state.events.push(envelope.clone());
        let _ = self.tx.send(envelope.clone());
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
}

#[derive(Debug)]
pub struct JsonlFileEventStore {
    _writer_lock: WriterLock,
    file_path: PathBuf,
    deterministic: bool,
    state: Mutex<JsonlState>,
    tx: broadcast::Sender<EventEnvelopeV1>,
}

#[derive(Debug)]
struct WriterLock {
    path: PathBuf,
    _file: File,
}

impl WriterLock {
    fn acquire(run_dir: &Path) -> Result<Self, EventStoreError> {
        let path = run_dir.join(WRITER_LOCK_FILE_NAME);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| EventStoreError::AcquireWriterLock {
                path: display_path(&path),
                source,
            })?;

        Ok(Self { path, _file: file })
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
struct JsonlState {
    file: File,
    next_seq: u64,
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

        let next_seq = scan_events_from_file(&file_path, None)?.next_seq;
        let (tx, _) = broadcast::channel(SUBSCRIBER_BUFFER);

        Ok(Self {
            _writer_lock: writer_lock,
            file_path,
            deterministic,
            state: Mutex::new(JsonlState { file, next_seq }),
            tx,
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
        let serialized =
            serde_json::to_string(&envelope).map_err(EventStoreError::SerializeEnvelope)?;

        state
            .file
            .write_all(serialized.as_bytes())
            .and_then(|_| state.file.write_all(b"\n"))
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
        drop(state);

        let _ = self.tx.send(envelope.clone());
        Ok(envelope)
    }

    fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let replayed = {
            let _state = lock_state(&self.state)?;
            scan_events_from_file(&self.file_path, Some(from_seq))?.events
        };

        Ok(Box::pin(tokio_stream::iter(replayed.into_iter().map(Ok))))
    }

    fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
        let (replayed, max_replayed_seq, rx) = {
            let _state = lock_state(&self.state)?;
            let replayed = scan_events_from_file(&self.file_path, Some(from_seq))?.events;
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

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

struct ScanResult {
    events: Vec<EventEnvelopeV1>,
    next_seq: u64,
}

fn scan_events_from_file(
    file_path: &Path,
    from_seq: Option<u64>,
) -> Result<ScanResult, EventStoreError> {
    let file = File::open(file_path).map_err(|source| EventStoreError::ReadLog {
        path: display_path(file_path),
        source,
    })?;

    let mut events = Vec::new();
    let mut expected_seq = 1;

    for (index, line_result) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = line_result.map_err(|source| EventStoreError::ReadLog {
            path: display_path(file_path),
            source,
        })?;

        let event: EventEnvelopeV1 =
            serde_json::from_str(&line).map_err(|source| EventStoreError::InvalidJsonLine {
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

        if from_seq.is_some_and(|start_seq| event.seq >= start_seq) {
            events.push(event);
        }

        expected_seq += 1;
    }

    Ok(ScanResult {
        events,
        next_seq: expected_seq,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, EventStream, InMemoryEventStore,
        JsonlFileEventStore,
    };
    use crate::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use tokio::time::{timeout, Duration};
    use tokio_stream::StreamExt;

    #[test]
    fn in_memory_append_assigns_monotonic_sequence_numbers() {
        let store = InMemoryEventStore::new();

        let first = store
            .append(run_started_draft("run_mem", 1))
            .expect("append first event");
        let second = store
            .append(run_started_draft("run_mem", 2))
            .expect("append second event");

        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn jsonl_append_assigns_monotonic_sequence_numbers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = JsonlFileEventStore::open(temp_dir.path(), "run_file", false)
            .expect("open jsonl event store");

        let first = store
            .append(run_started_draft("run_file", 1))
            .expect("append first event");
        let second = store
            .append(run_started_draft("run_file", 2))
            .expect("append second event");

        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn jsonl_open_scans_existing_file_to_resume_sequence() {
        let temp_dir = tempfile::tempdir().expect("tempdir");

        {
            let store = JsonlFileEventStore::open(temp_dir.path(), "run_resume", false)
                .expect("open jsonl event store");
            store
                .append(run_started_draft("run_resume", 1))
                .expect("append first event");
            store
                .append(run_started_draft("run_resume", 2))
                .expect("append second event");
        }

        let resumed = JsonlFileEventStore::open(temp_dir.path(), "run_resume", false)
            .expect("reopen jsonl event store");
        let appended = resumed
            .append(run_started_draft("run_resume", 3))
            .expect("append resumed event");

        assert_eq!(appended.seq, 3);
    }

    #[tokio::test]
    async fn replay_from_seq_returns_expected_suffix() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = JsonlFileEventStore::open(temp_dir.path(), "run_replay", false)
            .expect("open jsonl event store");

        for marker in 1..=4 {
            store
                .append(run_started_draft("run_replay", marker))
                .expect("append event");
        }

        let replayed = collect_stream(store.replay(3).expect("build replay stream")).await;
        let replayed_seqs: Vec<u64> = replayed.into_iter().map(|event| event.seq).collect();
        assert_eq!(replayed_seqs, vec![3, 4]);
    }

    #[tokio::test]
    async fn subscribe_replays_then_streams_live_events() {
        let store = InMemoryEventStore::new();
        store
            .append(run_started_draft("run_subscribe", 1))
            .expect("append first event");
        store
            .append(run_started_draft("run_subscribe", 2))
            .expect("append second event");

        let mut stream = store.subscribe(2).expect("build subscribe stream");

        let replayed = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("first event should arrive")
            .expect("stream should not end")
            .expect("stream item should be valid");
        assert_eq!(replayed.seq, 2);

        store
            .append(run_started_draft("run_subscribe", 3))
            .expect("append live event");

        let live = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("live event should arrive")
            .expect("stream should not end")
            .expect("stream item should be valid");
        assert_eq!(live.seq, 3);
    }

    #[test]
    fn replay_reports_invalid_json_lines_deterministically() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = JsonlFileEventStore::open(temp_dir.path(), "run_corrupt", true)
            .expect("open jsonl event store");

        store
            .append(run_started_draft("run_corrupt", 1))
            .expect("append first event");

        let mut file = OpenOptions::new()
            .append(true)
            .open(store.file_path())
            .expect("open events file for corruption fixture");
        file.write_all(b"{invalid json}\n")
            .expect("write corruption fixture");

        let err = match store.replay(1) {
            Ok(_) => panic!("replay should fail"),
            Err(err) => err,
        };
        assert!(
            matches!(err, EventStoreError::InvalidJsonLine { line: 2, .. }),
            "expected invalid JSON line error, got: {err}"
        );
    }

    #[test]
    fn deterministic_mode_writes_byte_identical_jsonl() {
        let temp_dir_a = tempfile::tempdir().expect("tempdir a");
        let temp_dir_b = tempfile::tempdir().expect("tempdir b");

        let file_a = run_deterministic_store_fixture(temp_dir_a.path());
        let file_b = run_deterministic_store_fixture(temp_dir_b.path());

        let bytes_a = fs::read(&file_a).expect("read first jsonl");
        let bytes_b = fs::read(&file_b).expect("read second jsonl");

        assert_eq!(bytes_a, bytes_b);
        assert_eq!(blake3::hash(&bytes_a), blake3::hash(&bytes_b));
    }

    fn run_deterministic_store_fixture(session_dir: &Path) -> String {
        let run_id = "run_deterministic";
        let store = JsonlFileEventStore::open(session_dir, run_id, true)
            .expect("open deterministic jsonl event store");

        for marker in 1..=3 {
            store
                .append(run_started_draft(run_id, marker))
                .expect("append deterministic fixture event");
        }

        store.file_path().display().to_string()
    }

    fn run_started_draft(run_id: &str, marker: u64) -> EventEnvelopeWithoutSeqV1 {
        EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{marker:04}"),
            run_id: run_id.to_string(),
            mono_ms: marker,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: format!("run-{marker}"),
                workspace_root: "/workspace/project".to_string(),
            }),
        }
    }

    async fn collect_stream(mut stream: EventStream) -> Vec<crate::event::EventEnvelopeV1> {
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.expect("stream item should be valid"));
        }
        events
    }
}
