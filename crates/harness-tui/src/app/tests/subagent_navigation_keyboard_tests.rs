use super::*;

pub(crate) fn transcript_task_inline_row_is_not_subagent_navigation() {
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

    let (column, row) = transcript_click_position(&app, "inspect child · Explore Agent");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        None
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
    assert_eq!(app.transcript_view.hovered_transcript_target(), None);
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

    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode, "in-chat task rows are passive");
}

pub(crate) fn keyboard_sidebar_subagent_selection_opens_child_session() {
    // arrange
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
        "tc_child_keyboard",
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
    app.focus = Focus::List;
    app.live_details_drawer_open = true;
    app.set_frame_area(TEST_FRAME_AREA);
    assert!(app.operator_sidebar_keyboard_active());
    assert!(!app.operator_sidebar_keyboard_targets().is_empty());
    assert_eq!(
        app.keymap.get_action(&key(KeyCode::Down)),
        Some(Action::HistoryDown)
    );

    // act
    app.handle_key(key(KeyCode::Down));
    assert_eq!(
        app.selected_operator_sidebar_keyboard_index_for_test(),
        Some(0)
    );
    app.handle_key(key(KeyCode::Down));
    assert_eq!(
        app.selected_operator_sidebar_keyboard_index_for_test(),
        Some(1)
    );

    app.handle_key(key(KeyCode::Enter));

    // assert
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(
        app.replay_mode,
        "keyboard opens inline child sessions read-only"
    );
}

pub(crate) fn transcript_task_inline_row_has_no_subagent_hitbox() {
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
        "tc_child_offset",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));

    let compact_area = Rect::new(0, 0, 80, 24);
    let (column, row) = transcript_click_position_in_area(&app, compact_area, "inspect child");
    assert_eq!(
        transcript_mouse_target(&app, compact_area, column, row),
        None
    );
    assert_eq!(
        transcript_mouse_target(
            &app,
            compact_area,
            FrameLayoutPlan::for_app(&app, compact_area)
                .transcript
                .expect("transcript area")
                .x
                .saturating_add(Theme::default().live_shell.rhythm.transcript_gutter_x)
                .saturating_sub(1),
            row
        ),
        None,
        "columns left of the rendered transcript pane must not activate the subagent hitbox"
    );
}

pub(crate) fn disk_backed_child_navigation_stays_in_live_tui_stack() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    let child_path = run_dir.path().join("agent_child");
    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "build"),
        provider_started(3, "req_1", "mock", "model-parent"),
        envelope(
            4,
            "req_1",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
                state: TaskScheduleState::Started,
                queue_key: None,
            }),
        ),
    ];
    write_events_jsonl(&parent_path, &parent_events);
    write_events_jsonl(
        &child_path,
        &[
            run_started(1),
            agent_spawned(2, "agent_child", "explore"),
            provider_started(3, "req_child", "mock", "model-child"),
        ],
    );
    assert!(!inspect_resume_plan(&parent_path).is_resumable);

    let mut app = AppState::new_live(Some(parent_path.clone()), false, Some(intent_sink));
    for event in parent_events {
        app.ingest_event(event);
    }
    app.navigate_to_child_session_id("agent_child".to_string());

    assert!(!app.should_quit);
    assert_eq!(app.session_path.as_deref(), Some(child_path.as_path()));
    assert!(app.replay_mode);
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_path && !snapshot.replay_mode));
    assert!(intents.lock().expect("lock intents").is_empty());

    app.navigate_to_parent_session();

    assert_eq!(app.session_path.as_deref(), Some(parent_path.as_path()));
    assert!(!app.replay_mode);
    assert!(!app.should_quit);
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.composer.prompt_buffer, "x");
    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
}
