use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn mouse_click_on_task_inline_row_opens_subagent_session() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).unwrap_or_abort();

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
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_click",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "inspect child".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));

    let (column, row) = transcript_click_position(&app, "inspect child");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    assert_ne!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated
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
    assert_ne!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated,
        "Harness inline task rows keep a flat surface on hover"
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

pub(crate) fn mouse_click_on_task_inline_row_uses_task_row_child_session() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).unwrap_or_abort();

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
            tool_call_id: "tc_child_click_task_row".to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: "digest-child-click-task-row".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_tool".to_string(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-child-tool-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_click_task_row".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let (column, row) = transcript_click_position(&app, "Explore Task — inspect child");
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
