use super::*;

#[path = "subagent_navigation_keyboard_tests.rs"]
mod subagent_navigation_keyboard_tests;
pub(super) use subagent_navigation_keyboard_tests::{
    disk_backed_child_navigation_stays_in_live_tui_stack as keyboard_disk_backed_child_navigation_stays_in_live_tui_stack,
    keyboard_sidebar_subagent_selection_opens_child_session as keyboard_keyboard_sidebar_subagent_selection_opens_child_session,
    live_subagent_hitbox_uses_rendered_transcript_area as keyboard_live_subagent_hitbox_uses_rendered_transcript_area,
    mouse_click_on_task_inline_row_opens_subagent_session as keyboard_mouse_click_on_task_inline_row_opens_subagent_session,
};

pub(super) fn mouse_click_on_task_inline_row_uses_task_row_child_session() {
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

    let (column, row) = transcript_click_position(&app, "inspect child · Explore Agent");
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

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

pub(super) fn mouse_up_on_completed_general_task_row_opens_child_session() {
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
            tool_call_id: "tc_general_child".to_string(),
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
            task_id: "task_general_child".to_string(),
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
            request_id: "req_child".to_string(),
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
            tool_call_id: "tc_general_child".to_string(),
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
        transcript_click_position(&app, "Subagent functionality smoke test · General Agent");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    let (_, result_row) = transcript_click_position(&app, "child completed");
    assert_eq!(result_row, row + 1);

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

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(
        app.replay_mode,
        "mouse-up opens inline child sessions read-only"
    );
}

pub(super) fn mouse_click_on_task_row_uses_harness_session_metadata() {
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

    let (column, row) =
        transcript_click_position(&app, "Smoke test subagent dispatch · Plan Agent");
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

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

pub(super) fn slash_exit_from_inline_subagent_restores_parent_before_quit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, Some(intent_sink));
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
        "tc_child_exit",
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

    app.navigate_to_child_session_id("agent_child".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode);

    app.execute_slash_command("exit", None);

    assert!(app.should_quit);
    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode);
    assert!(!app.current_subagent_session_present());
    let intents = intents.lock().expect("lock intents");
    assert!(intents
        .iter()
        .any(|intent| matches!(intent, UiIntent::QuitRequested)));
}
