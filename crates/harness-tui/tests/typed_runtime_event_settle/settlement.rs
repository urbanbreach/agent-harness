use super::*;

#[test]
fn live_settlement_projects_once_without_replaying_each_durable_event() {
    // Given: the full provider-finish, assistant-finish, task-completion sequence.
    let mut app = AppState::new_live(None, false, None);
    app.set_reduced_motion_for_evidence(true);
    app.restart_motion_epoch_for_evidence();
    app.set_generic_tool_output_visible_for_test(true);
    let mut events = vec![
        durable_envelope(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-1".into(),
                text: "question".to_string(),
            }),
        ),
        durable_envelope(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-1".into(),
                provider_id: "mock".to_string(),
                model_id: "model".to_string(),
                prompt_summary: "question".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: None,
            }),
        ),
        durable_envelope(
            3,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider-1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("output-digest".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        durable_envelope(
            4,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider-1".into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text {
                    text: "answer".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            }),
        ),
        durable_envelope(
            5,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task-1".into(),
                result_summary: "answer".to_string(),
                result_digest: "result-digest".to_string(),
                metadata: None,
            }),
        ),
    ];

    // When: each event arrives independently, only the three finish events settle.
    for (index, event) in events.iter().enumerate() {
        app.ingest_runtime_event(RuntimeEvent::Durable(Box::new(event.clone())));
        assert_eq!(
            app.canonical_projection_generation(),
            event.seq.saturating_sub(2)
        );
        assert_eq!(app.canonical_projection_error(), None);
        if index >= 2 {
            let expected = CanonicalSessionProjection::from_event_history(&events[..=index])
                .expect("each finish boundary must project");
            assert_eq!(app.canonical_projection(), Some(&expected));
        } else {
            assert_eq!(app.canonical_projection(), None);
        }
    }

    exercise_retained_turns(&mut app, &mut events);
    app.scroll_goto_bottom();

    // Then: a failing settlement reports the fresh-projection error without advancing state.
    let before = app.canonical_projection().cloned();
    let generation = app.canonical_projection_generation();
    let mut invalid = events[4].clone();
    invalid.seq = events.len() as u64 + 1;
    let mut attempted = events.clone();
    attempted.push(invalid.clone());
    let error = CanonicalSessionProjection::from_event_history(&attempted)
        .expect_err("duplicate event identity must fail")
        .to_string();
    app.ingest_runtime_event(RuntimeEvent::Durable(Box::new(invalid)));
    assert_eq!(app.canonical_projection_error(), Some(error.as_str()));
    assert_eq!(app.canonical_projection_generation(), generation);
    assert_eq!(app.canonical_projection(), before.as_ref());

    for event in attempted.iter().rev() {
        assert_eq!(app.selected_event(), Some(event));
        app.previous_event();
    }

    // Invalid replacement still exposes the first event at each sequence for inspection.
    let mut replacement = events.clone();
    let mut duplicate = events[0].clone();
    duplicate.event_id = "duplicate-sequence".into();
    replacement.insert(1, duplicate);
    let error = CanonicalSessionProjection::from_event_history(&replacement)
        .expect_err("duplicate sequence must fail")
        .to_string();
    app.replace_events(replacement);
    assert_eq!(app.canonical_projection_error(), Some(error.as_str()));
    assert!(app.canonical_projection().is_none());
    for _ in 0..events.len() {
        app.previous_event();
    }
    for event in &events {
        assert_eq!(app.selected_event(), Some(event));
        app.next_event();
    }
}

