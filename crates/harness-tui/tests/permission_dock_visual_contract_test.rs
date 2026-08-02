#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "deterministic rendering contracts use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    SCHEMA_VERSION,
};
use harness_tui::{app::AppState, render_test::render_to_buffer, ui, FrameLayoutPlan};
use ratatui::layout::Rect;

fn pending_permission_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-permission-dock-visual-contract".to_owned(),
        seq: 1,
        run_id: "run-permission-dock-visual-contract".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("visual-contract".to_owned())),
        correlation_id: Some("tool-call-permission-dock-visual-contract".to_owned()),
        causation_id: None,
        stream_key: Some("run:permission-dock-visual-contract".to_owned()),
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "permission-dock-visual-contract".to_owned(),
            kind: "edit_fs".to_owned(),
            tool_call_id: Some("tool-call-permission-dock-visual-contract".into()),
            summary: "Edit ui_permission_dock.rs".to_owned(),
            request_digest: "permission-dock-visual-contract".to_owned(),
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
fn permission_dock_paints_selected_option_row_and_preserves_unselected_surface() {
    // Given: a pending permission begins on its default Allow once choice.
    let app = pending_permission_app();
    let expected_selected_bg = app.theme().text.accent;
    let expected_unselected_bg = app.theme().surface.panel_elevated;

    // When: the live shell renders its inline permission dock.
    let area = Rect::new(0, 0, 120, 40);
    let buffer = render_to_buffer(&app, area, |app, frame, _area| ui::render_app(frame, app));
    let (selected_x, selected_y) = row_containing(&buffer, "1 (●)");
    let (unselected_x, unselected_y) = row_containing(&buffer, "2 (○)");
    let composer = FrameLayoutPlan::for_app(&app, area)
        .composer
        .expect("live shell must reserve composer geometry");
    let option_row_right = composer.right().saturating_sub(2);

    // Then: selection paints its entire content row, while other choices retain the dock surface.
    assert_eq!(buffer[(selected_x, selected_y)].bg, expected_selected_bg);
    assert_eq!(
        buffer[(option_row_right, selected_y)].bg,
        expected_selected_bg
    );
    assert_eq!(
        buffer[(unselected_x, unselected_y)].bg,
        expected_unselected_bg
    );
    assert_eq!(
        buffer[(option_row_right, unselected_y)].bg,
        expected_unselected_bg
    );

    let (hint_x, hint_y) = row_containing(&buffer, "1/4:select");
    assert_eq!(
        hint_x, selected_x,
        "permission hints should align with the option text inset"
    );
    assert_eq!(
        buffer[(hint_x, hint_y)].bg,
        expected_unselected_bg,
        "permission hints should remain on the dock surface"
    );
}
