use harness_tui::UnwrapOrAbort;
use std::path::PathBuf;

use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{AppState, SessionHistoryEntry};

fn catalog_entry(run_id: &str, parent: Option<&str>, updated: &str) -> SessionCatalogEntry {
    SessionCatalogEntry {
        run_id: run_id.to_string().into(),
        run_name: Some(run_id.replace('-', " ")),
        status: Some(RunStatus::Finished),
        last_updated_at: Some(updated.to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: Some("build".to_string()),
        provider_model: Some("default/gpt-5.5".to_string()),
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: parent.map(str::to_string),
    }
}

fn history_entry(run_id: &str, parent: Option<&str>, updated: &str) -> SessionHistoryEntry {
    SessionHistoryEntry {
        run_dir: PathBuf::from("/runs").join(run_id),
        catalog: catalog_entry(run_id, parent, updated),
    }
}

#[test]
fn leader_g_opens_lineage_browser_on_live_sessions() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    // arrange
    let mut app = AppState::new_live_with_session_history(
        Some(PathBuf::from("/runs/root")),
        false,
        None,
        vec![
            history_entry("root", None, "2026-05-03T00:00:00Z"),
            history_entry("child_a", Some("root"), "2026-05-03T00:01:00Z"),
            history_entry("child_b", Some("root"), "2026-05-03T00:02:00Z"),
        ],
    );

    assert!(!app.lineage_browser_visible);

    // act
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert!(app.keymap.leader_pending());

    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));

    // assert
    assert!(app.lineage_browser_visible);

    let vm = app.lineage_browser_view_model();
    assert_eq!(vm.rows.len(), 3);
    assert!(vm.rows.iter().any(|row| row.run_id == "root"));
}

#[test]
fn lineage_child_dialog_shows_navigation_options_for_child_session() {
    // arrange
    let mut app = AppState::new_live_with_session_history(
        Some(PathBuf::from("/runs/child_a")),
        false,
        None,
        vec![
            history_entry("root", None, "2026-05-03T00:00:00Z"),
            history_entry("child_a", Some("root"), "2026-05-03T00:01:00Z"),
            history_entry("child_b", Some("root"), "2026-05-03T00:02:00Z"),
        ],
    );

    // act
    app.open_lineage_browser();

    // assert
    assert!(app.lineage_child_dialog_view_model().is_none());

    app.lineage_browser.move_selection(1);
    let dialog = app.lineage_child_dialog_view_model().unwrap_or_abort();
    assert_eq!(dialog.run_id, "child_b");
    assert_eq!(dialog.parent_run_id.as_deref(), Some("root"));
    assert_eq!(dialog.child_index, 1);
    assert_eq!(dialog.child_total, 2);
    assert!(!dialog.first_child_shortcut.is_empty());
    assert!(!dialog.previous_shortcut.is_empty());
    assert!(!dialog.next_shortcut.is_empty());
    assert!(!dialog.parent_shortcut.is_empty());

    app.lineage_browser.move_selection(1);
    let dialog = app.lineage_child_dialog_view_model().unwrap_or_abort();
    assert_eq!(dialog.run_id, "child_a");
    assert_eq!(dialog.child_index, 2);
    assert_eq!(dialog.child_total, 2);
}

#[test]
fn lineage_child_dialog_renders_navigation_options_in_overlay() {
    use ratatui::{backend::TestBackend, Terminal};

    // arrange
    let mut app = AppState::new_live_with_session_history(
        Some(PathBuf::from("/runs/child_a")),
        false,
        None,
        vec![
            history_entry("root", None, "2026-05-03T00:00:00Z"),
            history_entry("child_a", Some("root"), "2026-05-03T00:01:00Z"),
            history_entry("child_b", Some("root"), "2026-05-03T00:02:00Z"),
        ],
    );

    // act
    app.open_lineage_browser();
    app.lineage_browser.move_selection(1);

    let dialog = app.lineage_child_dialog_view_model().unwrap_or_abort();
    assert_eq!(dialog.run_id, "child_b");

    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .unwrap_or_abort();

    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut rows: Vec<String> = Vec::with_capacity(area.height as usize);
    for row in 0..area.height {
        let mut line = String::new();
        for col in 0..area.width {
            let cell = &buffer[(col, row)];
            line.push_str(cell.symbol());
        }
        rows.push(line);
    }
    let rendered: String = rows.join("\n");

    // assert
    assert!(
        rendered.contains("Harness session tree"),
        "lineage browser overlay should render"
    );
    assert!(
        rendered.contains("First"),
        "child dialog should render First child navigation"
    );
    assert!(
        rendered.contains("Prev"),
        "child dialog should render Previous child navigation"
    );
    assert!(
        rendered.contains("Next"),
        "child dialog should render Next child navigation"
    );
    assert!(
        rendered.contains("Parent"),
        "child dialog should render Parent navigation"
    );
}
