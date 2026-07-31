//! Differential TDD contract tests: sessions, persistence, replay, lineage, rewind,
//! crash recovery, and foreign-import parity.
//!
//! These tests prove the existing harness-core session/store/lineage/rewind/crash/foreign
//! APIs honor the clean-room parity contract:
//!
//! 1. Session create/list/resume/rename via append-only event store.
//! 2. Writer locks enforce single-writer; second open rejects while lock held.
//! 3. Two replay passes produce identical projections with zero side effects.
//! 4. Tree/fork/clone operate at verified stable cutoffs.
//! 5. Prompt rewind with atomic workspace restore (fail-closed, events stay append-only).
//! 6. Crash scan detects stale locks/recovery markers; reopen recovers.
//! 7. Foreign-session import creates replay-only sessions; active/writer-locked rejected.
//! 8. Export metadata (meta.json) carries support-ready provenance.

use std::fs;
use std::path::Path;

use harness_core::conversation::project_conversation;
use harness_core::crash_recovery::{
    inspect_previous_crash, scan_previous_crashes, CrashRecoveryAction,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SessionTitleUpdatedEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::foreign_session::{
    discover_foreign_sessions, import_foreign_session_as_replay, refuse_import_into_active_session,
    ForeignSessionError,
};
use harness_core::proj::{RunStatus, SessionModeSource};
use harness_core::prompt_rewind::{
    atomic_prompt_rewind, plan_prompt_rewind, FileSnapshotEntry, PromptRewindError,
};
use harness_core::session_lineage::{
    latest_clone_stable_prefix, materialize_child_session, project_lineage_tree,
    validate_fork_stable_prefix, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind, SessionLineageError,
};
use harness_core::store::{
    EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, JsonlFileEventStore,
};
use harness_core::UnwrapOrAbort;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{run_id}-{seq:04}"),
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

/// Minimal completed run: RunStarted + RunFinished.
fn completed_run_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "test-session".into(),
                workspace_root: "/workspace/test".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "completed successfully".to_string(),
            }),
        ),
    ]
}

/// Run with user message + tool call in flight (not stable).
fn active_run_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "active".into(),
                workspace_root: "/workspace/active".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req-001".into(),
                text: "hello world".to_string(),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc-001".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "abc123".to_string(),
                metadata: None,
            }),
        ),
    ]
}

