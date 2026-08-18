use super::*;
use crate::ui::ui_transcript_test_helpers::{
    transcript_section_model_test_activity, transcript_section_model_test_tool_call,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderReasoningDeltaEvent,
    ToolCallRequestedEvent, SCHEMA_VERSION,
};
use harness_providers::UnwrapOrAbort;

const REQUEST_ID: &str = "spacing-visibility-turn";

fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("spacing-visibility-{seq}"),
        seq,
        run_id: "spacing-visibility-run".to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Supervisor, Some("default".to_string())),
        correlation_id: Some(REQUEST_ID.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn visibility_contract(thinking_visible: bool) -> Vec<(TranscriptRenderSurfaceKind, usize)> {
    let mut generic = transcript_section_model_test_tool_call("generic-1", "custom.tool");
    generic.status = ToolCallDisplayStatus::Succeeded;
    generic.first_seq = 1;
    generic.last_seq = 1;

    let mut bash = transcript_section_model_test_tool_call("bash-1", "bash");
    bash.status = ToolCallDisplayStatus::Succeeded;
    bash.first_seq = 3;
    bash.last_seq = 3;

    let mut activity = transcript_section_model_test_activity(REQUEST_ID, ActivityStatus::Done, "");
    activity.thinking_text = "hidden reasoning".to_string();
    activity.tool_calls = vec![generic, bash];
    activity.first_seq = 1;
    activity.last_seq = 3;

    let mut app = AppState::default();
    app.activities.push_back(activity);
    app.events.extend([
        event(
            1,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "generic-1".into(),
                tool_id: "custom.tool".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "generic-digest".to_string(),
                metadata: None,
            }),
        ),
        event(
            2,
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: REQUEST_ID.into(),
                delta: "hidden reasoning".to_string(),
            }),
        ),
        event(
            3,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "bash-1".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "bash-digest".to_string(),
                metadata: None,
            }),
        ),
    ]);
    app.transcript_view.show_transcript_thinking = thinking_visible;

    let theme = Theme::default();
    let turn = build_transcript_sections(&app)
        .into_iter()
        .next()
        .unwrap_or_abort();
    build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell)
        .into_iter()
        .map(|surface| (surface.kind, surface.leading_gap_rows))
        .collect()
}

#[test]
fn visible_reasoning_remains_between_collapsed_tool_surfaces() {
    // Given / When: reasoning is visible between two collapsed tools.
    let contract = visibility_contract(true);

    // Then: all three visible groupable surfaces remain densely packed.
    assert_eq!(
        contract,
        vec![
            (TranscriptRenderSurfaceKind::AssistantTool, 0),
            (TranscriptRenderSurfaceKind::AssistantReasoning, 0),
            (TranscriptRenderSurfaceKind::AssistantTool, 0),
        ]
    );
}

#[test]
fn hidden_reasoning_is_removed_before_collapsed_tool_spacing_resolves() {
    // Given / When: the production visibility toggle hides reasoning.
    let contract = visibility_contract(false);

    // Then: the now-adjacent collapsed tools remain densely packed.
    assert_eq!(
        contract,
        vec![
            (TranscriptRenderSurfaceKind::AssistantTool, 0),
            (TranscriptRenderSurfaceKind::AssistantTool, 0),
        ]
    );
}
