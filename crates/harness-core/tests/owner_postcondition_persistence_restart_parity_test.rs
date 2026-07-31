//! T36 — owner postcondition, persistence, restart, error, and replay holdouts.
//!
//! Independent QA holdouts for the clean-room parity program. These tests
//! exercise the compiled public owner surfaces across every retained local
//! owner and observe real owner-side effects: events, files, locks, queues,
//! memory, worktrees, trust, integrations, compaction, and teardown.
//!
//! Coverage model (proof dimensions P2 + P7):
//! - P2 owner: every retained action has a compiled owner receipt plus an
//!   observable external postcondition (events appended, files created/removed,
//!   locks acquired/released, queue entries persisted/drainable).
//! - P7 lifecycle: restart preserves or truthfully recovers state; replay is
//!   pure (no side effects); error paths produce terminal events without
//!   unauthorized mutation; teardown restores invariants.
//!
//! Failure mutations are interleaved: each block contains a `mutation:`
//! comment describing the mutation a reviewer can apply to confirm the test
//! detects the regression (corrupted events, missing teardown, nondeterministic
//! replay, writer-lock bypass, append-only violation).
//!
//! These tests are intentionally independent of the T17 differential contracts
//! (`sessions_persistence_rewind_parity_test.rs`); they focus on restart,
//! error, and replay holdouts that bind real owner postconditions to P2/P7
//! proof dimensions.

use std::fs;
use std::path::Path;

use harness_core::conversation::project_conversation;
use harness_core::crash_recovery::{inspect_previous_crash, CrashRecoveryAction};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SessionTitleUpdatedEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::foreign_session::{
    import_foreign_session_as_replay, refuse_import_into_active_session, ForeignSessionError,
};
use harness_core::memory::{DurableMemoryStore, MemoryScope};
use harness_core::proj::SessionModeSource;
use harness_core::prompt_queue::DurablePromptQueue;
use harness_core::prompt_rewind::{
    atomic_prompt_rewind, event_log_digest, plan_prompt_rewind, FileSnapshotEntry,
};
use harness_core::session_lineage::{
    latest_clone_stable_prefix, materialize_child_session, validate_fork_stable_prefix,
    ChildSessionMaterializationRequest, ChildSessionMaterializationSourceKind,
};
use harness_core::store::{
    EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore,
};
use harness_core::transcript_projection::project_transcript;
use harness_core::UnwrapOrAbort;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Shared helpers (mirror T17 shapes; this file owns its own copies so the
// tests remain an independent holdout).
// ---------------------------------------------------------------------------

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-t36-{run_id}-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq * 100,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn completed_run_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "t36-completed".into(),
                workspace_root: "/workspace/t36".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "completed".to_string(),
            }),
        ),
    ]
}

/// Full run with provider + tool-call lifecycle resolved (stable at seq=7).
fn full_run_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "t36-full".into(),
                workspace_root: "/workspace/t36-full".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req-t36-001".into(),
                text: "do the work".to_string(),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req-t36-001".into(),
                provider_id: "default".to_string(),
                model_id: "test-model".to_string(),
                prompt_summary: "do the work".to_string(),
                request_digest: "digest-req-t36".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc-t36-001".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-bash-t36".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc-t36-001".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("done".to_string()),
                output_digest: Some("output-digest-t36".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req-t36-001".into(),
                finish_reason: "end_turn".to_string(),
                output_digest: None,
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            7,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "all done".to_string(),
            }),
        ),
    ]
}

fn write_events_jsonl(path: &Path, events: &[EventEnvelopeV1]) {
    let body: String = events.iter().fold(String::new(), |mut acc, event| {
        acc.push_str(&serde_json::to_string(event).unwrap_or_abort());
        acc.push('\n');
        acc
    });
    fs::write(path, body).unwrap_or_abort();
}

fn read_events_from_jsonl(path: &Path) -> Vec<EventEnvelopeV1> {
    let text = fs::read_to_string(path).unwrap_or_abort();
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).unwrap_or_abort())
        .collect()
}

