use super::{
    EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, EventStream, InMemoryEventStore,
    JsonlFileEventStore,
};
use crate::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::thread;
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

#[cfg(target_os = "linux")]
#[test]
fn jsonl_open_recovers_dead_pid_writer_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_stale_pid_lock");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").expect("write stale lock");

    let store = JsonlFileEventStore::open(temp_dir.path(), "run_stale_pid_lock", false)
        .expect("open store after stale lock recovery");

    assert!(store.file_path().exists());
    let lock_contents = fs::read_to_string(run_dir.join(".writer.lock")).expect("read lock");
    assert!(lock_contents.contains(&format!("pid={}", std::process::id())));
}

#[cfg(target_os = "linux")]
#[test]
fn jsonl_open_serializes_concurrent_dead_pid_writer_lock_recovery() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_concurrent_stale_pid_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").expect("write stale lock");

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_recovers_legacy_empty_writer_lock_before_log_exists() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_legacy_empty_lock");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "").expect("write legacy lock");

    let store = JsonlFileEventStore::open(temp_dir.path(), "run_legacy_empty_lock", false)
        .expect("open store after legacy lock recovery");

    assert!(store.file_path().exists());
}

#[test]
fn jsonl_open_recovers_legacy_text_writer_lock_for_unborn_run_dir() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_legacy_text_lock");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "locked").expect("write legacy lock");

    let store = JsonlFileEventStore::open(temp_dir.path(), "run_legacy_text_lock", false)
        .expect("open store after legacy text lock recovery");

    assert!(store.file_path().exists());
}

#[test]
fn jsonl_open_serializes_concurrent_legacy_empty_writer_lock_recovery() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_concurrent_legacy_empty_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "").expect("write legacy lock");

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_serializes_concurrent_legacy_text_writer_lock_recovery() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_concurrent_legacy_text_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join(".writer.lock"), "locked").expect("write legacy lock");

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_rejects_legacy_empty_writer_lock_when_log_exists() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_locked_existing_log");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("events.jsonl"), "").expect("write event log");
    fs::write(run_dir.join(".writer.lock"), "").expect("write legacy lock");

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_existing_log", false)
        .expect_err("existing log lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_log_exists() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_locked_text_existing_log");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("events.jsonl"), "").expect("write event log");
    fs::write(run_dir.join(".writer.lock"), "locked").expect("write legacy lock");

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_log", false)
        .expect_err("existing log lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_meta_exists() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_locked_text_existing_meta");
    fs::create_dir_all(&run_dir).expect("create run dir");
    fs::write(run_dir.join("meta.json"), "{}").expect("write metadata");
    fs::write(run_dir.join(".writer.lock"), "locked").expect("write legacy lock");

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_meta", false)
        .expect_err("metadata lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_artifacts_exist() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_locked_text_existing_artifacts");
    fs::create_dir_all(run_dir.join("artifacts")).expect("create artifacts dir");
    fs::write(run_dir.join(".writer.lock"), "locked").expect("write legacy lock");

    let err =
        JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_artifacts", false)
            .expect_err("artifacts lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_writer_lock_drop_preserves_replaced_lock_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_replaced_lock");
    let lock_path = run_dir.join(".writer.lock");
    let store = JsonlFileEventStore::open(temp_dir.path(), "run_replaced_lock", false)
        .expect("open jsonl event store");

    fs::remove_file(&lock_path).expect("replace lock by removing original");
    fs::write(&lock_path, "pid=1\n").expect("write replacement lock");
    drop(store);

    assert_eq!(
        fs::read_to_string(&lock_path).expect("replacement lock should remain"),
        "pid=1\n"
    );
}

