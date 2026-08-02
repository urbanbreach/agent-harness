//! Task 7: Terminal color capability and degradation parity tests.
//!
//! Proves that the Harness theme contract supports the same color
//! degradation pipeline as the pinned reference binary:
//!
//! - `ColorLevel` enum with None / Basic / Ansi256 / TrueColor ordering
//! - `quantize_color` downgrades RGB → indexed → ANSI16 → Reset
//! - `Theme::quantized()` applies degradation to every color field
//! - `Theme::terminal_native()` uses only `Color::Reset` + named ANSI
//! - `Theme::ansi16_chrome_overrides()` pins semantic hues at 16-color
//! - `Theme::for_color_level()` selects the right strategy per level
//! - `detect_color_level()` reads env vars correctly
//! - Background clearing: terminal-native uses Reset (no RGB emitted)
//! - Adversarial: color/glyph mutations are detected

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::theme::{
    detect_color_level, indexed_to_rgb, nearest_indexed, quantize_color, resolve_to_rgb,
    ColorLevel, Theme,
};
use ratatui::style::Color;

// ---------------------------------------------------------------------------
// Group 1 — ColorLevel ordering and methods
// ---------------------------------------------------------------------------

#[test]
fn color_level_ordering_is_monotonic() {
    assert!(ColorLevel::None < ColorLevel::Basic);
    assert!(ColorLevel::Basic < ColorLevel::Ansi256);
    assert!(ColorLevel::Ansi256 < ColorLevel::TrueColor);
}

#[test]
fn color_level_has_color_gates() {
    assert!(!ColorLevel::None.has_color());
    assert!(ColorLevel::Basic.has_color());
    assert!(ColorLevel::Ansi256.has_color());
    assert!(ColorLevel::TrueColor.has_color());
}

#[test]
fn color_level_has_256_gates() {
    assert!(!ColorLevel::None.has_256());
    assert!(!ColorLevel::Basic.has_256());
    assert!(ColorLevel::Ansi256.has_256());
    assert!(ColorLevel::TrueColor.has_256());
}

#[test]
fn color_level_has_truecolor_gates() {
    assert!(!ColorLevel::None.has_truecolor());
    assert!(!ColorLevel::Basic.has_truecolor());
    assert!(!ColorLevel::Ansi256.has_truecolor());
    assert!(ColorLevel::TrueColor.has_truecolor());
}

#[test]
fn color_level_as_str_round_trips() {
    assert_eq!(ColorLevel::None.as_str(), "none");
    assert_eq!(ColorLevel::Basic.as_str(), "basic");
    assert_eq!(ColorLevel::Ansi256.as_str(), "256");
    assert_eq!(ColorLevel::TrueColor.as_str(), "truecolor");
}

#[test]
fn color_level_from_str_ci_resolves_known_spellings() {
    assert_eq!(ColorLevel::from_str_ci("none"), Some(ColorLevel::None));
    assert_eq!(ColorLevel::from_str_ci("mono"), Some(ColorLevel::None));
    assert_eq!(ColorLevel::from_str_ci("basic"), Some(ColorLevel::Basic));
    assert_eq!(ColorLevel::from_str_ci("16"), Some(ColorLevel::Basic));
    assert_eq!(ColorLevel::from_str_ci("256"), Some(ColorLevel::Ansi256));
    assert_eq!(
        ColorLevel::from_str_ci("truecolor"),
        Some(ColorLevel::TrueColor)
    );
    assert_eq!(
        ColorLevel::from_str_ci("TRUECOLOR"),
        Some(ColorLevel::TrueColor)
    );
    assert_eq!(ColorLevel::from_str_ci("unknown"), None);
}

#[test]
fn color_level_display_matches_as_str() {
    assert_eq!(format!("{}", ColorLevel::None), "none");
    assert_eq!(format!("{}", ColorLevel::Basic), "basic");
    assert_eq!(format!("{}", ColorLevel::Ansi256), "256");
    assert_eq!(format!("{}", ColorLevel::TrueColor), "truecolor");
}

// ---------------------------------------------------------------------------
// Group 2 — quantize_color at each level
// ---------------------------------------------------------------------------

#[test]
fn quantize_truecolor_passes_through_rgb() {
    let c = Color::Rgb(122, 162, 247);
    assert_eq!(quantize_color(c, ColorLevel::TrueColor), c);
}

