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

include!("owner_postcondition_persistence_restart_parity/01_persistence_replay_test.rs");
include!("owner_postcondition_persistence_restart_parity/02_queue_memory_recovery_test.rs");
include!("owner_postcondition_persistence_restart_parity/03_compaction_session_copy_test.rs");
include!("owner_postcondition_persistence_restart_parity/04_rewind_import_lifecycle_test.rs");
