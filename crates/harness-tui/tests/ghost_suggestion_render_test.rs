#![allow(
    clippy::expect_used,
    reason = "owner render tests use fail-fast assertions for deterministic fixtures"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, Focus};
use harness_tui::completion_controller::{CompletionRange, CompletionSource, CompletionTrigger};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn app_with_suggestion(draft: &str, full_prediction: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    for character in draft.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let request = app
        .composer_request_suggestion(draft)
        .expect("suggestion request");
    app.composer_advance_suggestion_clock(100);
    app.composer_apply_suggestion_response(&request, full_prediction)
        .expect("current suggestion response");
    app
}

fn render(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _| {
        ui::render_app(frame, app);
    })
}

#[test]
fn bordered_composer_renders_ghost_suggestion_inline_after_draft() {
    // arrange
    let app = app_with_suggestion("inspect ", "inspect the workspace");

    // act
    let rendered = render(&app, 80, 24);

    // assert
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("inspect the workspace")),
        "rendered shell:\n{rendered}"
    );
}

#[test]
fn bordered_composer_hides_ghost_away_from_global_draft_end() {
    // arrange
    let mut app = app_with_suggestion("first second", "first second ghost");
    app.composer.prompt_buffer = "first second!".to_string();
    app.composer.prompt_cursor = "first".chars().count();

    // act
    let rendered = render(&app, 80, 24);

    // assert
    assert!(!rendered.contains(" ghost"), "rendered shell:\n{rendered}");
}

#[test]
fn bordered_composer_ellipsizes_ghost_to_remaining_row_width() {
    // arrange
    let app = app_with_suggestion(
        "inspect ",
        "inspect the workspace and report every relevant detail at the tail",
    );

    // act
    let rendered = render(&app, 30, 12);

    // assert
    let composer_line = rendered
        .lines()
        .find(|line| line.contains("inspect"))
        .expect("composer line");
    assert!(
        composer_line.contains('…'),
        "composer line: {composer_line}\nrendered shell:\n{rendered}"
    );
}

#[test]
fn tab_accepts_visible_ghost_into_the_draft() {
    // arrange
    let mut app = app_with_suggestion("inspect ", "inspect the workspace");

    // act
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    // assert
    assert_eq!(app.composer.prompt_buffer, "inspect the workspace");
}

#[test]
fn right_arrow_accepts_visible_ghost_at_draft_end() {
    // arrange
    let mut app = app_with_suggestion("inspect ", "inspect the workspace");

    // act
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    // assert
    assert_eq!(app.composer.prompt_buffer, "inspect the workspace");
}

#[test]
fn composer_owned_surfaces_suppress_ghost_rendering_and_acceptance() {
    // arrange
    let mut shell = app_with_suggestion("inspect ", "inspect the workspace");
    shell.composer.shell_mode = true;
    let mut completion = app_with_suggestion("inspect ", "inspect the workspace");
    completion.composer_begin_completion(CompletionTrigger::new(
        CompletionRange::new(0, 0).expect("completion range"),
        "",
        CompletionSource::History,
    ));
    let mut overlay = app_with_suggestion("inspect ", "inspect the workspace");
    overlay.palette_visible = true;
    let mut unfocused = app_with_suggestion("inspect ", "inspect the workspace");
    unfocused.focus = Focus::Details;

    // act
    let rendered = [
        render(&shell, 80, 24),
        render(&completion, 80, 24),
        render(&overlay, 80, 24),
        render(&unfocused, 80, 24),
    ];
    for app in [&mut shell, &mut completion, &mut overlay, &mut unfocused] {
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    }

    // assert
    assert!(rendered
        .iter()
        .all(|frame| !frame.contains("the workspace")));
    assert_eq!(shell.composer.prompt_buffer, "inspect ");
    assert_eq!(completion.composer.prompt_buffer, "inspect ");
    assert_eq!(overlay.composer.prompt_buffer, "inspect ");
    assert_eq!(unfocused.composer.prompt_buffer, "inspect ");
}

#[test]
fn escape_dismisses_an_empty_prompt_prediction_until_reloaded() {
    // arrange
    let mut app = app_with_suggestion("", "review the workspace");
    app.focus = Focus::Prompt;
    assert!(render(&app, 80, 24).contains("review the workspace"));

    // act
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // assert
    assert!(!render(&app, 80, 24).contains("review the workspace"));
    assert!(app.composer.prompt_buffer.is_empty());
}

#[test]
fn debounced_history_source_produces_and_accepts_a_runtime_prediction() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.composer
        .prompt_history
        .push("inspect the workspace".to_owned());
    for character in "inspect ".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    // act
    assert!(!app.poll_local_ghost_suggestion(99));
    assert!(!render(&app, 80, 24).contains("the workspace"));
    assert!(app.poll_local_ghost_suggestion(1));

    // assert
    assert!(render(&app, 80, 24).contains("the workspace"));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.composer.prompt_buffer, "inspect the workspace");
}
