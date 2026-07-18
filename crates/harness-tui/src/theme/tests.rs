use super::*;

#[test]
fn harness_dark_theme_matches_palette_contract() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.surface.canvas, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.surface.shell, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.surface.panel, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.surface.panel_elevated, rgb(0x12, 0x16, 0x1E));
    assert_eq!(theme.surface.overlay, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.border.subtle, rgb(0x3A, 0x3D, 0x43));
    assert_eq!(theme.border.strong, rgb(0x48, 0x4B, 0x52));
    assert_eq!(theme.border.focus, rgb(0x60, 0x63, 0x6A));
    assert_eq!(theme.text.primary, rgb(0xD7, 0xDA, 0xE0));
    assert_eq!(theme.text.secondary, rgb(0x88, 0x8B, 0x91));
    assert_eq!(theme.text.tertiary, rgb(0x88, 0x8B, 0x91));
    assert_eq!(theme.text.accent, rgb(0xD9, 0x84, 0xD9));
    assert_eq!(theme.text.inverse, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.question_prompt.accent, rgb(0xD9, 0x84, 0xD9));
    assert_eq!(theme.question_prompt.secondary, rgb(0x5C, 0x9C, 0xF5));
    assert_eq!(theme.status.success, rgb(0x7F, 0xD8, 0x8F));
    assert_eq!(theme.status.warning, rgb(0xE5, 0xC0, 0x7B));
    assert_eq!(theme.status.error, rgb(0xE0, 0x6C, 0x75));
    assert_eq!(theme.status.info, rgb(0x56, 0xB6, 0xC2));
    assert_eq!(theme.status.disabled, rgb(0x80, 0x80, 0x80));
    assert_eq!(theme.agents.build, rgb(0x5C, 0x9C, 0xF5));
    assert_eq!(theme.agents.plan, rgb(0xD9, 0x84, 0xD9));
    assert_eq!(theme.agents.docs, rgb(0xE5, 0xC0, 0x7B));
    assert_eq!(theme.agents.ask, rgb(0xE8, 0xA0, 0xE8));
    assert_eq!(
        theme.agents.palette,
        [
            rgb(0x5C, 0x9C, 0xF5),
            rgb(0xD9, 0x84, 0xD9),
            rgb(0x7F, 0xD8, 0x8F),
            rgb(0xE5, 0xC0, 0x7B),
            rgb(0xE8, 0xA0, 0xE8),
            rgb(0xE0, 0x6C, 0x75),
            rgb(0x56, 0xB6, 0xC2),
        ]
    );
    assert_eq!(theme.agent_accent("build"), rgb(0x5C, 0x9C, 0xF5));
    assert_eq!(theme.agent_accent("Build"), rgb(0x5C, 0x9C, 0xF5));
    assert_eq!(theme.agent_accent("default"), rgb(0x5C, 0x9C, 0xF5));
    assert_eq!(theme.agent_accent("plan"), rgb(0xD9, 0x84, 0xD9));
    assert_eq!(theme.agent_accent("docs"), rgb(0xE5, 0xC0, 0x7B));
    assert_eq!(theme.agent_accent("ask"), rgb(0xE8, 0xA0, 0xE8));
    assert_eq!(theme.agent_accent("Plan"), theme.agent_accent(" plan "));
    assert_eq!(theme.agent_accent("worker"), theme.agent_accent("Worker"));
    assert_eq!(theme.scrollbar.track, rgb(0x0B, 0x0E, 0x14));
    assert_eq!(theme.scrollbar.thumb, rgb(0x32, 0x36, 0x3C));
    assert_eq!(theme.scrollbar.thumb_active, rgb(0x60, 0x63, 0x6A));
}

