use super::*;
use crate::agent::ProviderContextCheckpoint;
use crate::config::CompactionSettings;
use crate::event::{
    AssistantMessageFinishedEvent, CompactionWrittenEvent, ProviderRequestStartedEvent,
};
use crate::ids::RequestId;
use crate::proj::RecordedRuntimeContext;
use crate::UnwrapOrAbort;
use async_trait::async_trait;
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use std::sync::Arc;
use tokio_stream;

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

struct SummaryMockProvider {
    summary: String,
}

#[async_trait]
impl Provider for SummaryMockProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        let summary = self.summary.clone();
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(summary),
            ProviderStreamEvent::Done { usage: None },
        ]))
    }
}

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

fn append_user_message(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
    text: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: RequestId::new(request_id),
            text: text.to_string(),
        }),
    )
    .unwrap_or_abort();
}

fn append_stream_delta(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
    delta: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: RequestId::new(request_id),
            delta: delta.to_string(),
        }),
    )
    .unwrap_or_abort();
}

fn append_provider_started(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: RequestId::new(request_id),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "prompt".to_string(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    )
    .unwrap_or_abort();
}

fn append_assistant_finished(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: RequestId::new(request_id),
            tool_call_count: 0,
            assistant_message: None,
        }),
    )
    .unwrap_or_abort();
}

fn small_context_runtime_context(window: u32) -> RecordedRuntimeContext {
    RecordedRuntimeContext {
        profile: "alpha".to_string(),
        provider: "mock".to_string(),
        model: "model-1".to_string(),
        display_label: "Mock Model 1".to_string(),
        context_window_tokens: Some(window),
        ..Default::default()
    }
}

fn settings(enabled: bool, reserve_tokens: u32, keep_recent_tokens: u32) -> CompactionSettings {
    CompactionSettings {
        enabled,
        reserve_tokens,
        keep_recent_tokens,
        ..Default::default()
    }
}

fn setup_agent(run_state: &mut RunState, agent_id: &str) {
    run_state
        .agents
        .insert(agent_id.to_string(), test_agent_profile("alpha"));
}

fn large_text(fill: char, count: usize) -> String {
    fill.to_string().repeat(count)
}

fn count_session_compaction_events(events: &[EventEnvelopeV1]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
        .count()
}

fn last_session_compaction_event(
    events: &[EventEnvelopeV1],
) -> &crate::event::SessionCompactionEvent {
    events
        .iter()
        .rev()
        .find_map(|e| match &e.payload {
            EventV1::SessionCompaction(event) => Some(event),
            _ => None,
        })
        .expect("at least one SessionCompaction event")
}

// ---------------------------------------------------------------------------
// Checkpoint contract tests
// ---------------------------------------------------------------------------

async fn compact_two_turn_checkpoint_fixture(
    run_id: &str,
) -> (tempfile::TempDir, RunState, String, Vec<u8>) {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), run_id);
    run_state.recorded_runtime_context = Some(small_context_runtime_context(2000));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    // Two turns with ~1000 tokens each (4000 bytes / 4).
    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "First question",
    );
    append_provider_started(&clock, &redactor, &mut run_state, agent_id, "req_1");
    append_stream_delta(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        &large_text('A', 4000),
    );
    append_assistant_finished(&clock, &redactor, &mut run_state, agent_id, "req_1");
    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_2",
        "Second question",
    );
    append_provider_started(&clock, &redactor, &mut run_state, agent_id, "req_2");
    append_stream_delta(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_2",
        &large_text('B', 4000),
    );
    append_assistant_finished(&clock, &redactor, &mut run_state, agent_id, "req_2");

    let bytes_before = fs::read(&run_state.info.events_path).unwrap_or_abort();

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nCheckpoint summary".to_string(),
    });
    let result = compact_session(
        &clock,
        &redactor,
        &mut run_state,
        provider,
        agent_id,
        "proactive",
        &settings(true, 0, 500),
        None,
    )
    .await
    .unwrap_or_abort();
    assert!(
        result.is_some(),
        "fixture compaction should produce a result"
    );

    (temp_dir, run_state, agent_id.to_string(), bytes_before)
}