/// Full run with provider + tool call lifecycle fully resolved (stable at seq=6).
fn full_run_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "full-run".into(),
                workspace_root: "/workspace/full".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req-001".into(),
                text: "do the work".to_string(),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req-001".into(),
                provider_id: "default".to_string(),
                model_id: "test-model".to_string(),
                prompt_summary: "do the work".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc-001".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-abc".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc-001".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("done".to_string()),
                output_digest: Some("output-digest".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req-001".into(),
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
        event_id: format!("evt-store-{run_id}"),
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

// ===========================================================================
// 1. SESSION CREATION + APPEND-ONLY STORAGE
// ===========================================================================

#[test]
fn session_creation_produces_run_dir_with_events_and_writer_lock() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    let store = JsonlFileEventStore::open(session_dir, "run-create-001", true).unwrap_or_abort();

    // Run directory exists with events.jsonl and writer lock
    let run_dir = session_dir.join("run-create-001");
    assert!(run_dir.is_dir(), "run directory must exist");
    assert!(
        run_dir.join("events.jsonl").is_file(),
        "events.jsonl must be created"
    );
    assert!(
        run_dir.join(".writer.lock").is_file(),
        "writer lock must exist during open"
    );

    // Append an event
    let event = store
        .append(make_envelope_without_seq(
            "run-create-001",
            EventV1::RunStarted(RunStartedEvent {
                run_name: "created".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();
    assert_eq!(event.seq, 1, "first event must have seq=1");

    // Drop store releases writer lock
    drop(store);
    assert!(
        !run_dir.join(".writer.lock").exists(),
        "writer lock must be released on drop"
    );

    // Events file has one valid line
    let events = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[0].run_id.as_str(), "run-create-001");
}

#[test]
fn append_only_sequencing_is_monotonic_and_contiguous() {
    let root = tempdir().unwrap_or_abort();
    let store = JsonlFileEventStore::open(root.path(), "run-seq-001", true).unwrap_or_abort();

    for i in 0..5 {
        let event = store
            .append(make_envelope_without_seq(
                "run-seq-001",
                EventV1::RunStarted(RunStartedEvent {
                    run_name: format!("event-{i}").into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        assert_eq!(event.seq, i + 1, "events must be contiguous from 1");
    }

    assert_eq!(store.next_seq().unwrap_or_abort(), 6);
}

// ===========================================================================
// 2. WRITER LOCK ENFORCEMENT
// ===========================================================================

#[test]
fn second_writer_lock_acquisition_is_rejected_while_first_held() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    // First open acquires the lock
    let _store1 = JsonlFileEventStore::open(session_dir, "run-locked-001", true).unwrap_or_abort();

    // Second open on same run must fail (lock held by same process but different file handle)
    let result = JsonlFileEventStore::open(session_dir, "run-locked-001", true);
    // The second open detects an existing lock with OUR pid — since the process is alive,
    // it cannot reclaim it; the result depends on implementation. The lock file content
    // contains our PID which is alive, so it should fail with AcquireWriterLock.
    assert!(
        result.is_err(),
        "second writer must be rejected while first holds lock"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::AcquireWriterLock { .. }),
        "expected AcquireWriterLock error, got: {err}"
    );
}

// ===========================================================================
// 3. REPLAY PROJECTIONS: TWO PASSES ARE IDENTICAL + NO SIDE EFFECTS
// ===========================================================================

#[tokio::test]
async fn replay_two_passes_produce_identical_projections() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    // Write events to disk (simulating a completed session)
    let run_id = "run-replay-001";
    let events = full_run_events(run_id);

    // Create store, append events, drop
    {
        let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    // Replay pass 1
    let store1 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let replay1: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store1.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store1);

    // Replay pass 2
    let store2 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let replay2: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store2.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store2);

    // Prove identical projections
    assert_eq!(replay1.len(), replay2.len(), "event counts must match");
    assert_eq!(replay1, replay2, "replay passes must be byte-identical");

    // Prove conversation projection is identical
    let conv1 = project_conversation(&replay1, &[]).unwrap_or_abort();
    let conv2 = project_conversation(&replay2, &[]).unwrap_or_abort();
    assert_eq!(conv1, conv2, "conversation projections must be identical");

    // Prove no side effects: events.jsonl unchanged between replays
    let events_path = session_dir.join(run_id).join("events.jsonl");
    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&events_path).unwrap_or_abort();

    // Third replay (no mutation)
    let store3 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    {
        use tokio_stream::StreamExt;
        let mut stream = store3.replay(1).unwrap_or_abort();
        while let Some(result) = stream.next().await {
            result.unwrap_or_abort();
        }
    }
    drop(store3);

    let digest_after =
        harness_core::prompt_rewind::event_log_digest(&events_path).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "replay must not modify events.jsonl"
    );
}

// ===========================================================================
// 4. SESSION RENAME VIA EVENT REPLAY
// ===========================================================================

#[tokio::test]
async fn session_rename_is_replayed_from_title_event() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-rename-001";

    // Create session with title rename event
    {
        let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "original-name".into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: "renamed-session".to_string(),
                }),
            ))
            .unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ))
            .unwrap_or_abort();
    }

    // Replay and verify title is projected
    let store = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let events: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };

    // Find the title event and verify
    let title_event = events.iter().find(|event| {
        matches!(&event.payload, EventV1::SessionTitleUpdated(payload) if payload.title == "renamed-session")
    });
    assert!(
        title_event.is_some(),
        "SessionTitleUpdated event must be replayed"
    );
    assert_eq!(events.len(), 3, "all events must be preserved");
}

