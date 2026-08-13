use super::*;
use crate::transcript_scroll::PageFlipState;
use crossterm::event::{MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

fn live_app_with_clock() -> (AppState, Arc<Mutex<Instant>>) {
    let clock = Arc::new(Mutex::new(Instant::now()));
    let mut app = AppState::new_live(None, false, None);
    app.set_now_fn_for_test(Arc::new({
        let clock = Arc::clone(&clock);
        move || {
            *clock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }));
    (app, clock)
}

fn advance_clock(clock: &Mutex<Instant>, duration: Duration) {
    *clock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) += duration;
}

fn status_row<'a>(screen: &'a str, label: &str) -> &'a str {
    screen
        .lines()
        .rev()
        .find(|row| row.contains(label))
        .unwrap_or_else(|| panic!("expected active status row containing {label:?}"))
}

fn status_spinner_glyph(screen: &str, label: &str) -> Option<char> {
    let row = status_row(screen, label);
    let label_start = row.find(label)?;
    row[..label_start]
        .chars()
        .rev()
        .find(|glyph| !glyph.is_whitespace() && *glyph != '┃')
}

fn cancellable_live_app(intents: Arc<Mutex<Vec<UiIntent>>>) -> AppState {
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            intents
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(intent);
        })),
    );
    app.ingest_event(envelope(
        1,
        "req_stop",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_stop".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(provider_started(2, "req_stop", "default", "gpt-5.4-mini"));
    app
}

fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

pub(super) fn thinking_phase_clock_advances_between_provider_deltas() {
    // Given: a live provider request paused between reasoning deltas.
    let (mut app, clock) = live_app_with_clock();
    app.ingest_event(provider_started(
        1,
        "req_thinking_clock",
        "default",
        "gpt-5.4-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_thinking_clock",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_thinking_clock".into(),
            delta: "Planning the response".to_string(),
        }),
    ));

    // When: monotonic time advances without another provider event.
    advance_clock(&clock, Duration::from_millis(500));

    // Then: the visible thinking-phase clock continues advancing.
    let screen = render_text(&app, 140, 40);
    assert!(status_row(&screen, "Thinking…").contains("Thinking… 0.5s"));
}

pub(super) fn responding_phase_clock_advances_between_provider_deltas() {
    // Given: a live provider request paused between response deltas.
    let (mut app, clock) = live_app_with_clock();
    app.ingest_event(provider_started(
        1,
        "req_responding_clock",
        "default",
        "gpt-5.4-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_responding_clock",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_responding_clock".into(),
            delta: "Starting the answer".to_string(),
        }),
    ));

    // When: monotonic time advances without another provider event.
    advance_clock(&clock, Duration::from_millis(500));

    // Then: the visible responding-phase clock continues advancing.
    let screen = render_text(&app, 140, 40);
    assert!(status_row(&screen, "Responding…").contains("Responding… 0.5s"));
}

pub(super) fn thinking_spinner_advances_on_animation_tick() {
    // Given: a live thinking indicator rendered between provider deltas.
    let (mut app, _clock) = live_app_with_clock();
    app.ingest_event(provider_started(
        1,
        "req_thinking_spinner",
        "default",
        "gpt-5.4-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_thinking_spinner",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_thinking_spinner".into(),
            delta: "Planning the response".to_string(),
        }),
    ));
    let before = render_text(&app, 140, 40);
    let before_glyph = status_spinner_glyph(&before, "Thinking…");

    // When: the fixed-rate animation scheduler advances one spinner frame.
    for _ in 0..4 {
        app.advance_animation_tick_for_evidence();
    }

    // Then: the visible spinner advances without requiring a provider event.
    let after = render_text(&app, 140, 40);
    let after_glyph = status_spinner_glyph(&after, "Thinking…");
    assert_ne!(before_glyph, after_glyph);
}

pub(super) fn unrelated_request_delta_does_not_reset_active_phase_clock() {
    // Given: request A has been thinking for half a second.
    let (mut app, clock) = live_app_with_clock();
    app.ingest_event(provider_started(1, "req_a", "default", "gpt-5.4-mini"));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_a".into(),
            delta: "Thinking for A".to_string(),
        }),
    ));
    advance_clock(&clock, Duration::from_millis(500));

    // When: an unrelated background request emits its first reasoning delta.
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_b".into(),
            delta: "Thinking for B".to_string(),
        }),
    ));

    // Then: request A retains its original phase clock.
    assert_eq!(app.live_turn_phase_elapsed_ms_for("req_a"), Some(500));
}

pub(super) fn local_fresh_turn_resets_total_clock_before_request_id_arrives() {
    // Given: a previous live turn has been running for half a second.
    let (mut app, clock) = live_app_with_clock();
    app.begin_live_turn_timing(Some("req_previous"));
    advance_clock(&clock, Duration::from_millis(500));

    // When: a new prompt starts locally before its request ID is assigned.
    app.begin_live_turn_timing(None);

    // Then: the new turn starts with a fresh total clock.
    assert_eq!(app.live_turn_elapsed_ms(), Some(0));
}

