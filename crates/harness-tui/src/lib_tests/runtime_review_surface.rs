use super::*;
use crate::UnwrapOrAbort;

pub(super) fn runtime_state_overlay_is_quiet_and_actionable() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));

    let overlay = runtime_overlay_text(&app, 72);
    let rendered = render_live_lines(&app, 80, 24);

    assert_eq!(overlay.badge, "Degraded");
    assert_eq!(overlay.title, "Recovery in progress");
    assert_eq!(
        overlay.summary,
        "Live updates are catching up before sending resumes."
    );
    assert_eq!(
        overlay.detail.as_deref(),
        Some("live stream lagged by 2; replaying from seq 1")
    );
    assert_eq!(overlay.guidance, "Draft locally until recovery completes.");
    assert!(rendered.contains("Recovery in progress"));
    assert!(rendered.contains("Draft locally until recovery completes."));
    assert!(!rendered.contains("Draft preserved locally while recovery completes."));
}

pub(super) fn runtime_state_overlay_never_stacks_over_permission_modal() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_precedence",
        "tool_call_overlay_precedence",
    ));

    let rendered = render_live_lines(&app, 80, 24);

    assert!(ui::runtime_overlay_text_for_test(&app, 72).is_none());
    assert!(rendered.contains("Allow Edit"));
    assert!(rendered.contains("always-approve"));
    assert!(!rendered.contains("Recovery in progress"));
}

pub(super) fn degraded_and_disconnected_states_share_overlay_structure() {
    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));

    let degraded_overlay = runtime_overlay_text(&degraded, 72);
    let disconnected_overlay = runtime_overlay_text(&disconnected, 72);

    assert!(degraded_overlay.detail.is_some());
    assert!(disconnected_overlay.detail.is_some());
    assert_eq!(
        usize::from(degraded_overlay.detail.is_some()),
        usize::from(disconnected_overlay.detail.is_some())
    );
    assert!(degraded_overlay.title.len() <= 24);
    assert!(disconnected_overlay.title.len() <= 24);
    assert!(degraded_overlay.guidance.ends_with('.'));
    assert!(disconnected_overlay.guidance.ends_with('.'));
}

pub(super) fn failure_overlay_is_specific_without_visual_noise() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "runtime error: exit code 1\nstderr permission denied".to_string(),
    ));

    // Freeze-aligned: Failure does not paint an elevated Review required card.
    assert!(ui::runtime_overlay_text_for_test(&app, 72).is_none());
    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains("Review required"));
    assert!(!rendered.contains("Turn attention required"));
    assert_eq!(app.runtime_state().kind, app::RuntimeStateKind::Failure);
}

pub(super) fn details_drawer_toggles_without_leaving_live_surface() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());

    app.live_details_drawer_open = true;

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(app.details_drawer_open());
    let open_debug = render_live_buffer(&app, 80, 24);
    assert!(open_debug.contains("▼ MCP"));

    app.live_details_drawer_open = false;

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());
    let closed_debug = render_live_buffer(&app, 80, 24);
    assert!(!closed_debug.contains("▼ MCP"));
}

pub(super) fn operator_sidebar_matches_parity_information_architecture() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = orchestration_details_drawer_app(2);
    let sidebar = operator_sidebar_text(&app);

    assert_markers_in_order(
        &sidebar,
        &["Explain the refactor", "▼ MCP", "▼ LSP", "▶ Modified Files"],
    );
    assert!(
        sidebar.contains("No MCP integrations configured")
            || sidebar.contains("No MCP servers configured")
            || sidebar.contains("websearch Disconnected")
    );
    assert!(sidebar.contains("No active LSP servers"));
    assert!(!sidebar.contains("No modified files"));
    assert!(!sidebar.contains("Todo ·"));
    assert!(!sidebar.contains("Recovery ·"));
}

pub(super) fn operator_sidebar_uses_secondary_quiet_chrome() {
    let app = orchestration_details_drawer_app(2);
    let rendered = render_live_lines(&app, 160, 48);
    let buffer = render_live_cells(&app, 160, 48);
    let theme = Theme::default();
    let title = "Explain the refactor";
    let (row, _fg, bg) = row_text_and_palette(&buffer, 160, title).unwrap_or_abort();
    let start = row.find(title).unwrap_or_abort();
    let start = row[..start].chars().count();
    let end = start + title.chars().count();

    assert!(!row[..row.find(title).unwrap_or_abort()].contains('│'));
    assert!(bg[start..end]
        .iter()
        .all(|color| *color == theme.surface.shell));
    assert!(!rendered.contains('┌'));
    assert!(!rendered.contains('┐'));
    assert!(!rendered.contains('└'));
    assert!(!rendered.contains('┘'));
}