#[allow(
    deprecated,
    reason = "checkpoint contract tests exercise the deprecated CompactionWritten/CompactionApplied events"
)]
#[tokio::test]
async fn session_compaction_writes_checkpoint_artifact_linking_events_and_append_only_log() {
    // arrange
    // act
    let (temp_dir, run_state, agent_id, bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_contract").await;
    let run_id = "run_checkpoint_contract";

    // assert

    // Append-only proof: the pre-compaction bytes stay an untouched prefix.
    let bytes_after = fs::read(&run_state.info.events_path).unwrap_or_abort();
    assert!(
        bytes_after.starts_with(&bytes_before),
        "compaction must append to events.jsonl, never rewrite it"
    );

    let events = read_events(&run_state.info.events_path);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ArtifactWritten(_)))
            .count(),
        1,
        "exactly one ArtifactWritten event"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::CompactionWritten(_)))
            .count(),
        1,
        "exactly one CompactionWritten event"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::CompactionApplied(_)))
            .count(),
        1,
        "exactly one CompactionApplied event"
    );
    assert_eq!(count_session_compaction_events(&events), 1);

    let (artifact_event_seq, artifact_event) = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::ArtifactWritten(event) => Some((e.seq, event)),
            _ => None,
        })
        .expect("ArtifactWritten event");
    let (written_event_seq, written_event) = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::CompactionWritten(event) => Some((e.seq, event)),
            _ => None,
        })
        .expect("CompactionWritten event");
    let (applied_event_seq, applied_event) = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::CompactionApplied(event) => Some((e.seq, event)),
            _ => None,
        })
        .expect("CompactionApplied event");
    let session_compaction = last_session_compaction_event(&events);
    let session_compaction_seq = events
        .iter()
        .rev()
        .find(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
        .map(|e| e.seq)
        .unwrap_or_abort();

    // The artifact exists completely before the events that reference it.
    assert!(
        artifact_event_seq < written_event_seq,
        "ArtifactWritten precedes CompactionWritten"
    );
    assert!(
        written_event_seq < applied_event_seq,
        "CompactionWritten precedes CompactionApplied"
    );
    assert!(
        applied_event_seq < session_compaction_seq,
        "CompactionApplied precedes SessionCompaction"
    );

    // The artifact lives under the run-relative artifacts/compactions layout
    // and its digest matches what the linking events record.
    assert!(
        written_event
            .artifact_path
            .starts_with("artifacts/compactions/")
            && written_event.artifact_path.ends_with(".json"),
        "artifact path uses the artifacts/compactions layout: {}",
        written_event.artifact_path
    );
    let artifact_file = run_state.info.run_dir.join(&written_event.artifact_path);
    let artifact_bytes = fs::read(&artifact_file).unwrap_or_abort();
    let digest = blake3::hash(&artifact_bytes).to_hex().to_string();
    let artifact_len = u64::try_from(artifact_bytes.len()).unwrap_or_abort();
    assert_eq!(
        written_event.artifact_digest,
        Some(digest.clone()),
        "CompactionWritten links the artifact digest"
    );
    assert_eq!(written_event.artifact_bytes, artifact_len);

    // The ArtifactWritten index event agrees with the artifact on disk.
    assert_eq!(artifact_event.path, written_event.artifact_path);
    assert_eq!(artifact_event.digest, digest);
    assert_eq!(artifact_event.bytes, artifact_len);
    assert_eq!(
        artifact_event
            .metadata
            .get("artifact_kind")
            .map(String::as_str),
        Some("provider_context_checkpoint")
    );
    assert_eq!(
        artifact_event
            .metadata
            .get("checkpoint_id")
            .map(String::as_str),
        Some(written_event.checkpoint_id.as_str())
    );

    // The artifact deserializes to a checkpoint whose metadata matches the
    // events it is linked by.
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_slice(&artifact_bytes).unwrap_or_abort();
    assert!(session_compaction.first_kept_event_seq > 0);
    let through_seq = session_compaction.first_kept_event_seq - 1;
    assert_eq!(
        checkpoint.metadata.checkpoint_id,
        written_event.checkpoint_id
    );
    assert_eq!(checkpoint.metadata.agent_id, agent_id);
    assert_eq!(checkpoint.metadata.run_id, run_id);
    assert_eq!(checkpoint.metadata.through_seq, through_seq);
    assert_eq!(written_event.through_seq, through_seq);
    assert_eq!(applied_event.through_seq, through_seq);
    assert_eq!(applied_event.checkpoint_id, written_event.checkpoint_id);
    assert_eq!(written_event.trigger_reason, "proactive");
    assert!(written_event.tokens_before_estimate.unwrap_or(0) > 0);
    assert!(checkpoint.summary.contains("Checkpoint summary"));
    // Proactive triggers keep replayable turns in the log; the artifact
    // carries nothing extra.
    assert!(checkpoint.recent_turns.is_empty());
    assert_eq!(written_event.preserved_turns, 0);
    assert_eq!(applied_event.preserved_turns, Some(0));

    // The in-memory context carries the same checkpoint identity.
    let context = run_state
        .provider_context_by_agent
        .get(&agent_id)
        .expect("provider context updated");
    let metadata = context
        .checkpoint
        .as_ref()
        .expect("context carries checkpoint metadata");
    assert_eq!(metadata.checkpoint_id, written_event.checkpoint_id);
    assert_eq!(metadata.agent_id, agent_id);
    assert_eq!(metadata.run_id, run_id);
    assert_eq!(metadata.through_seq, through_seq);
    assert!(context
        .compacted_summary
        .as_ref()
        .unwrap_or_abort()
        .contains("Checkpoint summary"));

    let _ = temp_dir;
}

