use super::*;
use harness_core::{coord::NativeSubagentRegistration, subagent::*};

fn transition(kind: SubagentTransitionKind) -> SubagentTransitionV1 {
    SubagentTransitionV1 {
        payload_version: 1,
        child_id: SubagentId("child".into()),
        attempt_id: Some("req_child".into()),
        generation: 1,
        notification_seq: None,
        origin: LifecycleOrigin::Stream,
        transition: kind,
        metadata: SubagentTransitionMetadataV1 {
            ancestry: SubagentAncestry::new(
                Some(SubagentId("parent".into())),
                Some(SubagentId("parent".into())),
            ),
            execution_owner: SubagentExecutionOwner::RootSession {
                session_id: "parent".into(),
            },
            display_route: SubagentDisplayRoute {
                root_session_id: "parent".into(),
                parent_session_id: Some("parent".into()),
                child_session_id: "child".into(),
            },
            notification_route: SubagentNotificationRoute {
                parent_prompt_id: Some("req_parent".into()),
                background: true,
                await_to_completion: false,
                surface_completion: true,
            },
            injected_depth: InjectedSubagentDepth(0),
            isolation_requested: SubagentIsolationMode::None,
            context: ResolvedSubagentContext {
                effective_cwd: "/fixture".into(),
                policy_roots: vec!["/fixture".into()],
                isolation: ResolvedSubagentIsolation::SharedWorkspace,
            },
        },
        outcome: None,
        accounting: None,
        finalized_state: None,
    }
}

#[test]
fn native_subagent_lifecycle_drives_pane_child_view_and_one_terminal_row() {
    let temp = tempfile::tempdir().unwrap_or_abort();
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(Some(temp.path().join("parent")), false, Some(sink));
    app.set_frame_area(Rect::new(0, 0, 120, 40));
    for event in [
        run_started(1),
        agent_spawned(2, "parent", "default"),
        provider_started(3, "req_parent", "mock", "parent-model"),
        envelope(
            4,
            "req_parent",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "spawn".into(),
                tool_id: "spawn_subagent".into(),
                args_summary: r#"{"prompt":"inspect","description":"Inspect files"}"#.into(),
                args_digest: "digest".into(),
                metadata: None,
            }),
        ),
    ] {
        app.ingest_event(event);
    }
    app.ingest_event(envelope(
        5,
        "req_parent",
        EventV1::NativeSubagentRegistered(Box::new(NativeSubagentRegistration {
            payload_version: 1,
            child_id: "child".into(),
            spawner: "parent".into(),
            root_agent: "parent".into(),
            parent_tool: "spawn".into(),
            parent_request: Some("req_parent".into()),
            subagent_type: "explore".into(),
            persona: Some("reviewer".into()),
            role: None,
            fork_context: true,
            description: "Inspect files".into(),
            prompt: "inspect".into(),
            background: true,
            isolation: SubagentIsolationMode::None,
            source: None,
            model: "child-model".into(),
            system_prompt: String::new(),
            tools: vec![],
            permission_rules: Default::default(),
            max_iters: None,
            allowed_types: None,
            model_inherited: true,
            messaging_granted: false,
        })),
    ));
    let started = transition(SubagentTransitionKind::Spawned);
    app.ingest_event(envelope(
        6,
        "req_child",
        EventV1::SubagentTransition(Box::new(started.clone())),
    ));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("child".into())),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".into(),
            text: "Child-only prompt".into(),
        }),
    ));
    assert!(app.tasks_pane.visible);
    assert!(!app.tasks_pane.focused);
    let root = render_text(&app, 120, 40);
    assert!(root.contains("▾ Subagents 1"), "{root}");
    assert!(root.contains("Running 1 subagent"), "{root}");
    assert!(!root.contains("Child-only prompt"));
    app.ingest_event(envelope_with_actor(
        8,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("child".into())),
        EventV1::AssistantMessageFinished(harness_core::event::AssistantMessageFinishedEvent {
            request_id: "req_child".into(),
            tool_call_count: 0,
            parts: vec![harness_core::session::AssistantPart::Text {
                text: "Read [guide covering child lifecycles, permissions, navigation, and durable history](https://example.org/guide) and [API](https://example.org/api)."
                    .into(),
            }],
            provenance: None,
            assistant_message: None,
        }),
    ));
    app.composer.prompt_buffer = "parent draft".into();
    app.transcript_view.show_transcript_thinking = false;
    assert_native_inspection(&mut app, &intents);
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(
        harness_core::event::LiveEventEnvelope {
            event_id: "parent-live".into(),
            run_id: "run_app_tests".into(),
            mono_ms: 8,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("parent".into())),
            correlation_id: Some("req_parent".into()),
            causation_id: None,
            stream_key: None,
            payload: harness_core::event::LiveEventV1::ProviderTextDelta {
                request_id: "req_parent".into(),
                delta: "Parent kept streaming".into(),
            },
        },
    )));
    app.transcript_view.show_transcript_thinking = true;
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.current_session_id(), Some("parent"));
    assert_eq!(app.composer.prompt_buffer, "parent draft");
    assert!(!app.transcript_view.show_transcript_thinking);
    assert!(render_text(&app, 120, 40).contains("Parent kept streaming"));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.current_session_id(), Some("child"));
    assert!(
        app.transcript_view.show_transcript_thinking,
        "running child inspection state survives close and reopen"
    );
    app.handle_key(key(KeyCode::Esc));
    assert_native_completion(&mut app, started);
}

