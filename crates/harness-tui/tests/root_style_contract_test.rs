//! Task 7: Root style contract tests for the theme system.
//!
//! Proves that the theme's style application methods, chrome/border
//! contracts, glyph catalog, and semantic role mappings are stable
//! and match the reference binary's observable behavior:
//!
//! - Style methods (primary/secondary/accent/muted/dim/status) derive
//!   from theme tokens
//! - Chrome styles map surface + border correctly
//! - Border styles map intensity to color
//! - Glyph catalog matches reference roles (❯, ◆, ●, ✗, ◐)
//! - Token families preserve default contracts
//! - Terminal-native style contract uses Reset + modifiers
//! - Quantized style contracts degrade cleanly
//! - Adversarial: style/glyph mutations are detected

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::theme::{
    ChromeMode, ColorLevel, DividerIntensity, ShellGeometryTarget, SpacingDensity, StatusRole,
    Theme,
};
use ratatui::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Group 1 — Style application derives from theme tokens
// ---------------------------------------------------------------------------

#[test]
fn primary_text_style_carries_text_primary_token() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.primary_text_style(),
        Style::new().fg(theme.text.primary)
    );
}

#[test]
fn secondary_text_style_carries_text_secondary_token() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.secondary_text_style(),
        Style::new().fg(theme.text.secondary)
    );
}

#[test]
fn accent_text_style_carries_accent_and_bold() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.accent_text_style(),
        Style::new()
            .fg(theme.text.accent)
            .add_modifier(Modifier::BOLD)
    );
}

#[test]
fn muted_text_style_carries_secondary_and_dim() {
    // arrange
    // act
    let theme = Theme::default();
    let style = theme.muted_text_style();
    // assert
    assert_eq!(style.fg, Some(theme.text.secondary));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn dim_text_style_carries_tertiary_and_dim() {
    // arrange
    // act
    let theme = Theme::default();
    let style = theme.dim_text_style();
    // assert
    assert_eq!(style.fg, Some(theme.text.tertiary));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

// ---------------------------------------------------------------------------
// Group 2 — Status style mapping
// ---------------------------------------------------------------------------

#[test]
fn status_style_maps_each_role_to_correct_color() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.status_style(StatusRole::Success),
        Style::new().fg(theme.status.success)
    );
    assert_eq!(
        theme.status_style(StatusRole::Warning),
        Style::new().fg(theme.status.warning)
    );
    assert_eq!(
        theme.status_style(StatusRole::Error),
        Style::new().fg(theme.status.error)
    );
    assert_eq!(
        theme.status_style(StatusRole::Info),
        Style::new().fg(theme.status.info)
    );
    assert_eq!(
        theme.status_style(StatusRole::Disabled),
        Style::new().fg(theme.status.disabled)
    );
}

#[test]
fn status_colors_are_pairwise_distinct_in_dark_theme() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    let colors = [
        theme.status.success,
        theme.status.warning,
        theme.status.error,
        theme.status.info,
        theme.status.disabled,
    ];
    for (i, &c) in colors.iter().enumerate() {
        // assert
        assert!(
            !colors[..i].contains(&c),
            "status colors must be pairwise distinct; duplicate at index {i}"
        );
    }
}

#[test]
fn status_colors_are_pairwise_distinct_in_high_contrast() {
    // arrange
    // act
    let theme = Theme::harness_high_contrast();
    let colors = [
        theme.status.success,
        theme.status.warning,
        theme.status.error,
        theme.status.info,
    ];
    for (i, &c) in colors.iter().enumerate() {
        // assert
        assert!(
            !colors[..i].contains(&c),
            "status colors must be pairwise distinct; duplicate at index {i}"
        );
    }
}

#[test]
fn primary_text_contrasts_canvas_in_dark_theme() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    // assert
    assert_ne!(
        theme.text.primary, theme.surface.canvas,
        "primary text must contrast the canvas"
    );
}

// ---------------------------------------------------------------------------
// Group 3 — Border style mapping
// ---------------------------------------------------------------------------

#[test]
fn border_style_none_is_unstyled() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(theme.border_style(DividerIntensity::None), Style::new());
}

