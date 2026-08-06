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
    app.secondary_surfaces
        .toggle_section(OperatorSidebarSection::Mcp);
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

pub(super) fn status_dashboard_opens_via_action_and_palette_dispatch() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    assert!(!app.secondary_surfaces.status_dialog_visible());

    // When
    app.execute_action(Action::OpenStatusDialog);

    // Then
    assert!(app.secondary_surfaces.status_dialog_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));
    assert!(app.overlay_state().status_dialog_visible);

    // When
    app.handle_key(key(KeyCode::Esc));

    // Then
    assert!(!app.secondary_surfaces.status_dialog_visible());
    assert_ne!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));
}

pub(super) fn status_dashboard_opens_via_dashboard_slash() {
    // Given
    let mut app = AppState::new_live(None, false, None);

    // When
    app.execute_slash_command("dashboard", None);

    // Then
    assert!(app.secondary_surfaces.status_dialog_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));
}

pub(super) fn status_dashboard_allows_normal_quit_sequence() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenStatusDialog);

    // When
    let quit_key = key_with_modifiers(KeyCode::Char('q'), KeyModifiers::CONTROL);
    app.handle_key(quit_key);
    app.handle_key(quit_key);

    // Then
    assert!(
        app.should_quit,
        "status dialog must not swallow Ctrl+Q quit confirmation"
    );
}

pub(super) fn status_dashboard_renders_empty_sections_from_app_state() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenStatusDialog);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));

    // When
    let rendered = render_text(&app, 100, 36);

    // Then
    assert!(
        rendered.contains("Status"),
        "dashboard header missing:\n{rendered}"
    );
    assert!(
        rendered.contains("No MCP Servers") || rendered.contains("MCP"),
        "expected MCP section (empty or listed):\n{rendered}"
    );
    assert!(
        rendered.contains("No Plugins") || rendered.contains("Plugins:"),
        "expected plugins section:\n{rendered}"
    );
    assert!(
        rendered.contains("Edit attribution: none yet") || rendered.contains("Edit attribution:"),
        "expected edit attribution section:\n{rendered}"
    );
    assert!(
        rendered.contains("Operator") && rendered.contains("operator dashboard:"),
        "expected operator dashboard line:\n{rendered}"
    );
    assert!(
        rendered.contains("Crash/recovery: none") || rendered.contains("Crash/recovery:"),
        "expected crash/recovery operator line:\n{rendered}"
    );
}

pub(super) fn status_dashboard_renders_populated_sections_from_app_state() {
    // Given
    let root = std::env::temp_dir().join(format!(
        "harness-tui-status-dashboard-product-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("workspace");

    let mut app = AppState::new_live(None, false, None);
    app.seed_operator_host_probes(Some(root.as_path()));
    app.set_plugin_lifecycle_summary(Some(harness_core::integrations::PluginLifecycleSummary {
        installed: 2,
        enabled: 1,
        disabled: 1,
    }));
    app.set_status_banner(Some(
        "previous crash detected — recovery action available".to_string(),
    ));
    app.set_auto_fallback_last_banner(Some("provider fallback: a → b".to_string()));

    // When
    app.execute_action(Action::OpenStatusDialog);
    let rendered = render_text(&app, 100, 40);

    // Then
    assert!(
        rendered.contains("Status"),
        "dashboard header missing:\n{rendered}"
    );
    assert!(
        rendered.contains("Plugins: 2 installed (1 enabled, 1 disabled)"),
        "expected populated plugins line:\n{rendered}"
    );
    assert!(
        rendered.contains("operator dashboard:")
            && rendered.contains("bound of")
            && rendered.contains("probes"),
        "expected operator dashboard bound counts:\n{rendered}"
    );
    assert!(
        rendered.contains("Crash/recovery: previous crash detected"),
        "expected crash/recovery from status banner:\n{rendered}"
    );
    assert!(
        rendered.contains("Fallback banner: provider fallback:")
            || rendered.contains("Fallback chain:"),
        "expected fallback projection lines:\n{rendered}"
    );

    // When
    app.handle_key(key(KeyCode::Esc));

    // Then
    assert!(!app.secondary_surfaces.status_dialog_visible());
    let closed = render_text(&app, 100, 40);
    assert!(
        !closed.contains("operator dashboard:"),
        "dashboard body must not remain after Esc dismiss:\n{closed}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
