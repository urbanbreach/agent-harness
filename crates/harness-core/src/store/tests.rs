use super::{
    decode_jsonl_line, scan_events_from_cursor, serialize_jsonl_line, EventEnvelopeWithoutSeqV1,
    EventStore, EventStoreError, EventStream, InMemoryEventStore, JsonlFileEventStore, ScanCursor,
};
use crate::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
use crate::UnwrapOrAbort;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::thread;
use tokio::time::{timeout, Duration};
use tokio_stream::StreamExt;

#[test]
fn in_memory_append_assigns_monotonic_sequence_numbers() {
    // arrange
    // act
    // assert
    let store = InMemoryEventStore::new();

    let first = store
        .append(run_started_draft("run_mem", 1))
        .unwrap_or_abort();
    let second = store
        .append(run_started_draft("run_mem", 2))
        .unwrap_or_abort();

    assert_eq!(first.seq, 1);
    assert_eq!(second.seq, 2);
}

#[test]
fn jsonl_append_assigns_monotonic_sequence_numbers() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let store = JsonlFileEventStore::open(temp_dir.path(), "run_file", false).unwrap_or_abort();

    let first = store
        .append(run_started_draft("run_file", 1))
        .unwrap_or_abort();
    let second = store
        .append(run_started_draft("run_file", 2))
        .unwrap_or_abort();

    assert_eq!(first.seq, 1);
    assert_eq!(second.seq, 2);
}

#[test]
fn jsonl_serialization_builds_one_complete_record_buffer() {
    let envelope = run_started_draft("run_line", 1).with_seq(1);

    let line = serialize_jsonl_line(&envelope).unwrap_or_abort();

    assert_eq!(line.last(), Some(&b'\n'));
    assert!(!line[..line.len() - 1].contains(&b'\n'));
    let decoded = serde_json::from_slice::<crate::event::EventEnvelopeV1>(&line).unwrap_or_abort();
    assert_eq!(decoded, envelope);
}

#[test]
fn jsonl_line_decode_borrows_the_read_buffer() {
    let raw_line = b"{\"seq\":1}\r\n";

    let decoded = decode_jsonl_line(raw_line, Path::new("events.jsonl")).unwrap_or_abort();

    assert_eq!(decoded.as_ptr(), raw_line.as_ptr());
}

#[test]
fn jsonl_tail_scan_indexes_only_records_after_cursor() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let file_path = temp_dir.path().join("events.jsonl");
    let first =
        serialize_jsonl_line(&run_started_draft("run_tail", 1).with_seq(1)).unwrap_or_abort();
    let second =
        serialize_jsonl_line(&run_started_draft("run_tail", 2).with_seq(2)).unwrap_or_abort();
    let first_len = u64::try_from(first.len()).unwrap_or_abort();
    fs::write(&file_path, [first, second].concat()).unwrap_or_abort();

    let scan = scan_events_from_cursor(
        &file_path,
        ScanCursor {
            offset: first_len,
            next_seq: 2,
            line: 1,
        },
    )
    .unwrap_or_abort();

    let indexed = scan
        .index
        .iter()
        .map(|entry| (entry.seq, entry.line, entry.offset))
        .collect::<Vec<_>>();
    assert_eq!(indexed, vec![(2, 2, first_len)]);
}

#[test]
fn jsonl_open_scans_existing_file_to_resume_sequence() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();

    {
        let store =
            JsonlFileEventStore::open(temp_dir.path(), "run_resume", false).unwrap_or_abort();
        store
            .append(run_started_draft("run_resume", 1))
            .unwrap_or_abort();
        store
            .append(run_started_draft("run_resume", 2))
            .unwrap_or_abort();
    }

    let resumed = JsonlFileEventStore::open(temp_dir.path(), "run_resume", false).unwrap_or_abort();
    let appended = resumed
        .append(run_started_draft("run_resume", 3))
        .unwrap_or_abort();

    assert_eq!(appended.seq, 3);
}

#[cfg(target_os = "linux")]
#[test]
fn jsonl_open_recovers_dead_pid_writer_lock() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_stale_pid_lock");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").unwrap_or_abort();

    let store =
        JsonlFileEventStore::open(temp_dir.path(), "run_stale_pid_lock", false).unwrap_or_abort();

    assert!(store.file_path().exists());
    let lock_contents = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(lock_contents.contains(&format!("pid={}", std::process::id())));
}

#[cfg(target_os = "linux")]
#[test]
fn jsonl_open_serializes_concurrent_dead_pid_writer_lock_recovery() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_concurrent_stale_pid_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").unwrap_or_abort();

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_recovers_legacy_empty_writer_lock_before_log_exists() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_legacy_empty_lock");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "").unwrap_or_abort();

    let store = JsonlFileEventStore::open(temp_dir.path(), "run_legacy_empty_lock", false)
        .unwrap_or_abort();

    assert!(store.file_path().exists());
}

#[test]
fn jsonl_open_recovers_legacy_text_writer_lock_for_unborn_run_dir() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_legacy_text_lock");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    let store =
        JsonlFileEventStore::open(temp_dir.path(), "run_legacy_text_lock", false).unwrap_or_abort();

    assert!(store.file_path().exists());
}

