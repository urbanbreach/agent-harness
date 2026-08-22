use super::*;
use crate::UnwrapOrAbort;

pub(super) fn module_replay_mode_snapshot_renders_two_pane_layout() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_replay_fixture(sample_replay_events());
    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    assert_buffer_snapshot(
        "replay_mode_snapshot_renders_two_pane_layout",
        terminal.backend().buffer(),
    );
}

pub(super) fn replay_mode_r_key_reports_removed_reload() {
    let run_dir = write_replay_fixture(sample_replay_events());
    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();

    let mut app = AppState::new_replay(run_dir.path().to_path_buf(), events);
    app.handle_key(key(KeyCode::Char('r')));

    assert_eq!(
        app.status_banner.as_deref(),
        Some("event log reload has been removed")
    );
}

pub(super) fn live_mode_snapshot_renders_grouped_streams() {
    let mut app = AppState::new_live(None, false, None);
    for event in sample_live_events() {
        app.ingest_event(event);
    }
    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    assert_buffer_snapshot(
        "live_mode_snapshot_renders_grouped_streams",
        terminal.backend().buffer(),
    );
}

pub(super) fn live_mode_renders_activity_and_transcript() {
    let mut app = AppState::new_live(None, false, None);
    for event in sample_live_events() {
        app.ingest_event(event);
    }
    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("hello world"),
        "live mode must center the conversation surface"
    );
    assert!(
        !debug.contains("Activity ("),
        "live mode should not render the old activity cockpit by default"
    );
    assert!(
        debug.contains("hello world"),
        "transcript must show streaming content"
    );
}