#[test]
fn semantic_theme_families_preserve_default_contracts() {
    let theme = Theme::default();
    let tokens = theme.token_families();

    assert_eq!(
        tokens.semantic.chrome.chromeless.surface,
        theme.surface.shell
    );
    assert_eq!(tokens.semantic.chrome.divided.surface, theme.surface.panel);
    assert_eq!(tokens.semantic.chrome.card.surface, theme.surface.overlay);
    assert_eq!(theme.surface.shell, theme.surface.panel);
    assert_ne!(theme.surface.panel, theme.surface.panel_elevated);
    assert_ne!(theme.surface.shell, theme.surface.panel_elevated);
    assert_eq!(
        tokens.semantic.dividers.subtle.color,
        Some(theme.border.subtle)
    );
    assert_eq!(
        tokens.semantic.dividers.strong.color,
        Some(theme.border.strong)
    );
    assert_eq!(
        tokens.semantic.dividers.focus.color,
        Some(theme.border.focus)
    );
    assert_eq!(
        tokens.semantic.density.minimum.heights,
        theme.live_shell.heights
    );
    assert_eq!(
        tokens.semantic.density.split.rhythm,
        theme.live_shell.rhythm
    );
    assert_eq!(
        tokens.semantic.density.primary.content_margin_x,
        theme.live_shell.primary.content_margin_x
    );
    assert_eq!(
        tokens.semantic.composer.primary.padding_x,
        theme.live_shell.rhythm.composer_padding_x
    );
    assert_eq!(tokens.palette.surfaces, theme.surface);
    assert_eq!(tokens.palette.borders, theme.border);
    assert_eq!(tokens.palette.text, theme.text);
    assert_eq!(tokens.palette.status, theme.status);
    assert_eq!(
        tokens.live_shell.geometry.breakpoints,
        ShellBreakpoints::DEFAULT
    );
    assert_eq!(tokens.live_shell.geometry.minimum, theme.live_shell.minimum);
    assert_eq!(tokens.live_shell.geometry.primary, theme.live_shell.primary);
    assert_eq!(tokens.live_shell.spacing.heights, theme.live_shell.heights);
    assert_eq!(tokens.live_shell.spacing.rhythm, theme.live_shell.rhythm);
    assert_eq!(
        tokens.live_shell.glyphs.preferred.status,
        theme.live_shell.glyphs
    );
    assert_eq!(
        tokens.live_shell.glyphs.preferred.transcript,
        theme.live_shell.transcript_glyphs
    );
    assert_eq!(tokens.live_shell.copy.startup, theme.live_shell.startup);
    assert_eq!(
        tokens.live_shell.copy.empty_state,
        theme.live_shell.empty_state
    );
    assert_eq!(
        tokens.live_shell.glyphs.ascii.status.pending_permission,
        "?"
    );
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.user_marker, ">");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.tool_marker, "*");
}

#[test]
fn default_theme_matches_harness_dark_contract() {
    let default = Theme::default();
    let harness_dark = Theme::harness_dark();

    assert_eq!(default, harness_dark);
    assert_eq!(default.token_families(), harness_dark.token_families());
}

#[test]
fn semantic_chrome_tokens_map_to_harness_dark_defaults() {
    let theme = Theme::default();
    let tokens = theme.token_families();

    assert_eq!(
        tokens.semantic.chrome.chromeless.mode,
        ChromeMode::Chromeless
    );
    assert_eq!(
        tokens.semantic.chrome.chromeless.surface,
        theme.surface.shell
    );
    assert_eq!(
        tokens.semantic.chrome.chromeless.border,
        tokens.semantic.dividers.none
    );
    assert_eq!(tokens.semantic.chrome.divided.mode, ChromeMode::Divided);
    assert_eq!(tokens.semantic.chrome.divided.surface, theme.surface.panel);
    assert_eq!(
        tokens.semantic.chrome.divided.border,
        tokens.semantic.dividers.subtle
    );
    assert_eq!(tokens.semantic.chrome.card.mode, ChromeMode::Card);
    assert_eq!(tokens.semantic.chrome.card.surface, theme.surface.overlay);
    assert_eq!(
        tokens.semantic.chrome.card.border,
        tokens.semantic.dividers.subtle
    );
    assert_eq!(
        tokens.semantic.dividers.none.intensity,
        DividerIntensity::None
    );
    assert_eq!(tokens.semantic.dividers.none.color, None);
    assert_eq!(
        tokens.semantic.dividers.subtle.color,
        Some(theme.border.subtle)
    );
    assert_eq!(
        tokens.semantic.dividers.strong.color,
        Some(theme.border.strong)
    );
    assert_eq!(
        tokens.semantic.dividers.focus.color,
        Some(theme.border.focus)
    );
}

