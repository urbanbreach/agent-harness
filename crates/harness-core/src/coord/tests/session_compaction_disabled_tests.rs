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
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

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
            metadata: Some(crate::event::ProviderRequestStartedMetadata {
                context_budget: Some(pressured_compaction_budget()),
                ..Default::default()
            }),
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
            parts: Vec::new(),
            provenance: None,
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
            first_kept_entry_id: None,
            tokens_before: 1000,
            tokens_after: None,
            summary_usage: None,
            summary_provider_id: None,
            summary_model_id: None,
            read_files: Vec::new(),
            modified_files: Vec::new(),
            current_intent: None,
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
async fn disabled_compaction_is_noop() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_disabled");
    run_state.recorded_runtime_context = Some(small_context_runtime_context(100));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    append_user_message(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "Question",
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

    let provider = Arc::new(SummaryMockProvider {
        summary: "## Goal\nShould not be used".to_string(),
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

    assert!(result.is_none(), "disabled compaction should be a no-op");

    let events = read_events(&run_state.info.events_path);
    assert_eq!(count_session_compaction_events(&events), 0);
}