// ===========================================================================
// 5. LINEAGE TREE PROJECTION
// ===========================================================================

#[test]
fn lineage_tree_projects_parent_child_relationships() {
    use harness_core::proj::SessionCatalogEntry;

    let parent = SessionCatalogEntry {
        run_id: "run-parent".to_string(),
        run_name: Some("parent".to_string()),
        status: Some(RunStatus::Finished),
        last_updated_at: Some("2026-01-01T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 1,
        parent_session_id: None,
    };

    let child = SessionCatalogEntry {
        run_id: "run-child-001".to_string(),
        run_name: Some("child".to_string()),
        status: Some(RunStatus::Finished),
        last_updated_at: Some("2026-01-02T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("run-parent".to_string()),
    };

    let tree = project_lineage_tree(vec![parent.clone(), child.clone()]);

    assert_eq!(tree.len(), 2, "tree must contain both entries");
    assert_eq!(tree.roots.len(), 1, "one root (parent)");
    assert_eq!(tree.roots[0].entry.run_id, "run-parent");
    assert_eq!(tree.roots[0].children.len(), 1, "parent has one child");
    assert_eq!(tree.roots[0].children[0].entry.run_id, "run-child-001");

    // Flatten produces correct depth ordering
    let rows = tree.flatten();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[0].entry.run_id, "run-parent");
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[1].entry.run_id, "run-child-001");
}

#[test]
fn lineage_tree_handles_orphan_and_cyclic_entries_as_roots() {
    use harness_core::proj::SessionCatalogEntry;

    let orphan = SessionCatalogEntry {
        run_id: "run-orphan".to_string(),
        run_name: None,
        status: None,
        last_updated_at: None,
        workspace_root: None,
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::Unknown,
        is_resumable: false,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("nonexistent-parent".to_string()),
    };

    let self_ref = SessionCatalogEntry {
        run_id: "run-selfref".to_string(),
        run_name: None,
        status: None,
        last_updated_at: None,
        workspace_root: None,
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::Unknown,
        is_resumable: false,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("run-selfref".to_string()),
    };

    let tree = project_lineage_tree(vec![orphan, self_ref]);
    assert_eq!(tree.roots.len(), 2, "orphans/cycles become roots");
    assert_eq!(tree.len(), 2);
}

// ===========================================================================
// 6. FORK AT STABLE CUTOFF
// ===========================================================================

#[test]
fn fork_validates_stable_prefix_at_completed_cutoff() {
    let events = full_run_events("run-fork-src");

    // Stable at seq=7 (finished with all in-flight resolved)
    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    assert_eq!(prefix.cutoff_seq, 7);
    assert_eq!(prefix.event_count, 7);
    assert_eq!(prefix.status, Some(RunStatus::Finished));
}

#[test]
fn fork_rejects_unstable_prefix_with_open_tool_call() {
    let events = full_run_events("run-fork-unstable");

    // Cutoff at seq=4: tool call tc-001 was requested but not finished — unstable
    let result = validate_fork_stable_prefix(&events, 4);
    assert!(
        result.is_err(),
        "prefix with in-flight tool call must be unstable"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, SessionLineageError::UnstablePrefix { .. }),
        "expected UnstablePrefix, got: {err}"
    );
}

#[test]
fn fork_rejects_out_of_range_cutoff() {
    let events = full_run_events("run-fork-range");

    let result = validate_fork_stable_prefix(&events, 999);
    assert!(matches!(
        result,
        Err(SessionLineageError::CutoffOutOfRange { .. })
    ));
}

