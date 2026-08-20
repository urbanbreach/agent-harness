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

include!("sessions_persistence_rewind_parity/01_storage_replay_test.rs");
include!("sessions_persistence_rewind_parity/02_lineage_fork_clone_test.rs");
include!("sessions_persistence_rewind_parity/03_rewind_crash_scan_test.rs");
include!("sessions_persistence_rewind_parity/04_import_replay_reopen_test.rs");