fn assert_single_concurrent_writer(session_dir: &Path, run_id: &str) {
    let start = Arc::new(Barrier::new(3));
    let finish = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let session_dir = session_dir.to_path_buf();
            let run_id = run_id.to_string();
            let start = Arc::clone(&start);
            let finish = Arc::clone(&finish);
            thread::spawn(move || {
                start.wait();
                let result = JsonlFileEventStore::open(&session_dir, &run_id, false);
                match result {
                    Ok(store) => {
                        finish.wait();
                        drop(store);
                        true
                    }
                    Err(EventStoreError::AcquireWriterLock { .. }) => {
                        finish.wait();
                        false
                    }
                    Err(err) => panic!("unexpected open error: {err}"),
                }
            })
        })
        .collect::<Vec<_>>();

    start.wait();
    finish.wait();

    let successes = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread should not panic"))
        .filter(|success| *success)
        .count();
    assert_eq!(successes, 1, "exactly one writer should acquire the lock");
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

#[tokio::test]
async fn jsonl_subscribe_replays_high_sequence_suffix_then_live_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = JsonlFileEventStore::open(temp_dir.path(), "run_file_subscribe", false)
        .expect("open jsonl event store");

    for marker in 1..=5 {
        store
            .append(run_started_draft("run_file_subscribe", marker))
            .expect("append event");
    }

    let mut stream = store.subscribe(5).expect("build subscribe stream");
    let replayed = timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("replayed event should arrive")
        .expect("stream should not end")
        .expect("stream item should be valid");
    assert_eq!(replayed.seq, 5);

    store
        .append(run_started_draft("run_file_subscribe", 6))
        .expect("append live event");

    let live = timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("live event should arrive")
        .expect("stream should not end")
        .expect("stream item should be valid");
    assert_eq!(live.seq, 6);
}

#[tokio::test]
async fn jsonl_replay_index_handles_crlf_logs() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_crlf_replay";
    let file_path = {
        let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false)
            .expect("open jsonl event store");
        for marker in 1..=3 {
            store
                .append(run_started_draft(run_id, marker))
                .expect("append event");
        }
        store.file_path().to_path_buf()
    };

    let lf_log = fs::read_to_string(&file_path).expect("read lf log");
    fs::write(&file_path, lf_log.replace('\n', "\r\n")).expect("rewrite as crlf log");

    let store = JsonlFileEventStore::open_existing(temp_dir.path(), run_id, false)
        .expect("reopen crlf jsonl event store");
    let replayed = collect_stream(store.replay(2).expect("build replay stream")).await;
    let replayed_seqs: Vec<u64> = replayed.into_iter().map(|event| event.seq).collect();

    assert_eq!(replayed_seqs, vec![2, 3]);
}

#[tokio::test]
async fn jsonl_open_repairs_truncated_final_jsonl_line_and_preserves_complete_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_truncated_tail";
    let file_path = {
        let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false)
            .expect("open jsonl event store");
        store
            .append(run_started_draft(run_id, 1))
            .expect("append first event");
        store
            .append(run_started_draft(run_id, 2))
            .expect("append second event");
        store.file_path().to_path_buf()
    };

    OpenOptions::new()
        .append(true)
        .open(&file_path)
        .expect("open events file for truncated tail")
        .write_all(b"{")
        .expect("write truncated tail");

    let repaired = JsonlFileEventStore::open_existing(temp_dir.path(), run_id, false)
        .expect("reopen should repair truncated final JSONL line");
    assert_eq!(repaired.next_seq().expect("read next seq"), 3);
    let replayed = collect_stream(repaired.replay(1).expect("build replay stream")).await;
    let replayed_seqs = replayed.iter().map(|event| event.seq).collect::<Vec<_>>();
    assert_eq!(replayed_seqs, vec![1, 2]);

    let contents = fs::read_to_string(repaired.file_path()).expect("read repaired log");
    assert!(!contents.ends_with('{'));
    assert_eq!(contents.lines().count(), 2);

    let appended = repaired
        .append(run_started_draft(run_id, 3))
        .expect("append after truncated tail repair");
    assert_eq!(appended.seq, 3);
    let replayed = collect_stream(repaired.replay(1).expect("build replay stream")).await;
    let replayed_seqs = replayed
        .into_iter()
        .map(|event| event.seq)
        .collect::<Vec<_>>();
    assert_eq!(replayed_seqs, vec![1, 2, 3]);
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