#[test]
fn semantic_composer_tokens_have_primary_split_minimum_variants() {
    let theme = Theme::default();
    let tokens = theme.token_families();

    assert_eq!(
        tokens.semantic.composer.minimum.target,
        ShellGeometryTarget::Minimum
    );
    assert_eq!(tokens.semantic.composer.minimum.chrome, ChromeMode::Card);
    assert_eq!(
        tokens.semantic.composer.minimum.divider,
        DividerIntensity::Subtle
    );
    assert_eq!(
        tokens.semantic.composer.minimum.density,
        SpacingDensity::Compact
    );
    assert_eq!(
        tokens.semantic.composer.minimum.surface,
        theme.surface.panel_elevated
    );
    assert_eq!(
        tokens.semantic.composer.minimum.border,
        Some(theme.border.subtle)
    );
    assert_eq!(
        tokens.semantic.composer.split.target,
        ShellGeometryTarget::Split
    );
    assert_eq!(tokens.semantic.composer.split.chrome, ChromeMode::Divided);
    assert_eq!(
        tokens.semantic.composer.split.divider,
        DividerIntensity::Subtle
    );
    assert_eq!(
        tokens.semantic.composer.split.density,
        SpacingDensity::Standard
    );
    assert_eq!(
        tokens.semantic.composer.split.surface,
        theme.surface.panel_elevated
    );
    assert_eq!(
        tokens.semantic.composer.split.border,
        Some(theme.border.subtle)
    );
    assert_eq!(
        tokens.semantic.composer.primary.target,
        ShellGeometryTarget::Primary
    );
    assert_eq!(tokens.semantic.composer.primary.chrome, ChromeMode::Divided);
    assert_eq!(
        tokens.semantic.composer.primary.divider,
        DividerIntensity::Subtle
    );
    assert_eq!(
        tokens.semantic.composer.primary.density,
        SpacingDensity::Roomy
    );
    assert_eq!(
        tokens.semantic.composer.primary.surface,
        theme.surface.panel_elevated
    );
    assert_eq!(
        tokens.semantic.composer.primary.border,
        Some(theme.border.subtle)
    );
    assert_eq!(
        tokens.semantic.composer.minimum.padding_x,
        theme.live_shell.rhythm.composer_padding_x
    );
    assert_eq!(
        tokens.semantic.composer.split.padding_x,
        theme.live_shell.rhythm.composer_padding_x
    );
    assert_eq!(
        tokens.semantic.composer.primary.padding_x,
        theme.live_shell.rhythm.composer_padding_x
    );
    assert_eq!(
        tokens
            .semantic
            .composer
            .select(ShellGeometryTarget::Minimum),
        tokens.semantic.composer.minimum
    );
    assert_eq!(
        tokens.semantic.composer.select(ShellGeometryTarget::Split),
        tokens.semantic.composer.split
    );
    assert_eq!(
        tokens
            .semantic
            .composer
            .select(ShellGeometryTarget::Primary),
        tokens.semantic.composer.primary
    );
}

