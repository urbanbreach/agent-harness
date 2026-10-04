use super::*;
use harness_core::event::{LiveEventEnvelope, LiveEventV1, SubagentProgressEvent};

fn publish(app: &mut AppState, generation: u64, attempt: &str, count: u32) {
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: format!("progress-{generation}-{count}"),
        run_id: "run_app_tests".into(),
        mono_ms: 8,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("child".into())),
        correlation_id: Some(attempt.into()),
        causation_id: None,
        stream_key: None,
        payload: LiveEventV1::SubagentProgress(SubagentProgressEvent {
            child_id: "child".into(),
            attempt_id: attempt.into(),
            generation,
            duration_ms: 2500,
            turn_count: 1,
            tool_call_count: count,
            tokens_used: Some(9000),
            context_window_tokens: Some(10_000),
            context_usage_pct: Some(90),
            tools_used: vec!["read".into()],
            error_count: 0,
        }),
    })));
}

pub(super) fn while_child_open(app: &mut AppState) {
    assert_eq!(app.current_session_id(), Some("child"));
    let durable_count = app.events().count();
    publish(app, 1, "req_child", 7);
    publish(app, 0, "old_attempt", 99);
    publish(app, 1, "other_attempt", 99);
    assert_count(app, 7);
    assert_eq!(app.events().count(), durable_count, "progress is transient");
}

pub(super) fn assert_count(app: &AppState, count: usize) {
    assert_eq!(
        app.projection
            .native_subagent_task("spawn")
            .map(|task| task.child_tool_call_count),
        Some(count)
    );
}

pub(super) fn assert_native_completion(app: &mut AppState, mut finished: SubagentTransitionV1) {
    finished.transition = SubagentTransitionKind::Finished;
    finished.outcome = Some(SubagentTerminalOutcome::Completed);
    finished.accounting = Some(SubagentTerminalAccounting {
        tool_calls: 8,
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
    publish(app, 1, "req_child", 99);
    assert_count(app, 8);
    for id in [
        "spawn",
        "background-notification:child:1",
        "background-notification:child:2",
    ] {
        app.set_tool_group_outputs_expanded(&[id.into()], true);
    }
    let root = render_text(&app, 120, 40);
    assert_eq!(
        root.matches("Subagent completed in 1.2s").count(),
        1,
        "{root}"
    );
    assert_eq!(root.matches("Subagent started:").count(), 1, "{root}");
    app.ingest_event(envelope(
        11,
        "req_child",
        EventV1::BackgroundTaskNotification(harness_core::event::BackgroundTaskNotificationEvent {
            parent_session_id: "parent".into(),
            parent_agent_id: Some("parent".into()),
            child_session_id: "child".into(),
            child_request_id: "req_child".into(),
            task_id: "req_child".into(),
            description: "Inspect files".into(),
            status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
            summary: "Inspection finished".into(),
            terminal_event_id: "event-9".into(),
            terminal_task_id: "req_child".into(),
            delivered_turn_request_id: Some("wake-parent".into()),
        }),
    ));
    let after_delivery = render_text(app, 120, 40);
    assert_eq!(
        root, after_delivery,
        "completion delivery must not change the transcript"
    );
    assert!(app.task_pane_rows().is_empty());
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert_eq!(
        app.task_pane_rows().len(),
        2,
        "Ctrl+G must focus the hidden pane so h shows completed children"
    );
    app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.current_session_id(), Some("child"));
    let completed_child = render_text(app, 120, 40);
    assert!(
        completed_child.contains("Worked for 1.2s"),
        "{completed_child}"
    );
    assert!(
        !completed_child.contains("Ctrl+c:cancel"),
        "{completed_child}"
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    let mut resumed = finished.clone();
    resumed.generation = 2;
    resumed.attempt_id = Some("req_child_again".into());
    resumed.transition = SubagentTransitionKind::Spawned;
    resumed.outcome = None;
    resumed.accounting = None;
    app.ingest_event(envelope(
        12,
        "req_child_again",
        EventV1::SubagentTransition(Box::new(resumed.clone())),
    ));
    let root = render_text(app, 120, 40);
    assert_eq!(
        root.matches("Subagent completed in 1.2s").count(),
        1,
        "{root}"
    );
    resumed.transition = SubagentTransitionKind::Finished;
    resumed.outcome = finished.outcome;
    resumed.accounting = finished.accounting;
    app.ingest_event(envelope(
        13,
        "req_child_again",
        EventV1::SubagentTransition(Box::new(resumed)),
    ));
    let root = render_text(app, 120, 40);
    assert_eq!(
        root.matches("Subagent completed in 1.2s").count(),
        2,
        "{root}"
    );
    assert_eq!(root.matches("Subagent started:").count(), 2, "{root}");
    assert!(!root.contains("Running 1 subagent"), "{root}");
    assert!(
        !root.contains("Ran 3 subagents"),
        "start and finish rows count each child once\n{root}"
    );
    let events = app.events().cloned().collect();
    app.replace_events(events);
    let replayed = render_text(app, 120, 40);
    assert_eq!(
        replayed.matches("Subagent completed in 1.2s").count(),
        2,
        "{replayed}"
    );
}