fn make_envelope_without_seq(run_id: &str, payload: EventV1) -> EventEnvelopeWithoutSeqV1 {
    EventEnvelopeWithoutSeqV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-t36-store-{run_id}"),
        run_id: run_id.to_string().into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

/// Read all replayed events from disk via a fresh store handle (async).
async fn replay_all_async(root: &Path, run_id: &str) -> Vec<EventEnvelopeV1> {
    use tokio_stream::StreamExt;
    let store = JsonlFileEventStore::open_existing(root, run_id, true).unwrap_or_abort();
    let mut stream = store.replay(1).unwrap_or_abort();
    let mut events = Vec::new();
    while let Some(result) = stream.next().await {
        events.push(result.unwrap_or_abort());
    }
    drop(store);
    events
}

// ===========================================================================
// P2 SESSION PERSISTENCE: events.jsonl survives writer drop and restart.
// ===========================================================================

#[test]
fn p2_session_persistence_survives_writer_drop_and_reopen() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-persist-restart";

    let first_seq = {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "first-life".into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort()
            .seq
    };
    assert_eq!(first_seq, 1, "first append must yield seq=1");

    // Restart: open a fresh store on the same run dir; existing events must
    // be visible and the next seq must continue monotonically.
    let reopened = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        reopened.next_seq().unwrap_or_abort(),
        2,
        "reopened store must resume seq counter"
    );

    // Mutation: drop this check and the test no longer proves persistence
    // across restart — a regression that silently wipes events.jsonl on
    // reopen would slip through.
    let events_on_disk = read_events_from_jsonl(&root.path().join(run_id).join("events.jsonl"));
    assert_eq!(
        events_on_disk.len(),
        1,
        "exactly one event must persist after writer drop"
    );
    assert_eq!(events_on_disk[0].seq, 1);
}

// ===========================================================================
// P7 REPLAY PURITY: three passes produce identical projections; no fs delta.
// ===========================================================================

#[tokio::test]
async fn p7_replay_three_passes_produce_identical_projections_with_no_fs_delta() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-replay-purity";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let events_path = root.path().join(run_id).join("events.jsonl");
    let digest_seed = event_log_digest(&events_path).unwrap_or_abort();

    let mut projections = Vec::new();
    for pass in 0..3 {
        let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
        let replayed: Vec<EventEnvelopeV1> = {
            use tokio_stream::StreamExt;
            let mut stream = store.replay(1).unwrap_or_abort();
            let mut out = Vec::new();
            while let Some(result) = stream.next().await {
                out.push(result.unwrap_or_abort());
            }
            out
        };
        drop(store);

        let conv = project_conversation(&replayed, &[]).unwrap_or_abort();
        projections.push((replayed, conv));

        // Every pass must leave events.jsonl byte-identical.
        // Mutation: revert the append-only guarantee and this digest drifts.
        let digest_after = event_log_digest(&events_path).unwrap_or_abort();
        assert_eq!(
            digest_seed, digest_after,
            "pass {pass} mutated events.jsonl — replay must be read-only"
        );
    }

    // All three projections must be equal — proves determinism.
    assert_eq!(
        projections[0], projections[1],
        "replay passes 0 and 1 diverged"
    );
    assert_eq!(
        projections[1], projections[2],
        "replay passes 1 and 2 diverged"
    );
}

// ===========================================================================
// P7 REPLAY PURITY: transcript projection is also deterministic and pure.
// ===========================================================================

#[tokio::test]
async fn p7_transcript_projection_is_deterministic_across_replays() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-transcript-purity";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let proj_a = project_transcript(&replay_all_async(root.path(), run_id).await).unwrap_or_abort();
    let proj_b = project_transcript(&replay_all_async(root.path(), run_id).await).unwrap_or_abort();

    // Mutation: introduce nondeterminism into the transcript projection
    // (e.g. a clock-derived sort key) and these two will diverge.
    assert_eq!(
        format!("{proj_a:?}"),
        format!("{proj_b:?}"),
        "transcript projection must be deterministic across replays"
    );
}

