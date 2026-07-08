use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn mouse_up_on_completed_general_task_row_opens_child_session() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).unwrap_or_abort();

    let mut app = AppState::new_live(Some(parent_path), false, None);
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
            tool_call_id: "tc_general_child".into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"Subagent functionality smoke test"}"#.to_string(),
            args_digest: "digest-general-child".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "general", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_general_child".to_string().into(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-general-child-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_general_child".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(TaskTerminalScope::ToolCall),
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(22),
                    elapsed_ms: Some(16),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".into(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Subagent functionality smoke test".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        8,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_general_child".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-general-child-result".to_string()),
            output_json: Some(serde_json::json!({
                "description": "Subagent functionality smoke test",
                "status": "completed",
                "child_tool_call_count": 0,
                "duration_ms": 16,
                "child_session_id": "agent_child",
                "child_request_id": "req_child"
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_general_child".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let (column, row) =
        transcript_click_position(&app, "General Task — Subagent functionality smoke test");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(
        app.hovered_transcript_target(),
        Some(&TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
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
    assert!(
        app.replay_mode,
        "mouse-up opens inline child sessions read-only"
    );
}