#[allow(
    deprecated,
    reason = "checkpoint contract tests exercise the deprecated CompactionWritten/CompactionApplied events"
)]
#[tokio::test]
async fn session_compaction_restore_reconstructs_same_context_from_event_and_checkpoint_pair() {
    // arrange
    // act
    let (temp_dir, run_state, agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_restore").await;
    let run_id = "run_checkpoint_restore";

    // assert
    let in_memory = run_state
        .provider_context_by_agent
        .get(&agent_id)
        .cloned()
        .expect("provider context updated");
    assert!(
        in_memory.checkpoint.is_some(),
        "in-memory context carries checkpoint metadata"
    );

    let first = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    let second = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    assert_eq!(first, second, "restore must be deterministic");

    let restored = first
        .get(&agent_id)
        .expect("restored context for the agent")
        .clone();
    assert_eq!(
        restored, in_memory,
        "restart must reconstruct the same provider context from the event and checkpoint artifact pair"
    );
    assert!(restored
        .compacted_summary
        .as_ref()
        .unwrap_or_abort()
        .contains("Checkpoint summary"));
}

#[allow(
    deprecated,
    reason = "checkpoint contract tests exercise the deprecated CompactionWritten/CompactionApplied events"
)]
#[tokio::test]
async fn session_compaction_restore_fails_when_checkpoint_artifact_is_missing() {
    // arrange
    let (temp_dir, run_state, _agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_missing").await;
    let run_id = "run_checkpoint_missing";

    let events = read_events(&run_state.info.events_path);
    let written_event = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::CompactionWritten(event) => Some(event),
            _ => None,
        })
        .expect("CompactionWritten event");
    let artifact_file = run_state.info.run_dir.join(&written_event.artifact_path);

    // act
    fs::remove_file(&artifact_file).unwrap_or_abort();

    // assert
    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore must fail when the checkpoint artifact is gone");
    assert!(
        matches!(err, CoordinatorError::ResumeRestoreFailed { .. }),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Checkpoint trust-boundary regression tests
// ---------------------------------------------------------------------------

#[allow(
    deprecated,
    reason = "checkpoint trust-boundary tests read the deprecated CompactionWritten event"
)]
fn find_written_event(events: &[EventEnvelopeV1]) -> &CompactionWrittenEvent {
    events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::CompactionWritten(event) => Some(event),
            _ => None,
        })
        .expect("CompactionWritten event")
}

/// Rewrite the `artifact_path` recorded by every `CompactionWritten` line in
/// the append-only event log. Restore then observes the hostile path exactly
/// as an attacker-controlled session log would present it.
fn rewrite_compaction_written_artifact_path(events_path: &Path, replacement: &str) {
    let text = fs::read_to_string(events_path).unwrap_or_abort();
    let mut rewritten = Vec::new();
    let mut touched = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(line).unwrap_or_abort();
        if value["payload"]["event_type"] == "compaction_written" {
            value["payload"]["data"]["artifact_path"] =
                serde_json::Value::String(replacement.to_string());
            touched = true;
        }
        rewritten.push(serde_json::to_string(&value).unwrap_or_abort());
    }
    assert!(
        touched,
        "expected a CompactionWritten event line to rewrite"
    );
    fs::write(events_path, rewritten.join("\n") + "\n").unwrap_or_abort();
}

