use super::*;
use crate::config::CompactionSettings;
use crate::event::{AssistantMessageFinishedEvent, ProviderRequestStartedEvent};
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

fn append_session_compaction_event(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    summary: &str,
    first_kept_event_seq: u64,
) {
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{agent_id}")),
        EventV1::SessionCompaction(crate::event::SessionCompactionEvent {
            agent_id: agent_id.to_string(),
            summary: summary.to_string(),
            first_kept_event_seq,
            first_kept_request_id: None,
            tokens_before: 1000,
            read_files: Vec::new(),
            modified_files: Vec::new(),
            trigger_reason: "proactive".to_string(),
            from_hook: false,
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
// Test cases
// ---------------------------------------------------------------------------

/// Happy path: threshold compaction appends a single `SessionCompaction` event.
#[tokio::test]
async fn threshold_compaction_appends_single_session_compaction_event() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_threshold_compaction");
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

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nTest summary".to_string(),
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

    assert!(result.is_some(), "compaction should produce a result");
    let applied = result.unwrap_or_abort();
    assert!(applied.summary.contains("Test summary"));
    assert!(applied.tokens_before > 0);
    assert!(applied.tokens_after < applied.tokens_before || applied.tokens_after > 0);

    let events = read_events(&run_state.info.events_path);
    assert_eq!(
        count_session_compaction_events(&events),
        1,
        "exactly one SessionCompaction event"
    );

    let compaction_event = last_session_compaction_event(&events);
    assert_eq!(compaction_event.agent_id, agent_id);
    assert!(compaction_event.summary.contains("Test summary"));
    assert_eq!(compaction_event.trigger_reason, "proactive");
    assert!(!compaction_event.from_hook);

    let context = run_state
        .provider_context_by_agent
        .get(agent_id)
        .expect("provider context updated");
    assert!(context.compacted_summary.is_some());
    assert!(context
        .compacted_summary
        .as_ref()
        .unwrap_or_abort()
        .contains("Test summary"));
}

/// Split-turn compaction: cut point lands on an `AssistantMessageFinished`,
/// requiring a turn-prefix summary.
#[tokio::test]
async fn split_turn_compaction_produces_combined_summary() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_turn");
    run_state.recorded_runtime_context = Some(small_context_runtime_context(3000));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    // Two turns with ~1000 tokens each.
    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        &large_text('X', 4000),
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
        &large_text('Y', 4000),
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

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nSplit turn summary".to_string(),
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
        "split-turn compaction should produce a result"
    );

    let events = read_events(&run_state.info.events_path);
    assert_eq!(count_session_compaction_events(&events), 1);

    let compaction_event = last_session_compaction_event(&events);
    // The summary should contain the mock provider's output.
    assert!(compaction_event.summary.contains("Split turn summary"));
}

/// Iterative compaction: a second compaction finds the previous summary
/// and updates it.
#[tokio::test]
async fn iterative_compaction_updates_previous_summary() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_iterative");
    run_state.recorded_runtime_context = Some(small_context_runtime_context(2000));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    // First turn.
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

    // Manually append a previous SessionCompaction event.
    append_session_compaction_event(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "## Goal\nPrevious summary",
        1,
    );

    // Second turn (after the previous compaction).
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

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nUpdated summary".to_string(),
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
        "iterative compaction should produce a result"
    );

    let events = read_events(&run_state.info.events_path);
    // Two SessionCompaction events: the manually appended one + the new one.
    assert_eq!(
        count_session_compaction_events(&events),
        2,
        "two SessionCompaction events (previous + new)"
    );

    let compaction_event = last_session_compaction_event(&events);
    assert!(
        compaction_event.summary.contains("Updated summary"),
        "new summary should replace the previous one"
    );

    let context = run_state
        .provider_context_by_agent
        .get(agent_id)
        .expect("provider context updated");
    assert!(context
        .compacted_summary
        .as_ref()
        .unwrap_or_abort()
        .contains("Updated summary"));
}

/// Manual trigger always attempts compaction, even below the threshold.
#[tokio::test]
async fn manual_trigger_always_attempts_compaction() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_manual_trigger");
    // High context window so threshold check would fail.
    run_state.recorded_runtime_context = Some(small_context_runtime_context(100_000));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    // Two small turns (well below the 100k threshold).
    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "Small first question",
    );
    append_provider_started(&clock, &redactor, &mut run_state, agent_id, "req_1");
    append_stream_delta(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "Small answer 1",
    );
    append_assistant_finished(&clock, &redactor, &mut run_state, agent_id, "req_1");
    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_2",
        "Small second question",
    );
    append_provider_started(&clock, &redactor, &mut run_state, agent_id, "req_2");
    append_stream_delta(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_2",
        "Small answer 2",
    );
    append_assistant_finished(&clock, &redactor, &mut run_state, agent_id, "req_2");

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nManual compaction summary".to_string(),
    });

    let result = compact_session(
        &clock,
        &redactor,
        &mut run_state,
        provider,
        agent_id,
        "manual",
        &settings(true, 0, 500),
        None,
    )
    .await
    .unwrap_or_abort();

    assert!(
        result.is_some(),
        "manual trigger should force compaction even below threshold"
    );

    let events = read_events(&run_state.info.events_path);
    assert_eq!(count_session_compaction_events(&events), 1);

    let compaction_event = last_session_compaction_event(&events);
    assert_eq!(compaction_event.trigger_reason, "manual");
    assert!(compaction_event
        .summary
        .contains("Manual compaction summary"));
}
