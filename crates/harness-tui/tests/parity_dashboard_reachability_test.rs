use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::dashboard_integration::DashboardPane;
use ratatui::{backend::TestBackend, layout::Rect, Terminal};
use serde_json::Value;

const VIEWPORTS: [(u16, u16); 7] = [
    (80, 24),
    (100, 30),
    (120, 40),
    (120, 50),
    (79, 24),
    (60, 20),
    (140, 40),
];

fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("dashboard-reachability-{seq}"),
        seq,
        run_id: "dashboard-reachability".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("dashboard-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:dashboard-reachability".to_string()),
        payload,
    }
}

fn started() -> EventEnvelopeV1 {
    event(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "dashboard reachability".to_string().into(),
            workspace_root: "/tmp/dashboard-reachability".to_string(),
        }),
    )
}

fn settled() -> EventEnvelopeV1 {
    event(
        2,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "dashboard settled".to_string(),
        }),
    )
}

fn render(app: &AppState, viewport: (u16, u16)) -> String {
    let backend = TestBackend::new(viewport.0, viewport.1);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, app))
        .expect("dashboard render");
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn status_entry_reaches_source_observed_dashboard_at_all_viewports() {
    // arrange
    // Given: the production status entry and the checked-in comparator journey contract.
    let dashboard_manifest: Value =
        serde_json::from_str(include_str!("dashboard_coverage_manifest.json"))
            .expect("dashboard manifest");
    assert_eq!(
        dashboard_manifest["viewports"].as_array().map(Vec::len),
        Some(7)
    );
    assert_eq!(
        dashboard_manifest["journeys"].as_array().map(Vec::len),
        Some(15)
    );
    assert_ne!(
        dashboard_manifest["reference_binary"],
        dashboard_manifest["candidate_binary"]
    );

    let group_d = include_str!("../src/leaf_actions/group_d_dashboard.rs");
    let app_source = include_str!("../src/app.rs");
    let status_dialog = include_str!("../src/ui_overlays/status_dialog.rs");
    assert!(group_d.contains("OpenDashboard"));
    assert!(app_source.contains("DashboardIntegration"));
    assert!(status_dialog.contains("render_interactive_dashboard"));

    for viewport in VIEWPORTS {
        // When: `/status` is opened before any source event arrives (rest).
        let mut app = AppState::new_live(None, false, None);
        app.execute_slash_command("status", None);
        app.open_status_dashboard_at(Rect::new(0, 0, viewport.0, viewport.1));
        let rest = render(&app, viewport);

        // Then: the live dashboard, not the static operator summary, is visible.
        assert!(app.status_dashboard_is_active());
        assert_eq!(app.status_dashboard_focus(), Some(DashboardPane::Roster));
        assert!(
            rest.contains("Peek / tail") || rest.contains("dashboard peek"),
            "rest frame lost dashboard panes: {rest}"
        );
        assert!(
            rest.contains("Reply") || rest.contains("reply composer"),
            "rest frame lost reply pane: {rest}"
        );

        // When: the source starts the run (working), then records completion (settled).
        app.replace_events(vec![started()]);
        app.open_status_dashboard_at(Rect::new(0, 0, viewport.0, viewport.1));
        let working = render(&app, viewport);
        assert!(
            working.contains("working"),
            "working frame lost source status: {working}"
        );

        app.replace_events(vec![started(), settled()]);
        app.open_status_dashboard_at(Rect::new(0, 0, viewport.0, viewport.1));
        let settled = render(&app, viewport);
        assert!(
            settled.contains("settled"),
            "settled frame lost source status: {settled}"
        );

        // act
        // When: the user traverses the live dashboard and exits normally.
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
        // assert
        assert_eq!(app.status_dashboard_focus(), Some(DashboardPane::Peek));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!app.status_dashboard_is_active());
    }
}