fn assert_task_query_editing(app: &mut AppState) {
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_paste("child-model");
    assert_eq!(
        app.task_pane_rows().len(),
        1,
        "filter hides unmatched group headers"
    );
    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::Delete));
    assert_eq!(app.tasks_pane.query.editor.text(), "hild-model");
    app.handle_key(key(KeyCode::Char('c')));
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.tasks_pane.query.editing);
    assert!(app.tasks_pane.query.active);
    app.handle_key(key(KeyCode::Char('f')));
    assert_eq!(
        app.tasks_pane.query.editor.text(),
        "child-model",
        "reopen retains the query"
    );
    assert_task_query_shortcuts(app);
    app.handle_key(key(KeyCode::Esc));
    assert!(app.tasks_pane.focused);
    assert!(!app.tasks_pane.query.active);
    app.handle_key(key(KeyCode::Home));
}

fn assert_task_query_shortcuts(app: &mut AppState) {
    app.handle_key(key(KeyCode::End));
    for (code, expected) in [('b', "child-model"), ('k', "child-mode"), ('u', "")] {
        app.handle_key(key_with_modifiers(
            KeyCode::Char(code),
            KeyModifiers::CONTROL,
        ));
        assert_eq!(app.tasks_pane.query.editor.text(), expected);
    }
    app.handle_paste("review\u{2003}src/lib.rs");
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.tasks_pane.query.editor.text(), "review\u{2003}");
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Delete));
    assert_eq!(app.tasks_pane.query.editor.text(), "rview\u{2003}");
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("foo_bar.rs");
    app.handle_key(key(KeyCode::Home));
    for expected in [7, 8] {
        app.handle_key(key_with_modifiers(KeyCode::Char('f'), KeyModifiers::ALT));
        assert_eq!(
            app.tasks_pane.query.editor.cursor().insertion_index(),
            expected
        );
    }
    app.handle_key(key_with_modifiers(KeyCode::Delete, KeyModifiers::CONTROL));
    assert_eq!(app.tasks_pane.query.editor.text(), "foo_bar.");
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.tasks_pane.query.editor.text().is_empty());
}

fn assert_native_inspection(app: &mut AppState, intents: &Arc<Mutex<Vec<UiIntent>>>) {
    app.execute_action(Action::ToggleTasks);
    assert_task_query_editing(app);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.current_session_id(), Some("child"));
    let child = render_text(app, 120, 40);
    assert!(child.contains("Reviewer"), "{child}");
    assert!(child.contains("forked"));
    assert!(child.contains("Child-only prompt"));
    app.set_frame_area(Rect::new(0, 0, 120, 40));
    assert_child_link_navigation(app);
    assert_child_search(app);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.help_is_open());
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(
        app.current_session_id(),
        Some("child"),
        "closing child help keeps the takeover open"
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        intents.lock().unwrap_or_abort().iter().filter(|intent| matches!(intent, UiIntent::CancelSubagent { session_id } if session_id == "child")).count(), 2
    );
}

fn assert_child_link_navigation(app: &mut AppState) {
    app.set_frame_area(Rect::new(0, 0, 40, 40));
    app.handle_key(key(KeyCode::Char('o')));
    assert!(app.transcript_view.highlighted_link.is_none());
    app.composer.vim_mode = true;
    app.handle_key(key(KeyCode::Char('o')));
    assert_eq!(app.transcript_view.highlighted_link, Some(0));
    assert!(app
        .transcript_view
        .hyperlinks
        .iter()
        .any(|link| link.continues_previous));
    app.handle_key(key(KeyCode::Char('o')));
    let selected = app.transcript_view.highlighted_link.unwrap_or_abort();
    assert_eq!(
        app.transcript_view.hyperlinks[selected].destination, "https://example.org/guide",
        "the printed URL is a distinct target after the whole wrapped label"
    );
    app.handle_key(key(KeyCode::Char('o')));
    let selected = app.transcript_view.highlighted_link.unwrap_or_abort();
    assert_eq!(
        app.transcript_view.hyperlinks[selected].destination,
        "https://example.org/api"
    );
    app.handle_key(key(KeyCode::Char('O')));
    app.handle_key(key(KeyCode::Char('O')));
    app.set_frame_area(Rect::new(0, 0, 40, 40));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.take_link_to_open().as_deref(),
        Some("https://example.org/guide")
    );
    assert!(
        app.transcript_viewer.is_none(),
        "Enter opens the highlighted link"
    );
    app.handle_key(key(KeyCode::Char('O')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.take_link_to_open().as_deref(),
        Some("https://example.org/api")
    );
    app.handle_key(key(KeyCode::Char('j')));
    assert!(app.transcript_view.highlighted_link.is_none());
    assert_eq!(app.current_session_id(), Some("child"));
    app.composer.vim_mode = false;
    app.set_frame_area(Rect::new(0, 0, 120, 40));
}

