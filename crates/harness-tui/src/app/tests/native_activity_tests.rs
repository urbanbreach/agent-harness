use super::*;
use harness_core::event::{LiveEventEnvelope, LiveEventV1, ProviderRetryFailure};
use std::time::{Duration, Instant};

fn child_app() -> (AppState, Arc<Mutex<Instant>>) {
    let mut app = AppState::new_live(Some("/fixture/parent".into()), false, None);
    let clock = Arc::new(Mutex::new(Instant::now()));
    let now = Arc::clone(&clock);
    app.set_now_fn_for_test(Arc::new(move || *now.lock().unwrap_or_abort()));
    app.set_frame_area(Rect::new(0, 0, 120, 40));
    let mut started = provider_started(5, "req_child", "mock", "child-model");
    started.actor.agent_id = Some("child".into());
    for event in [
        run_started(1),
        envelope(
            2,
            "req_parent",
            EventV1::NativeSubagentRegistered(Box::new(registration())),
        ),
        envelope(
            3,
            "req_child",
            EventV1::SubagentTransition(Box::new(transition(SubagentTransitionKind::Spawned))),
        ),
        envelope_with_actor(
            4,
            "req_child",
            EventActor::new(ActorKind::Worker, Some("child".into())),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".into(),
                text: "Inspect files".into(),
            }),
        ),
        started,
    ] {
        app.ingest_event(event);
    }
    (app, clock)
}

fn live(app: &mut AppState, payload: LiveEventV1) {
    live_at(app, payload, 6);
}

fn live_at(app: &mut AppState, payload: LiveEventV1, mono_ms: u64) {
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: "activity".into(),
        run_id: "run_app_tests".into(),
        mono_ms,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("child".into())),
        correlation_id: Some("req_child".into()),
        causation_id: None,
        stream_key: None,
        payload,
    })));
}

fn tool(app: &mut AppState, name: &str, args: serde_json::Value) {
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("child".into())),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "call".into(),
            tool_id: name.into(),
            args_summary: args.to_string(),
            args_digest: "digest".into(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("child".into())),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "call".into(),
        }),
    ));
}

fn open_child(app: &mut AppState) {
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current_session_id(), Some("child"));
}

#[test]
fn child_argument_activity_survives_navigation_and_expires_without_rewriting_parent() {
    for (name, label) in [
        (None, "Preparing tool call…"),
        (Some("write"), "Writing file…"),
        (Some("edit"), "Writing edit…"),
        (Some("bash"), "Writing command…"),
        (Some("spawn_subagent"), "Writing subagent prompt…"),
        (Some("use_tool"), "Preparing MCP tool…"),
        (Some("search_tool"), "Searching MCP tools…"),
        (Some("todowrite"), "Updating todo list…"),
        (Some("workflow"), "Writing workflow…"),
        (Some("send_feedback"), "Writing feedback draft…"),
        (Some("image_gen"), "Writing image prompt…"),
        (Some("image_to_video"), "Writing video prompt…"),
        (Some("ask_user_question"), "Preparing question…"),
        (
            Some("linear__list_issues"),
            "Preparing (Linear) List Issues…",
        ),
    ] {
        let (mut app, clock) = child_app();
        if name == Some("bash") {
            live(
                &mut app,
                LiveEventV1::ProviderTextDelta {
                    request_id: "req_child".into(),
                    delta: "Working.".into(),
                },
            );
        }
        live(
            &mut app,
            LiveEventV1::ProviderToolInputDelta {
                request_id: "req_child".into(),
                tool_call_id: "draft".into(),
                tool_name: name.map(str::to_owned),
                delta: "{".into(),
            },
        );
        // A subsequent nameless fragment must retain the name and ordinal.
        live(
            &mut app,
            LiveEventV1::ProviderToolInputDelta {
                request_id: "req_child".into(),
                tool_call_id: "draft".into(),
                tool_name: None,
                delta: " ".into(),
            },
        );
        assert!(
            render_text(&app, 160, 40).contains(label),
            "parent did not show {label}"
        );
        open_child(&mut app);
        let screen = render_text(&app, 160, 40);
        assert!(screen.contains(label), "{screen}");
        assert!(
            !screen.contains("◆ bash"),
            "draft arguments became a tool row"
        );
        *clock.lock().unwrap_or_abort() += Duration::from_secs(11);
        let expected = if name == Some("bash") {
            "Responding…"
        } else {
            "Waiting for response…"
        };
        assert!(render_text(&app, 160, 40).contains(expected));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            render_text(&app, 160, 40).contains(label),
            "staleness rewrote the parent's last reported activity"
        );
    }
    let (mut app, _) = child_app();
    for (id, name, mono_ms) in [("one", "bash", 6), ("two", "edit", 2006)] {
        live_at(
            &mut app,
            LiveEventV1::ProviderToolInputDelta {
                request_id: "req_child".into(),
                tool_call_id: id.into(),
                tool_name: Some(name.into()),
                delta: "{".into(),
            },
            mono_ms,
        );
    }
    open_child(&mut app);
    assert!(render_text(&app, 160, 40).contains("Writing edit (2)… 2.0s"));
    let narrow = render_text(&app, 70, 24);
    let status = narrow.lines().nth(17).unwrap_or_abort();
    assert!(status.contains("Writing edit (2)…"), "{narrow}");
    assert!(
        !status.contains("2.0s"),
        "phase timer appeared below its width threshold"
    );
    let wide = render_text(&app, 72, 24);
    assert!(
        wide.lines().nth(17).unwrap_or_abort().contains("2.0s"),
        "{wide}"
    );
}

