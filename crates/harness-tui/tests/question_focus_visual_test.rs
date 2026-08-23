#![allow(
    clippy::panic,
    reason = "render contract tests use fail-fast assertions"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{AppState, Focus, UiIntent};
use harness_tui::{ui, UnwrapOrAbort};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use std::sync::{Arc, Mutex};

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

fn multi_question() -> serde_json::Value {
    serde_json::json!({
        "questions": [
            {
                "question": "Choose modes",
                "header": "Modes",
                "options": [
                    {"label": "One", "description": "First mode"},
                    {"label": "Two", "description": "Second mode"},
                    {"label": "Three", "description": "Third mode"},
                    {"label": "Four", "description": "Fourth mode"},
                    {"label": "Five", "description": "Fifth mode"},
                    {"label": "Six", "description": "Sixth mode"},
                    {"label": "Seven", "description": "Seventh mode"},
                    {"label": "Eight", "description": "Eighth mode"},
                    {"label": "Nine", "description": "Ninth mode"},
                    {"label": "Ten", "description": "Tenth mode"}
                ],
                "multiple": true,
                "custom": true
            },
            {
                "question": "Choose output",
                "header": "Output",
                "options": [{"label": "Text", "description": "Plain text"}],
                "multiple": false,
                "custom": true
            }
        ]
    })
}

fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

fn key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
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
    let (choice_foreground, choice_background) = cell_colors(&buffer, "1 [ ] Red", 6);
    let (custom_foreground, custom_background) = cell_colors(&buffer, "z [ ]", 0);
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
    assert!(focused.contains("Enter:submit"));
    assert!(focused.contains("Tab:next answer"));
    assert!(focused.contains("X:dismiss"));

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
    assert!(rendered_text(&app).contains("Tab/Space:question"));
}

#[test]
fn multi_question_matches_grok_markers_counter_and_navigation_copy() {
    // Given: the first page of a multi-question prompt.
    let app = question_app(Focus::Prompt, multi_question());

    // When: the question dock is rendered.
    let rendered = rendered_text(&app);

    // Then: Grok's marker anatomy, sticky freeform row, and page footer are visible.
    assert!(rendered.contains("1 [ ] One"), "{rendered}");
    assert!(
        rendered.contains("z [ ] Type your answer here"),
        "{rendered}"
    );
    assert!(
        rendered.contains("[1/2] ↑/↓ navigate · ←/→ question · y copy"),
        "{rendered}"
    );
    assert!(!rendered.contains(" Confirm "), "{rendered}");
}

#[test]
fn unfocused_option_description_collapses_to_one_ellipsized_line() {
    // Given: a single question whose second option has a long description.
    let app = question_app(
        Focus::Prompt,
        serde_json::json!({
            "questions": [{
                "question": "Choose a mode",
                "header": "Mode",
                "options": [
                    {"label": "Focused", "description": "The focused description stays expanded."},
                    {
                        "label": "Other",
                        "description": "This unfocused description is intentionally long enough to exceed one terminal row and must collapse with an ellipsis instead of wrapping into the next option row."
                    }
                ],
                "multiple": false,
                "custom": false
            }]
        }),
    );

    // When: the question dock is rendered with the first option focused.
    let rendered = rendered_text(&app);

    // Then: the focused description remains complete and the other row is ellipsized.
    assert!(
        rendered.contains("The focused description stays expanded."),
        "{rendered}"
    );
    assert!(rendered.contains('…'), "{rendered}");
}

#[test]
fn alphabetic_shortcut_selects_tenth_option_and_ctrl_c_cancels() {
    // Given: a multi-question prompt and an intent sink.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(question_event(multi_question()));

    // When: `a` selects option ten, then Ctrl+C cancels the question.
    app.handle_key(key(crossterm::event::KeyCode::Char('a')));
    assert!(rendered_text(&app).contains("[2/2]"));
    app.handle_key(key(crossterm::event::KeyCode::Left));
    assert!(rendered_text(&app).contains("a [x] Ten"));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    // Then: the question resolves as cancelled without answer data.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        [UiIntent::ResolvePermission {
            permission_id: "perm-question-focus".to_string(),
            decision: PermissionDecision::Deny,
            reason: None,
            grant_scope: None,
        }]
    );
}
