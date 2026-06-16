use super::*;

#[path = "subagent_navigation_keyboard_tests.rs"]
mod subagent_navigation_keyboard_tests;
pub(super) use subagent_navigation_keyboard_tests::{
    disk_backed_child_navigation_stays_in_live_tui_stack as keyboard_disk_backed_child_navigation_stays_in_live_tui_stack,
    keyboard_sidebar_subagent_selection_opens_child_session as keyboard_keyboard_sidebar_subagent_selection_opens_child_session,
    live_subagent_hitbox_uses_rendered_transcript_area as keyboard_live_subagent_hitbox_uses_rendered_transcript_area,
    mouse_click_on_task_inline_row_opens_subagent_session as keyboard_mouse_click_on_task_inline_row_opens_subagent_session,
};

pub(super) fn mouse_click_on_subagent_footer_navigates_parent_previous_and_next() {
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
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_a",
        "agent_child_a",
        "req_child_a",
    ));
    app.ingest_event(child_task_requested(
        5,
        "req_parent",
        "tc_child_b",
        "agent_child_b",
        "req_child_b",
    ));
    app.ingest_event(child_agent_spawned(6, "agent_child_a", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child_a",
        EventActor::new(ActorKind::Worker, Some("agent_child_a".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_a".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child-a".to_string(),
            prompt_summary: "inspect child a".to_string(),
            request_digest: "digest-child-a-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(child_agent_spawned(8, "agent_child_b", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        9,
        "req_child_b",
        EventActor::new(ActorKind::Worker, Some("agent_child_b".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_b".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child-b".to_string(),
            prompt_summary: "inspect child b".to_string(),
            request_digest: "digest-child-b-prompt".to_string(),
            metadata: None,
        }),
    ));

    app.navigate_to_child_session_id("agent_child_a".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child_a"));
    assert!(app.replay_mode, "inline child sessions open read-only");
    assert!(render_debug(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height).contains("Next ]"));
    assert!(
        !render_debug(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height).contains("▼ MCP"),
        "subagent chat should use the main transcript shell without the replay sidebar"
    );

    let (next_column, next_row) = footer_click_position(&app, "Next");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: next_column,
            row: next_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("agent_child_b"));

    let (previous_column, previous_row) = footer_click_position(&app, "Prev");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: previous_column,
            row: previous_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("agent_child_a"));

    let (parent_column, parent_row) = footer_click_position(&app, "Parent");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: parent_column,
            row: parent_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode);
}

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
        transcript_click_position(&app, "General Task — Subagent functionality smoke test");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    let (_, detail_row) = transcript_click_position(&app, "└ 0 toolcalls · 16ms");
    assert_eq!(detail_row, row + 1);

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

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

pub(super) fn mouse_click_on_subagent_hint_opens_first_child_session() {
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
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_hint",
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

    let hint = format!(
        "{} view subagents",
        app.keymap.get_binding_str(Action::SessionChildFirst)
    );
    let (column, row) = transcript_click_position(&app, &hint);
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::FirstSubagentSession)
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
    assert!(
        app.replay_mode,
        "hint opens inline child sessions read-only"
    );
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