#[test]
fn child_waits_tool_descriptions_and_retries_use_native_activity_presentation() {
    for (name, args, parent, child) in [
        (
            "read",
            serde_json::json!({"description":" \n  Inspect  files \nignore"}),
            "Inspect  files…".to_string(),
            "Inspect  files…".to_string(),
        ),
        (
            "read",
            serde_json::json!({"description": "e\u{301}".repeat(21)}),
            format!("{}…", "e\u{301}".repeat(20)),
            format!("{}…", "e\u{301}".repeat(20)),
        ),
        (
            "read",
            serde_json::json!({"description": format!("{}👩‍💻", "a".repeat(39))}),
            format!("{}👩 …", "a".repeat(39)),
            format!("{}👩 …", "a".repeat(39)),
        ),
        (
            "get_task_output",
            serde_json::json!({"task_id":"unknown","timeout_ms":100}),
            "Waiting on task output…".into(),
            "waiting · send a message to interrupt".into(),
        ),
        (
            "get_task_output",
            serde_json::json!({"task_id":"unknown","timeout_ms":0}),
            "Waiting for response…".into(),
            "Waiting for response…".into(),
        ),
        (
            "spawn_subagent",
            serde_json::json!({"description":"Inspect files", "prompt":"Inspect files", "background":false}),
            "Waiting for subagent…".into(),
            "Waiting for subagent…".into(),
        ),
        (
            "wait_tasks",
            serde_json::json!({"task_ids":["unknown"]}),
            "Waiting on tasks…".into(),
            "waiting · send a message to interrupt".into(),
        ),
        (
            "Await",
            serde_json::json!({}),
            "Sleeping…".into(),
            "waiting · send a message to interrupt".into(),
        ),
    ] {
        let (mut app, _) = child_app();
        live(
            &mut app,
            LiveEventV1::ProviderTextDelta {
                request_id: "req_child".into(),
                delta: "Working.".into(),
            },
        );
        tool(&mut app, name, args);
        assert!(
            render_text(&app, 180, 40).contains(&parent),
            "missing {parent}"
        );
        open_child(&mut app);
        let screen = render_text(&app, 180, 40);
        assert!(screen.contains(&child), "{screen}");
        if child.starts_with("waiting ·") {
            assert!(!screen.contains("[stop]"));
        }
    }
    for (failure, headline) in [
        (ProviderRetryFailure::Connection, "Connection failed"),
        (
            ProviderRetryFailure::HttpStatus(503),
            "Service unavailable (503)",
        ),
        (ProviderRetryFailure::HttpStatus(429), "Rate limited (429)"),
        (
            ProviderRetryFailure::Authentication,
            "Authentication temporarily unavailable",
        ),
        (
            ProviderRetryFailure::Serialization,
            "Couldn't read the response",
        ),
        (
            ProviderRetryFailure::IdleTimeout,
            "No response from the model",
        ),
        (ProviderRetryFailure::EmptyResponse, "Empty response"),
        (ProviderRetryFailure::Truncated, "Response truncated"),
    ] {
        let (mut app, _) = child_app();
        let retry = harness_core::event::ProviderRequestRetryMetadata {
            attempt: 2,
            max_attempts: 5,
            delay_ms: None,
            category: None,
            failure: Some(failure),
        };
        let mut event = provider_started(8, "req_child", "mock", "child-model");
        if let EventV1::ProviderRequestStarted(data) = &mut event.payload {
            data.metadata = Some(harness_core::event::ProviderRequestStartedMetadata {
                retry: Some(retry),
                ..Default::default()
            });
        }
        live(
            &mut app,
            LiveEventV1::ProviderRetrying {
                turn_id: "req_child".into(),
                retry,
            },
        );
        open_child(&mut app);
        let before_request = render_text(&app, 180, 40);
        assert!(
            before_request.contains(&format!("{headline} | Retrying (attempt 2)...")),
            "{before_request}"
        );
        app.ingest_event(event);
        let screen = render_text(&app, 180, 40);
        assert!(
            screen.contains(&format!("{headline} | Retrying (attempt 2)...")),
            "{screen}"
        );
        live(
            &mut app,
            LiveEventV1::ProviderTextDelta {
                request_id: "req_child".into(),
                delta: "Recovered.".into(),
            },
        );
        let resumed = render_text(&app, 180, 40);
        assert!(resumed.contains("Responding…"), "{resumed}");
        assert!(!resumed.contains("Retrying (attempt"));
    }
    let (mut app, _) = child_app();
    open_child(&mut app);
    live(
        &mut app,
        LiveEventV1::CompactionProgress {
            agent_id: "child".into(),
            generation: 1,
            trigger_reason: "threshold".into(),
            preview: Some(String::new()),
        },
    );
    assert!(render_text(&app, 160, 40).contains("Compacting…"));
    live(
        &mut app,
        LiveEventV1::CompactionProgress {
            agent_id: "child".into(),
            generation: 1,
            trigger_reason: "threshold".into(),
            preview: None,
        },
    );
    assert!(render_text(&app, 160, 40).contains("Waiting for response…"));
}