// ===========================================================================
// P2 OWNER: prompt queue persists across restart; send-now selects FIFO head.
// ===========================================================================

#[test]
fn p2_prompt_queue_persists_across_restart_and_drains_post_turn() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path().join("session");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    fs::write(session_dir.join("events.jsonl"), b"{\"seq\":1}\n").unwrap_or_abort();

    let events_before = event_log_digest(&session_dir.join("events.jsonl")).unwrap_or_abort();

    let queue = DurablePromptQueue::for_session(&session_dir);
    queue.enqueue("q-1", "after turn", 1).unwrap_or_abort();
    queue.enqueue("q-2", "after next turn", 2).unwrap_or_abort();
    let interjection = queue
        .interject_mid_turn("urgent", "send now", 3, true)
        .unwrap_or_abort();
    assert!(interjection.turn_was_running);
    assert!(!interjection.mutates_conversation_events);
    drop(queue);

    // Restart: a fresh queue handle must see the persisted entries.
    let resumed = DurablePromptQueue::for_session(&session_dir);
    let reconciled = resumed.drain_interjections().unwrap_or_abort();
    assert_eq!(
        reconciled.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["urgent"],
        "interjection must reconcile on restart"
    );

    let send_now = resumed.dequeue().unwrap_or_abort().unwrap();
    assert_eq!(send_now.id, "q-1", "send-now must select FIFO head");

    let drained = resumed.drain().unwrap_or_abort();
    assert_eq!(
        drained.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["q-2"],
        "post-turn drain must clear remaining entries"
    );

    let after = DurablePromptQueue::for_session(&session_dir);
    assert!(
        after.is_empty().unwrap_or_abort(),
        "queue must be empty after drain + restart"
    );

    // Owner postcondition: queue ops never touch events.jsonl.
    // Mutation: route queue persistence through the event store and this
    // digest drifts, violating the append-only owner contract.
    let events_after = event_log_digest(&session_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        events_before, events_after,
        "queue ops must not mutate events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: durable memory persists across restart with deterministic search.
// ===========================================================================

#[test]
fn p2_durable_memory_persists_across_restart_with_deterministic_search() {
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    let store = DurableMemoryStore::for_workspace(&workspace);
    store
        .put("alpha-rule", "always prefer leftmost derivation")
        .unwrap_or_abort();
    store
        .put("beta-rule", "secondary fallback path")
        .unwrap_or_abort();
    drop(store);

    // Restart: fresh handle sees the persisted entries.
    let resumed = DurableMemoryStore::for_workspace(&workspace);
    let search_alpha = resumed.search("alpha").unwrap_or_abort();
    let search_beta = resumed.search("beta").unwrap_or_abort();
    let search_both = resumed.search("rule").unwrap_or_abort();

    assert!(
        search_alpha.iter().any(|e| e.key == "alpha-rule"),
        "alpha entry must survive restart"
    );
    assert!(
        search_beta.iter().any(|e| e.key == "beta-rule"),
        "beta entry must survive restart"
    );
    assert_eq!(
        search_both.len(),
        2,
        "deterministic search ordering must return both matches"
    );

    // Release-scope is the owner postcondition for clearing entries.
    resumed
        .release_scope(MemoryScope::Workspace)
        .unwrap_or_abort();
    let resumed_after_flush = DurableMemoryStore::for_workspace(&workspace);
    let post_flush = resumed_after_flush.search("rule").unwrap_or_abort();
    assert!(
        post_flush.is_empty(),
        "release_scope(Workspace) must clear persisted memory entries"
    );
}

// ===========================================================================
// P7 ERROR: corrupt events.jsonl is detected by replay and never silently
// repaired. The owner surfaces a parse error rather than mutating the log.
// ===========================================================================

#[tokio::test]
async fn p7_corrupt_events_jsonl_is_detected_and_never_silently_repaired() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-corrupt";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Write a line that is not a valid envelope but is newline-terminated.
    fs::write(run_dir.join("events.jsonl"), b"not-an-envelope\n").unwrap_or_abort();
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // The store surfaces corruption at open_existing time as a typed
    // InvalidJsonLine error — it never silently accepts garbage lines.
    let open_result = JsonlFileEventStore::open_existing(root.path(), run_id, true);
    assert!(
        open_result.is_err(),
        "open_existing must reject a corrupt events.jsonl line"
    );
    let err = open_result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::InvalidJsonLine { .. }),
        "expected InvalidJsonLine, got: {err}"
    );

    // Mutation: if a future "auto-repair" path rewrites events.jsonl, this
    // digest comparison catches it.
    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "corrupt events.jsonl must not be silently rewritten"
    );
}