#[test]
fn command_task_viewer_subscribes_updates_and_discards_late_output_after_close() {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_frame_area(Rect::new(0, 0, 120, 40));
    for event in [
        run_started(1),
        agent_spawned(2, "parent", "default"),
        provider_started(3, "req_parent", "mock", "parent-model"),
        envelope(
            4,
            "req_parent",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "shell".into(),
                tool_id: "bash".into(),
                args_summary: r#"{"command":"build","description":"Build project"}"#.into(),
                args_digest: "digest".into(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            "req_parent",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "command".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("command".into()),
                metadata: Some(TaskScheduleMetadata {
                    lineage: Some(TaskLineageMetadata {
                        parent_tool_call_id: Some("shell".into()),
                        ..Default::default()
                    }),
                }),
            }),
        ),
    ] {
        app.ingest_event(event);
    }
    app.handle_key(key_with_modifiers(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.transcript_viewer.is_some());
    assert!(
        matches!(intents.lock().unwrap_or_abort().as_slice(), [UiIntent::InspectCommand {task_id: Some(id)}] if id == "command")
    );
    let mut snapshot = harness_core::coord::CommandSnapshot {
        result: GetCommandOrSubagentOutputResult {
            task_id: "command".into(),
            command: "build".into(),
            status: "running".into(),
            ..Default::default()
        },
        owner_agent_id: Some("parent".into()),
        original_owner_agent_id: Some("parent".into()),
        parent_tool_call_id: "shell".into(),
        parent_task_id: Some("req_parent".into()),
        description: None,
        cwd: "/fixture".into(),
        pid: None,
        started_mono_ms: 0,
        finished_mono_ms: None,
        stdout: "Building modules\n".into(),
        stderr: String::new(),
    };
    assert!(app.apply_command_output(snapshot.clone()));
    assert!(render_text(&app, 120, 40).contains("Building modules"));
    snapshot.result.task_id = "another-command".into();
    assert!(!app.apply_command_output(snapshot.clone()));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.transcript_viewer.is_none());
    assert!(matches!(
        intents.lock().unwrap_or_abort().last(),
        Some(UiIntent::InspectCommand { task_id: None })
    ));
    snapshot.result.task_id = "command".into();
    assert!(!app.apply_command_output(snapshot));
}

fn assert_native_completion(app: &mut AppState, mut finished: SubagentTransitionV1) {
    finished.transition = SubagentTransitionKind::Finished;
    finished.outcome = Some(SubagentTerminalOutcome::Completed);
    finished.accounting = Some(SubagentTerminalAccounting {
        tool_calls: 0,
        turns: 1,
        duration_ms: 1234,
        tokens_used: None,
        output_tokens_used: None,
        total_tokens_used: None,
        output_usage_incomplete: true,
    });
    for seq in [9, 10] {
        app.ingest_event(envelope(
            seq,
            "req_child",
            EventV1::SubagentTransition(Box::new(finished.clone())),
        ));
    }
    app.set_tool_group_outputs_expanded(&["spawn".into()], true);
    let root = render_text(&app, 120, 40);
    assert_eq!(
        root.matches("Subagent completed in 1.2s").count(),
        1,
        "{root}"
    );
    assert_eq!(root.matches("Subagent started:").count(), 1, "{root}");
    assert!(app.task_pane_rows().is_empty());
    app.tasks_pane.show_done = true;
    assert_eq!(app.task_pane_rows().len(), 2);
}

fn assert_child_search(app: &mut AppState) {
    app.composer.vim_mode = true;
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Backspace));
    assert!(app.transcript_view.search.editing);
    app.handle_paste("example\\.org");
    assert_eq!(app.transcript_view.search_match_count, 2);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.transcript_view.search_match, 1);
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.transcript_view.search.editing);
    app.handle_paste("ignored after acceptance");
    assert_eq!(app.transcript_view.search.editor.text(), "example\\.org");
    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(app.transcript_view.search_match, 0);
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.current_session_id(), Some("child"));
    assert!(!app.transcript_view.search.has_bar());
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_paste("[");
    assert_eq!(app.transcript_view.search_match_count, 0);
    assert!(render_text(app, 120, 40).contains("bad pattern"));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.transcript_view.search.editor.text(), "[");
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.current_session_id(), Some("child"));
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.transcript_view.search.has_bar());
    for (query, count) in [
        (
            "Read guide covering child lifecycles, permissions, navigation, and durable history",
            1,
        ),
        (r"\[guide", 0),
        ("^", 0),
    ] {
        app.handle_key(key(KeyCode::Char('/')));
        app.handle_paste(query);
        assert_eq!(app.transcript_view.search_match_count, count, "{query}");
        app.handle_key(key(KeyCode::Esc));
    }
    app.composer.vim_mode = false;
}
