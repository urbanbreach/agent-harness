use super::*;
use crate::UnwrapOrAbort;

pub(super) fn harness_chat_theme_is_default() {
    let default = Theme::default();
    let harness_chat = Theme::harness_chat();

    assert_eq!(default.surface, harness_chat.surface);
    assert_eq!(default.border, harness_chat.border);
    assert_eq!(default.text, harness_chat.text);
    assert_eq!(default.status, harness_chat.status);
}

pub(super) fn theme_tokens_cover_live_shell_states() {
    let default = Theme::default();
    let tokens = default.token_families();

    assert_eq!(default.live_shell.glyphs.streaming, "◐");
    assert_eq!(default.live_shell.glyphs.done, "●");
    assert_eq!(default.live_shell.glyphs.error, "✗");
    assert_eq!(default.live_shell.glyphs.pending_permission, "◷");
    assert_eq!(default.live_shell.glyphs.queued, "◴");
    assert_eq!(default.live_shell.glyphs.running, "◐");
    assert_eq!(default.live_shell.glyphs.succeeded, "●");
    assert_eq!(default.live_shell.glyphs.failed, "✗");
    assert_eq!(tokens.live_shell.glyphs.ascii.status.streaming, "o");
    assert_eq!(
        tokens.live_shell.glyphs.ascii.status.pending_permission,
        "?"
    );
    assert_eq!(tokens.live_shell.glyphs.ascii.status.failed, "x");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.user_marker, ">");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.tool_marker, "*");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.card_top, "  ");

    assert_eq!(default.live_shell.heights.header, 1);
    assert_eq!(default.live_shell.heights.tabs, 3);
    assert_eq!(default.live_shell.heights.status, 1);
    assert_eq!(default.live_shell.heights.footer, 1);
    assert_eq!(default.live_shell.heights.prompt_block(), 5);
    assert_eq!(default.live_shell.rhythm.transcript_gutter_x, 2);
    assert_eq!(default.live_shell.rhythm.status_separator, 2);
    assert_eq!(default.live_shell.minimum.centered_content_width, 80);
    assert_eq!(default.live_shell.minimum.content_margin_x, 0);
    assert_eq!(default.live_shell.primary.centered_content_width, 90);
    assert_eq!(default.live_shell.primary.content_margin_x, 0);
    assert_eq!(tokens.palette.surfaces, default.surface);
    assert_eq!(tokens.palette.borders, default.border);
    assert_eq!(
        tokens.live_shell.geometry.breakpoints,
        crate::theme::ShellBreakpoints::DEFAULT
    );
    assert_eq!(
        tokens.live_shell.geometry.minimum,
        default.live_shell.minimum
    );
    assert_eq!(
        tokens.live_shell.spacing.heights,
        default.live_shell.heights
    );
    assert_eq!(tokens.live_shell.spacing.rhythm, default.live_shell.rhythm);
    assert_eq!(tokens.live_shell.copy.startup, default.live_shell.startup);
    assert_eq!(
        tokens.live_shell.copy.empty_state,
        default.live_shell.empty_state
    );
    assert_eq!(
        default.live_shell.primary.target,
        ShellGeometryTarget::Primary
    );
    assert_eq!(
        default.live_shell.minimum.target,
        ShellGeometryTarget::Minimum
    );
}

pub(super) fn harness_dark_theme_has_exact_palette() {
    let theme = Theme::harness_dark();

    assert_eq!(
        theme.surface.canvas,
        ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
    );
    assert_eq!(
        theme.surface.shell,
        ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
    );
    assert_eq!(
        theme.surface.panel,
        ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
    );
    assert_eq!(
        theme.surface.panel_elevated,
        ratatui::style::Color::Rgb(0x12, 0x16, 0x1E)
    );
    assert_eq!(
        theme.surface.overlay,
        ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
    );
    assert_eq!(
        theme.border.subtle,
        ratatui::style::Color::Rgb(0x3A, 0x3D, 0x43)
    );
    assert_eq!(
        theme.border.strong,
        ratatui::style::Color::Rgb(0x48, 0x4B, 0x52)
    );
    assert_eq!(
        theme.border.focus,
        ratatui::style::Color::Rgb(0x60, 0x63, 0x6A)
    );
    assert_eq!(
        theme.text.primary,
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEC)
    );
    assert_eq!(
        theme.text.secondary,
        ratatui::style::Color::Rgb(0x88, 0x8B, 0x91)
    );
    assert_eq!(
        theme.text.tertiary,
        ratatui::style::Color::Rgb(0x88, 0x8B, 0x91)
    );
    assert_eq!(
        theme.text.accent,
        ratatui::style::Color::Rgb(0xD9, 0x84, 0xD9)
    );
    assert_eq!(
        theme.text.inverse,
        ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
    );
    assert_eq!(
        theme.status.success,
        ratatui::style::Color::Rgb(0x7F, 0xD8, 0x8F)
    );
    assert_eq!(
        theme.status.warning,
        ratatui::style::Color::Rgb(0xE5, 0xC0, 0x7B)
    );
    assert_eq!(
        theme.status.error,
        ratatui::style::Color::Rgb(0xE0, 0x6C, 0x75)
    );
    assert_eq!(
        theme.status.info,
        ratatui::style::Color::Rgb(0x56, 0xB6, 0xC2)
    );
    assert_eq!(
        theme.status.disabled,
        ratatui::style::Color::Rgb(0x80, 0x80, 0x80)
    );
    assert_eq!(
        theme.agents.build,
        ratatui::style::Color::Rgb(0x5C, 0x9C, 0xF5)
    );
    assert_eq!(
        theme.agents.plan,
        ratatui::style::Color::Rgb(0xD9, 0x84, 0xD9)
    );
}