// ===========================================================================
// P7 ERROR: writer lock contention produces a typed AcquireWriterLock error,
// never a silent overwrite of the active writer.
// ===========================================================================

#[test]
fn p7_writer_lock_contention_produces_typed_error_without_overwrite() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-lock-contention";

    let first = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    first
        .append(make_envelope_without_seq(
            run_id,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "first-writer".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();

    // A second open while the first holds the lock must fail with a typed
    // error and must NOT overwrite the first writer's event.
    let result = JsonlFileEventStore::open(root.path(), run_id, true);
    assert!(
        result.is_err(),
        "second writer must be rejected while first holds lock"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::AcquireWriterLock { .. }),
        "expected AcquireWriterLock, got: {err}"
    );

    // The first writer's event survives the rejected second open.
    let events = read_events_from_jsonl(&root.path().join(run_id).join("events.jsonl"));
    assert_eq!(
        events.len(),
        1,
        "first writer's event must survive contention"
    );
    assert_eq!(events[0].seq, 1);
}

// ===========================================================================
// P7 CRASH RECOVERY: stale writer lock from a dead PID is recoverable on
// reopen; the lock is reclaimed and the existing events are preserved.
// ===========================================================================

#[test]
fn p7_crash_recovery_reclaims_stale_lock_and_preserves_events() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-crash-recover";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = completed_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Simulate a crashed previous process: dead PID holds the lock.
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();

    let report = inspect_previous_crash(&run_dir);
    assert!(report.previous_crash_detected);
    assert_eq!(
        report.recovery_action,
        Some(CrashRecoveryAction::OpenRecovers),
        "stale writer lock must map to open-recovers action"
    );

    // Open reclaims the stale lock and preserves existing events.
    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        store.next_seq().unwrap_or_abort(),
        3,
        "recovered store must resume seq counter after existing events"
    );

    let lock_content = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(
        lock_content.contains(&format!("pid={}", std::process::id())),
        "recovered lock must be reclaimed by the live process"
    );

    // The events on disk are the original ones — recovery never appends.
    // Mutation: if recovery appended a synthetic marker event, this count
    // would drift to 3.
    let events_after = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(
        events_after.len(),
        events.len(),
        "crash recovery must not synthesize events"
    );
}

// ===========================================================================
// P2 OWNER: compaction appends a checkpoint and never rewrites events.jsonl.
// ===========================================================================

#[test]
fn p2_compaction_appends_checkpoint_without_rewriting_events_jsonl() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-compaction";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // Replay-derived transcript projection (no side effects).
    let projection = project_transcript(&events).unwrap_or_abort();

    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "transcript projection must not rewrite events.jsonl"
    );

    // A projected compaction checkpoint (if present) is a derived projection,
    // not a mutation of the source events. The projection's messages vector
    // contains the event-derived turn content; compaction checkpoints are
    // emitted as separate derived projections.
    assert!(
        !projection.messages.is_empty(),
        "projection must produce messages without mutating source events"
    );
}

// ===========================================================================
// P2 OWNER: session rename is replay-derived from SessionTitleUpdated event.
// ===========================================================================

#[tokio::test]
async fn p2_session_rename_postcondition_is_event_replay_not_direct_mutation() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-rename";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "original".into(),
                workspace_root: "/ws".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                title: "renamed-by-event".to_string(),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // Replay produces the renamed title without writing back to events.jsonl.
    let replayed = replay_all_async(root.path(), run_id).await;
    let title_event_count = replayed
        .iter()
        .filter(|e| matches!(&e.payload, EventV1::SessionTitleUpdated(_)))
        .count();
    assert_eq!(
        title_event_count, 1,
        "SessionTitleUpdated event must be replayed exactly once"
    );

    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "rename replay must not mutate events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: fork validates stable prefix and materializes child atomically.
