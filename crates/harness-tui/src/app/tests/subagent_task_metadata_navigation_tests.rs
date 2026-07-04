use super::*;

pub(crate) fn mouse_click_on_task_row_uses_harness_session_metadata() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_harness_child".to_string(),
            tool_id: "task".to_string(),
            args_summary:
                r#"{"description":"Smoke test subagent dispatch","subagent_type":"plan"}"#
                    .to_string(),
            args_digest: "digest-harness-child".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "plan", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Smoke test subagent dispatch".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        7,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_harness_child".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-harness-child-result".to_string()),
            output_json: Some(serde_json::json!({
                "description": "Smoke test subagent dispatch",
                "metadata": {
                    "sessionId": "agent_child",
                    "requestId": "req_child"
                },
                "duration_ms": 1700,
                "child_tool_call_count": 0
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let (column, row) = transcript_click_position(&app, "Plan Task — Smoke test subagent dispatch");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_ne!(
        app.overlay_stack().top(),
        Some(OverlayKind::SubagentActions)
    );
    assert!(!render_text(&app, 140, 40).contains("Subagent Actions"));
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}