#[allow(
    deprecated,
    reason = "checkpoint trust-boundary tests exercise the deprecated CompactionWritten event"
)]
#[tokio::test]
async fn session_compaction_restore_fails_when_checkpoint_bytes_do_not_match_recorded_digest() {
    // arrange
    let (temp_dir, run_state, _agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_tampered_digest").await;
    let run_id = "run_checkpoint_tampered_digest";
    let events = read_events(&run_state.info.events_path);
    let written_event = find_written_event(&events);
    let artifact_file = run_state.info.run_dir.join(&written_event.artifact_path);

    // act: tamper the artifact bytes after the digest was recorded.
    let mut artifact_bytes = fs::read(&artifact_file).unwrap_or_abort();
    artifact_bytes.push(b' ');
    fs::write(&artifact_file, &artifact_bytes).unwrap_or_abort();

    // assert: restore fails closed instead of deserializing the drift.
    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore must fail when checkpoint bytes drift from the recorded digest");
    let message = err.to_string();
    assert!(
        matches!(err, CoordinatorError::ResumeRestoreFailed { .. }),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("digest mismatch"),
        "unexpected error: {message}"
    );
    let _ = temp_dir;
}

#[allow(
    deprecated,
    reason = "checkpoint trust-boundary tests exercise the deprecated CompactionWritten event"
)]
#[tokio::test]
async fn session_compaction_restore_rejects_absolute_checkpoint_artifact_path() {
    // arrange
    let (temp_dir, run_state, _agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_absolute_path").await;
    let run_id = "run_checkpoint_absolute_path";
    let events = read_events(&run_state.info.events_path);
    let written_event = find_written_event(&events);
    let absolute_artifact = run_state.info.run_dir.join(&written_event.artifact_path);
    assert!(
        absolute_artifact.is_absolute(),
        "sanity: joined artifact path is absolute"
    );

    // act: record the same existing artifact under its absolute path.
    rewrite_compaction_written_artifact_path(
        &run_state.info.events_path,
        absolute_artifact.to_str().unwrap_or_abort(),
    );

    // assert
    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore must reject absolute checkpoint artifact paths");
    let message = err.to_string();
    assert!(
        matches!(err, CoordinatorError::ResumeRestoreFailed { .. }),
        "unexpected error: {message}"
    );
    assert!(message.contains("absolute"), "unexpected error: {message}");
    let _ = temp_dir;
}

#[allow(
    deprecated,
    reason = "checkpoint trust-boundary tests exercise the deprecated CompactionWritten event"
)]
#[tokio::test]
async fn session_compaction_restore_rejects_parent_traversal_checkpoint_artifact_path() {
    // arrange
    let (temp_dir, run_state, _agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_traversal").await;
    let run_id = "run_checkpoint_traversal";
    let events = read_events(&run_state.info.events_path);
    let written_event = find_written_event(&events);

    // Plant a readable checkpoint outside the compactions directory so the
    // failure is caused by the traversal guard, not a missing file.
    let artifact_bytes =
        fs::read(run_state.info.run_dir.join(&written_event.artifact_path)).unwrap_or_abort();
    fs::write(
        run_state.info.run_dir.join("evil_checkpoint.json"),
        &artifact_bytes,
    )
    .unwrap_or_abort();

    // act
    rewrite_compaction_written_artifact_path(
        &run_state.info.events_path,
        "artifacts/compactions/../evil_checkpoint.json",
    );

    // assert
    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore must reject parent traversal in recorded artifact paths");
    let message = err.to_string();
    assert!(
        matches!(err, CoordinatorError::ResumeRestoreFailed { .. }),
        "unexpected error: {message}"
    );
    assert!(message.contains("traversal"), "unexpected error: {message}");
    let _ = temp_dir;
}

#[cfg(unix)]
#[allow(
    deprecated,
    reason = "checkpoint trust-boundary tests exercise the deprecated CompactionWritten event"
)]
#[tokio::test]
async fn session_compaction_restore_rejects_symlinked_checkpoint_artifact_escape() {
    // arrange
    let (temp_dir, run_state, _agent_id, _bytes_before) =
        compact_two_turn_checkpoint_fixture("run_checkpoint_symlink_escape").await;
    let run_id = "run_checkpoint_symlink_escape";
    let events = read_events(&run_state.info.events_path);
    let written_event = find_written_event(&events);
    let artifact_file = run_state.info.run_dir.join(&written_event.artifact_path);

    // Move the bytes outside the run and replace the artifact with a symlink.
    // The recorded digest still matches the kept bytes, so only the escape
    // guard can reject this restore.
    let outside_target = temp_dir.path().join("outside_evil_checkpoint.json");
    let artifact_bytes = fs::read(&artifact_file).unwrap_or_abort();
    fs::write(&outside_target, &artifact_bytes).unwrap_or_abort();
    fs::remove_file(&artifact_file).unwrap_or_abort();
    std::os::unix::fs::symlink(&outside_target, &artifact_file).unwrap_or_abort();

    // act
    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore must reject a checkpoint artifact that escapes the run via symlink");

    // assert
    let message = err.to_string();
    assert!(
        matches!(err, CoordinatorError::ResumeRestoreFailed { .. }),
        "unexpected error: {message}"
    );
    assert!(message.contains("escape"), "unexpected error: {message}");
    let _ = temp_dir;
}