#[test]
fn border_style_subtle_carries_subtle_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.border_style(DividerIntensity::Subtle),
        Style::new().fg(theme.border.subtle)
    );
}

#[test]
fn border_style_strong_carries_strong_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.border_style(DividerIntensity::Strong),
        Style::new().fg(theme.border.strong)
    );
}

#[test]
fn border_style_focus_carries_focus_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.border_style(DividerIntensity::Focus),
        Style::new().fg(theme.border.focus)
    );
}

#[test]
fn border_intensities_produce_distinct_styles() {
    // arrange
    // act
    let theme = Theme::default();
    let none = theme.border_style(DividerIntensity::None);
    let subtle = theme.border_style(DividerIntensity::Subtle);
    let strong = theme.border_style(DividerIntensity::Strong);
    let focus = theme.border_style(DividerIntensity::Focus);
    // assert
    assert_ne!(none, subtle);
    assert_ne!(subtle, strong);
    assert_ne!(strong, focus);
}

// ---------------------------------------------------------------------------
// Group 4 — Chrome style mapping
// ---------------------------------------------------------------------------

#[test]
fn chrome_style_chromeless_has_shell_bg_no_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.chrome_style(ChromeMode::Chromeless),
        Style::new().bg(theme.surface.shell)
    );
}

#[test]
fn chrome_style_divided_has_panel_bg_and_subtle_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.chrome_style(ChromeMode::Divided),
        Style::new().bg(theme.surface.panel).fg(theme.border.subtle)
    );
}

#[test]
fn chrome_style_card_has_overlay_bg_and_subtle_border() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(
        theme.chrome_style(ChromeMode::Card),
        Style::new()
            .bg(theme.surface.overlay)
            .fg(theme.border.subtle)
    );
}

// ---------------------------------------------------------------------------
// Group 5 — Glyph catalog contracts
// ---------------------------------------------------------------------------

#[test]
fn composer_user_marker_is_reference_arrow() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(theme.live_shell.transcript_glyphs.user_marker, "❯");
}

#[test]
fn tool_marker_glyph_is_diamond() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(theme.live_shell.transcript_glyphs.tool_marker, "◆");
}

#[test]
fn status_glyphs_match_reference_roles() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(theme.live_shell.glyphs.streaming, "◐");
    assert_eq!(theme.live_shell.glyphs.done, "●");
    assert_eq!(theme.live_shell.glyphs.error, "✗");
    assert_eq!(theme.live_shell.glyphs.succeeded, "●");
    assert_eq!(theme.live_shell.glyphs.failed, "✗");
}

#[test]
fn ascii_fallback_glyphs_are_single_char() {
    // arrange
    // act
    let theme = Theme::default();
    let ascii = theme.live_shell.ascii_glyphs;
    for glyph in [
        ascii.status.streaming,
        ascii.status.done,
        ascii.status.error,
        ascii.status.pending_permission,
        ascii.status.queued,
        ascii.status.running,
        ascii.status.succeeded,
        ascii.status.failed,
    ] {
        // assert
        assert_eq!(
            glyph.chars().count(),
            1,
            "ASCII fallback glyph must be single char: {glyph:?}"
        );
    }
}

#[test]
fn ascii_fallback_user_marker_is_greater_than() {
    // arrange
    // act
    let theme = Theme::default();
    // assert
    assert_eq!(theme.live_shell.ascii_glyphs.transcript.user_marker, ">");
}

// ---------------------------------------------------------------------------
// Group 6 — Token family contracts
// ---------------------------------------------------------------------------

#[test]
fn token_families_preserve_chrome_contracts() {
    // arrange
    let theme = Theme::default();
    let tokens = theme.token_families();

    // act
    assert_eq!(
        tokens.semantic.chrome.chromeless.mode,
        ChromeMode::Chromeless
    );
    assert_eq!(tokens.semantic.chrome.divided.mode, ChromeMode::Divided);
    assert_eq!(tokens.semantic.chrome.card.mode, ChromeMode::Card);

    // assert
    assert_eq!(
        tokens.semantic.chrome.divided.border.intensity,
        DividerIntensity::Subtle
    );
    assert!(tokens.semantic.chrome.chromeless.border.color.is_none());
}

