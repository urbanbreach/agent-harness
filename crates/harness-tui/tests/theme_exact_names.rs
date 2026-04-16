use harness_tui::theme::{
    ChromeMode, DividerIntensity, ShellBreakpoints, ShellGeometryTarget, SpacingDensity, Theme,
};

const UI_CHROME: &str = include_str!("../src/ui_chrome.rs");

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
    assert_ne!(theme.surface.shell, theme.surface.panel);
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
}

#[test]
fn default_theme_matches_opencode_dark_contract() {
    let default = Theme::default();
    let opencode_dark = Theme::opencode_dark();

    assert_eq!(default, opencode_dark);
    assert_eq!(default.token_families(), opencode_dark.token_families());
}

#[test]
fn semantic_chrome_tokens_map_to_opencode_dark_defaults() {
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
fn semantic_live_shell_surfaces_separate_transcript_rail_and_dock() {
    let theme = Theme::default();
    let tokens = theme.token_families();

    assert_eq!(
        tokens.semantic.chrome.chromeless.surface,
        theme.surface.shell
    );
    assert_eq!(tokens.semantic.chrome.divided.surface, theme.surface.panel);
    assert_eq!(tokens.semantic.chrome.card.surface, theme.surface.overlay);
    assert_ne!(theme.surface.shell, theme.surface.panel);
    assert_ne!(theme.surface.panel, theme.surface.panel_elevated);
    assert_ne!(theme.surface.shell, theme.surface.panel_elevated);

    assert!(UI_CHROME.contains("pub(super) fn live_transcript_shell_surface"));
    assert!(UI_CHROME.contains("pub(super) fn live_anchor_panel_surface"));
    assert!(UI_CHROME.contains("pub(super) fn live_control_dock_surface"));
    assert!(UI_CHROME.contains("pub(super) fn live_transcript_shell_section"));
    assert!(UI_CHROME.contains("pub(super) fn live_control_dock_section"));
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