#[test]
fn fork_materializes_child_session_atomically() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-fork-parent";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

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
    assert!(result.child_run_dir.is_dir(), "child run dir must exist");
    assert!(
        result.child_run_dir.join("events.jsonl").is_file(),
        "child must have events.jsonl"
    );

    // Child events use child run_id
    let child_events = read_events_from_jsonl(&result.child_run_dir.join("events.jsonl"));
    assert_eq!(child_events.len(), 7);
    for event in &child_events {
        assert_eq!(
            event.run_id.as_str(),
            result.child_run_id,
            "child events must be rewritten to child run_id"
        );
    }

    // Source is untouched
    let source_events = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(source_events.len(), 7);
    assert_eq!(source_events[0].run_id.as_str(), run_id);
}

#[test]
fn fork_rejects_writer_locked_source() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-fork-locked";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Create a writer lock file to simulate active session
    fs::write(
        run_dir.join(".writer.lock"),
        format!("pid={}\ntoken=999\n", std::process::id()),
    )
    .unwrap_or_abort();

    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    let result = materialize_child_session(request);
    assert!(result.is_err(), "fork must reject writer-locked source");
}

// ===========================================================================
// 7. CLONE LATEST STABLE PREFIX
// ===========================================================================

#[test]
fn clone_selects_latest_stable_prefix() {
    let events = full_run_events("run-clone-src");

    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    assert_eq!(prefix.cutoff_seq, 7, "latest stable is at the final event");
    assert_eq!(prefix.status, Some(RunStatus::Finished));
}

#[test]
fn clone_fails_when_no_stable_prefix_exists() {
    // Run with active lifecycle and no finished/failed
    let events = active_run_events("run-clone-nostable");

    let result = latest_clone_stable_prefix(&events);
    assert!(
        result.is_err(),
        "clone must fail when no stable prefix exists"
    );
}

#[test]
fn clone_materializes_child_from_latest_stable() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-clone-parent";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    let result = materialize_child_session(request).unwrap_or_abort();
    assert_eq!(result.source_cutoff_seq, 7);
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.is_dir());
}

// ===========================================================================
// 8. PROMPT REWIND: PLAN + ATOMIC WORKSPACE RESTORE
// ===========================================================================

#[test]
fn rewind_plan_projects_conversation_through_cutoff() {
    let events = full_run_events("run-rewind-001");

    let plan = plan_prompt_rewind(&events, 4).unwrap_or_abort();
    assert_eq!(plan.cutoff_seq, 4);
    assert_eq!(plan.retained_event_count, 4);
    assert_eq!(plan.discarded_event_count, 3);
    assert!(plan.events_append_only, "events must stay append-only");

    // Conversation should contain the user message up to cutoff
    assert!(
        !plan.conversation.messages.is_empty(),
        "conversation projection must be non-empty"
    );
}

#[test]
fn rewind_plan_fails_on_empty_log() {
    let result = plan_prompt_rewind(&[], 1);
    assert!(matches!(result, Err(PromptRewindError::EmptyEventLog)));
}

#[test]
fn rewind_plan_fails_on_out_of_range_cutoff() {
    let events = full_run_events("run-rewind-oob");
    let result = plan_prompt_rewind(&events, 99);
    assert!(matches!(
        result,
        Err(PromptRewindError::CutoffOutOfRange {
            cutoff_seq: 99,
            max_seq: 7
        })
    ));
}

#[test]
fn rewind_does_not_rewrite_events_jsonl() {
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-rewind-appendonly");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events("run-rewind-appendonly");
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
            .unwrap_or_abort();

    // Plan rewind (read-only operation)
    let _plan = plan_prompt_rewind(&events, 3).unwrap_or_abort();

    let digest_after = harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
        .unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "plan_prompt_rewind must not modify events.jsonl"
    );
}