pub(super) fn command_palette_state_filters_existing_commands() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('n')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "n");
    assert_eq!(app.palette_cursor, 1);
    assert!(!app.palette_filtered.is_empty());
    assert!(app.palette_filtered.iter().all(|command| {
        let id = command
            .strip_prefix("suggested:")
            .unwrap_or(command.as_str());
        crate::keybindings::palette_model::find(id).is_some()
    }));
}

pub(super) fn hovered_wheel_target_uses_layout_plan() {
    let area = ratatui::layout::Rect::new(0, 0, 140, 40);

    let mut default_app = app::AppState::new_live(None, false, None);
    default_app.live_details_drawer_open = true;
    default_app.ingest_event(operator_sidebar_edit_only_event(1));

    let mut themed_app = app::AppState::new_live(None, false, None);
    themed_app.live_details_drawer_open = true;
    themed_app.ingest_event(operator_sidebar_edit_only_event(1));
    let mut custom_theme = Theme::default();
    custom_theme.live_shell.primary.centered_content_width = 72;
    custom_theme.live_shell.primary.content_margin_x = 10;
    custom_theme.live_shell.primary.activity_drawer_width = 18;
    custom_theme.live_shell.primary.details_sidebar_width = 36;
    themed_app.set_theme_for_test(custom_theme);

    let default_plan = layout::FrameLayoutPlan::for_app(&default_app, area);
    let default_transcript = default_plan.transcript.unwrap_or_abort();
    let themed_plan = layout::FrameLayoutPlan::for_app(&themed_app, area);
    let themed_sidebar = themed_plan.details_overlay.unwrap_or_abort();
    let default_target = ui::hovered_wheel_target(
        &default_app,
        area,
        default_transcript.x.saturating_add(2),
        default_transcript.y.saturating_add(1),
    );
    let themed_target = ui::hovered_wheel_target(
        &themed_app,
        area,
        themed_sidebar.x.saturating_add(1),
        themed_sidebar.y.saturating_add(1),
    );

    assert_ne!(default_plan.details_overlay, themed_plan.details_overlay);
    assert_eq!(default_target, Some(ui::WheelTarget::Transcript));
    assert_eq!(themed_target, Some(ui::WheelTarget::Inspector));
}

pub(super) fn layout_plan_minimum_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));
    let dock = plan.dock.unwrap_or_abort();

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 80, 0));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 24, 80, 0));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 80, 19))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 19, 76, 3))
    );
    assert_eq!(dock.shell, ratatui::layout::Rect::new(2, 19, 76, 5));
    assert_eq!(dock.status, plan.status);
    assert_eq!(dock.composer, plan.composer.unwrap_or_abort());
    assert_eq!(
        dock.disclosure,
        Some(ratatui::layout::Rect::new(2, 22, 76, 1))
    );
    assert_eq!(plan.disclosure, dock.disclosure);
}

pub(super) fn layout_plan_primary_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    for event in session_view_events() {
        app.ingest_event(event);
    }
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));
    let dock = plan.dock.unwrap_or_abort();

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 100, 0));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 30, 100, 0));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 25))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 25, 96, 3))
    );
    assert_eq!(dock.shell, ratatui::layout::Rect::new(2, 25, 96, 5));
    assert_eq!(dock.status, plan.status);
    assert_eq!(dock.composer, plan.composer.unwrap_or_abort());
    assert_eq!(
        dock.disclosure,
        Some(ratatui::layout::Rect::new(2, 28, 96, 1))
    );
    assert_eq!(plan.disclosure, dock.disclosure);
}

pub(super) fn layout_plan_primary_empty_operator_rail_keeps_fixed_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 25))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.details_overlay, None);
    assert_eq!(plan.wheel_hit_areas.transcript, plan.transcript);
    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
}

