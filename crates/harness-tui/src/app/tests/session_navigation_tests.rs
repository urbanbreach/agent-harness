use super::*;
use crate::UnwrapOrAbort;

#[path = "session_navigation_parent_child_tests.rs"]
mod session_navigation_parent_child_tests;
pub(super) use session_navigation_parent_child_tests::parent_transcript_hides_child_prompt_before_task_tool_finishes as parent_child_parent_transcript_hides_child_prompt_before_task_tool_finishes;

pub(super) fn replay_mode_tab_toggles_focus_but_blocks_draft_edits() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());

    assert_eq!(app.focus, Focus::Details);

    // Tab toggles Prompt/Details even in replay mode.
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Prompt);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Details);

    // Draft edits remain blocked in replay mode even with prompt focus.
    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.composer.prompt_buffer.is_empty());
}

pub(super) fn child_session_navigation_keybinds_follow_default_contract() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    let child_a_dir = run_dir.path().join("child_a");
    let child_b_dir = run_dir.path().join("child_b");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "build"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_link_requested(4, "req_parent", "tc_child_a", Some("child_a"), None),
        child_link_requested(5, "req_parent", "tc_child_b", Some("child_b"), None),
    ];
    let child_a_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
        child_link_requested(4, "req_child_a", "tc_parent_a", None, Some("child_a")),
    ];
    let child_b_events = vec![
        run_started(1),
        agent_spawned(2, "child_b", "worker-b"),
        provider_started(3, "req_child_b", "mock", "model-child-b"),
        child_link_requested(4, "req_child_b", "tc_parent_b", None, Some("child_b")),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_a_dir, &child_a_events);
    write_events_jsonl(&child_b_dir, &child_b_events);
    for child_dir in [&child_a_dir, &child_b_dir] {
        fs::write(
            child_dir.join("meta.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "run_id": child_dir.file_name().and_then(|name| name.to_str()).unwrap(),
                "run_name": "child",
                "workspace_root": run_dir.path().display().to_string(),
                "config_digest": "digest",
                "harness_version": "test",
                "harness_lineage": {
                    "relationship": "task_child_session",
                    "parent_run_id": "parent",
                    "parent_session_id": "parent",
                }
            }))
            .unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut parent_app =
        AppState::new_live(Some(parent_dir.clone()), false, Some(Arc::clone(&sink)));
    parent_app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events.clone() {
        parent_app.ingest_event(event);
    }
    parent_app.focus = Focus::Prompt;
    parent_app.handle_key(key(KeyCode::Right));
    assert_eq!(
        parent_app.session_path.as_deref(),
        Some(parent_dir.as_path())
    );
    parent_app.focus = Focus::Details;
    parent_app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    parent_app.handle_key(key(KeyCode::Down));
    assert_eq!(
        parent_app.session_path.as_deref(),
        Some(child_a_dir.as_path())
    );
    assert!(parent_app.replay_mode);
    parent_app.focus = Focus::Details;
    parent_app.handle_key(key(KeyCode::Up));
    assert_eq!(
        parent_app.session_path.as_deref(),
        Some(parent_dir.as_path())
    );
    assert!(!parent_app.replay_mode);

    let mut child_app =
        AppState::new_live(Some(child_a_dir.clone()), false, Some(Arc::clone(&sink)));
    child_app.apply_keybindings(default_navigation_keybindings());
    for event in child_a_events {
        child_app.ingest_event(event);
    }
    child_app.focus = Focus::Prompt;
    child_app.handle_key(key(KeyCode::Right));
    child_app.focus = Focus::Details;
    child_app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    child_app.handle_key(key(KeyCode::Down));
    child_app.handle_key(key(KeyCode::Up));
    assert!(child_app.composer.prompt_buffer.is_empty());

    let mut reverse_app = AppState::new_live(Some(child_b_dir.clone()), false, Some(sink));
    reverse_app.apply_keybindings(default_navigation_keybindings());
    for event in child_b_events {
        reverse_app.ingest_event(event);
    }
    reverse_app.focus = Focus::Prompt;
    reverse_app.handle_key(key(KeyCode::Left));
    reverse_app.focus = Focus::Details;
    reverse_app.handle_key(key(KeyCode::Left));
    assert!(reverse_app.composer.prompt_buffer.is_empty());

    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[
            UiIntent::ReplaySession {
                run_id: "parent".into(),
                run_dir: parent_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "child_a".into(),
                run_dir: child_a_dir,
            },
        ]
    );
}

