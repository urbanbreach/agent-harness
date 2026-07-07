use super::*;
use crate::UnwrapOrAbort;

pub(super) fn live_shell_geometry_contract_is_rule_based() {
    let theme = Theme::default();
    let session_contract = |width, height| {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        layout::session_geometry_contract(area, theme.live_shell_layout(width, height))
    };

    assert_eq!(session_contract(95, 40), session_contract(96, 40));
    assert_eq!(session_contract(101, 30), session_contract(100, 30));
    assert_eq!(session_contract(81, 25), session_contract(80, 25));

    assert_eq!(
        session_contract(95, 40).sidebar_mode,
        layout::SessionSidebarMode::Overlay { width: 42 }
    );
    assert_eq!(
        session_contract(120, 30).sidebar_mode,
        layout::SessionSidebarMode::Overlay { width: 42 }
    );
}

pub(super) fn live_shell_threshold_edges_are_stable() {
    let theme = Theme::default();
    let session_contract = |width, height| {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        layout::session_geometry_contract(area, theme.live_shell_layout(width, height))
    };

    let expectations = [
        (
            89,
            40,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            90,
            35,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            90,
            36,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            99,
            29,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            99,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            100,
            29,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            100,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
    ];

    for (width, height, header_mode, footer_mode, sidebar_mode) in expectations {
        let contract = session_contract(width, height);
        assert_eq!(
            contract.header_mode, header_mode,
            "unexpected header mode for {width}x{height}"
        );
        assert_eq!(
            contract.footer_mode, footer_mode,
            "unexpected footer mode for {width}x{height}"
        );
        assert_eq!(
            contract.sidebar_mode, sidebar_mode,
            "unexpected sidebar mode for {width}x{height}"
        );
        assert_eq!(contract.palette_overlay_max_width, None);
        assert_eq!(contract.slash_overlay_max_width, None);
    }
}

pub(super) fn dense_minimum_shell_hides_sidebar_and_caps_overlays() {
    let theme = Theme::default();
    let area = ratatui::layout::Rect::new(0, 0, 60, 18);
    let contract = layout::session_geometry_contract(area, theme.live_shell_layout(60, 18));

    assert_eq!(contract.header_mode, layout::SessionHeaderMode::Hidden);
    assert_eq!(contract.footer_mode, layout::SessionFooterMode::Minimal);
    assert_eq!(contract.sidebar_mode, layout::SessionSidebarMode::Hidden);
    assert_eq!(contract.palette_overlay_max_width, Some(46));
    assert_eq!(contract.slash_overlay_max_width, None);

    let non_dense = layout::session_geometry_contract(
        ratatui::layout::Rect::new(0, 0, 61, 19),
        theme.live_shell_layout(61, 19),
    );
    assert_ne!(non_dense.sidebar_mode, layout::SessionSidebarMode::Hidden);
    assert_eq!(non_dense.palette_overlay_max_width, None);
    assert_eq!(non_dense.slash_overlay_max_width, None);

    let mut dense = app::AppState::new_live(None, false, None);
    dense.live_details_drawer_open = true;
    let dense_plan = layout::FrameLayoutPlan::for_app(&dense, area);
    assert!(dense_plan.operator_sidebar.is_none());
    assert!(dense_plan.details_overlay.is_none());

    let mut palette = app::AppState::new_live(None, false, None);
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_plan = layout::FrameLayoutPlan::for_app(&palette, area);
    assert_eq!(
        palette_plan.palette_overlay.map(|overlay| overlay.width),
        Some(58)
    );
}

pub(super) fn slash_overlay_matches_composer_text_input_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Char('/')));

    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);
    let composer = plan.dock.unwrap_or_abort().composer;
    let overlay = plan.slash_overlay.unwrap_or_abort();
    let content = layout::slash_command_overlay_content_area(overlay);
    let theme = Theme::default();
    let body_width = composer.width.saturating_sub(1);
    let input_padding = theme
        .live_shell
        .rhythm
        .composer_padding_x
        .min(body_width.saturating_sub(1));
    let input_x = composer.x.saturating_add(1).saturating_add(input_padding);
    let input_width = body_width.saturating_sub(input_padding.saturating_mul(2));

    assert_eq!(overlay.x, input_x);
    assert_eq!(overlay.width, input_width);
    assert_eq!(overlay.y.saturating_add(overlay.height), composer.y);
    assert_eq!(
        overlay.height,
        u16::try_from(app.slash_filtered.len()).unwrap_or(u16::MAX)
    );
    assert!(overlay.height <= 10);
    assert_eq!(content.x, overlay.x);
    assert_eq!(content.width, overlay.width);
    assert_eq!(content.y, overlay.y);
    assert_eq!(content.height, overlay.height);
}