#[test]
fn quantize_truecolor_passes_through_indexed() {
    let c = Color::Indexed(141);
    assert_eq!(quantize_color(c, ColorLevel::TrueColor), c);
}

#[test]
fn quantize_truecolor_passes_through_named() {
    assert_eq!(
        quantize_color(Color::Red, ColorLevel::TrueColor),
        Color::Red
    );
    assert_eq!(
        quantize_color(Color::Reset, ColorLevel::TrueColor),
        Color::Reset
    );
}

#[test]
fn quantize_ansi256_maps_rgb_to_indexed() {
    let q = quantize_color(Color::Rgb(122, 162, 247), ColorLevel::Ansi256);
    assert!(
        matches!(q, Color::Indexed(_)),
        "expected Indexed, got {q:?}"
    );
}

#[test]
fn quantize_ansi256_passes_indexed_through() {
    assert_eq!(
        quantize_color(Color::Indexed(141), ColorLevel::Ansi256),
        Color::Indexed(141)
    );
}

#[test]
fn quantize_ansi256_passes_named_through() {
    assert_eq!(quantize_color(Color::Red, ColorLevel::Ansi256), Color::Red);
}

#[test]
fn quantize_basic_maps_rgb_to_named_ansi16() {
    let q = quantize_color(Color::Rgb(255, 0, 0), ColorLevel::Basic);
    assert!(
        matches!(q, Color::Red | Color::LightRed),
        "expected Red/LightRed, got {q:?}"
    );
}

#[test]
fn quantize_basic_maps_indexed_to_named_ansi16() {
    let q = quantize_color(Color::Indexed(196), ColorLevel::Basic);
    assert!(
        matches!(q, Color::Red | Color::LightRed),
        "expected Red/LightRed, got {q:?}"
    );
}

#[test]
fn quantize_basic_passes_named_through() {
    assert_eq!(quantize_color(Color::Red, ColorLevel::Basic), Color::Red);
    assert_eq!(quantize_color(Color::Blue, ColorLevel::Basic), Color::Blue);
}

#[test]
fn quantize_none_resets_everything() {
    assert_eq!(
        quantize_color(Color::Rgb(100, 200, 50), ColorLevel::None),
        Color::Reset
    );
    assert_eq!(
        quantize_color(Color::Indexed(111), ColorLevel::None),
        Color::Reset
    );
    assert_eq!(quantize_color(Color::Red, ColorLevel::None), Color::Reset);
}

// ---------------------------------------------------------------------------
// Group 3 — indexed_to_rgb and nearest_indexed
// ---------------------------------------------------------------------------

#[test]
fn indexed_to_rgb_handles_ansi16() {
    assert_eq!(indexed_to_rgb(0), (0, 0, 0));
    assert_eq!(indexed_to_rgb(1), (128, 0, 0));
    assert_eq!(indexed_to_rgb(9), (255, 0, 0));
    assert_eq!(indexed_to_rgb(15), (255, 255, 255));
}

#[test]
fn indexed_to_rgb_handles_color_cube() {
    // Index 16 = first cube cell = (0, 0, 0)
    assert_eq!(indexed_to_rgb(16), (0, 0, 0));
    // Index 21 = (0, 0, 255)
    assert_eq!(indexed_to_rgb(21), (0, 0, 255));
    // Index 196 = (255, 0, 0) in the cube
    let (r, g, b) = indexed_to_rgb(196);
    assert_eq!(r, 255);
    assert_eq!(g, 0);
    assert_eq!(b, 0);
}

#[test]
fn indexed_to_rgb_handles_grayscale_ramp() {
    // Index 232 = first gray = 8
    assert_eq!(indexed_to_rgb(232), (8, 8, 8));
    // Index 255 = last gray = 238
    assert_eq!(indexed_to_rgb(255), (238, 238, 238));
}

#[test]
fn nearest_indexed_finds_closest_cube_entry() {
    // Pure red should map to a cube cell with red=255
    let idx = nearest_indexed(255, 0, 0);
    let (r, _g, _b) = indexed_to_rgb(idx);
    assert_eq!(r, 255);
}

#[test]
fn nearest_indexed_finds_grayscale_for_gray_input() {
    // Mid-gray should be closer to the grayscale ramp than the cube
    let idx = nearest_indexed(128, 128, 128);
    assert!(
        idx >= 232,
        "gray input should map to grayscale ramp, got {idx}"
    );
}