pub(super) fn replay_child_navigation_does_not_emit_live_intents() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    let child_a_dir = run_dir.path().join("child_a");
    let child_b_dir = run_dir.path().join("child_b");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_link_requested(4, "req_parent", "tc_child_a", Some("child_a"), None),
        child_link_requested(5, "req_parent", "tc_child_b", Some("child_b"), None),
    ];
    let child_a_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
        child_link_requested(4, "req_child_a", "tc_parent_a", None, Some("parent")),
    ];
    let child_b_events = vec![
        run_started(1),
        agent_spawned(2, "child_b", "worker-b"),
        provider_started(3, "req_child_b", "mock", "model-child-b"),
        child_link_requested(4, "req_child_b", "tc_parent_b", None, Some("parent")),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_a_dir, &child_a_events);
    write_events_jsonl(&child_b_dir, &child_b_events);

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_replay(parent_dir.clone(), parent_events);
    app.on_ui_intent = Some(sink);
    app.apply_keybindings(default_navigation_keybindings());
    app.set_launch_metadata(LaunchMetadata::new(
        "planner",
        "mock",
        Some("model-parent".to_string()),
    ));
    app.focus = Focus::Details;

    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.session_path.as_deref(), Some(child_a_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-a");
    assert!(app.replay_mode);
    assert!(app.composer.prompt_buffer.is_empty());

    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.session_path.as_deref(), Some(child_b_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-b");

    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.session_path.as_deref(), Some(child_a_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-a");

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert_eq!(app.active_profile(), "planner");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn replay_handoff_parent_navigation_continues_resumable_parent_session() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    let child_dir = run_dir.path().join("child_a");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_1", "mock", "model-parent"),
        envelope(
            4,
            "req_1",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "ready to continue".to_string(),
            }),
        ),
    ];
    let child_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_dir, &child_events);
    fs::write(
        child_dir.join("meta.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "run_id": "child_a",
            "run_name": "child",
            "workspace_root": run_dir.path().display().to_string(),
            "config_digest": "digest",
            "harness_version": "test",
            "harness_lineage": {
                "relationship": "task_child_session",
                "parent_run_id": "parent",
                "parent_session_id": "parent",
            }
        }))
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_replay(child_dir.clone(), child_events);
    app.enable_replay_navigation_handoff(Arc::clone(&sink));
    app.apply_keybindings(default_navigation_keybindings());

    app.handle_key(key(KeyCode::Up));

    assert!(app.should_quit);
    assert_eq!(app.session_path.as_deref(), Some(child_dir.as_path()));
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ContinueSession {
            run_id: "parent".into(),
            run_dir: parent_dir,
        }]
    );
}

pub(super) fn task_child_navigation_opens_inline_subagent_view_without_child_run_dir() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).unwrap_or_abort();

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "agent_child", "req_child"),
        agent_spawned(5, "agent_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".into(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        envelope(
            8,
            "req_child",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_child".into(),
                delta: "child subagent transcript is visible only in child view".to_string(),
            }),
        ),
        envelope(
            9,
            "req_child",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_child".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-child-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            10,
            "req_parent",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_parent".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-parent-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ];

    let mut app = AppState::new_replay(parent_dir.clone(), parent_events);
    app.apply_keybindings(default_navigation_keybindings());
    assert!(!render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Down));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("agent_child").as_path())
    );
    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    assert!(render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_dir && snapshot.replay_mode));

    app.handle_key(key(KeyCode::Up));

    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert!(app.replay_mode);
    assert!(!render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));
}

pub(super) fn parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).unwrap_or_abort();

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "z_child", "req_child"),
        agent_spawned(5, "z_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".into(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        child_task_requested(
            8,
            "req_child",
            "tc_grandchild",
            "a_grandchild",
            "req_grandchild",
        ),
        agent_spawned(9, "a_grandchild", "explore"),
    ];

    let mut app = AppState::new_replay(parent_dir, parent_events);
    app.apply_keybindings(default_navigation_keybindings());

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Down));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("z_child").as_path()),
        "parent right arrow should open the direct child, not a nested grandchild"
    );
    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.child_session_ids == vec!["z_child".to_string()]));
}

pub(super) fn live_inline_child_navigation_restores_live_parent_mode() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).unwrap_or_abort();

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "agent_child", "req_child"),
        agent_spawned(5, "agent_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".into(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        envelope(
            8,
            "req_child",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_child".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-child-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            "req_parent",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_parent".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-parent-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ];

    let mut app = AppState::new_live(Some(parent_dir.clone()), false, None);
    app.set_launch_metadata(LaunchMetadata::new(
        "build",
        "mock",
        Some("model-parent".to_string()),
    ));
    app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events {
        app.ingest_event(event);
    }

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Down));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("agent_child").as_path())
    );
    assert!(app.replay_mode);
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_dir && !snapshot.replay_mode));

    app.handle_key(key(KeyCode::Esc));

    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert!(!app.replay_mode);
    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.composer.prompt_buffer, "x");
}

pub(super) fn live_parent_events_update_parent_snapshot_while_inline_child_is_selected() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).unwrap_or_abort();

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "build"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "agent_child", "req_child"),
        agent_spawned(5, "agent_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".into(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        envelope(
            8,
            "req_child",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_child".into(),
                delta: "child-only transcript".to_string(),
            }),
        ),
    ];

    let mut app = AppState::new_live(Some(parent_dir.clone()), false, None);
    app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events {
        app.ingest_event(event);
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode);

    app.ingest_event(envelope(
        9,
        "req_parent",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_parent".into(),
            delta: "parent response after child opened".to_string(),
        }),
    ));

    assert!(render_debug(&app, 140, 40).contains("child-only transcript"));
    assert!(!render_debug(&app, 140, 40).contains("parent response after child opened"));

    app.ingest_event(envelope(
        10,
        "req_child",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_child".into(),
            delta: " and live child update".to_string(),
        }),
    ));

    let child_render = render_debug(&app, 140, 40);
    assert!(child_render.contains("child-only transcript and live child update"));
    assert!(!child_render.contains("parent response after child opened"));

    app.handle_key(key(KeyCode::Up));

    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert!(!app.replay_mode);
    let parent_render = render_debug(&app, 140, 40);
    assert!(parent_render.contains("parent response after child opened"));
    let parent_activity = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_parent")
        .expect("parent activity after returning from child view");
    assert_eq!(parent_activity.model_id, "model-parent");
}