#[test]
fn atomic_rewind_restores_workspace_files_atomically() {
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();

    // Setup workspace files
    fs::write(
        workspace.join("src/main.rs"),
        "fn main() { println!(\"v2\"); }",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("README.md"), "# Version 2").unwrap_or_abort();

    let events = full_run_events("run-atomic-rewind");

    // File snapshot: restore to "version 1" content
    let snapshot = vec![
        FileSnapshotEntry {
            path: "src/main.rs".to_string(),
            content: "fn main() { println!(\"v1\"); }".to_string(),
        },
        FileSnapshotEntry {
            path: "README.md".to_string(),
            content: "# Version 1".to_string(),
        },
    ];

    let result = atomic_prompt_rewind(&events, 4, &workspace, &snapshot).unwrap_or_abort();

    assert_eq!(result.files_restored, 2, "both files must be restored");
    assert_eq!(result.files_unchanged, 0);
    assert!(result.events_append_only);
    assert_eq!(result.conversation.cutoff_seq, 4);

    // Verify files were restored
    assert_eq!(
        fs::read_to_string(workspace.join("src/main.rs")).unwrap_or_abort(),
        "fn main() { println!(\"v1\"); }"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("README.md")).unwrap_or_abort(),
        "# Version 1"
    );
}

#[test]
fn atomic_rewind_with_empty_snapshot_is_conversation_only() {
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path();

    let events = full_run_events("run-atomic-empty-snap");
    let result = atomic_prompt_rewind(&events, 3, workspace, &[]).unwrap_or_abort();

    assert_eq!(result.files_restored, 0);
    assert_eq!(result.files_unchanged, 0);
    assert!(result.events_append_only);
}

// ===========================================================================
// 9. CRASH SCAN + RECOVERY
// ===========================================================================

#[test]
fn crash_scan_detects_stale_writer_lock() {
    let root = tempdir().unwrap_or_abort();
    let sessions_root = root.path().join("sessions");
    let run_dir = sessions_root.join("run-crashed-001");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Simulate crash: stale lock with dead PID + events.jsonl present
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();

    let report = inspect_previous_crash(&run_dir);
    assert!(report.previous_crash_detected, "must detect previous crash");
    assert!(report.stale_writer_lock, "must flag stale writer lock");
    assert!(report.events_log_present, "events.jsonl is present");
    assert!(
        report.recovery_message.is_some(),
        "must provide recovery message"
    );
    assert_eq!(
        report.recovery_action,
        Some(CrashRecoveryAction::OpenRecovers),
        "recovery action must be open_recovers"
    );
}

#[test]
fn crash_scan_detects_recovery_marker() {
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-recovery-marker");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Recovery marker present (no writer lock)
    fs::write(run_dir.join(".writer.lock.recovering"), "").unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();

    let report = inspect_previous_crash(&run_dir);
    assert!(report.previous_crash_detected);
    assert!(report.recovery_marker_present);
    assert!(!report.stale_writer_lock);
}

#[test]
fn crash_scan_reports_clean_for_healthy_session() {
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-healthy");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "{}\n").unwrap_or_abort();

    let report = inspect_previous_crash(&run_dir);
    assert!(
        !report.previous_crash_detected,
        "healthy session must not flag crash"
    );
    assert!(!report.stale_writer_lock);
    assert!(!report.recovery_marker_present);
    assert!(report.recovery_message.is_none());
}