// ---------------------------------------------------------------------------
// Group 4 — resolve_to_rgb
// ---------------------------------------------------------------------------

#[test]
fn resolve_to_rgb_handles_rgb() {
    assert_eq!(resolve_to_rgb(Color::Rgb(12, 34, 56)), Some((12, 34, 56)));
}

#[test]
fn resolve_to_rgb_handles_indexed() {
    assert_eq!(resolve_to_rgb(Color::Indexed(16)), Some(indexed_to_rgb(16)));
}

#[test]
fn resolve_to_rgb_handles_named() {
    assert_eq!(resolve_to_rgb(Color::Black), Some((0, 0, 0)));
    assert_eq!(resolve_to_rgb(Color::Red), Some((128, 0, 0)));
    assert_eq!(resolve_to_rgb(Color::White), Some((255, 255, 255)));
}

#[test]
fn resolve_to_rgb_returns_none_for_reset() {
    assert_eq!(resolve_to_rgb(Color::Reset), None);
}

// ---------------------------------------------------------------------------
// Group 5 — Theme::quantized() at each level
// ---------------------------------------------------------------------------

#[test]
fn quantized_truecolor_is_identity() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::TrueColor);
    assert_eq!(q.surface.canvas, theme.surface.canvas);
    assert_eq!(q.text.primary, theme.text.primary);
    assert_eq!(q.status.success, theme.status.success);
    assert_eq!(q.border.subtle, theme.border.subtle);
}

#[test]
fn quantized_ansi256_maps_rgb_to_indexed() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::Ansi256);
    assert!(
        matches!(q.surface.canvas, Color::Indexed(_)),
        "canvas should be Indexed after Ansi256 quantization, got {:?}",
        q.surface.canvas
    );
    assert!(
        matches!(q.text.primary, Color::Indexed(_)),
        "text.primary should be Indexed after Ansi256 quantization"
    );
    assert!(
        matches!(q.status.success, Color::Indexed(_)),
        "status.success should be Indexed after Ansi256 quantization"
    );
}

#[test]
fn quantized_basic_maps_rgb_to_named_ansi16() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::Basic);
    // All RGB colors should become named ANSI colors
    assert!(
        !matches!(q.surface.canvas, Color::Rgb(..) | Color::Indexed(_)),
        "canvas should be named ANSI after Basic quantization, got {:?}",
        q.surface.canvas
    );
    assert!(
        !matches!(q.text.primary, Color::Rgb(..) | Color::Indexed(_)),
        "text.primary should be named ANSI after Basic quantization"
    );
}

#[test]
fn quantized_none_strips_all_color() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::None);
    assert_eq!(q.surface.canvas, Color::Reset);
    assert_eq!(q.text.primary, Color::Reset);
    assert_eq!(q.status.success, Color::Reset);
    assert_eq!(q.border.subtle, Color::Reset);
    assert_eq!(q.scrollbar.thumb, Color::Reset);
}

#[test]
fn quantized_preserves_live_shell_geometry() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::Ansi256);
    assert_eq!(q.live_shell.heights.header, theme.live_shell.heights.header);
    assert_eq!(
        q.live_shell.rhythm.composer_padding_x,
        theme.live_shell.rhythm.composer_padding_x
    );
}

#[test]
fn quantized_preserves_glyph_catalog() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::None);
    assert_eq!(
        q.live_shell.glyphs.streaming,
        theme.live_shell.glyphs.streaming
    );
    assert_eq!(
        q.live_shell.transcript_glyphs.user_marker,
        theme.live_shell.transcript_glyphs.user_marker
    );
}

// ---------------------------------------------------------------------------
// Group 6 — Theme::terminal_native()
// ---------------------------------------------------------------------------

#[test]
fn terminal_native_uses_only_reset_and_named_ansi() {
    let theme = Theme::terminal_native();

    // Check all surface colors
    for color in [
        theme.surface.canvas,
        theme.surface.shell,
        theme.surface.panel,
        theme.surface.panel_elevated,
        theme.surface.overlay,
        theme.surface.card,
    ] {
        assert!(
            !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
            "terminal_native surface must not use RGB/Indexed, got {color:?}"
        );
    }

    // Check all border colors
    for color in [theme.border.subtle, theme.border.strong, theme.border.focus] {
        assert!(
            !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
            "terminal_native border must not use RGB/Indexed, got {color:?}"
        );
    }

    // Check all text colors
    for color in [
        theme.text.primary,
        theme.text.secondary,
        theme.text.tertiary,
        theme.text.inverse,
    ] {
        assert!(
            !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
            "terminal_native text must not use RGB/Indexed, got {color:?}"
        );
    }
}

