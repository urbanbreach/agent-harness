use super::*;

pub(super) fn full_surface_scope_matrix_is_defined() {
    assert_eq!(
        FULL_SURFACE_SCOPE_MATRIX,
        [
            SurfaceScopeContract {
                surface: ParitySurface::StartupHome,
                hierarchy: ShellHierarchyContract::ComposeFirstHome,
                chrome: ChromeContract::FocusedStartupCard,
                composer: ComposerContract::StartupPrimaryCallToAction,
                sidebar: SidebarContract::NotApplicable,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::LiveEmpty,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::LiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::LiveRun,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::LiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::CompletedPostRun,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::DisabledLiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::ReplayShell,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::ReplayReadOnlyProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::OperatorSidebar,
                hierarchy: ShellHierarchyContract::OperatorSidebarSecondary,
                chrome: ChromeContract::SecondaryPane,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SecondaryOnly,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::ReviewSurfaces,
                hierarchy: ShellHierarchyContract::ReviewSecondary,
                chrome: ChromeContract::ReviewShell,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::NotApplicable,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::PermissionModal,
                hierarchy: ShellHierarchyContract::InterruptiveOverlay,
                chrome: ChromeContract::ElevatedModal,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::CommandPalette,
                hierarchy: ShellHierarchyContract::CommandOverlay,
                chrome: ChromeContract::ElevatedCommandOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::SlashOverlay,
                hierarchy: ShellHierarchyContract::CommandOverlay,
                chrome: ChromeContract::ElevatedCommandOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::RuntimeStateOverlay,
                hierarchy: ShellHierarchyContract::InterruptiveOverlay,
                chrome: ChromeContract::ElevatedRuntimeOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
        ]
    );
}

pub(super) fn live_shell_noise_budget_contract_is_defined() {
    assert_eq!(
        LIVE_SHELL_NOISE_BUDGET,
        LiveShellNoiseBudgetContract {
            dedicated_live_metadata_headline: LiveMetadataHeadlineContract::Prohibited,
            live_metadata_placement: LiveMetadataPlacementContract::StatusOrFooterOnly,
            hint_disclosure: HintDisclosureContract::ProgressiveBySpace,
            composer_rows: ComposerRowContract::NotPinnedToThreeRows,
            stable_shell_contexts: [
                ParitySurface::StartupHome,
                ParitySurface::LiveEmpty,
                ParitySurface::LiveRun,
                ParitySurface::CompletedPostRun,
                ParitySurface::ReplayShell,
                ParitySurface::PermissionModal,
                ParitySurface::CommandPalette,
                ParitySurface::SlashOverlay,
                ParitySurface::RuntimeStateOverlay,
            ],
        }
    );
}

pub(super) fn legacy_three_row_composer_contract_removed() {
    assert_eq!(
        LIVE_SHELL_NOISE_BUDGET.composer_rows,
        ComposerRowContract::NotPinnedToThreeRows
    );

    let quiet_shell = [
        "Assistant · model-1",
        "┃",
        "┃",
        "┃  default · local/-",
        "Success  ·  run finished · session shell preserved  0  Ctrl+p commands  ·  ? help  ·  q quit",
    ];

    assert_live_shell_composer_progressive_disclosure(&quiet_shell, None, "Ctrl+p commands");
    assert!(find_line_containing(&quiet_shell, "Composer").is_none());
}

pub(super) fn live_shell_composer_contract_matches_shell_parity() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 30, None, None, "Ctrl+p commands");

    let mut multiline = app::AppState::new_live(None, false, None);
    multiline.prompt_buffer = (1..=8)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    multiline.prompt_cursor = multiline.prompt_buffer.chars().count();

    let rendered = render_live_lines(&multiline, 100, 30);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (_, first_input_row, last_shell_row) = live_shell_composer_input_span(&lines);

    assert!(find_line_containing_in_range(&lines, 0, last_shell_row + 1, "Composer ·").is_none());
    assert_eq!(
        lines[first_input_row..=last_shell_row]
            .iter()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("▎  line ")
                    || trimmed.starts_with("┃  line ")
                    || trimmed.starts_with("╹  line ")
            })
            .count(),
        6,
        "multiline composer should stay capped\n{rendered}"
    );
    assert!(
        lines[first_input_row].contains("line 3"),
        "cursor-following composer should keep the latest visible window in view\n{rendered}"
    );
    assert!(rendered.contains("line 8"));
}

