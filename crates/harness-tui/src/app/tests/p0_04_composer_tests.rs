use super::*;
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    EventV1, ProviderRequestStartedEvent, ProviderStreamDeltaEvent, TaskScheduleState,
    TaskScheduledEvent,
};
use std::sync::{Arc, Mutex};

fn capturing_live_app() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let captured = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        captured.lock().unwrap_or_abort().push(intent);
    });
    (AppState::new_live(None, false, Some(sink)), intents)
}

fn active_turn(app: &mut AppState) {
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".into(),
            provider_id: "mock".into(),
            model_id: "p0-04-model".into(),
            prompt_summary: "active".into(),
            request_digest: "digest-p0-04-active".into(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_active",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_active".into(),
            delta: "streaming".into(),
        }),
    ));
}

fn intent_count(intents: &[UiIntent], predicate: impl Fn(&UiIntent) -> bool) -> usize {
    intents.iter().filter(|intent| predicate(intent)).count()
}

#[test]
fn multiline_enter_inserts_newline() {
    // Given: multiline mode with a draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: Enter is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Then: the newline is inserted without submitting.
    assert_eq!(app.composer.prompt_buffer, "alpha\n");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

#[test]
fn multiline_shift_enter_submits_once() {
    // Given: multiline mode with a valid draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    // Then: exactly one prompt submission is emitted.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
    assert_eq!(intents.len(), 1);
}

#[test]
fn multiline_alt_s_submits_once() {
    // Given: multiline mode with a valid draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: the terminal-safe explicit send chord is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));

    // Then: exactly one prompt submission is emitted.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
    assert_eq!(intents.len(), 1);
}

#[test]
fn active_turn_interject_submits_without_interrupting() {
    // Given: an active turn and a valid draft.
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("interject this");

    // When: Ctrl+Alt+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));

    // Then: one submission is emitted and no interrupt is emitted.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::InterruptSession { .. })
        }),
        0
    );
}

#[test]
fn active_turn_cancel_replace_interrupts_before_submitting() {
    // Given: an active turn and a valid draft.
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("replace this");

    // When: Ctrl+Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    // Then: the active task is interrupted before exactly one submission.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.len(), 2);
    assert!(matches!(
        intents.first(),
        Some(UiIntent::InterruptSession { .. })
    ));
    assert!(matches!(
        intents.get(1),
        Some(UiIntent::SubmitPrompt { .. })
    ));
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
}

#[test]
fn cancel_replace_empty_draft_emits_no_intents() {
    // Given: an active turn and an empty draft.
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);

    // When: Ctrl+Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    // Then: neither interrupt nor submission is emitted and the draft stays empty.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(app.composer.prompt_buffer.is_empty());
}

#[test]
fn cancel_replace_disconnected_configured_profile_preserves_draft_and_error() {
    // Given: a configured local profile with no connected provider and a draft.
    let (mut app, intents) = capturing_live_app();
    app.set_launch_metadata(LaunchMetadata::new("configured", "local", None));
    app.handle_paste("keep this draft");

    // When: Ctrl+Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    // Then: the draft remains, the disconnect error is visible, and no intent is emitted.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert!(app.status_banner.is_some());
}

#[test]
fn replay_multiline_actions_remain_read_only() {
    // Given: replay mode with a focused multiline draft.
    let (mut app, intents) = capturing_live_app();
    app.replay_mode = true;
    app.focus = Focus::Prompt;
    app.composer.multiline_mode = true;
    app.composer.prompt_buffer = "replay draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.len();

    // When: every P0-04 submission action is pressed.
    for key in [
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
    ] {
        app.handle_key(key);
    }

    // Then: replay emits no runtime intent and preserves its draft.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.composer.prompt_buffer, "replay draft");
}

#[test]
fn multiline_getter_badge_and_queue_state_are_visible() {
    // Given: multiline mode and an existing queued prompt.
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_ref(
        "p0-04-model",
        "mock:p0-04-model",
    ));
    active_turn(&mut app);
    app.composer.multiline_mode = true;
    app.handle_paste("draft");
    app.queued_prompt_count = 1;
    assert!(app.has_live_turn_activity());

    // When: the visible composer state is queried and rendered.
    let rendered = render_text(&app, 100, 30);

    // Then: the getter and existing queue state agree with the visible state.
    assert!(app.composer.composer_multiline_mode());
    assert_eq!(app.queued_prompt_count, 1);
    assert!(rendered.contains("queued 1"));
    assert!(rendered.contains("MULTILINE"));
    assert!(
        !rendered.contains("Enter:queue"),
        "multiline footer must not advertise Enter as queue\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:newline"),
        "multiline footer missing newline action\n{rendered}"
    );
    assert!(
        rendered.contains("Alt+s:send"),
        "multiline footer missing send action\n{rendered}"
    );
    assert!(rendered.contains("Alt+i:interject"));
    assert!(rendered.contains("Alt+r:replace"));
}