fn assert_live_shell_headerless_contract(app: &app::AppState, width: u16, height: u16) {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    let plan = layout::FrameLayoutPlan::for_app(app, area);
    let rendered = render_live_lines(app, width, height);
    let transcript = plan.transcript.unwrap_or_abort();

    assert_eq!(
        plan.session_contract.header_mode,
        layout::SessionHeaderMode::Hidden
    );
    assert_eq!(
        plan.header.height, 0,
        "root header must stay hidden\n{rendered}"
    );
    assert!(
        plan.live_anchor.is_none(),
        "live anchor should stay removed\n{rendered}"
    );
    assert_eq!(transcript.y, plan.shell.y);
    if let Some(sidebar) = plan.operator_sidebar {
        assert_eq!(sidebar.y, plan.shell.y);
    }
}

pub(super) fn live_shell_hidden_header_modes_remove_in_shell_anchor() {
    let mut split_live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        split_live.ingest_event(event);
    }
    assert_live_shell_headerless_contract(&split_live, 96, 40);

    let mut primary_details = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        primary_details.ingest_event(event);
    }
    primary_details.live_details_drawer_open = true;
    assert_live_shell_headerless_contract(&primary_details, 100, 30);

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    for event in session_view_events() {
        completed.ingest_event(event);
    }
    completed.ingest_event(envelope(
        11,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));
    assert_live_shell_headerless_contract(&completed, 100, 30);

    let mut recovery = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    recovery.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "tool execution failed".to_string(),
        }),
    ));
    recovery.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_headerless_contract(&recovery, 100, 30);
}

