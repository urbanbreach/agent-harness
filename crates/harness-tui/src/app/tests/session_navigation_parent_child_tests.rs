use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn parent_transcript_hides_child_prompt_before_task_tool_finishes() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".into(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_child_pending".into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: "digest-child-pending".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    let mut child_prompt = envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Supervisor, None),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".into(),
            text: "Inspect child prompt that belongs to the subagent".to_string(),
        }),
    );
    child_prompt.stream_key = Some("agent:agent_child".to_string());
    app.ingest_event(child_prompt);

    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide the child prompt immediately after submission: {parent_debug}"
    );

    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_turn".to_string().into(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:model-child".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        8,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".into(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Inspect child prompt that belongs to the subagent".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        9,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_child_pending".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child task scheduled".to_string()),
            output_digest: Some("digest-child-output".to_string()),
            output_json: Some(serde_json::json!({
                "child_session_id": "agent_child",
                "child_request_id": "req_child",
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_pending".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide child prompts before the task tool finishes: {parent_debug}"
    );

    let child_dir = tempfile::tempdir().unwrap_or_abort();
    let child_path = child_dir.path().join("agent_child");
    fs::create_dir_all(&child_path).unwrap_or_abort();
    app.session_path = Some(child_path);
    let child_debug = render_debug(&app, 140, 40);
    assert!(
        child_debug.contains("Inspect child prompt that belongs to the subagent"),
        "the inline child session should still render its own prompt: {child_debug}"
    );
}