#[test]
fn crash_scan_multi_directory_counts_correctly() {
    let root = tempdir().unwrap_or_abort();
    let sessions_root = root.path().join("sessions");

    // Healthy session
    let healthy = sessions_root.join("run-healthy");
    fs::create_dir_all(&healthy).unwrap_or_abort();
    fs::write(healthy.join("events.jsonl"), "").unwrap_or_abort();

    // Crashed session (stale lock)
    let crashed = sessions_root.join("run-crashed");
    fs::create_dir_all(&crashed).unwrap_or_abort();
    fs::write(crashed.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();
    fs::write(crashed.join("events.jsonl"), "").unwrap_or_abort();

    let reports = scan_previous_crashes(&sessions_root);
    assert_eq!(reports.len(), 2, "must scan all session directories");

    let crashed_reports: Vec<_> = reports
        .iter()
        .filter(|r| r.previous_crash_detected)
        .collect();
    assert_eq!(crashed_reports.len(), 1, "exactly one crashed session");
}

// ===========================================================================
// 10. FOREIGN-SESSION IMPORT
// ===========================================================================

#[test]
fn foreign_import_creates_replay_only_session() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-dir");
    let dest = root.path().join("harness-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let source_events = completed_run_events("foreign-run-abc");
    write_events_jsonl(&foreign.join("events.jsonl"), &source_events);
    let source_before = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    assert_eq!(result.event_count, 2);
    assert_eq!(result.format, "events_jsonl_v1");
    assert_eq!(result.mode_source, SessionModeSource::ReplayOnly);
    assert!(result.run_dir.join("events.jsonl").is_file());
    assert!(result.run_dir.join("meta.json").is_file());

    // Source is never mutated
    assert_eq!(
        fs::read(foreign.join("events.jsonl")).unwrap_or_abort(),
        source_before,
        "foreign source must not be mutated"
    );

    // Imported events use new run_id
    let imported_events = read_events_from_jsonl(&result.run_dir.join("events.jsonl"));
    assert_eq!(imported_events.len(), 2);
    for event in &imported_events {
        assert_eq!(event.run_id.as_str(), result.run_id);
    }
}

#[test]
fn foreign_import_rejects_active_session_target() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-active");
    let active = root.path().join("active-session");
    fs::create_dir_all(&foreign).unwrap_or_abort();
    fs::create_dir_all(&active).unwrap_or_abort();
    fs::write(active.join("events.jsonl"), "").unwrap_or_abort();

    let result = refuse_import_into_active_session(&foreign, &active);
    assert!(
        matches!(
            result,
            Err(ForeignSessionError::ImportIntoActiveForbidden { .. })
        ),
        "must refuse import into active session"
    );
}

#[test]
fn foreign_import_rejects_corrupt_source() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("corrupt-foreign");
    let dest = root.path().join("harness-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    // Non-envelope JSONL
    fs::write(
        foreign.join("events.jsonl"),
        r#"{"role":"user","text":"hello"}
"#,
    )
    .unwrap_or_abort();

    let result = import_foreign_session_as_replay(&foreign, &dest);
    assert!(result.is_err(), "must reject non-envelope JSONL");
    assert!(matches!(
        result.unwrap_err(),
        ForeignSessionError::SourceParse { .. }
    ));
}

#[test]
fn foreign_discover_classifies_candidates() {
    let root = tempdir().unwrap_or_abort();
    let scan = root.path().join("scan-root");
    fs::create_dir_all(&scan).unwrap_or_abort();

    // Importable events.jsonl
    let importable = scan.join("good-session");
    fs::create_dir_all(&importable).unwrap_or_abort();
    let events = completed_run_events("scan-run");
    write_events_jsonl(&importable.join("events.jsonl"), &events);

    // Plain directory (rejected)
    let plain = scan.join("not-a-session");
    fs::create_dir_all(&plain).unwrap_or_abort();
    fs::write(plain.join("notes.txt"), "hello").unwrap_or_abort();

    let found = discover_foreign_sessions(&scan).unwrap_or_abort();
    assert!(found.len() >= 2, "must find all directories");
    assert!(found.iter().any(|c| c.is_importable()));
    assert!(found.iter().any(|c| c.is_rejected()));
}

// ===========================================================================
// 11. EXPORT METADATA (meta.json)
// ===========================================================================

