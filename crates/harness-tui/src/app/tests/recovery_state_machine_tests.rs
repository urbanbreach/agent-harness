use super::*;

fn live_app_with_intents() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let captured_intents = Arc::clone(&intents);
    let app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            captured_intents.lock().unwrap_or_abort().push(intent);
        })),
    );
    (app, intents)
}

pub(super) fn disconnect_before_stream_preserves_draft_and_refuses_submission() {
    // arrange
    let (mut app, intents) = live_app_with_intents();
    app.composer.prompt_buffer = "preserved draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // act
    assert!(app.apply_runtime_event_stream_closed());
    app.handle_key(key(KeyCode::Enter));
    let runtime = app.runtime_state();

    // assert
    assert_eq!(runtime.kind, RuntimeStateKind::Disconnected);
    assert!(runtime.summary.contains("reopen the TUI"), "{runtime:?}");
    assert!(
        runtime.composer_hint.contains("Draft preserved"),
        "{runtime:?}"
    );
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn mid_stream_disconnect_preserves_transcript_and_refuses_submission() {
    // arrange
    let (mut app, intents) = live_app_with_intents();
    app.ingest_event(provider_started(
        1,
        "req_mid_stream_disconnect",
        "default",
        "gpt-5.4-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_mid_stream_disconnect",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_mid_stream_disconnect".into(),
            delta: "partial response".to_string(),
        }),
    ));
    app.composer.prompt_buffer = "follow-up draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // act
    assert!(app.apply_runtime_event_stream_closed());
    app.handle_key(key(KeyCode::Enter));
    let runtime = app.runtime_state();

    // assert
    assert_eq!(runtime.kind, RuntimeStateKind::Disconnected);
    assert!(
        runtime.summary.contains("transcript preserved"),
        "{runtime:?}"
    );
    assert!(app.composer_disabled());
    assert_eq!(
        app.activities.back().unwrap_or_abort().transcript_text,
        "partial response"
    );
    assert_eq!(app.composer.prompt_buffer, "follow-up draft");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn stop_affordance_is_disabled_while_disconnected() {
    // arrange
    let (mut app, _) = live_app_with_intents();
    app.ingest_event(envelope(
        1,
        "req_disconnected_stop",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_disconnected_stop".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(provider_started(
        2,
        "req_disconnected_stop",
        "default",
        "gpt-5.4-mini",
    ));
    assert!(crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA).is_some());

    // act
    assert!(app.apply_runtime_event_stream_closed());
    let screen = render_text(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);

    // assert
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Disconnected);
    assert!(!app.live_turn_stop_available());
    assert!(crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA).is_none());
    assert!(!screen.contains("[stop]"), "{screen}");
}

pub(super) fn failure_banner_maps_to_actionable_retry_copy() {
    // arrange
    let mut app = AppState::new_live(None, false, None);

    // act
    app.set_status_banner(Some("provider request failed: 503".to_string()));
    let runtime = app.runtime_state();

    // assert
    assert_eq!(runtime.kind, RuntimeStateKind::Failure);
    assert!(!runtime.summary.is_empty());
    assert!(runtime.summary.contains("retry"), "{runtime:?}");
    assert!(!runtime.composer_hint.is_empty());
    assert!(runtime.composer_hint.contains("retry"), "{runtime:?}");
}