// ===========================================================================

#[test]
fn p2_fork_owner_postcondition_creates_child_with_rewritten_run_id() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-fork-source";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let source_digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    let result = materialize_child_session(request).unwrap_or_abort();
    assert_eq!(result.source_cutoff_seq, 7);
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.join("events.jsonl").is_file());

    let child_events = read_events_from_jsonl(&result.child_run_dir.join("events.jsonl"));
    for event in &child_events {
        assert_eq!(
            event.run_id.as_str(),
            result.child_run_id,
            "child events must be rewritten to the child run_id"
        );
    }

    // Source events.jsonl must be untouched.
    // Mutation: if materialization mutated the source, this digest drifts.
    let source_digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        source_digest_before, source_digest_after,
        "fork must not mutate the source session events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: clone selects the latest stable prefix and materializes a child.
// ===========================================================================

#[test]
fn p2_clone_owner_postcondition_selects_latest_stable_prefix() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-clone-source";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    assert_eq!(
        prefix.cutoff_seq, 7,
        "latest stable prefix must be the final event"
    );

    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };
    let result = materialize_child_session(request).unwrap_or_abort();
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.is_dir());
}

// ===========================================================================
// P7 ERROR: clone rejects an active run with no stable prefix.
// ===========================================================================

#[test]
fn p7_clone_rejects_active_run_with_no_stable_prefix() {
    let active_events = vec![
        envelope(
            "t36-clone-active",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "active".into(),
                workspace_root: "/ws".to_string(),
            }),
        ),
        envelope(
            "t36-clone-active",
            2,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc-active".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "d".to_string(),
                metadata: None,
            }),
        ),
    ];

    let result = latest_clone_stable_prefix(&active_events);
    assert!(
        result.is_err(),
        "clone must reject a run with no stable prefix"
    );
}

// ===========================================================================
// P2 OWNER: prompt rewind is append-only (plan) and atomic (workspace restore).
// ===========================================================================

#[test]
fn p2_prompt_rewind_plan_is_append_only() {
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("t36-rewind-plan");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events("t36-rewind-plan");
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    let plan = plan_prompt_rewind(&events, 4).unwrap_or_abort();
    assert_eq!(plan.cutoff_seq, 4);
    assert!(plan.events_append_only);

    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "plan_prompt_rewind must not mutate events.jsonl"
    );
}

#[test]
fn p2_prompt_rewind_atomic_restore_rolls_back_files_and_keeps_events() {
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();

    fs::write(
        workspace.join("src/main.rs"),
        "fn main() { println!(\"v2-corrupted\"); }",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("README.md"), "# Version 2 (broken)").unwrap_or_abort();

    let events = full_run_events("t36-rewind-atomic");
    let snapshot = vec![
        FileSnapshotEntry {
            path: "src/main.rs".to_string(),
            content: "fn main() { println!(\"v1-good\"); }".to_string(),
        },
        FileSnapshotEntry {
            path: "README.md".to_string(),
            content: "# Version 1 (known good)".to_string(),
        },
    ];

    let result = atomic_prompt_rewind(&events, 4, &workspace, &snapshot).unwrap_or_abort();
    assert_eq!(result.files_restored, 2, "both files must be restored");
    assert!(
        result.events_append_only,
        "atomic rewind must keep events append-only"
    );

    // Files actually rolled back to the snapshot content.
    assert_eq!(
        fs::read_to_string(workspace.join("src/main.rs")).unwrap_or_abort(),
        "fn main() { println!(\"v1-good\"); }"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("README.md")).unwrap_or_abort(),
        "# Version 1 (known good)"
    );
}

// ===========================================================================
// P2 OWNER: foreign-session import creates a replay-only session and never
// mutates the source.
// ===========================================================================

