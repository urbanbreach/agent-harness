#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "deterministic rendering contracts use fail-fast asserts"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    SCHEMA_VERSION,
};
use harness_tui::{app::AppState, render_test::render_to_buffer, ui, FrameLayoutPlan};
use ratatui::layout::Rect;

fn pending_question_app() -> AppState {
    pending_question_app_with_options(serde_json::json!([
        {"label": "Red", "description": "Choose red"},
        {"label": "Green", "description": "Choose green"},
        {"label": "Blue", "description": "Choose blue"}
    ]))
}

fn pending_question_app_with_options(options: serde_json::Value) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-question-dock-visual-contract".to_owned(),
        seq: 1,
        run_id: "run-question-dock-visual-contract".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("visual-contract".to_owned())),
        correlation_id: Some("tool-call-question-dock-visual-contract".to_owned()),
        causation_id: None,
        stream_key: Some("run:question-dock-visual-contract".to_owned()),
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "question-dock-visual-contract".to_owned(),
            kind: "question".to_owned(),
            tool_call_id: Some("tool-call-question-dock-visual-contract".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which color?",
                    "header": "Color",
                    "options": options
                }]
            })
            .to_string(),
            request_digest: "question-dock-visual-contract".to_owned(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    });
    app
}

fn row_containing(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(x) = row.find(needle) {
            return (u16::try_from(x).expect("terminal width fits in u16"), y);
        }
    }
    panic!("expected terminal row containing {needle:?}");
}

#[test]
fn overflowing_question_dock_reserves_the_right_gutter_for_a_scrollbar() {
    let options = (1..=24)
        .map(|index| {
            serde_json::json!({
                "label": format!("Option {index}"),
                "description": format!("Choose option {index}")
            })
        })
        .collect::<Vec<_>>();
    let mut app = pending_question_app_with_options(serde_json::Value::Array(options));
    for _ in 0..18 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let area = Rect::new(0, 0, 100, 20);
    let buffer = render_to_buffer(&app, area, |app, frame, _area| ui::render_app(frame, app));
    let composer = FrameLayoutPlan::for_app(&app, area)
        .composer
        .expect("live shell must reserve composer geometry");
    let scrollbar_x = composer.right().saturating_sub(1);
    let (_, selected_y) = row_containing(&buffer, "19 (○) Option 19");
    let scrollbar_cells = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer[(x, y)].symbol() == "█")
        .collect::<Vec<_>>();

    let &(thumb_x, thumb_y) = scrollbar_cells
        .first()
        .expect("overflowing question must render a scrollbar thumb");
    let (_, custom_y) = row_containing(&buffer, "z (○) Type your answer here");
    assert_eq!(thumb_x, scrollbar_x);
    assert_eq!(buffer[(thumb_x, thumb_y)].fg, app.theme().scrollbar.thumb);
    assert!(custom_y < composer.bottom());
    assert_eq!(
        buffer[(scrollbar_x.saturating_sub(1), selected_y)].symbol(),
        " "
    );
}

#[test]
fn overflowing_question_dock_height_tracks_the_source_viewport_cap() {
    let options = (1..=24)
        .map(|index| {
            serde_json::json!({
                "label": format!("Option {index}"),
                "description": format!("Choose option {index}")
            })
        })
        .collect::<Vec<_>>();
    let app = pending_question_app_with_options(serde_json::Value::Array(options));
    let compact = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 20))
        .composer
        .expect("compact question shell must reserve composer geometry");
    let roomy = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 40))
        .composer
        .expect("roomy question shell must reserve composer geometry");

    assert_eq!(compact.height, 14);
    assert_eq!(roomy.height, 19);
}

#[test]
fn wrapped_question_option_uses_visual_rows_for_follow_and_sticky_input() {
    let options = (1..=24)
        .map(|index| {
            serde_json::json!({
                "label": format!("Region {index}"),
                "description": format!(
                    "Choose deployment region {index} with a deliberately long description that wraps"
                )
            })
        })
        .collect::<Vec<_>>();
    let mut app = pending_question_app_with_options(serde_json::Value::Array(options));
    for _ in 0..18 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let area = Rect::new(0, 0, 60, 20);
    let buffer = render_to_buffer(&app, area, |app, frame, _area| ui::render_app(frame, app));
    let (_, selected_y) = row_containing(&buffer, "19 (○) Region 19");
    let (_, custom_y) = row_containing(&buffer, "z (○) Type your answer here");

    assert_eq!(
        buffer[(8, selected_y.saturating_add(1))].bg,
        app.theme().question_prompt.selected
    );
    assert!(selected_y.saturating_add(1) < custom_y);
}

#[test]
fn question_dock_paints_the_keyboard_cursor_as_a_full_selection_row() {
    // Given: a question dock receives keyboard focus on its second option.
    let mut app = pending_question_app();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let area = Rect::new(0, 0, 100, 30);
    let expected_selection_bg = app.theme().question_prompt.selected;
    let expected_selection_fg = app.theme().question_prompt.accent;
    let expected_unselected_bg = app.theme().question_prompt.surface;

    // When: the live question dock renders.
    let buffer = render_to_buffer(&app, area, |app, frame, _area| ui::render_app(frame, app));
    let (selected_x, selected_y) = row_containing(&buffer, "2 (○) Green");
    let (unselected_x, unselected_y) = row_containing(&buffer, "1 (○) Red");
    let composer = FrameLayoutPlan::for_app(&app, area)
        .composer
        .expect("live shell must reserve composer geometry");
    let option_row_right = composer.right().saturating_sub(3);

    // Then: both option text and trailing cells use the selected-row surface.
    assert_eq!(buffer[(selected_x, selected_y)].bg, expected_selection_bg);
    assert_eq!(buffer[(selected_x, selected_y)].fg, expected_selection_fg);
    assert_eq!(
        buffer[(option_row_right, selected_y)].bg,
        expected_selection_bg
    );
    assert_eq!(
        buffer[(unselected_x, unselected_y)].bg,
        expected_unselected_bg
    );
    assert_eq!(
        buffer[(option_row_right, unselected_y)].bg,
        expected_unselected_bg
    );
}
