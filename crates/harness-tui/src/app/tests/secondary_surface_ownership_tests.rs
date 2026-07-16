use super::*;
use harness_core::event::{EventV1, UserMessageSubmittedEvent};
use std::path::PathBuf;

fn sample_user_message_event(seq: u64) -> EventEnvelopeV1 {
    let request_id = format!("req_secondary_{seq}");
    envelope(
        seq,
        request_id.as_str(),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.clone().into(),
            text: format!("secondary surface ownership prompt {seq}"),
        }),
    )
}

fn projection_fingerprint(app: &AppState) -> (usize, Vec<u64>, Vec<String>) {
    let event_seqs: Vec<u64> = app.events.iter().map(|event| event.seq).collect();
    let activity_ids: Vec<String> = app
        .activities
        .iter()
        .map(|activity| activity.request_id.clone())
        .collect();
    (app.events.len(), event_seqs, activity_ids)
}

pub(super) fn secondary_surface_toggle_does_not_mutate_session_projection() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(sample_user_message_event(1));
    app.ingest_event(sample_user_message_event(2));

    let before = projection_fingerprint(&app);
    assert_eq!(before.0, 2, "precondition: two projected events");
    assert_eq!(before.2.len(), 2, "precondition: two projected activities");

    app.secondary_surfaces.open_status_dialog();
    app.secondary_surfaces
        .set_selected_section(Some(OperatorSidebarSection::Todo));
    app.secondary_surfaces.set_focused(true);
    app.secondary_surfaces.toggle_section(OperatorSidebarSection::Mcp);
    app.secondary_surfaces.close_status_dialog();
    app.secondary_surfaces.set_focused(false);
    app.secondary_surfaces.set_selected_section(None);

    let after = projection_fingerprint(&app);
    assert_eq!(
        after, before,
        "secondary surface presentation toggles must not mutate SessionProjection"
    );
    assert!(
        !app.secondary_surfaces.status_dialog_visible(),
        "status dialog should close after explicit close"
    );
}

pub(super) fn replay_activities_unchanged_when_opening_closing_status_dialog() {
    let events = vec![sample_user_message_event(1), sample_user_message_event(2)];
    let mut app = AppState::new_replay(PathBuf::from("/tmp/t05-secondary-surface-replay"), events);

    let before = projection_fingerprint(&app);
    assert_eq!(before.0, 2);
    assert_eq!(before.2.len(), 2);

    app.secondary_surfaces.open_status_dialog();
    assert!(app.secondary_surfaces.status_dialog_visible());
    assert!(
        app.overlay_stack().top() == Some(OverlayKind::StatusDialog)
            || app.secondary_surfaces.status_dialog_visible(),
        "status dialog open must be owned by SecondarySurfaceState"
    );

    let mid = projection_fingerprint(&app);
    assert_eq!(
        mid, before,
        "opening status dialog must not change replay-derived projection"
    );

    app.secondary_surfaces.close_status_dialog();
    assert!(!app.secondary_surfaces.status_dialog_visible());

    let after = projection_fingerprint(&app);
    assert_eq!(
        after, before,
        "closing status dialog must not change replay-derived projection"
    );
}

pub(super) fn status_dialog_visibility_is_owned_by_secondary_surface_state() {
    let mut app = AppState::new_live(None, false, None);
    assert!(!app.secondary_surfaces.status_dialog_visible());
    assert!(!app.overlay_state().status_dialog_visible);

    app.secondary_surfaces.open_status_dialog();
    assert!(app.secondary_surfaces.status_dialog_visible());
    assert!(app.overlay_state().status_dialog_visible);

    app.secondary_surfaces.close_status_dialog();
    assert!(!app.secondary_surfaces.status_dialog_visible());
    assert!(!app.overlay_state().status_dialog_visible);
}