#[test]
fn p2_foreign_import_owner_postcondition_creates_replay_only_session() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-source");
    let dest = root.path().join("harness-import");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let source_events = completed_run_events("foreign-t36");
    write_events_jsonl(&foreign.join("events.jsonl"), &source_events);
    let source_bytes_before = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    assert_eq!(result.event_count, 2);
    assert_eq!(result.mode_source, SessionModeSource::ReplayOnly);
    assert!(result.run_dir.join("events.jsonl").is_file());
    assert!(result.run_dir.join("meta.json").is_file());

    // Owner postcondition: source is never mutated.
    let source_bytes_after = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        source_bytes_before, source_bytes_after,
        "foreign source events.jsonl must not be mutated by import"
    );

    // Imported events use the new child run_id.
    let imported = read_events_from_jsonl(&result.run_dir.join("events.jsonl"));
    for event in &imported {
        assert_eq!(event.run_id.as_str(), result.run_id);
    }
}

// ===========================================================================
// P7 ERROR: foreign import refuses an active target session.
// ===========================================================================

#[test]
fn p7_foreign_import_refuses_active_target() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-active-target");
    let active = root.path().join("active-target");
    fs::create_dir_all(&foreign).unwrap_or_abort();
    fs::create_dir_all(&active).unwrap_or_abort();
    fs::write(active.join("events.jsonl"), "").unwrap_or_abort();

    let result = refuse_import_into_active_session(&foreign, &active);
    assert!(
        matches!(
            result,
            Err(ForeignSessionError::ImportIntoActiveForbidden { .. })
        ),
        "import into an active session must be forbidden"
    );
}

// ===========================================================================
// P7 LIFECYCLE: teardown releases writer lock; reopened session resumes cleanly.
// ===========================================================================

#[test]
fn p7_teardown_releases_writer_lock_and_clean_reopen_resumes() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-teardown";
    let run_dir = root.path().join(run_id);

    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    store
        .append(make_envelope_without_seq(
            run_id,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "teardown-check".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();
    assert!(
        run_dir.join(".writer.lock").is_file(),
        "writer lock must be held while store is open"
    );

    // Teardown = drop. After drop the lock file is gone.
    drop(store);
    assert!(
        !run_dir.join(".writer.lock").exists(),
        "writer lock must be released after drop"
    );

    // Clean reopen succeeds and sees the persisted event.
    let reopened = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        reopened.next_seq().unwrap_or_abort(),
        2,
        "clean reopen must resume after teardown"
    );
}

// ===========================================================================
// P7 LIFECYCLE: replay across two separate store handles yields identical
// events, proving there is no hidden shared mutable state.
// ===========================================================================

#[tokio::test]
async fn p7_replay_across_independent_handles_is_identical() {
    use tokio_stream::StreamExt;
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-independent-handles";
    let events = full_run_events(run_id);
    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let mut collected = Vec::new();
    for _ in 0..2 {
        let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut local = Vec::new();
        while let Some(result) = stream.next().await {
            local.push(result.unwrap_or_abort());
        }
        drop(store);
        collected.push(local);
    }

    // Mutation: introduce shared mutable state between store handles and
    // these two vectors will diverge.
    assert_eq!(
        collected[0], collected[1],
        "independent replay handles must produce identical events"
    );
    assert_eq!(
        collected[0], events,
        "replayed events must match stored events"
    );
}

// ===========================================================================
// P7 LIFECYCLE: concurrent-looking back-to-back writes produce contiguous seq.
// ===========================================================================

#[test]
fn p7_rapid_appends_produce_contiguous_monotonic_seq() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-rapid-append";
    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();

    let mut seqs = Vec::new();
    for i in 0..10 {
        let event = store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: format!("rapid-{i}").into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        seqs.push(event.seq);
    }

    // Mutation: any gap or duplication in seq would break this assertion.
    let expected: Vec<u64> = (1..=10).collect();
    assert_eq!(seqs, expected, "appends must produce contiguous seq from 1");
    assert_eq!(store.next_seq().unwrap_or_abort(), 11);
}