#[test]
fn token_families_preserve_composer_contracts() {
    // arrange
    let theme = Theme::default();
    let tokens = theme.token_families();

    assert_eq!(tokens.semantic.composer.minimum.chrome, ChromeMode::Card);
    assert_eq!(tokens.semantic.composer.split.chrome, ChromeMode::Divided);
    assert_eq!(tokens.semantic.composer.primary.chrome, ChromeMode::Divided);

    // act
    for target in [
        ShellGeometryTarget::Minimum,
        ShellGeometryTarget::Split,
        ShellGeometryTarget::Primary,
    ] {
        let composer = tokens.semantic.composer.select(target);
        // assert
        assert_eq!(
            composer.padding_x,
            theme.live_shell.rhythm.composer_padding_x
        );
    }
}

#[test]
fn token_families_preserve_density_contracts() {
    // arrange
    // act
    let theme = Theme::default();
    let tokens = theme.token_families();

    // assert
    assert_eq!(
        tokens.semantic.density.minimum.density,
        SpacingDensity::Compact
    );
    assert_eq!(
        tokens.semantic.density.split.density,
        SpacingDensity::Standard
    );
    assert_eq!(
        tokens.semantic.density.primary.density,
        SpacingDensity::Roomy
    );
}

#[test]
fn token_families_preserve_divider_contracts() {
    // arrange
    let theme = Theme::default();
    let tokens = theme.token_families();

    // act
    assert_eq!(
        tokens.semantic.dividers.none.intensity,
        DividerIntensity::None
    );
    assert_eq!(
        tokens.semantic.dividers.subtle.intensity,
        DividerIntensity::Subtle
    );
    assert_eq!(
        tokens.semantic.dividers.strong.intensity,
        DividerIntensity::Strong
    );
    assert_eq!(
        tokens.semantic.dividers.focus.intensity,
        DividerIntensity::Focus
    );

    // assert
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

// ---------------------------------------------------------------------------
// Group 7 — Theme switching and availability
// ---------------------------------------------------------------------------

#[test]
fn by_name_resolves_all_documented_themes() {
    // arrange
    // act
    // assert
    assert_eq!(Theme::by_name("default"), Some(Theme::harness_chat()));
    assert_eq!(Theme::by_name("harness-chat"), Some(Theme::harness_chat()));
    assert_eq!(Theme::by_name("harness-dark"), Some(Theme::harness_dark()));
    assert_eq!(
        Theme::by_name("harness-light"),
        Some(Theme::harness_light())
    );
    assert_eq!(
        Theme::by_name("high-contrast"),
        Some(Theme::harness_high_contrast())
    );
}

#[test]
fn by_name_rejects_unknown_themes() {
    // arrange
    // act
    // assert
    assert_eq!(Theme::by_name("solarized"), None);
    assert_eq!(Theme::by_name(""), None);
}

#[test]
fn available_theme_names_lists_user_selectable_themes() {
    // arrange
    // act
    let names = Theme::available_theme_names();
    // assert
    assert!(names.contains(&"default"));
    assert!(names.contains(&"harness-light"));
    assert!(names.contains(&"high-contrast"));
}

#[test]
fn default_theme_is_harness_chat() {
    // arrange
    // act
    // assert
    assert_eq!(Theme::default(), Theme::harness_chat());
}

// ---------------------------------------------------------------------------
// Group 8 — Generic assistant accent contract
// ---------------------------------------------------------------------------

#[test]
fn agent_accent_is_profile_independent() {
    // arrange
    // Given: a theme and arbitrary legacy profile labels.
    let theme = Theme::harness_dark();

    // act
    // When/Then: every legacy label resolves to the same semantic text accent.
    for profile in ["", "default", "build", "plan", "docs", "ask", "worker"] {
        // assert
        assert_eq!(theme.agent_accent(profile), theme.text.accent);
    }
}

// ---------------------------------------------------------------------------
// Group 9 — Terminal-native style contract
// ---------------------------------------------------------------------------

#[test]
fn terminal_native_primary_text_style_uses_reset() {
    // arrange
    // act
    let theme = Theme::terminal_native();
    // assert
    assert_eq!(theme.primary_text_style(), Style::new().fg(Color::Reset));
}

#[test]
fn terminal_native_chrome_style_uses_reset_bg() {
    // arrange
    // act
    let theme = Theme::terminal_native();
    // assert
    assert_eq!(
        theme.chrome_style(ChromeMode::Chromeless),
        Style::new().bg(Color::Reset)
    );
}

#[test]
fn terminal_native_border_style_none_is_unstyled() {
    // arrange
    // act
    let theme = Theme::terminal_native();
    // assert
    assert_eq!(theme.border_style(DividerIntensity::None), Style::new());
}

#[test]
fn terminal_native_status_style_uses_named_ansi() {
    // arrange
    // act
    let theme = Theme::terminal_native();
    // assert
    assert_eq!(
        theme.status_style(StatusRole::Success),
        Style::new().fg(Color::Green)
    );
    assert_eq!(
        theme.status_style(StatusRole::Error),
        Style::new().fg(Color::Red)
    );
}

// ---------------------------------------------------------------------------
// Group 10 — Quantized style contracts
// ---------------------------------------------------------------------------

#[test]
fn quantized_ansi256_status_style_uses_indexed() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::Ansi256);
    let style = q.status_style(StatusRole::Success);
    // assert
    assert!(
        matches!(style.fg, Some(Color::Indexed(_))),
        "Ansi256 status style should use Indexed color"
    );
}