#[test]
fn jsonl_open_serializes_concurrent_legacy_empty_writer_lock_recovery() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_concurrent_legacy_empty_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "").unwrap_or_abort();

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_serializes_concurrent_legacy_text_writer_lock_recovery() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_concurrent_legacy_text_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    assert_single_concurrent_writer(temp_dir.path(), run_id);
}

#[test]
fn jsonl_open_rejects_legacy_empty_writer_lock_when_log_exists() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_locked_existing_log");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "").unwrap_or_abort();

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_existing_log", false)
        .expect_err("existing log lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_log_exists() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_locked_text_existing_log");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_log", false)
        .expect_err("existing log lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_meta_exists() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_locked_text_existing_meta");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join("meta.json"), "{}").unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    let err = JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_meta", false)
        .expect_err("metadata lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_open_rejects_legacy_text_writer_lock_when_artifacts_exist() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_locked_text_existing_artifacts");
    fs::create_dir_all(run_dir.join("artifacts")).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    let err =
        JsonlFileEventStore::open(temp_dir.path(), "run_locked_text_existing_artifacts", false)
            .expect_err("artifacts lock should remain exclusive");

    assert!(matches!(err, EventStoreError::AcquireWriterLock { .. }));
}

#[test]
fn jsonl_writer_lock_drop_preserves_replaced_lock_file() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_replaced_lock");
    let lock_path = run_dir.join(".writer.lock");
    let store =
        JsonlFileEventStore::open(temp_dir.path(), "run_replaced_lock", false).unwrap_or_abort();

    fs::remove_file(&lock_path).unwrap_or_abort();
    fs::write(&lock_path, "pid=1\n").unwrap_or_abort();
    drop(store);

    assert_eq!(fs::read_to_string(&lock_path).unwrap_or_abort(), "pid=1\n");
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
        .map(|handle| handle.join().unwrap_or_abort())
        .filter(|success| *success)
        .count();
    assert_eq!(successes, 1, "exactly one writer should acquire the lock");
}

#[tokio::test]
async fn replay_from_seq_returns_expected_suffix() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let store = JsonlFileEventStore::open(temp_dir.path(), "run_replay", false).unwrap_or_abort();

    for marker in 1..=4 {
        store
            .append(run_started_draft("run_replay", marker))
            .unwrap_or_abort();
    }

    let replayed = collect_stream(store.replay(3).unwrap_or_abort()).await;
    let replayed_seqs: Vec<u64> = replayed.into_iter().map(|event| event.seq).collect();
    assert_eq!(replayed_seqs, vec![3, 4]);
}

#[tokio::test]
async fn jsonl_replay_extends_index_from_appended_tail() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_tail_refresh";
    let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false).unwrap_or_abort();
    store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
    let second = serialize_jsonl_line(&run_started_draft(run_id, 2).with_seq(2)).unwrap_or_abort();
    OpenOptions::new()
        .append(true)
        .open(store.file_path())
        .unwrap_or_abort()
        .write_all(&second)
        .unwrap_or_abort();

    let replayed = collect_stream(store.replay(2).unwrap_or_abort()).await;
    let appended = store.append(run_started_draft(run_id, 3)).unwrap_or_abort();

    let replayed_seqs = replayed.iter().map(|event| event.seq).collect::<Vec<_>>();
    assert_eq!((replayed_seqs, appended.seq), (vec![2], 3));
}

#[tokio::test]
async fn jsonl_replay_repairs_live_partial_tail_before_next_append() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_live_partial_tail";
    let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false).unwrap_or_abort();
    store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
    OpenOptions::new()
        .append(true)
        .open(store.file_path())
        .unwrap_or_abort()
        .write_all(b"{")
        .unwrap_or_abort();

    let replayed = collect_stream(store.replay(1).unwrap_or_abort()).await;
    let appended = store.append(run_started_draft(run_id, 2)).unwrap_or_abort();
    let contents = fs::read_to_string(store.file_path()).unwrap_or_abort();

    let replayed_seqs = replayed.iter().map(|event| event.seq).collect::<Vec<_>>();
    assert_eq!(
        (replayed_seqs, appended.seq, contents.lines().count()),
        (vec![1], 2, 2)
    );
}

#[tokio::test]
async fn subscribe_replays_then_streams_live_events() {
    // arrange
    // act
    // assert
    let store = InMemoryEventStore::new();
    store
        .append(run_started_draft("run_subscribe", 1))
        .unwrap_or_abort();
    store
        .append(run_started_draft("run_subscribe", 2))
        .unwrap_or_abort();

    let mut stream = store.subscribe(2).unwrap_or_abort();

    let replayed = timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(replayed.seq, 2);

    store
        .append(run_started_draft("run_subscribe", 3))
        .unwrap_or_abort();

    let live = timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(live.seq, 3);
}