pub(super) fn live_shell_composer_progressive_disclosure_by_width() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 90, 36, None, None, "Ctrl+p commands");

    assert_live_shell_document_composer_contract(&ready, 80, 24, None, None, "Ctrl+p commands");

    assert_live_shell_document_composer_contract(&ready, 60, 18, None, None, "Ctrl+p commands");
}

pub(super) fn live_run_shell_places_under_input_controls_above_the_status_strip() {
    let mut app = app::AppState::new_live(None, false, None);
    let mut events = session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }

    assert_live_shell_document_composer_contract(&app, 100, 30, None, None, "Ctrl+p commands");
    assert_live_shell_document_composer_contract(&app, 80, 24, None, None, "Ctrl+p commands");

    let dense = render_live_lines(&app, 60, 18);
    assert!(!dense.contains("↑/↓ history"));
    assert!(!dense.contains("Enter send"));
}

pub(super) fn live_shell_composer_disabled_states_share_same_structure() {
    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 30, None, None, "Degraded");

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    assert_live_shell_document_composer_contract(
        &disconnected,
        100,
        30,
        None,
        None,
        "Disconnected",
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_document_composer_contract(&failure, 100, 30, None, None, "Failure");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_task_6"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 30, None, None, "Tab focus");
}

pub(super) fn compact_geometry_uses_overlay_sidebar_and_minimal_footer() {
    let mut compact = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        compact.ingest_event(event);
    }
    compact.live_details_drawer_open = true;
    let compact_plan =
        layout::FrameLayoutPlan::for_app(&compact, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert!(compact_plan.operator_sidebar.is_none());
    assert!(compact_plan.details_overlay.is_some());

    let compact_render = render_live_lines(&compact, 80, 24);
    assert!(!compact_render.contains("run run_fixture ·"));
    assert!(!compact_render.contains("e events"));

    let mut dense = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        dense.ingest_event(event);
    }
    dense.live_details_drawer_open = true;
    let dense_plan =
        layout::FrameLayoutPlan::for_app(&dense, ratatui::layout::Rect::new(0, 0, 60, 18));
    assert!(dense_plan.details_overlay.is_none());

    let dense_render = render_live_lines(&dense, 60, 18);
    assert!(!dense_render.contains("run run_fixture"));
    assert!(!dense_render.contains("i details"));
    assert!(!dense_render.contains("No MCP integrations configured"));
}

pub(super) fn focus_order_cycles_transcript_sidebar_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.active_tab = app::Tab::Run;
    app.live_details_drawer_open = true;

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::List);
    assert!(app.details_drawer_open());

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::Prompt);
    assert!(!app.details_drawer_open());

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.details_drawer_open());
}

pub(super) fn hovered_wheel_target_uses_sidebar_overlay_hit_areas() {
    let mut app = app::AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);
    let overlay = plan.details_overlay.expect("overlay sidebar area");
    let transcript = plan.transcript.expect("transcript area");

    let overlay_column = overlay.x.saturating_add(1);
    let overlay_row = overlay.y.saturating_add(1);
    let transcript_column = transcript.x.saturating_add(1);
    let transcript_row = transcript.y.saturating_add(1);

    assert_eq!(plan.wheel_hit_areas.overlay, Some(overlay));
    assert_eq!(
        ui::hovered_wheel_target(&app, area, overlay_column, overlay_row),
        Some(ui::WheelTarget::Inspector)
    );
    assert_eq!(
        ui::hovered_wheel_target(&app, area, transcript_column, transcript_row),
        Some(ui::WheelTarget::Transcript)
    );
}

pub(super) fn session_view_tracks_request_turn_and_tool_state() {
    let events = session_view_events();

    let mut live = app::AppState::new_live(None, false, None);
    for event in events.clone() {
        live.ingest_event(event);
    }
    assert_session_view_state(&live);

    let replay = app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);
    assert_session_view_state(&replay);
}

pub(super) fn session_view_ignores_duplicate_seq_without_losing_ui_state() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));
    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(app.active_permission().is_some());
    assert!(app.permission_submission_pending("perm_1"));

    app.focus = app::Focus::Prompt;
    app.prompt_buffer = "draft".to_string();
    app.prompt_cursor = "draft".chars().count();

    app.ingest_event(envelope(
        1,
        Some("req_duplicate"),
        harness_core::event::EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "duplicate-seq".to_string(),
            workspace_root: "/tmp".to_string(),
        }),
    ));

    assert_eq!(app.events.len(), 1);
    assert!(app.active_permission().is_some());
    assert!(app.permission_submission_pending("perm_1"));
    assert_eq!(app.prompt_buffer, "draft");
    assert_eq!(app.prompt_cursor, "draft".chars().count());
}