pub(super) fn operator_sidebar_uses_explicit_empty_states() {
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = app::AppState::new_live(None, false, None);
    let sidebar = operator_sidebar_text(&app);

    assert!(sidebar.contains("▼ MCP"));
    assert!(
        sidebar.contains("No MCP integrations configured")
            || sidebar.contains("No MCP servers configured")
            || sidebar.contains("websearch Disconnected")
    );
    assert!(sidebar.contains("▼ LSP"));
    assert!(sidebar.contains("No active LSP servers"));
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("No modified files"));
}

pub(super) fn operator_sidebar_recovery_section_surfaces_artifacts_and_navigation_hints() {
    let sidebar = operator_sidebar_text(&operator_sidebar_child_navigation_replay_app());

    assert!(sidebar.contains("▼ MCP"));
    assert!(sidebar.contains("▼ LSP"));
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("Recovery"));
    assert!(!sidebar.contains("Parent session · parent_run"));
    assert!(!sidebar.contains("Child session · child_run"));
    assert!(!sidebar.contains("Artifact · artifacts/toolcalls/task/result.json"));
}

pub(super) fn operator_sidebar_modified_files_include_diff_artifact_paths() {
    let sidebar = operator_sidebar_text(&operator_sidebar_modified_files_live_app());

    assert_markers_in_order(&sidebar, &["▼ Modified Files", "src/ui_secondary.rs"]);
    assert!(!sidebar.contains("artifacts/edit-1.diff"));
    assert!(!sidebar.contains("Recovery"));
}

pub(super) fn operator_sidebar_preserves_section_order_and_copy() {
    let app = orchestration_details_drawer_app(2);
    let sidebar = operator_sidebar_text(&app);

    assert_markers_in_order(
        &sidebar,
        &["Explain the refactor", "▼ MCP", "▼ LSP", "▶ Modified Files"],
    );

    let empty = operator_sidebar_text(&app::AppState::new_live(None, false, None));
    assert!(empty.contains("▼ MCP"));
    assert!(empty.contains("▼ LSP"));
    assert!(empty.contains("▶ Modified Files"));
}

pub(super) fn live_shell_no_longer_renders_debug_inspector_labels() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.live_details_drawer_open = true;

    let rendered = render_live_lines(&app, 160, 48);
    assert!(!rendered.contains("Request ID"));
    assert!(!rendered.contains("Provider:"));
    assert!(!rendered.contains("Model:"));
    assert!(!rendered.contains("Prompt summary"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(!rendered.contains("Todo · 1"));
}

pub(super) fn review_surfaces_are_command_driven_without_tab_contract() {
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }
    live.focus = app::Focus::List;

    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "event log".chars() {
        live.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!live
        .palette_filtered
        .iter()
        .any(|c| c == "harness.open_event_log"));
    live.handle_key(key(crossterm::event::KeyCode::Esc));

    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(live.review_surface(), Some(app::ReviewSurface::Help));
    let live_help_debug = render_live_buffer(&live, 80, 24);
    assert!(live_help_debug.contains("Keyboard Shortcuts"));
    assert!(live_help_debug.contains("Essentials"));

    live.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(live.review_surface(), None);
    assert!(!live.details_drawer_open());

    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    replay.focus = app::Focus::List;
    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "event log".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!replay
        .palette_filtered
        .iter()
        .any(|c| c == "harness.open_event_log"));
    replay.handle_key(key(crossterm::event::KeyCode::Esc));

    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Help));
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_help_debug.contains("Keyboard Shortcuts"));
    assert!(replay_help_debug.contains("Essentials"));
    assert!(!replay_help_debug.contains("Allow Edit"));

    replay.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(replay.review_surface(), None);
}

pub(super) fn review_surfaces_restore_panel_chrome() {
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }
    live.focus = app::Focus::List;

    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let help_rendered = render_live_lines(&live, 100, 30);
    assert!(
        help_rendered.contains("Keyboard Shortcuts") || help_rendered.contains("Essentials"),
        "help surface restores freeze-aligned keyboard shortcuts chrome\n{help_rendered}"
    );
    assert!(
        help_rendered.contains("Enter details"),
        "help footer must render its final action completely at 100 columns\n{help_rendered}"
    );
    assert!(!help_rendered.contains("Allow Edit"));
}