#[tokio::test]
async fn jsonl_subscribe_replays_high_sequence_suffix_then_live_events() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let store =
        JsonlFileEventStore::open(temp_dir.path(), "run_file_subscribe", false).unwrap_or_abort();

    for marker in 1..=5 {
        store
            .append(run_started_draft("run_file_subscribe", marker))
            .unwrap_or_abort();
    }

    let mut stream = store.subscribe(5).unwrap_or_abort();
    let replayed = timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(replayed.seq, 5);

    store
        .append(run_started_draft("run_file_subscribe", 6))
        .unwrap_or_abort();

    let live = timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(live.seq, 6);
}

#[tokio::test]
async fn jsonl_replay_index_handles_crlf_logs() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_crlf_replay";
    let file_path = {
        let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false).unwrap_or_abort();
        for marker in 1..=3 {
            store
                .append(run_started_draft(run_id, marker))
                .unwrap_or_abort();
        }
        store.file_path().to_path_buf()
    };

    let lf_log = fs::read_to_string(&file_path).unwrap_or_abort();
    fs::write(&file_path, lf_log.replace('\n', "\r\n")).unwrap_or_abort();

    let store =
        JsonlFileEventStore::open_existing(temp_dir.path(), run_id, false).unwrap_or_abort();
    let replayed = collect_stream(store.replay(2).unwrap_or_abort()).await;
    let replayed_seqs: Vec<u64> = replayed.into_iter().map(|event| event.seq).collect();

    assert_eq!(replayed_seqs, vec![2, 3]);
}

#[tokio::test]
async fn jsonl_open_repairs_truncated_final_jsonl_line_and_preserves_complete_events() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_truncated_tail";
    let file_path = {
        let store = JsonlFileEventStore::open(temp_dir.path(), run_id, false).unwrap_or_abort();
        store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
        store.append(run_started_draft(run_id, 2)).unwrap_or_abort();
        store.file_path().to_path_buf()
    };

    OpenOptions::new()
        .append(true)
        .open(&file_path)
        .unwrap_or_abort()
        .write_all(b"{")
        .unwrap_or_abort();

    let repaired =
        JsonlFileEventStore::open_existing(temp_dir.path(), run_id, false).unwrap_or_abort();
    assert_eq!(repaired.next_seq().unwrap_or_abort(), 3);
    let replayed = collect_stream(repaired.replay(1).unwrap_or_abort()).await;
    let replayed_seqs = replayed.iter().map(|event| event.seq).collect::<Vec<_>>();
    assert_eq!(replayed_seqs, vec![1, 2]);

    let contents = fs::read_to_string(repaired.file_path()).unwrap_or_abort();
    assert!(!contents.ends_with('{'));
    assert_eq!(contents.lines().count(), 2);

    let appended = repaired
        .append(run_started_draft(run_id, 3))
        .unwrap_or_abort();
    assert_eq!(appended.seq, 3);
    let replayed = collect_stream(repaired.replay(1).unwrap_or_abort()).await;
    let replayed_seqs = replayed
        .into_iter()
        .map(|event| event.seq)
        .collect::<Vec<_>>();
    assert_eq!(replayed_seqs, vec![1, 2, 3]);
}

#[test]
fn replay_reports_invalid_json_lines_deterministically() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let store = JsonlFileEventStore::open(temp_dir.path(), "run_corrupt", true).unwrap_or_abort();

    store
        .append(run_started_draft("run_corrupt", 1))
        .unwrap_or_abort();

    let mut file = OpenOptions::new()
        .append(true)
        .open(store.file_path())
        .unwrap_or_abort();
    file.write_all(b"{invalid json}\n").unwrap_or_abort();

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
    // arrange
    // act
    // assert
    let temp_dir_a = tempfile::tempdir().unwrap_or_abort();
    let temp_dir_b = tempfile::tempdir().unwrap_or_abort();

    let file_a = run_deterministic_store_fixture(temp_dir_a.path());
    let file_b = run_deterministic_store_fixture(temp_dir_b.path());

    let bytes_a = fs::read(&file_a).unwrap_or_abort();
    let bytes_b = fs::read(&file_b).unwrap_or_abort();

    assert_eq!(bytes_a, bytes_b);
    assert_eq!(blake3::hash(&bytes_a), blake3::hash(&bytes_b));
}

fn run_deterministic_store_fixture(session_dir: &Path) -> String {
    let run_id = "run_deterministic";
    let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();

    for marker in 1..=3 {
        store
            .append(run_started_draft(run_id, marker))
            .unwrap_or_abort();
    }

    store.file_path().display().to_string()
}

fn run_started_draft(run_id: &str, marker: u64) -> EventEnvelopeWithoutSeqV1 {
    EventEnvelopeWithoutSeqV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{marker:04}"),
        run_id: run_id.to_string().into(),
        mono_ms: marker,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload: EventV1::RunStarted(RunStartedEvent {
            run_name: format!("run-{marker}").into(),
            workspace_root: "/workspace/project".to_string(),
        }),
    }
}

async fn collect_stream(mut stream: EventStream) -> Vec<crate::event::EventEnvelopeV1> {
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.unwrap_or_abort());
    }
    events
}
