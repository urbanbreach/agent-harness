use harness_tui::app::{AppState, Focus};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn draft_shell() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.startup_mode = false;
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "Review the narrow footer".to_string();
    app
}

#[test]
fn narrow_draft_footer_keeps_shortcuts_discoverable_without_partial_hints() {
    // arrange
    let app = draft_shell();

    // act
    let rendered = render_to_string(&app, Rect::new(0, 0, 60, 20), |app, frame, _area| {
        ui::render_app(frame, app);
    });

    // assert
    assert!(
        rendered.contains("Enter:send") && rendered.contains("Ctrl+x:shortcuts"),
        "narrow draft footer must keep complete send and shortcuts hints\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab"),
        "lower-priority mode hint should yield to pinned shortcuts help\n{rendered}"
    );
}