#[test]
fn terminal_native_backgrounds_are_reset() {
    let theme = Theme::terminal_native();
    assert_eq!(theme.surface.canvas, Color::Reset);
    assert_eq!(theme.surface.shell, Color::Reset);
    assert_eq!(theme.surface.panel, Color::Reset);
    assert_eq!(theme.surface.overlay, Color::Reset);
}

#[test]
fn terminal_native_primary_text_is_reset() {
    let theme = Theme::terminal_native();
    assert_eq!(theme.text.primary, Color::Reset);
    assert_eq!(theme.text.secondary, Color::Reset);
}

#[test]
fn terminal_native_uses_named_ansi_for_status_accents() {
    let theme = Theme::terminal_native();
    // Status accents should be named ANSI, not Reset
    assert_ne!(theme.status.success, Color::Reset);
    assert_ne!(theme.status.error, Color::Reset);
    assert_ne!(theme.status.warning, Color::Reset);
    assert_ne!(theme.status.info, Color::Reset);
    // And should not be RGB/Indexed
    assert!(!matches!(
        theme.status.success,
        Color::Rgb(..) | Color::Indexed(_)
    ));
    assert!(!matches!(
        theme.status.error,
        Color::Rgb(..) | Color::Indexed(_)
    ));
}

#[test]
fn terminal_native_survives_quantization_at_all_levels() {
    let theme = Theme::terminal_native();
    for level in [
        ColorLevel::Basic,
        ColorLevel::Ansi256,
        ColorLevel::TrueColor,
    ] {
        let q = theme.quantized(level);
        // Reset and named ANSI colors pass through quantization unchanged
        assert_eq!(q.surface.canvas, theme.surface.canvas);
        assert_eq!(q.text.primary, theme.text.primary);
        assert_eq!(q.status.success, theme.status.success);
    }
    // None strips everything to Reset
    let q = theme.quantized(ColorLevel::None);
    assert_eq!(q.surface.canvas, Color::Reset);
    assert_eq!(q.status.success, Color::Reset);
}

#[test]
fn terminal_native_resolvable_by_name() {
    assert_eq!(
        Theme::by_name("terminal-native"),
        Some(Theme::terminal_native())
    );
}

// ---------------------------------------------------------------------------
// Group 7 — Theme::ansi16_chrome_overrides()
// ---------------------------------------------------------------------------

#[test]
fn ansi16_overrides_dark_uses_bright_variants() {
    let theme = Theme::harness_dark();
    let t = theme
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(true);

    // Dark canvas: bright variants for semantic accents
    assert_eq!(t.status.error, Color::LightRed);
    assert_eq!(t.status.success, Color::LightGreen);
    assert_eq!(t.status.warning, Color::LightYellow);
}

#[test]
fn ansi16_overrides_light_uses_normal_variants() {
    let theme = Theme::harness_light();
    let t = theme
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(false);

    // Light canvas: normal variants for semantic accents
    assert_eq!(t.status.error, Color::Red);
    assert_eq!(t.status.success, Color::Green);
    assert_eq!(t.status.warning, Color::Yellow);
}

#[test]
fn ansi16_overrides_dark_elevated_bg_is_dark_gray() {
    let t = Theme::harness_dark()
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(true);
    assert_eq!(t.surface.panel_elevated, Color::DarkGray);
    assert_eq!(t.surface.overlay, Color::DarkGray);
}

#[test]
fn ansi16_overrides_light_elevated_bg_is_gray() {
    let t = Theme::harness_light()
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(false);
    assert_eq!(t.surface.panel_elevated, Color::Gray);
    assert_eq!(t.surface.overlay, Color::Gray);
}

#[test]
fn ansi16_overrides_dark_border_hierarchy_is_distinct() {
    let t = Theme::harness_dark()
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(true);
    // Border hierarchy: dim → muted → high-contrast
    assert_eq!(t.border.subtle, Color::DarkGray);
    assert_eq!(t.border.strong, Color::Gray);
    assert_eq!(t.border.focus, Color::White);
    assert_ne!(t.border.subtle, t.border.focus);
}