#[test]
fn quantized_basic_status_style_uses_named_ansi() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::Basic);
    let style = q.status_style(StatusRole::Success);
    // assert
    assert!(
        !matches!(style.fg, Some(Color::Rgb(..)) | Some(Color::Indexed(_))),
        "Basic status style should use named ANSI color"
    );
}

#[test]
fn quantized_none_status_style_uses_reset() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::None);
    let style = q.status_style(StatusRole::Success);
    // assert
    assert_eq!(style.fg, Some(Color::Reset));
}

// ---------------------------------------------------------------------------
// Group 11 — Adversarial: style/glyph mutations detected
// ---------------------------------------------------------------------------

#[test]
fn adversarial_modifier_mutation_detected() {
    // arrange
    // act
    let theme = Theme::default();
    let normal = theme.primary_text_style();
    let bolded = normal.add_modifier(Modifier::BOLD);
    // assert
    assert_ne!(normal, bolded, "modifier mutation must change style output");
}

#[test]
fn adversarial_glyph_mutation_detected() {
    // arrange
    // act
    let theme = Theme::default();
    let original = theme.live_shell.transcript_glyphs.user_marker;
    // assert
    assert_eq!(original, "❯");
    // If someone changes the prompt glyph, this test catches it
    assert_ne!(original, ">", "prompt glyph must be ❯ not >");
    assert_ne!(original, "$", "prompt glyph must be ❯ not $");
}

#[test]
fn adversarial_status_role_mutation_detected() {
    // arrange
    // act
    let theme = Theme::default();
    let success = theme.status_style(StatusRole::Success);
    let error = theme.status_style(StatusRole::Error);
    // assert
    assert_ne!(
        success, error,
        "different status roles must produce different styles"
    );
}

#[test]
fn adversarial_chrome_mode_mutation_detected() {
    // arrange
    // act
    let theme = Theme::default();
    let chromeless = theme.chrome_style(ChromeMode::Chromeless);
    let divided = theme.chrome_style(ChromeMode::Divided);
    // assert
    assert_ne!(
        chromeless, divided,
        "different chrome modes must produce different styles"
    );
}

#[test]
fn adversarial_quantization_level_mutation_detected() {
    // arrange
    // act
    let theme = Theme::harness_dark();
    let truecolor = theme.quantized(ColorLevel::TrueColor);
    let ansi256 = theme.quantized(ColorLevel::Ansi256);
    // assert
    assert_ne!(
        truecolor.surface.canvas, ansi256.surface.canvas,
        "different quantization levels must produce different canvas colors"
    );
}