pub(super) fn layout_plan_split_empty_operator_rail_keeps_fixed_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 96, 40));

    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 96, 34))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.details_overlay, None);
    assert_eq!(plan.wheel_hit_areas.transcript, plan.transcript);
    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
}

pub(super) fn wide_primary_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 160, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Primary);
    assert_eq!(plan.shell.x, 0);
    assert_eq!(plan.shell.width, area.width);
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

pub(super) fn split_window_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 96, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Split);
    assert_eq!(plan.shell.x, 0);
    assert_eq!(plan.shell.width, area.width);
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

pub(super) fn live_layout_breakpoints_choose_shell_variant() {
    let theme = Theme::default();

    let minimum = theme.live_shell_layout(80, 24);
    assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
    assert_eq!(minimum.activity_drawer_width, 20);
    assert_eq!(minimum.inspector_drawer_width, 20);
    assert_eq!(minimum.details_sidebar_width, 42);
    assert_eq!(minimum.transcript_min_width, 28);
    assert_eq!(minimum.centered_content_width, 80);

    let split = theme.live_shell_layout(96, 40);
    assert_eq!(split.target, ShellGeometryTarget::Split);
    assert_eq!(split.activity_drawer_width, 18);
    assert_eq!(split.inspector_drawer_width, 24);
    assert_eq!(split.details_sidebar_width, 42);
    assert_eq!(split.transcript_min_width, 32);
    assert_eq!(split.centered_content_width, 86);

    let primary = theme.live_shell_layout(100, 30);
    assert_eq!(primary.target, ShellGeometryTarget::Primary);
    assert_eq!(primary.activity_drawer_width, 24);
    assert_eq!(primary.inspector_drawer_width, 28);
    assert_eq!(primary.details_sidebar_width, 42);
    assert_eq!(primary.transcript_min_width, 40);
    assert_eq!(primary.centered_content_width, 90);

    assert_eq!(
        theme.live_shell.target(89, 40),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(90, 35),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(90, 36), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(99, 30),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(99, 40), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(100, 29),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(100, 30),
        ShellGeometryTarget::Primary
    );
}

pub(super) fn layout_breakpoints_match_shell_parity_contract() {
    let mut wide = app::AppState::new_live(None, false, None);
    wide.active_tab = app::Tab::Run;
    for event in session_view_events() {
        wide.ingest_event(event);
    }
    let wide_plan =
        layout::FrameLayoutPlan::for_app(&wide, ratatui::layout::Rect::new(0, 0, 160, 48));
    assert_eq!(wide_plan.header.height, 0);
    assert_eq!(wide_plan.operator_sidebar, None);

    let mut primary = app::AppState::new_live(None, false, None);
    primary.active_tab = app::Tab::Run;
    for event in session_view_events() {
        primary.ingest_event(event);
    }
    let primary_plan =
        layout::FrameLayoutPlan::for_app(&primary, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(primary_plan.header.height, 0);
    assert_eq!(primary_plan.operator_sidebar, None);

    let mut split = app::AppState::new_live(None, false, None);
    split.active_tab = app::Tab::Run;
    for event in session_view_events() {
        split.ingest_event(event);
    }
    let split_plan =
        layout::FrameLayoutPlan::for_app(&split, ratatui::layout::Rect::new(0, 0, 96, 40));
    assert_eq!(split_plan.header.height, 0);
    assert_eq!(split_plan.operator_sidebar, None);

    let mut overlay = app::AppState::new_live(None, false, None);
    overlay.live_details_drawer_open = true;
    let overlay_plan =
        layout::FrameLayoutPlan::for_app(&overlay, ratatui::layout::Rect::new(0, 0, 80, 48));
    assert_eq!(overlay_plan.header.height, 0);
    assert!(overlay_plan.operator_sidebar.is_none());
    assert_eq!(
        overlay_plan.details_overlay,
        Some(ratatui::layout::Rect::new(38, 0, 42, 42))
    );

    let mut compact = app::AppState::new_live(None, false, None);
    compact.live_details_drawer_open = true;
    let compact_plan =
        layout::FrameLayoutPlan::for_app(&compact, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(compact_plan.header.height, 0);
    assert!(compact_plan.operator_sidebar.is_none());
    assert_eq!(
        compact_plan.details_overlay,
        Some(ratatui::layout::Rect::new(38, 0, 42, 19))
    );

    let mut dense = app::AppState::new_live(None, false, None);
    dense.live_details_drawer_open = true;
    let dense_plan =
        layout::FrameLayoutPlan::for_app(&dense, ratatui::layout::Rect::new(0, 0, 60, 18));
    assert_eq!(dense_plan.header.height, 0);
    assert!(dense_plan.operator_sidebar.is_none());
    assert!(dense_plan.details_overlay.is_none());
}
