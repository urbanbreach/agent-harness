use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::AppState;
use harness_tui::dashboard_integration::DashboardPane;

#[test]
fn status_entry_reaches_interactive_dashboard_and_restores_focus() {
    // arrange
    // Given: the production live shell is open.
    let mut app = AppState::new_live(None, false, None);

    // When: the user enters the production `/status` command.
    app.execute_slash_command("status", None);

    // Then: the dashboard integration owns the active status surface.
    assert!(app.status_dashboard_is_active());
    assert_eq!(app.status_dashboard_focus(), Some(DashboardPane::Roster));

    // When: the user traverses dashboard focus and then closes the surface.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.status_dashboard_focus(), Some(DashboardPane::Peek));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // act
    // Then: normal shell focus is restored and the dashboard is closed.
    // assert
    assert!(!app.status_dashboard_is_active());
}