pub(super) fn live_shell_minimum_modes_stay_headerless() {
    for (width, height) in [(80, 24), (60, 18)] {
        let mut app = app::AppState::new_live(None, false, None);
        for event in session_view_events() {
            app.ingest_event(event);
        }

        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let plan = layout::FrameLayoutPlan::for_app(&app, area);
        let rendered = render_live_lines(&app, width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(
            plan.session_contract.header_mode,
            layout::SessionHeaderMode::Hidden
        );
        assert_eq!(
            plan.header.height, 0,
            "minimum layouts should remove the root header\n{rendered}"
        );
        assert!(
            plan.live_anchor.is_none(),
            "minimum layouts must not add an in-shell anchor\n{rendered}"
        );
        assert_eq!(count_lines_containing(&lines, "run run_fixture"), 0);
    }
}

pub(super) fn live_shell_redesign_guardrails_preserve_primary_contract() {
    let scope_for = |surface| {
        FULL_SURFACE_SCOPE_MATRIX
            .iter()
            .copied()
            .find(|scope| scope.surface == surface)
            .unwrap_or_else(|| panic!("missing redesign guardrail scope for {surface:?}"))
    };

    for surface in [
        ParitySurface::LiveEmpty,
        ParitySurface::LiveRun,
        ParitySurface::CompletedPostRun,
        ParitySurface::ReplayShell,
    ] {
        let scope = scope_for(surface);
        assert_eq!(
            scope.hierarchy,
            ShellHierarchyContract::TranscriptFirstSession,
            "{surface:?} redesign guardrail must preserve transcript-first hierarchy"
        );
        assert!(
            !scope.default_tab_chrome,
            "{surface:?} redesign guardrail must preserve no default tab chrome"
        );
        assert!(
            !scope.debug_inspector_in_primary_path,
            "{surface:?} redesign guardrail must keep debug inspector out of the primary path"
        );
    }

    assert_eq!(
        scope_for(ParitySurface::ReplayShell).composer,
        ComposerContract::ReplayReadOnlyProgressiveDisclosure,
        "replay read-only redesign guardrail must stay explicit"
    );

    let theme = Theme::default();
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    for (label, width, height, expected_header, expected_footer, expected_sidebar) in [
        (
            "dense 60x18 breakpoint support",
            60,
            18,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Minimal,
            layout::SessionSidebarMode::Hidden,
        ),
        (
            "minimum 80x24 breakpoint support",
            80,
            24,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Reduced,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "split 96x40 breakpoint support",
            96,
            40,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "primary 100x30 breakpoint support",
            100,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "wide 160x30 breakpoint support",
            160,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Persistent { width: 42 },
        ),
    ] {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let contract =
            layout::session_geometry_contract(area, theme.live_shell_layout(width, height));
        let plan = layout::FrameLayoutPlan::for_app(&live, area);
        let rendered = render_live_lines(&live, width, height);

        assert_eq!(
            contract.header_mode, expected_header,
            "{label}: redesign guardrail must preserve breakpoint support without reintroducing header chrome"
        );
        assert_eq!(
            contract.footer_mode, expected_footer,
            "{label}: redesign guardrail must preserve footer breakpoint support"
        );
        assert_eq!(
            contract.sidebar_mode, expected_sidebar,
            "{label}: redesign guardrail must preserve transcript-first/operator-sidebar breakpoint support"
        );
        assert!(
            plan.transcript.is_some(),
            "{label}: transcript-first redesign guardrail must keep a transcript surface"
        );
        assert!(
            !rendered.contains("Tabs"),
            "{label}: redesign guardrail must preserve no default tab chrome\n{rendered}"
        );
        assert!(
            plan.details_overlay.is_none(),
            "{label}: redesign guardrail should not route the primary path through overlay chrome"
        );

        match expected_sidebar {
            layout::SessionSidebarMode::Persistent { .. } => {
                let transcript = plan.transcript.unwrap_or_abort();
                let sidebar = plan.operator_sidebar.unwrap_or_abort();
                assert!(
                    transcript.x < sidebar.x,
                    "{label}: transcript-first layout must keep transcript left of the operator sidebar"
                );
                assert!(
                    transcript.width > sidebar.width,
                    "{label}: transcript-first layout must keep transcript primary over the operator sidebar"
                );
            }
            layout::SessionSidebarMode::Overlay { .. } | layout::SessionSidebarMode::Hidden => {
                assert!(
                    plan.operator_sidebar.is_none(),
                    "{label}: compact redesign guardrail must avoid default persistent sidebar chrome"
                );
            }
        }
    }

    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.focus = app::Focus::Prompt;
    for ch in "blocked in replay".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    replay.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_plan =
        layout::FrameLayoutPlan::for_app(&replay, ratatui::layout::Rect::new(0, 0, 100, 30));
    let replay_render = render_live_lines(&replay, 100, 30);
    assert!(
        replay_plan.transcript.is_some(),
        "replay read-only redesign guardrail must preserve transcript-first shell structure"
    );
    assert!(
        replay_plan.operator_sidebar.is_some(),
        "replay read-only redesign guardrail must preserve the operator sidebar when primary geometry allows"
    );
    assert!(
        replay.composer.prompt_buffer.is_empty(),
        "replay read-only redesign guardrail must drop typed draft text after submit attempts"
    );
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    let replay_header_row = find_line_containing_all(&replay_lines, &["Replay", "read-only"])
        .unwrap_or_else(|| {
            panic!("replay read-only redesign guardrail must preserve replay identity\n{replay_render}")
        });
    let replay_disabled_row = find_line_containing_all_from(
        &replay_lines,
        replay_header_row + 1,
        &["▎", "Replay is read-only."],
    )
    .filter(|row| !replay_lines[*row].contains("run "))
    .unwrap_or_else(|| {
        panic!("replay read-only redesign guardrail must preserve a disabled composer row\n{replay_render}")
    });
    let replay_shortcuts_row = find_line_containing_from(&replay_lines, replay_disabled_row + 1, "shortcuts")
        .unwrap_or_else(|| {
            panic!("replay read-only redesign guardrail must preserve shortcut affordances\n{replay_render}")
        });

    assert!(
        replay_header_row < replay_disabled_row,
        "replay identity should stay above the disabled composer guidance\n{replay_render}"
    );
    assert!(
        replay_disabled_row < replay_shortcuts_row,
        "shortcut guidance should remain below the disabled composer guidance\n{replay_render}"
    );
    assert!(
        !replay_render.contains("blocked in replay"),
        "replay read-only redesign guardrail must not surface submitted draft text\n{replay_render}"
    );
    assert!(
        !replay_render.contains("Tabs"),
        "replay read-only redesign guardrail must preserve no default tab chrome\n{replay_render}"
    );
}