#[test]
fn ansi16_overrides_preserves_live_shell_geometry() {
    let t = Theme::harness_dark()
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(true);
    let original = Theme::harness_dark();
    assert_eq!(
        t.live_shell.heights.header,
        original.live_shell.heights.header
    );
    assert_eq!(
        t.live_shell.rhythm.composer_padding_x,
        original.live_shell.rhythm.composer_padding_x
    );
}

#[test]
fn ansi16_overrides_scrollbar_thumb_visible_against_track() {
    let t = Theme::harness_dark()
        .quantized(ColorLevel::Basic)
        .ansi16_chrome_overrides(true);
    assert_ne!(t.scrollbar.thumb, t.scrollbar.track);
}

// ---------------------------------------------------------------------------
// Group 8 — Theme::for_color_level()
// ---------------------------------------------------------------------------

#[test]
fn for_color_level_truecolor_returns_rgb_theme() {
    let theme = Theme::harness_dark();
    let adapted = theme.for_color_level(ColorLevel::TrueColor);
    assert_eq!(adapted.surface.canvas, theme.surface.canvas);
    assert_eq!(adapted.text.primary, theme.text.primary);
}

#[test]
fn for_color_level_ansi256_returns_indexed_theme() {
    let adapted = Theme::harness_dark().for_color_level(ColorLevel::Ansi256);
    assert!(
        matches!(adapted.surface.canvas, Color::Indexed(_)),
        "Ansi256 adaptation should produce Indexed colors"
    );
}

#[test]
fn for_color_level_basic_returns_ansi16_theme() {
    let adapted = Theme::harness_dark().for_color_level(ColorLevel::Basic);
    // Basic adaptation applies ansi16_chrome_overrides
    assert!(
        !matches!(adapted.surface.canvas, Color::Rgb(..) | Color::Indexed(_)),
        "Basic adaptation should produce named ANSI colors"
    );
    assert_eq!(adapted.status.error, Color::LightRed); // dark theme → bright
}

#[test]
fn for_color_level_none_returns_terminal_native() {
    let adapted = Theme::harness_dark().for_color_level(ColorLevel::None);
    assert_eq!(adapted.surface.canvas, Color::Reset);
    assert_eq!(adapted.text.primary, Color::Reset);
    // Status accents should be named ANSI from terminal_native
    assert_eq!(adapted.status.success, Color::Green);
    assert_eq!(adapted.status.error, Color::Red);
}

// ---------------------------------------------------------------------------
// Group 9 — detect_color_level()
// ---------------------------------------------------------------------------

#[test]
fn detect_color_level_no_color_returns_none() {
    assert_eq!(
        detect_color_level(Some("1"), Some("xterm-256color"), Some("xterm-256color")),
        ColorLevel::None
    );
}

#[test]
fn detect_color_level_truecolor_returns_truecolor() {
    assert_eq!(
        detect_color_level(None, Some("truecolor"), Some("xterm-256color")),
        ColorLevel::TrueColor
    );
    assert_eq!(
        detect_color_level(None, Some("24bit"), Some("xterm-256color")),
        ColorLevel::TrueColor
    );
}

#[test]
fn detect_color_level_dumb_returns_basic() {
    assert_eq!(
        detect_color_level(None, None, Some("dumb")),
        ColorLevel::Basic
    );
}

#[test]
fn detect_color_level_xterm_returns_ansi256() {
    assert_eq!(
        detect_color_level(None, None, Some("xterm-256color")),
        ColorLevel::Ansi256
    );
}

#[test]
fn detect_color_level_no_env_returns_basic() {
    assert_eq!(detect_color_level(None, None, None), ColorLevel::Basic);
}

// ---------------------------------------------------------------------------
// Group 10 — Background clearing behavior
// ---------------------------------------------------------------------------

#[test]
fn terminal_native_clears_background_with_reset() {
    // The reference binary's Core-8 frames emit only SGR 49 (default bg)
    // and SGR 39 (default fg), never 48;2 or 38;2 RGB sequences.
    // terminal_native must use Color::Reset for all background surfaces.
    let theme = Theme::terminal_native();
    assert_eq!(theme.surface.canvas, Color::Reset);
    assert_eq!(theme.surface.shell, Color::Reset);
    assert_eq!(theme.surface.panel, Color::Reset);
    assert_eq!(theme.surface.overlay, Color::Reset);
    assert_eq!(theme.scrollbar.track, Color::Reset);
}

