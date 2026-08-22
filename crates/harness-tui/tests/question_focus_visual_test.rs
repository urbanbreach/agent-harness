#![allow(
    clippy::panic,
    reason = "render contract tests use fail-fast assertions"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus};
use harness_tui::{ui, UnwrapOrAbort};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 28;

fn question_event(summary: serde_json::Value) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-question-focus".to_string(),
        seq: 1,
        run_id: "run-question-focus".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("question-focus-test".to_string())),
        correlation_id: Some("tool-question-focus".to_string()),
        causation_id: None,
        stream_key: Some("run:run-question-focus".to_string()),
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm-question-focus".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool-question-focus".into()),
            summary: summary.to_string(),
            request_digest: "digest-question-focus".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    }
}

fn question_app(focus: Focus, summary: serde_json::Value) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_event(summary));
    app.focus = focus;
    app
}

fn render(app: &AppState) -> Buffer {
    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer().clone()
}

fn rendered_text(app: &AppState) -> String {
    render(app)
        .content
        .chunks(usize::from(WIDTH))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell_colors(buffer: &Buffer, needle: &str, column_offset: usize) -> (Color, Color) {
    for y in buffer.area.y..buffer.area.bottom() {
        let row = (buffer.area.x..buffer.area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(start) = row.find(needle) {
            let column = row[..start].chars().count().saturating_add(column_offset);
            let x = buffer
                .area
                .x
                .saturating_add(u16::try_from(column).unwrap_or_abort());
            let cell = &buffer[(x, y)];
            return (cell.fg, cell.bg);
        }
    }
    panic!("rendered buffer does not contain {needle:?}");
}

fn single_question() -> serde_json::Value {
    serde_json::json!({
        "questions": [{
            "question": "Which color?",
            "header": "Color",
            "options": [{"label": "A", "description": "Option A"}],
            "multiple": false,
        }]
    })
}

#[test]
fn focused_question_keeps_active_cursor_fill() {
    // arrange
    // Given: a focused single-select question.
    let app = question_app(Focus::Prompt, single_question());
    let selected_surface = app.theme().question_prompt.selected;

    // When: the question dock is rendered.
    let buffer = render(&app);

    // act
    // Then: the keyboard cursor row keeps the active fill.
    let (_, background) = cell_colors(&buffer, "1 (○) A  Option A", 6);
    // assert
    assert_eq!(background, selected_surface);
}

#[test]
fn unfocused_question_removes_cursor_fill_and_dims_content() {
    // arrange
    // Given: a single-select question while scrollback owns focus.
    let app = question_app(Focus::Details, single_question());
    let question_surface = app.theme().question_prompt.surface;
    let primary = app.theme().question_prompt.primary;

    // When: the question dock is rendered.
    let buffer = render(&app);

    // act
    // Then: the row fill is gone and primary text is blended 66% toward the surface.
    let (foreground, background) = cell_colors(&buffer, "1 (○) A  Option A", 6);
    // assert
    assert_eq!(background, question_surface);
    assert_ne!(foreground, question_surface);
    assert_ne!(foreground, primary);
}

#[test]
fn unfocused_multi_select_custom_question_dims_without_hiding_markers() {
    // arrange
    // Given: an unfocused multi-select question with a freeform row.
    let summary = serde_json::json!({
        "questions": [{
            "question": "Choose colors",
            "header": "Colors",
            "options": [
                {"label": "Red", "description": "Warm"},
                {"label": "Blue", "description": "Cool"}
            ],
            "multiple": true,
            "custom": true,
        }]
    });
    let app = question_app(Focus::Details, summary);
    let question_surface = app.theme().question_prompt.surface;

    // When: the question dock is rendered.
    let buffer = render(&app);

    // act
    // Then: both the choice cursor and freeform marker remain legible on the base surface.
    let (choice_foreground, choice_background) = cell_colors(&buffer, "1 ([ ]) Red", 8);
    let (custom_foreground, custom_background) = cell_colors(&buffer, "z ([ ])", 0);
    // assert
    assert_eq!(choice_background, question_surface);
    assert_eq!(custom_background, question_surface);
    assert_ne!(choice_foreground, question_surface);
    assert_ne!(custom_foreground, question_surface);
}

#[test]
fn question_footer_labels_follow_focus_and_selection_state() {
    // arrange
    let mut app = question_app(Focus::Prompt, single_question());
    let focused = rendered_text(&app);
    assert!(focused.contains("Esc:scrollback"));
    assert!(focused.contains("Tab:next option"));

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(rendered_text(&app).contains("Esc:unselect"));

    // act
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    // assert
    assert!(rendered_text(&app).contains("Tab:focus"));
}
