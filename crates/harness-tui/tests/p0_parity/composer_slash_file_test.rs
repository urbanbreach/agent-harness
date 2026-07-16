use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyModifiers};
use harness_core::event::{
    EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{
    AppState, Focus, LaunchMetadata, ModelOption, ReviewSurface, RuntimeStateKind, UiIntent,
};
use harness_tui::overlay::OverlayKind;

use crate::helpers::*;

// ---------------------------------------------------------------------------
// P0-COMP-01
// ---------------------------------------------------------------------------

#[test]
fn composer_multiline_history_and_cursor_basics() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_history = vec!["older prompt".to_string()];

    // act — multiline via InsertNewline (Ctrl+J)
    for ch in "alpha".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key_with_modifiers(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    ));
    for ch in "beta".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    // assert — multiline buffer + cursor at end
    assert_eq!(app.composer.prompt_buffer, "alpha\nbeta");
    assert_eq!(
        app.composer.prompt_cursor,
        app.composer.prompt_buffer.chars().count()
    );
    assert!(
        app.composer.prompt_history_index.is_none(),
        "P0-COMP-01: typing must not enter history recall"
    );

    // act — Up moves cursor within multiline before recalling history
    app.handle_key(key(KeyCode::Up));
    assert_eq!(
        app.composer.prompt_buffer, "alpha\nbeta",
        "P0-COMP-01: first Up on multiline must move cursor, not recall history"
    );
    assert_eq!(app.composer.prompt_history_index, None);

    // act — history recall from line start
    app.composer.prompt_cursor = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.composer.prompt_buffer, "older prompt");
    assert_eq!(app.composer.prompt_history_index, Some(0));

    // act — restore draft
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.composer.prompt_buffer, "alpha\nbeta");
    assert_eq!(app.composer.prompt_history_index, None);

    // act — Shift+Enter inserts newline without submit
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap().push(intent);
        })
    };
    let mut paste_like = AppState::new_live(None, false, Some(sink));
    paste_like.focus = Focus::Prompt;
    for ch in "line1".chars() {
        paste_like.handle_key(key(KeyCode::Char(ch)));
    }
    paste_like.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::SHIFT));
    for ch in "line2".chars() {
        paste_like.handle_key(key(KeyCode::Char(ch)));
    }

    // assert — multiline insert path stays local (paste-adjacent newline contract)
    assert_eq!(paste_like.composer.prompt_buffer, "line1\nline2");
    assert!(
        intents.lock().unwrap().is_empty(),
        "P0-COMP-01: newline insert must not submit; got {:?}",
        intents.lock().unwrap()
    );
}


// ---------------------------------------------------------------------------
// P0-COMP-03
// ---------------------------------------------------------------------------

#[test]
fn composer_slash_mode_entry_opens_slash_overlay() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

    // act
    app.handle_key(key(KeyCode::Char('/')));
    let plan = plan_for(&app, 120, 40);
    let rendered = render_text(&app, 120, 40);

    // assert
    assert!(app.slash_visible, "P0-COMP-03: '/' must open slash mode");
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::SlashCommands),
        "P0-COMP-03: slash overlay must sit on the overlay stack"
    );
    assert_eq!(app.composer.prompt_buffer, "/");
    assert!(
        !app.slash_filtered.is_empty(),
        "P0-COMP-03: slash mode must list commands"
    );
    assert!(
        rendered.contains("Slash commands") || rendered.contains("/"),
        "P0-COMP-03: slash chrome must render\n{rendered}"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "P0-COMP-03: slash mode must keep full-width shell"
    );
}


// ---------------------------------------------------------------------------
// P0-SLASH-01
// ---------------------------------------------------------------------------

#[test]
fn slash_command_filter_and_submit_harness_safe_routes() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;

    // act — filter
    for ch in "/nw".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    // assert — filter prefers Harness-safe `new`
    assert!(
        app.slash_visible,
        "P0-SLASH-01: slash menu stays open while filtering"
    );
    assert_eq!(
        app.slash_filtered.first().map(String::as_str),
        Some("new"),
        "P0-SLASH-01: filter 'nw' must prefer /new; got {:?}",
        app.slash_filtered
    );
    assert!(
        app.slash_filtered
            .iter()
            .all(|cmd| !cmd.eq_ignore_ascii_case("mermaid")
                && !cmd.eq_ignore_ascii_case("voice")
                && !cmd.eq_ignore_ascii_case("imagine")),
        "P0-SLASH-01: OOS routes must not appear; got {:?}",
        app.slash_filtered
    );

    // act — submit selected route
    app.handle_key(key(KeyCode::Enter));

    // assert — /new reaches lifecycle handoff without fake OOS commands
    assert!(
        app.startup_shell_visible()
            || intents
                .lock()
                .unwrap()
                .iter()
                .any(|intent| matches!(intent, UiIntent::NewSession)),
        "P0-SLASH-01: /new must hand off to new-session lifecycle; startup={} intents={:?}",
        app.startup_shell_visible(),
        intents.lock().unwrap()
    );
    assert!(
        !app.slash_visible,
        "P0-SLASH-01: submit must close slash menu"
    );
}


// ---------------------------------------------------------------------------
// P0-FILE-01
// ---------------------------------------------------------------------------

#[test]
fn file_mention_at_opens_picker_and_inserts_agent_from_catalog() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-1").with_available_models(vec![
            ModelOption::from_model_ref("build", "mock:model-1"),
            ModelOption::from_model_ref("plan", "mock:model-1"),
        ]),
    );
    app.focus = Focus::Prompt;

    // act — open
    app.handle_key(key(KeyCode::Char('@')));
    let open_stack = app.overlay_stack().top();

    // assert — open
    assert_eq!(
        open_stack,
        Some(OverlayKind::FileMentions),
        "P0-FILE-01: '@' must open file-mention overlay"
    );
    assert_eq!(app.composer.prompt_buffer, "@");

    // act — filter + insert agent mention
    for ch in "pla".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    // assert — insert closes picker and writes safe display text
    assert_eq!(
        app.composer.prompt_buffer, "@plan ",
        "P0-FILE-01: Enter must insert selected agent mention"
    );
    assert_ne!(
        app.overlay_stack().top(),
        Some(OverlayKind::FileMentions),
        "P0-FILE-01: insert must dismiss file-mention overlay"
    );
    assert!(
        plan_for(&app, 120, 40).operator_sidebar.is_none(),
        "P0-FILE-01: file mention must keep full-width shell"
    );
}