#[test]
fn rgb_theme_emits_explicit_backgrounds() {
    // The harness_dark theme uses explicit RGB for backgrounds.
    // This is correct for truecolor terminals; for lower capabilities
    // the theme should be quantized or terminal_native should be used.
    let theme = Theme::harness_dark();
    assert!(
        matches!(theme.surface.canvas, Color::Rgb(..)),
        "harness_dark canvas should be RGB for truecolor"
    );
}

#[test]
fn quantized_none_clears_all_backgrounds_to_reset() {
    let theme = Theme::harness_dark();
    let q = theme.quantized(ColorLevel::None);
    assert_eq!(q.surface.canvas, Color::Reset);
    assert_eq!(q.surface.shell, Color::Reset);
    assert_eq!(q.surface.panel, Color::Reset);
    assert_eq!(q.surface.overlay, Color::Reset);
}

// ---------------------------------------------------------------------------
// Group 11 — Theme::is_dark()
// ---------------------------------------------------------------------------

#[test]
fn harness_dark_is_dark() {
    assert!(Theme::harness_dark().is_dark());
}

#[test]
fn harness_light_is_not_dark() {
    assert!(!Theme::harness_light().is_dark());
}

#[test]
fn terminal_native_defaults_to_dark() {
    // Reset/named colors fall back to "dark" (the default polarity)
    assert!(Theme::terminal_native().is_dark());
}

// ---------------------------------------------------------------------------
// Group 12 — Adversarial: mutations detected
// ---------------------------------------------------------------------------

#[test]
fn adversarial_rgb_mutation_detected_by_quantize_ansi256() {
    // If we mutate one RGB channel, the quantized indexed value should change
    let original = Color::Rgb(0x0B, 0x0E, 0x14);
    let mutated = Color::Rgb(0xFF, 0x0E, 0x14);
    let q_orig = quantize_color(original, ColorLevel::Ansi256);
    let q_mut = quantize_color(mutated, ColorLevel::Ansi256);
    assert_ne!(
        q_orig, q_mut,
        "RGB mutation must produce a different Ansi256 quantized value"
    );
}

#[test]
fn adversarial_rgb_mutation_detected_by_quantize_basic() {
    let original = Color::Rgb(0x7F, 0xD8, 0x8F); // status.success
    let mutated = Color::Rgb(0xFF, 0x00, 0x00); // pure red
    let q_orig = quantize_color(original, ColorLevel::Basic);
    let q_mut = quantize_color(mutated, ColorLevel::Basic);
    assert_ne!(
        q_orig, q_mut,
        "RGB mutation must produce a different Basic quantized value"
    );
}

#[test]
fn adversarial_theme_color_mutation_detected_by_comparison() {
    // Mutating one theme color field should be detectable
    let theme = Theme::harness_dark();
    let mut mutated = theme;
    mutated.status.success = Color::Rgb(0xFF, 0x00, 0x00); // wrong success color
    assert_ne!(
        theme.status.success, mutated.status.success,
        "status.success mutation must be detectable"
    );
    assert_ne!(
        theme.quantized(ColorLevel::Ansi256).status.success,
        mutated.quantized(ColorLevel::Ansi256).status.success,
        "mutation must propagate through quantization"
    );
}

#[test]
fn adversarial_border_color_mutation_detected() {
    let theme = Theme::harness_dark();
    let mut mutated = theme;
    mutated.border.subtle = Color::Rgb(0xFF, 0xFF, 0xFF); // wrong border
    assert_ne!(
        theme.border_style(harness_tui::theme::DividerIntensity::Subtle),
        mutated.border_style(harness_tui::theme::DividerIntensity::Subtle),
        "border color mutation must change border_style output"
    );
}

#[test]
fn adversarial_background_mutation_detected() {
    let theme = Theme::harness_dark();
    let mut mutated = theme;
    mutated.surface.shell = Color::Reset; // wrong shell for RGB theme
    assert_ne!(
        theme.chrome_style(harness_tui::theme::ChromeMode::Chromeless),
        mutated.chrome_style(harness_tui::theme::ChromeMode::Chromeless),
        "shell mutation must change chrome_style output"
    );
}