pub(super) fn replay_loading_does_not_arm_live_turn_clocks() {
    // Given: persisted events describe an incomplete thinking turn.
    let events = vec![
        provider_started(1, "req_replay", "default", "gpt-5.4-mini"),
        envelope(
            2,
            "req_replay",
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "req_replay".into(),
                delta: "Persisted reasoning".to_string(),
            }),
        ),
    ];

    // When: the events are loaded into replay mode.
    let app = AppState::new_replay(PathBuf::from("/tmp/replay"), events);

    // Then: replay projection does not start live monotonic clocks.
    assert_eq!(app.live_turn_elapsed_ms(), None);
    assert_eq!(app.live_turn_phase_elapsed_ms(), None);
}

pub(super) fn thinking_to_responding_keeps_shared_spinner_frame() {
    // Given: a thinking status on a fixed global animation frame.
    let (mut app, _clock) = live_app_with_clock();
    app.ingest_event(provider_started(
        1,
        "req_transition",
        "default",
        "gpt-5.4-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_transition",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_transition".into(),
            delta: "Thinking".to_string(),
        }),
    ));
    let thinking = render_text(&app, 140, 40);
    let thinking_glyph = status_spinner_glyph(&thinking, "Thinking…");

    // When: the same turn transitions to responding without an animation tick.
    app.ingest_event(envelope(
        3,
        "req_transition",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_transition".into(),
            delta: "Response".to_string(),
        }),
    ));

    // Then: the global spinner frame does not jump at the phase boundary.
    let responding = render_text(&app, 140, 40);
    let responding_glyph = status_spinner_glyph(&responding, "Responding…");
    assert_eq!(thinking_glyph, responding_glyph);
}

pub(super) fn stop_affordance_is_hidden_without_cancellable_task() {
    // Given: a streaming provider activity with no cancellable coordinator task.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(
        1,
        "req_no_stop",
        "default",
        "gpt-5.4-mini",
    ));

    // When: the live status row is rendered.
    let screen = render_text(&app, 140, 40);

    // Then: it does not advertise a decorative stop button.
    assert!(!status_row(&screen, "Waiting for response…").contains("[stop]"));
}

pub(super) fn clicking_stop_affordance_interrupts_active_task() {
    // Given: a live status row backed by a cancellable coordinator task.
    let intents = Arc::new(Mutex::new(Vec::new()));
    let mut app = cancellable_live_app(Arc::clone(&intents));
    let stop = crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA)
        .unwrap_or_else(|| panic!("expected live stop hit rectangle"));

    // When: the user clicks the visible stop affordance.
    let handled = app.handle_mouse(
        mouse_at(MouseEventKind::Down(MouseButton::Left), stop.x, stop.y),
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: the click emits the existing coordinator interrupt intent.
    assert!(handled);
    assert_eq!(
        *intents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec![UiIntent::InterruptSession {
            task_ids: vec!["task_stop".to_string()],
        }]
    );
    let screen = render_text(&app, 140, 40);
    assert!(screen.contains("Cancelling…"), "{screen}");
    let row = status_row(&screen, "Cancelling…");
    assert!(row.contains("0.0s"), "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
    assert!(crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA).is_some());
    assert!(app.has_active_animations_for_evidence());

    let before_glyph = status_spinner_glyph(&screen, "Cancelling…");
    for _ in 0..4 {
        app.advance_animation_tick_for_evidence();
    }
    let after = render_text(&app, 140, 40);
    let after_glyph = status_spinner_glyph(&after, "Cancelling…");
    assert_ne!(before_glyph, after_glyph);

    let stop = crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA)
        .unwrap_or_else(|| panic!("expected cancelling stop retry hit rectangle"));
    assert!(app.handle_mouse(
        mouse_at(MouseEventKind::Down(MouseButton::Left), stop.x, stop.y),
        TEST_FRAME_AREA,
        None,
        None,
        None,
    ));
    assert_eq!(
        intents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        2
    );
}

pub(super) fn hovering_stop_affordance_updates_live_status_state() {
    // Given: a live status row backed by a cancellable coordinator task.
    let mut app = cancellable_live_app(Arc::new(Mutex::new(Vec::new())));
    let stop = crate::ui::live_turn_stop_rect(&app, TEST_FRAME_AREA)
        .unwrap_or_else(|| panic!("expected live stop hit rectangle"));

    // When: the pointer moves over the stop affordance.
    let changed = app.handle_mouse(
        mouse_at(MouseEventKind::Moved, stop.x, stop.y),
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: the status renderer can apply its hover token.
    assert!(changed);
    assert!(app.live_turn_stop_hovered());
}

pub(super) fn queued_follow_up_does_not_take_clock_from_streaming_turn() {
    // Given: turn A is actively thinking with an advancing phase clock.
    let (mut app, clock) = live_app_with_clock();
    app.ingest_event(provider_started(1, "req_a", "default", "gpt-5.4-mini"));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_a".into(),
            delta: "Thinking for A".to_string(),
        }),
    ));
    advance_clock(&clock, Duration::from_millis(500));

    // When: durable submission for queued turn B arrives before A completes.
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".into(),
            text: "Queued follow-up".to_string(),
        }),
    ));

    // Then: the visible streaming turn A retains timing ownership.
    assert_eq!(app.live_turn_phase_elapsed_ms_for("req_a"), Some(500));
}