#[test]
fn live_shell_tokens_choose_primary_geometry_at_signoff_size() {
    let theme = Theme::default();
    let tokens = theme.token_families();
    let minimum = theme.live_shell_layout(80, 24);
    let split = theme.live_shell_layout(96, 40);
    let primary = theme.live_shell_layout(100, 30);
    let minimum_lifecycle = theme.lifecycle_surface_layout(80, 24);
    let split_lifecycle = theme.lifecycle_surface_layout(96, 40);
    let primary_lifecycle = theme.lifecycle_surface_layout(100, 30);

    assert_eq!(primary.target, ShellGeometryTarget::Primary);
    assert_eq!(split.target, ShellGeometryTarget::Split);
    assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
    assert_eq!(primary_lifecycle.target, ShellGeometryTarget::Primary);
    assert_eq!(split_lifecycle.target, ShellGeometryTarget::Split);
    assert_eq!(minimum_lifecycle.target, ShellGeometryTarget::Minimum);
    assert_eq!(minimum.centered_content_width, 80);
    assert_eq!(minimum.content_margin_x, 0);
    assert_eq!(minimum.details_sidebar_width, 42);
    assert_eq!(minimum_lifecycle.startup_card.width, 70);
    assert_eq!(minimum_lifecycle.startup_card.height, 12);
    assert_eq!(minimum_lifecycle.post_run_card.width, 72);
    assert_eq!(minimum_lifecycle.post_run_card.height, 12);
    assert_eq!(minimum_lifecycle.overlay.width, 76);
    assert_eq!(theme.live_shell.rhythm.composer_padding_x, 2);
    assert_eq!(theme.live_shell.rhythm.sidebar_padding_x, 2);
    assert_eq!(theme.live_shell.rhythm.sidebar_padding_y, 1);
    assert_eq!(theme.live_shell.rhythm.footer_prefix_gap, 2);
    assert_eq!(split.centered_content_width, 86);
    assert_eq!(split.content_margin_x, 0);
    assert_eq!(split.details_sidebar_width, 42);
    assert_eq!(split_lifecycle.startup_card.width, 92);
    assert_eq!(split_lifecycle.startup_card.height, 13);
    assert_eq!(split_lifecycle.post_run_card.width, 76);
    assert_eq!(split_lifecycle.post_run_card.height, 12);
    assert_eq!(split_lifecycle.overlay.width, 86);
    assert_eq!(primary.centered_content_width, 90);
    assert_eq!(primary.content_margin_x, 0);
    assert_eq!(primary.details_sidebar_width, 42);
    assert_eq!(primary_lifecycle.startup_card.width, 82);
    assert_eq!(primary_lifecycle.startup_card.height, 12);
    assert_eq!(primary_lifecycle.post_run_card.width, 78);
    assert_eq!(primary_lifecycle.post_run_card.height, 12);
    assert_eq!(primary_lifecycle.overlay.width, 90);
    assert_eq!(theme.live_shell.rhythm.status_separator, 2);
    assert_eq!(theme.live_shell.heights.status, 1);
    assert_eq!(theme.live_shell.transcript_glyphs.user_marker, "❯");
    assert_eq!(theme.live_shell.transcript_glyphs.tool_marker, "◆");
    assert_eq!(theme.live_shell.transcript_glyphs.card_top, "  ");
    assert_eq!(
        tokens.live_shell.geometry.target(100, 30),
        ShellGeometryTarget::Primary
    );
    assert_eq!(tokens.live_shell.glyphs.ascii.status.failed, "x");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.card_top, "  ");

    assert!(minimum.centered_content_width + minimum.content_margin_x.saturating_mul(2) <= 80);
    assert!(split.centered_content_width + split.content_margin_x.saturating_mul(2) <= 96);
    assert!(primary.centered_content_width + primary.content_margin_x.saturating_mul(2) <= 100);
}

#[test]
fn layout_plan_shell_width_tracks_theme_contracts() {
    let mut app = crate::app::AppState::new_live(None, false, None);
    app.active_tab = crate::app::Tab::Run;

    let minimum = crate::layout::FrameLayoutPlan::for_app(
        &app,
        ratatui::layout::Rect::new(
            0,
            0,
            ShellGeometry::MINIMUM.width,
            ShellGeometry::MINIMUM.height,
        ),
    );
    assert_eq!(minimum.shell.width, 80);

    let primary = crate::layout::FrameLayoutPlan::for_app(
        &app,
        ratatui::layout::Rect::new(
            0,
            0,
            ShellGeometry::PRIMARY.width,
            ShellGeometry::PRIMARY.height,
        ),
    );
    assert_eq!(primary.shell.width, 100);

    let split =
        crate::layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 96, 40));
    assert_eq!(split.shell.width, 96);
}

#[test]
fn diff_side_by_side_threshold_matches_geometry_contract() {
    assert_eq!(DIFF_SIDE_BY_SIDE_MIN_WIDTH, 96);
}
