use super::*;
use crate::config::CompactionSettings;
use crate::event::ToolCallStatus;
use crate::event::{
    AssistantMessageFinishedEvent, EditAppliedEvent, ProviderRequestStartedEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent,
};
use crate::ids::RequestId;
use crate::proj::RecordedRuntimeContext;
use crate::UnwrapOrAbort;
use async_trait::async_trait;
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use serde_json::json;
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
        EventV1::UserMessageSubmitted(crate::event::UserMessageSubmittedEvent {
            request_id: RequestId::new(request_id),
            text: text.to_string(),
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
        EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
            request_id: RequestId::new(request_id),
            delta: delta.to_string(),
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
            tool_call_count: 2,
            parts: Vec::new(),
            provenance: None,
            assistant_message: None,
        }),
    )
    .unwrap_or_abort();
}

fn append_tool_call_requested(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        Some(request_id.to_string()),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: "digest".to_string(),
            metadata: Some(tool_metadata(tool_id)),
        }),
    )
    .unwrap_or_abort();
}

fn append_tool_call_finished(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    request_id: &str,
    tool_call_id: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        Some(request_id.to_string()),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("done".to_string()),
            output_digest: Some("digest-out".to_string()),
            output_json: None,
            metadata: Some(tool_metadata("read")),
        }),
    )
    .unwrap_or_abort();
}

fn append_edit_applied(
    clock: &FakeClock,
    redactor: &DefaultRedactor,
    run_state: &mut RunState,
    agent_id: &str,
    edit_id: &str,
    path: &str,
) {
    let actor = EventActor::new(ActorKind::Worker, Some(agent_id.to_string()));
    append_payload_event(
        clock,
        redactor,
        run_state,
        actor,
        Some(format!("agent:{agent_id}")),
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: edit_id.to_string(),
            path: path.to_string(),
            new_file_digest: "digest-new".to_string(),
            diff_rel_path: None,
            diff_digest: None,
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

pub async fn operational_memory_records_read_and_modified_files_from_events() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_operational_memory");
    run_state.recorded_runtime_context = Some(small_context_runtime_context(3000));
    let agent_id = "agent_000001";
    setup_agent(&mut run_state, agent_id);

    // Turn 1: user asks a question, assistant answers and uses read + edit tools.
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
    append_tool_call_requested(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "toolcall_read",
        "read",
        r#"{"path": "src/lib.rs"}"#,
    );
    append_tool_call_finished(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "toolcall_read",
    );
    append_tool_call_requested(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "toolcall_edit",
        "edit",
        r#"{"path": "src/main.rs"}"#,
    );
    append_tool_call_finished(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "req_1",
        "toolcall_edit",
    );
    append_edit_applied(
        &clock,
        &redactor,
        &mut run_state,
        agent_id,
        "edit_000001",
        "src/main.rs",
    );
    append_assistant_finished(&clock, &redactor, &mut run_state, agent_id, "req_1");

    // Turn 2: keep recent so turn 1 is summarized.
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
        summary: "## Goal\nOperational memory summary".to_string(),
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

    assert!(result.is_some(), "compaction should produce a result");

    let events = read_events(&run_state.info.events_path);
    let compaction_event = last_session_compaction_event(&events);
    assert_eq!(compaction_event.agent_id, agent_id);
    assert!(
        compaction_event
            .read_files
            .iter()
            .any(|p| p == "src/lib.rs"),
        "read_files should contain src/lib.rs: {:?}",
        compaction_event.read_files
    );
    assert!(
        compaction_event
            .modified_files
            .iter()
            .any(|p| p == "src/main.rs"),
        "modified_files should contain src/main.rs: {:?}",
        compaction_event.modified_files
    );
    assert!(compaction_event
        .summary
        .contains("Operational memory summary"));
}