pub(super) fn live_historical_restore_rearms_streaming_turn_clocks() {
    // Given: a live app with persisted events for an incomplete thinking turn.
    let (mut app, clock) = live_app_with_clock();
    let events = vec![
        provider_started(1, "req_restored", "default", "gpt-5.4-mini"),
        envelope(
            2,
            "req_restored",
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "req_restored".into(),
                delta: "Restored reasoning".to_string(),
            }),
        ),
    ];

    // When: the live session snapshot is restored and time advances.
    app.replace_events(events);
    advance_clock(&clock, Duration::from_millis(500));

    // Then: projected elapsed baselines continue on monotonic clocks.
    assert_eq!(app.live_turn_elapsed_ms(), Some(501));
    assert_eq!(
        app.live_turn_phase_elapsed_ms_for("req_restored"),
        Some(500)
    );
}

pub(super) fn hidden_delegated_child_cannot_steal_rendered_parent_clock() {
    // Given: a visible parent and hidden delegated child are both streaming.
    let (mut app, clock) = live_app_with_clock();
    app.session_path = Some(PathBuf::from("/tmp/parent"));
    app.ingest_event(provider_started(1, "req_parent", "default", "gpt-5.4-mini"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_parent".into(),
            delta: "Parent reasoning".to_string(),
        }),
    ));
    app.ingest_event(child_agent_spawned(3, "child", "worker", "parent"));
    let mut child_started = provider_started(4, "req_agent_spawned", "default", "gpt-5.4-mini");
    child_started.actor = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    app.ingest_event(child_started);
    let mut child_reasoning = envelope(
        5,
        "req_agent_spawned",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_agent_spawned".into(),
            delta: "Child reasoning".to_string(),
        }),
    );
    child_reasoning.actor = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    app.ingest_event(child_reasoning);

    // When: time advances without another parent provider event.
    advance_clock(&clock, Duration::from_millis(500));

    // Then: the status renderer uses the same visible parent selected by timing.
    let screen = render_text(&app, 140, 40);
    let row = status_row(&screen, "Thinking…");
    assert!(row.contains("Thinking… 0.5s"), "status row: {row:?}");
}

pub(super) fn hidden_delegated_child_activation_does_not_steal_detached_page_flip() {
    let mut app = AppState::new_live(None, false, None);
    app.session_path = Some(PathBuf::from("/tmp/parent"));
    app.ingest_event(provider_started(1, "req_parent", "default", "gpt-5.4-mini"));
    app.ingest_event(child_agent_spawned(2, "child", "worker", "parent"));

    let mut child_message = envelope(
        3,
        "req_child",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".into(),
            text: "hidden child prompt".to_string(),
        }),
    );
    child_message.actor = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    app.ingest_event(child_message);
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_parent".into(),
            finish_reason: "stop".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
    let detached = PageFlipState::Detached {
        activity_first_seq: 1,
        scroll_top: 9,
    };
    app.set_transcript_page_flip_state(detached);

    let mut child_started = provider_started(5, "req_child", "default", "gpt-5.4-mini");
    child_started.actor = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    app.ingest_event(child_started);

    assert_eq!(app.transcript_page_flip_state(), detached);
}

pub(super) fn hidden_child_event_does_not_adopt_foreground_local_echo() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-test").with_mode_label("Test"),
    );
    for character in "foreground prompt".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.activities.iter().any(|activity| {
        activity.request_id.is_empty()
            && activity
                .user_message
                .as_ref()
                .is_some_and(|message| message.text == "foreground prompt")
    }));
    app.session_path = Some(PathBuf::from("/tmp/parent"));
    app.ingest_event(child_agent_spawned(1, "child", "worker", "parent"));

    let mut child_message = envelope(
        2,
        "req_child",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".into(),
            text: "hidden child prompt".to_string(),
        }),
    );
    child_message.actor = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    app.ingest_event(child_message);
    assert!(app.activities.iter().any(|activity| {
        activity.request_id.is_empty()
            && activity
                .user_message
                .as_ref()
                .is_some_and(|message| message.text == "foreground prompt")
    }));
    let _ = render_text(&app, 140, 40);

    let local_echo = app
        .activities
        .iter()
        .find(|activity| {
            activity
                .user_message
                .as_ref()
                .is_some_and(|message| message.text == "foreground prompt")
        })
        .expect("foreground local echo");
    assert!(local_echo.request_id.is_empty());
    assert_eq!(local_echo.status, ActivityStatus::Streaming);
    assert!(app.transcript_page_flip_state().is_preserving());

    app.ingest_event(envelope(
        3,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".into(),
            text: "foreground prompt".to_string(),
        }),
    ));

    let foreground = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_parent")
        .expect("adopted foreground prompt");
    assert_eq!(foreground.first_seq, 3);
    assert_eq!(foreground.status, ActivityStatus::Streaming);
    assert_eq!(
        app.transcript_page_flip_state().activity_first_seq(),
        Some(3)
    );
}