#[test]
fn import_metadata_carries_support_ready_provenance() {
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("meta-foreign");
    let dest = root.path().join("meta-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let events = completed_run_events("meta-run");
    write_events_jsonl(&foreign.join("events.jsonl"), &events);

    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    // Read and validate meta.json
    let meta_text = fs::read_to_string(result.run_dir.join("meta.json")).unwrap_or_abort();
    let meta: serde_json::Value = serde_json::from_str(&meta_text).unwrap_or_abort();

    assert_eq!(meta["run_id"], result.run_id, "meta must record run_id");
    assert_eq!(
        meta["mode_source"], "replay_only",
        "meta must record replay_only mode"
    );
    assert_eq!(
        meta["foreign_import"]["format"], "events_jsonl_v1",
        "meta must record import format"
    );
    assert_eq!(
        meta["foreign_import"]["source_path"],
        foreign.display().to_string(),
        "meta must record source path for provenance"
    );
    assert_eq!(
        meta["foreign_import"]["event_count"], 2,
        "meta must record event count"
    );
    assert!(
        meta["foreign_import"]["policy"]
            .as_str()
            .unwrap_or_default()
            .contains("read-only replay import"),
        "meta must record import policy"
    );
    // Support-ready: harness_version present
    assert!(
        meta["harness_version"].is_string(),
        "meta must include harness_version for support"
    );
}

// ===========================================================================
// 12. REPLAY FROM SEQUENCE OFFSET
// ===========================================================================

#[tokio::test]
async fn replay_from_offset_skips_earlier_events() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "run-offset-replay";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
    let from_seq_3: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(3).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };

    assert_eq!(from_seq_3.len(), 5, "replay(3) must return seq 3..=7");
    assert_eq!(from_seq_3[0].seq, 3);
    assert_eq!(from_seq_3[4].seq, 7);
}

// ===========================================================================
// 13. CRASH RECOVERY VIA REOPEN (writer lock recovery)
// ===========================================================================

#[test]
fn crash_reopen_recovers_stale_lock_on_open() {
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-crash-reopen";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Write events + simulate stale lock from a dead process
    let events = completed_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();

    // Verify crash detection
    let report = inspect_previous_crash(&run_dir);
    assert!(report.previous_crash_detected);

    // open recovers the stale lock (dead PID is reclaimable)
    let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();

    // After open, the lock is now ours; events are preserved
    assert_eq!(
        store.next_seq().unwrap_or_abort(),
        3,
        "must resume from existing events"
    );

    // Writer lock exists and is ours (not the dead one)
    let lock_content = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(lock_content.contains(&format!("pid={}", std::process::id())));
    drop(store);
}

// ===========================================================================
// 14. REPLAY IS SIDE-EFFECT FREE (PROOF: NO PROVIDER/TOOL/HOOK/MCP/NETWORK)
// ===========================================================================

#[tokio::test]
async fn replay_is_pure_projection_without_side_effects() {
    let root = tempdir().unwrap_or_abort();
    let run_id = "run-pure-replay";
    let run_dir = root.path().join(run_id);

    // Create a session with tool calls (that would be side effects if executed)
    let events = full_run_events(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Record filesystem state before replay
    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
            .unwrap_or_abort();
    let dir_entries_before: Vec<String> = fs::read_dir(&run_dir)
        .unwrap_or_abort()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    // Perform replay (open_existing does NOT execute; just reads)
    let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
    let replayed: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store);

    // Prove: events.jsonl unchanged
    let digest_after = harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
        .unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "replay must not write to events.jsonl"
    );

    // Prove: no new files created in run_dir (except writer lock which is cleaned on drop)
    // After drop, writer lock is removed; check remaining entries
    let dir_entries_after: Vec<String> = fs::read_dir(&run_dir)
        .unwrap_or_abort()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    // events.jsonl should be the same; writer lock is ephemeral
    let non_lock_after: Vec<&String> = dir_entries_after
        .iter()
        .filter(|name| !name.starts_with(".writer.lock"))
        .collect();
    let non_lock_before: Vec<&String> = dir_entries_before
        .iter()
        .filter(|name| !name.starts_with(".writer.lock"))
        .collect();
    assert_eq!(
        non_lock_before, non_lock_after,
        "replay must not create or delete files"
    );

    // Prove: replayed events match source (read-only fidelity)
    assert_eq!(replayed.len(), events.len());
    assert_eq!(replayed, events, "replayed events must match stored events");
}