fn exercise_retained_turns(app: &mut AppState, events: &mut Vec<EventEnvelopeV1>) {
    use serde_json::json;

    plain_turn(app, events, "turn-2");
    plain_turn(app, events, "turn-buffered");
    append(
        app,
        events,
        "turn-1",
        "tool_call_requested",
        json!({
            "tool_call_id":"old-tool", "tool_id":"fixture.echo", "args_summary":"older tool",
            "args_digest":"fixture", "metadata":null
        }),
    );
    append(
        app,
        events,
        "turn-1",
        "tool_call_started",
        json!({"tool_call_id":"old-tool"}),
    );
    append(
        app,
        events,
        "turn-1",
        "permission_requested",
        json!({
            "permission_id":"old-permission", "tool_call_id":"old-tool", "kind":"question",
            "summary":"Choose an option", "request_digest":"fixture", "timeout_ms":30000,
            "default_decision":"deny"
        }),
    );
    // New prompts can relocate a pending question transiently. Settlement must
    // restore canonical ownership and retain the older outstanding permission.
    plain_turn(app, events, "turn-3");
    assert_eq!(
        app.active_permission().map(|(id, _)| id),
        Some("old-permission".into())
    );
    append(
        app,
        events,
        "turn-1",
        "permission_resolved",
        json!({
            "permission_id":"old-permission", "decision":"allow", "reason":"answered"
        }),
    );
    assert!(app.active_permission().is_none());
    plain_turn(app, events, "turn-4");
    // These mutate nested canonical parts without advancing the parent message.
    append(
        app,
        events,
        "turn-1",
        "tool_call_finished",
        json!({
            "tool_call_id":"old-tool", "status":"succeeded", "output_summary":"older tool completed",
            "output_digest":"fixture", "output_json":null, "metadata":null
        }),
    );
    append(
        app,
        events,
        "turn-1",
        "artifact_written",
        json!({
            "tool_call_id":"old-tool", "path":"evidence/older-tool.txt", "digest":"fixture", "bytes":3
        }),
    );
    app.set_reduced_motion_for_evidence(true);
    app.expand_all_tool_outputs_for_test();
    app.set_frame_area(Rect::new(0, 0, 160, 80));
    app.scroll_goto_top();
    let text = render_to_string(app, Rect::new(0, 0, 160, 80), |app, frame, _| {
        ui::render_app(frame, app)
    });
    for expected in [
        "question",
        "answer turn-2",
        "answer turn-buffered",
        "answer turn-3",
        "answer turn-4",
        "older tool completed",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    for index in 5..=8 {
        plain_turn(app, events, &format!("turn-{index}"));
    }
    // Reopening the newest completed task makes room for the previously sixth
    // terminal row. A retained display prefix alone cannot recover that row.
    append(
        app,
        events,
        "turn-8",
        "task_scheduled",
        json!({
            "task_id":"turn-8", "queue_key":"provider_model:mock:model", "state":"started"
        }),
    );
    append(
        app,
        events,
        "turn-8",
        "provider_request_finished",
        json!({
            "request_id":"provider-turn-8", "finish_reason":"stop", "usage":null, "metadata":null
        }),
    );
    let tasks = app
        .orchestration_visible_rows()
        .into_iter()
        .map(|task| task.task_id)
        .collect::<Vec<_>>();
    assert_eq!(
        tasks,
        ["turn-8", "turn-7", "turn-6", "turn-5", "turn-4", "turn-3"]
    );
    assert_eq!(
        app.canonical_projection(),
        Some(
            &CanonicalSessionProjection::from_event_history(events)
                .expect("valid extended history")
        )
    );
}

fn plain_turn(app: &mut AppState, events: &mut Vec<EventEnvelopeV1>, turn: &str) {
    use serde_json::json;
    let provider = format!("provider-{turn}");
    for (kind, data) in [
        (
            "user_message_submitted",
            json!({"request_id":turn,"text":format!("question {turn}")}),
        ),
        (
            "task_scheduled",
            json!({"task_id":turn,"queue_key":"provider_model:mock:model","state":"started","metadata":null}),
        ),
        (
            "provider_request_started",
            json!({"request_id":provider,"provider_id":"mock","model_id":"model","prompt_summary":"question","request_digest":"fixture","metadata":null}),
        ),
        (
            "assistant_message_finished",
            json!({"request_id":provider,"tool_call_count":0,"parts":[{"kind":"text","text":format!("answer {turn}")}]}),
        ),
        (
            "provider_request_finished",
            json!({"request_id":provider,"finish_reason":"stop","usage":null,"metadata":null}),
        ),
        (
            "task_completed",
            json!({"task_id":turn,"result_summary":"completed","result_digest":"fixture","metadata":{"task_scope":"agent_turn"}}),
        ),
    ] {
        append(app, events, turn, kind, data);
    }
}

fn append(
    app: &mut AppState,
    events: &mut Vec<EventEnvelopeV1>,
    turn: &str,
    kind: &str,
    data: serde_json::Value,
) {
    let payload = serde_json::from_value(serde_json::json!({"event_type":kind,"data":data}))
        .expect("valid fixture");
    let mut event = durable_envelope(events.len() as u64 + 1, payload);
    event.correlation_id = Some(turn.into());
    let area = Rect::new(0, 0, 100, 48);
    app.set_frame_area(area);
    app.ingest_event(event.clone());
    app.set_frame_area(area);
    let capture = |app: &AppState| {
        (
            harness_tui::render_test::render_to_buffer(app, area, |app, frame, _| {
                ui::render_app(frame, app)
            }),
            app.transcript_interaction_snapshot(),
        )
    };
    let prepared = capture(app);
    // Reapplying the same display setting discards prepared layouts. Cached
    // painting and selection must match a freshly prepared frame at every event.
    app.set_generic_tool_output_visible_for_test(true);
    app.set_frame_area(area);
    assert_eq!(prepared, capture(app), "stale frame after {kind}");
    events.push(event);
}
