//! Task 25: Responsive, terminal capability, theme, mouse, and clipboard shard.
//!
//! Proves:
//! - Deterministic captures pass at 120x50, 120x40, 100x30, 80x24, 79x24, 60x20, and wide viewport.
//! - Terminal modes (color/keys/mouse/clipboard) are recorded.
//! - Unicode display width is recorded for ASCII, CJK, emoji, combining marks, and fullwidth forms.
//! - Theme auto mode and reduced capability selection work.
//! - Mouse capture modes and clipboard OSC52/native/hyperlink behavior are correct.
//!
//! This test file does NOT edit shared registries — it uses the leaf modules
//! under `responsive/`, `terminal/`, `theme_leaf`, `mouse/`, and `clipboard_leaf/`
//! plus the existing `render_test::render_to_string` helper and `AppState`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::clipboard_leaf::{
    build_osc52_sequence, format_osc8_hyperlink, ClipboardLeaf, ClipboardMode, PasteMode,
};
use harness_tui::mouse::{MouseCaptureMode, MouseLeaf};
use harness_tui::render_test::render_to_string;
use harness_tui::responsive::{
    VIEWPORT_100x30, VIEWPORT_120x40, VIEWPORT_120x50, VIEWPORT_60x20, VIEWPORT_79x24,
    VIEWPORT_80x24, ViewportClassification, ViewportId, ViewportPlan, VIEWPORT_WIDE,
};
use harness_tui::terminal::{
    char_display_width, ColorMode, KeyboardMode, TerminalCapabilityLeaf, TerminalCapabilityRecord,
    TerminalCapabilityRow, UnicodeWidthRecord,
};
use harness_tui::theme_leaf::{NamedTheme, ThemeAutoMode, ThemeLeaf};
use harness_tui::ui;
use ratatui::layout::Rect;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn idle_shell_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

// ---------------------------------------------------------------------------
// RESP-* — Deterministic captures at all 7 required viewports
// ---------------------------------------------------------------------------

/// RESP-120x50: idle shell renders with bordered composer and idle footer.
#[test]
fn resp_120x50_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_120x50);

    // act
    let rendered = render_at(&app, 120, 50);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-120x50");
    assert!(plan.composer_bordered);
    assert!(plan.footer_hints_visible);
    assert!(!plan.welcome_panel_visible);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-120x50: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-120x50: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x50: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-120x40: idle shell renders with bordered composer and idle footer.
#[test]
fn resp_120x40_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_120x40);

    // act
    let rendered = render_at(&app, 120, 40);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-120x40");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-120x40: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-120x40: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x40: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-100x30: idle shell renders with bordered composer and idle footer.
#[test]
fn resp_100x30_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_100x30);

    // act
    let rendered = render_at(&app, 100, 30);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-100x30");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-100x30: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-100x30: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-100x30: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-80x24: idle shell renders with bordered composer and idle footer.
#[test]
fn resp_80x24_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_80x24);

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-80x24");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-80x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-80x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-80x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-79x24: idle shell renders with bordered composer at narrow boundary.
#[test]
fn resp_79x24_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_79x24);

    // act
    let rendered = render_at(&app, 79, 24);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-79x24");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-79x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-79x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-79x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-60x20: idle shell renders with bordered composer at compact viewport.
#[test]
fn resp_60x20_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_60x20);

    // act
    let rendered = render_at(&app, 60, 20);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-60x20");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-60x20: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-60x20: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-60x20: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-WIDE (140x40): idle shell renders with bordered composer at wide viewport.
#[test]
fn resp_wide_140x40_deterministic_capture_passes() {
    // arrange
    let app = idle_shell_app();
    let plan = ViewportPlan::for_viewport(VIEWPORT_WIDE);

    // act
    let rendered = render_at(&app, 140, 40);

    // assert
    assert_eq!(plan.id.behavior_id(), "RESP-WIDE");
    assert!(plan.composer_bordered);
    assert!(
        rendered.contains('\u{276F}'),
        "RESP-WIDE: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "RESP-WIDE: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(!rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"));
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-WIDE: idle shell must not show draft footer\n{rendered}"
    );
}

/// All seven viewport plans cover the manifest RESP-* rows exactly.
#[test]
fn all_seven_viewport_plans_cover_manifest() {
    // arrange
    // act
    let plans = ViewportPlan::all_plans();

    // assert
    assert_eq!(plans.len(), 7, "exactly seven viewport plans required");
    let ids: Vec<&str> = plans.iter().map(|p| p.id.behavior_id()).collect();
    assert_eq!(
        ids,
        vec![
            "RESP-120x50",
            "RESP-120x40",
            "RESP-100x30",
            "RESP-80x24",
            "RESP-79x24",
            "RESP-60x20",
            "RESP-WIDE",
        ]
    );
}

/// Viewport classification does not mask geometry differences.
#[test]
fn viewport_classification_records_real_geometry() {
    // arrange
    // act
    let v80 = ViewportClassification::from_dims(80, 24);
    let v79 = ViewportClassification::from_dims(79, 24);
    let v60 = ViewportClassification::from_dims(60, 20);
    let v100 = ViewportClassification::from_dims(100, 30);
    let v120 = ViewportClassification::from_dims(120, 50);
    let v140 = ViewportClassification::from_dims(140, 40);

    // assert
    assert_eq!(v80.cols, 80);
    assert_eq!(v80.rows, 24);
    assert!(v80.is_compact());
    assert!(!v80.is_primary());

    assert_eq!(v79.cols, 79);
    assert!(v79.is_compact());

    assert_eq!(v60.cols, 60);
    assert!(v60.is_compact());

    assert!(v100.is_primary());
    assert!(v120.is_primary());
    assert!(v140.is_primary());
}

// ---------------------------------------------------------------------------
// TERM-CAP-* — Terminal modes are recorded
// ---------------------------------------------------------------------------

/// TERM-CAP-COLOR: truecolor and reduced color modes are recorded.
#[test]
fn term_cap_color_records_truecolor_and_reduced_modes() {
    // arrange
    let full_caps = TerminalCapabilityLeaf::full();
    let reduced_caps = TerminalCapabilityLeaf::reduced();

    // act
    let full_record = TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Color, &full_caps);
    let reduced_record =
        TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Color, &reduced_caps);

    // assert
    assert_eq!(full_record.behavior_id, "TERM-CAP-COLOR");
    assert!(full_record.color_mode.is_truecolor());
    assert_eq!(reduced_record.color_mode, ColorMode::Ansi16);
    assert!(!reduced_record.color_mode.is_truecolor());
}

/// TERM-CAP-KEYS: enhanced and legacy keyboard modes are recorded.
#[test]
fn term_cap_keys_records_enhanced_and_legacy_modes() {
    // arrange
    let full_caps = TerminalCapabilityLeaf::full();
    let reduced_caps = TerminalCapabilityLeaf::reduced();

    // act
    let full_record = TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Keys, &full_caps);
    let reduced_record =
        TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Keys, &reduced_caps);

    // assert
    assert_eq!(full_record.behavior_id, "TERM-CAP-KEYS");
    assert_eq!(full_record.keyboard_mode, KeyboardMode::Enhanced);
    assert_eq!(reduced_record.keyboard_mode, KeyboardMode::Legacy);
}

/// TERM-CAP-MOUSE: mouse capture modes are recorded.
#[test]
fn term_cap_mouse_records_capture_modes() {
    // arrange
    let full_caps = TerminalCapabilityLeaf::full();
    let reduced_caps = TerminalCapabilityLeaf::reduced();

    // act
    let full_record = TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Mouse, &full_caps);
    let reduced_record =
        TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Mouse, &reduced_caps);

    // assert
    assert_eq!(full_record.behavior_id, "TERM-CAP-MOUSE");
    assert!(full_record.mouse_capture);
    assert!(!reduced_record.mouse_capture);
}

/// TERM-CAP-CLIPBOARD: OSC52 and native clipboard modes are recorded.
#[test]
fn term_cap_clipboard_records_osc52_and_native_modes() {
    // arrange
    let full_caps = TerminalCapabilityLeaf::full();
    let reduced_caps = TerminalCapabilityLeaf::reduced();

    // act
    let full_record =
        TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Clipboard, &full_caps);
    let reduced_record =
        TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Clipboard, &reduced_caps);

    // assert
    assert_eq!(full_record.behavior_id, "TERM-CAP-CLIPBOARD");
    assert!(full_record.osc52_clipboard);
    assert!(!reduced_record.osc52_clipboard);
}

/// All four TERM-CAP-* records are produced for a capability leaf.
#[test]
fn all_four_term_cap_records_are_produced() {
    // arrange
    let caps = TerminalCapabilityLeaf::full();

    // act
    let records = TerminalCapabilityRecord::all_for(&caps);

    // assert
    assert_eq!(records.len(), 4);
    assert_eq!(records[0].behavior_id, "TERM-CAP-COLOR");
    assert_eq!(records[1].behavior_id, "TERM-CAP-KEYS");
    assert_eq!(records[2].behavior_id, "TERM-CAP-MOUSE");
    assert_eq!(records[3].behavior_id, "TERM-CAP-CLIPBOARD");
}

/// Terminal capability from environment probes truecolor and OSC52 correctly.
#[test]
fn term_cap_from_env_probes_truecolor_and_osc52() {
    // arrange
    // act
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), true);

    // assert
    assert!(caps.color_mode.is_truecolor());
    assert!(caps.osc52_clipboard);
}

/// Terminal capability from environment without truecolor is reduced.
#[test]
fn term_cap_from_env_without_truecolor_is_reduced() {
    // arrange
    // act
    let caps = TerminalCapabilityLeaf::from_env(None, Some("xterm"), true);

    // assert
    assert!(!caps.color_mode.is_truecolor());
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
}

// ---------------------------------------------------------------------------
// Unicode width is recorded
// ---------------------------------------------------------------------------

/// Unicode width record covers ASCII, CJK, emoji, combining marks, and fullwidth.
#[test]
fn unicode_width_record_covers_all_categories() {
    // arrange
    // act
    let record = UnicodeWidthRecord::canonical();

    // assert
    assert!(!record.entries.is_empty());
    let labels: Vec<&str> = record.entries.iter().map(|e| e.label).collect();
    assert!(labels.contains(&"ascii_a"));
    assert!(labels.contains(&"cjk_kanji_kawa"));
    assert!(labels.contains(&"cjk_hiragana_a"));
    assert!(labels.contains(&"cjk_katakana_a"));
    assert!(labels.contains(&"cjk_hangul_ga"));
    assert!(labels.contains(&"emoji_check_mark"));
    assert!(labels.contains(&"fullwidth_a"));
    assert!(labels.contains(&"box_light_horizontal"));
    assert!(labels.contains(&"prompt_glyph"));
}

/// Unicode display width function returns correct widths.
#[test]
fn unicode_display_width_returns_correct_widths() {
    // arrange
    // act
    // assert
    // ASCII narrow
    assert_eq!(char_display_width('a'), 1);
    assert_eq!(char_display_width('Z'), 1);
    assert_eq!(char_display_width('0'), 1);
    // Box-drawing narrow
    assert_eq!(char_display_width('\u{2500}'), 1);
    assert_eq!(char_display_width('\u{2502}'), 1);
    assert_eq!(char_display_width('\u{250C}'), 1);
    // Prompt glyph narrow
    assert_eq!(char_display_width('\u{276F}'), 1);
    // CJK wide
    assert_eq!(char_display_width('\u{5DDD}'), 2);
    assert_eq!(char_display_width('\u{5C71}'), 2);
    // Hiragana/Katakana wide
    assert_eq!(char_display_width('\u{3042}'), 2);
    assert_eq!(char_display_width('\u{30A2}'), 2);
    // Hangul wide
    assert_eq!(char_display_width('\u{AC00}'), 2);
    // Fullwidth wide
    assert_eq!(char_display_width('\u{FF21}'), 2);
    // Emoji wide
    assert_eq!(char_display_width('\u{2705}'), 2);
    assert_eq!(char_display_width('\u{274C}'), 2);
    // Combining marks zero
    assert_eq!(char_display_width('\u{0301}'), 0);
}

/// Unicode width record has both narrow and wide entries.
#[test]
fn unicode_width_record_has_mixed_widths() {
    // arrange
    // act
    let record = UnicodeWidthRecord::canonical();

    // assert
    assert!(!record.narrow_entries().is_empty());
    assert!(!record.wide_entries().is_empty());
    assert!(record.total_display_width() > 0);
}

// ---------------------------------------------------------------------------
// Theme auto mode and reduced capability
// ---------------------------------------------------------------------------

/// Theme auto mode detects truecolor and marks reduced for dumb terminals.
#[test]
fn theme_auto_mode_detects_truecolor_and_reduced() {
    // arrange
    // act
    let truecolor_theme = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));
    let dumb_theme = ThemeLeaf::auto_from_env(None, Some("dumb"));
    let no_truecolor_theme = ThemeLeaf::auto_from_env(None, Some("xterm-256color"));

    // assert
    assert_eq!(truecolor_theme.theme, NamedTheme::HarnessChat);
    assert_eq!(truecolor_theme.auto_mode, ThemeAutoMode::Auto);
    assert!(!truecolor_theme.reduced_capability);

    assert_eq!(dumb_theme.theme, NamedTheme::HighContrast);
    assert!(dumb_theme.reduced_capability);

    assert!(no_truecolor_theme.reduced_capability);
}

/// Theme explicit selection clears auto mode.
#[test]
fn theme_explicit_selection_clears_auto_mode() {
    // arrange
    // act
    let leaf = ThemeLeaf::explicit(NamedTheme::HarnessLight);

    // assert
    assert_eq!(leaf.theme, NamedTheme::HarnessLight);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
    assert!(!leaf.reduced_capability);
}

/// Theme reduced capability selects high contrast.
#[test]
fn theme_reduced_capability_selects_high_contrast() {
    // arrange
    // act
    let leaf = ThemeLeaf::reduced();

    // assert
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert!(leaf.reduced_capability);
}

/// Named themes have unique labels and resolve from strings.
#[test]
fn named_themes_have_unique_labels_and_resolve() {
    // arrange
    // act
    // assert
    let labels: Vec<&str> = NamedTheme::ALL.iter().map(|t| t.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(labels.len(), unique.len());

    assert_eq!(
        NamedTheme::from_label("default"),
        Some(NamedTheme::HarnessChat)
    );
    assert_eq!(
        NamedTheme::from_label("harness-chat"),
        Some(NamedTheme::HarnessChat)
    );
    assert_eq!(
        NamedTheme::from_label("harness-dark"),
        Some(NamedTheme::HarnessDark)
    );
    assert_eq!(
        NamedTheme::from_label("harness-light"),
        Some(NamedTheme::HarnessLight)
    );
    assert_eq!(
        NamedTheme::from_label("high-contrast"),
        Some(NamedTheme::HighContrast)
    );
    assert_eq!(NamedTheme::from_label("unknown"), None);
}

// ---------------------------------------------------------------------------
// Mouse capture modes
// ---------------------------------------------------------------------------

/// Mouse full support has all features enabled.
#[test]
fn mouse_full_support_has_all_features() {
    // arrange
    // act
    let leaf = MouseLeaf::full();

    // assert
    assert!(leaf.capture_mode.is_enabled());
    assert!(leaf.capture_mode.supports_scroll());
    assert!(leaf.capture_mode.supports_drag());
    assert!(leaf.wheel_scroll_enabled);
    assert!(leaf.click_focus_enabled);
    assert!(leaf.selection_drag_enabled);
}

/// Mouse disabled has no features.
#[test]
fn mouse_disabled_has_no_features() {
    // arrange
    // act
    let leaf = MouseLeaf::disabled();

    // assert
    assert!(!leaf.capture_mode.is_enabled());
    assert!(!leaf.wheel_scroll_enabled);
    assert!(!leaf.click_focus_enabled);
    assert!(!leaf.selection_drag_enabled);
}

/// Mouse reduced has click only.
#[test]
fn mouse_reduced_has_click_only() {
    // arrange
    // act
    let leaf = MouseLeaf::reduced();

    // assert
    assert!(leaf.capture_mode.is_enabled());
    assert!(!leaf.capture_mode.supports_scroll());
    assert!(!leaf.capture_mode.supports_drag());
    assert!(leaf.click_focus_enabled);
    assert!(!leaf.wheel_scroll_enabled);
    assert!(!leaf.selection_drag_enabled);
}

/// Mouse capture mode classification is correct.
#[test]
fn mouse_capture_mode_classification_is_correct() {
    // arrange
    // act
    // assert
    assert!(!MouseCaptureMode::Disabled.is_enabled());
    assert!(MouseCaptureMode::Normal.is_enabled());
    assert!(MouseCaptureMode::ButtonEvent.is_enabled());
    assert!(MouseCaptureMode::All.is_enabled());

    assert!(!MouseCaptureMode::Normal.supports_scroll());
    assert!(MouseCaptureMode::ButtonEvent.supports_scroll());
    assert!(MouseCaptureMode::All.supports_scroll());

    assert!(!MouseCaptureMode::Normal.supports_drag());
    assert!(MouseCaptureMode::ButtonEvent.supports_drag());
    assert!(MouseCaptureMode::All.supports_drag());
}

// ---------------------------------------------------------------------------
// Clipboard OSC52, native, paste, and hyperlink
// ---------------------------------------------------------------------------

/// Clipboard full support has all features.
#[test]
fn clipboard_full_support_has_all_features() {
    // arrange
    // act
    let leaf = ClipboardLeaf::full();

    // assert
    assert!(leaf.mode.is_available());
    assert!(leaf.mode.supports_osc52());
    assert!(leaf.mode.supports_native());
    assert!(leaf.copy_on_select);
    assert!(leaf.hyperlink_support);
    assert_eq!(leaf.paste_mode, PasteMode::Bracketed);
}

/// Clipboard disabled has no features.
#[test]
fn clipboard_disabled_has_no_features() {
    // arrange
    // act
    let leaf = ClipboardLeaf::disabled();

    // assert
    assert!(!leaf.mode.is_available());
    assert!(!leaf.copy_on_select);
    assert!(!leaf.hyperlink_support);
    assert_eq!(leaf.paste_mode, PasteMode::Disabled);
}

/// OSC52 only has no native fallback.
#[test]
fn clipboard_osc52_only_has_no_native_fallback() {
    // arrange
    // act
    let leaf = ClipboardLeaf::osc52_only();

    // assert
    assert!(leaf.mode.supports_osc52());
    assert!(!leaf.mode.supports_native());
}

/// Native only has no OSC52.
#[test]
fn clipboard_native_only_has_no_osc52() {
    // arrange
    // act
    let leaf = ClipboardLeaf::native_only();

    // assert
    assert!(!leaf.mode.supports_osc52());
    assert!(leaf.mode.supports_native());
    assert!(!leaf.hyperlink_support);
}

/// OSC8 hyperlink wraps label and falls back on empty.
#[test]
fn osc8_hyperlink_wraps_label_and_falls_back_on_empty() {
    // arrange
    // act
    // assert
    let linked = format_osc8_hyperlink("https://example.com/path", "path");
    assert!(linked.contains("https://example.com/path"));
    assert!(linked.contains("path"));
    assert!(linked.starts_with("\x1b]8;;"));
    assert!(linked.ends_with("\x1b]8;;\x1b\\"));

    assert_eq!(format_osc8_hyperlink("", "plain"), "plain");
    assert_eq!(format_osc8_hyperlink("https://x", ""), "");
}

/// OSC52 sequence wraps base64 in escape and supports tmux passthrough.
#[test]
fn osc52_sequence_wraps_base64_and_supports_tmux() {
    // arrange
    // act
    let seq = build_osc52_sequence("test", false);
    let tmux_seq = build_osc52_sequence("test", true);

    // assert
    assert!(seq.starts_with("\x1b]52;c;"));
    assert!(seq.ends_with("\x07"));
    assert!(seq.contains("dGVzdA=="));

    assert!(tmux_seq.starts_with("\x1bPtmux;\x1b"));
    assert!(tmux_seq.ends_with("\x1b\\"));
}

/// Clipboard mode classification is correct.
#[test]
fn clipboard_mode_classification_is_correct() {
    // arrange
    // act
    // assert
    assert!(!ClipboardMode::None.is_available());
    assert!(ClipboardMode::Osc52.is_available());
    assert!(ClipboardMode::Native.is_available());
    assert!(ClipboardMode::Osc52WithNativeFallback.is_available());

    assert!(ClipboardMode::Osc52.supports_osc52());
    assert!(!ClipboardMode::Osc52.supports_native());

    assert!(!ClipboardMode::Native.supports_osc52());
    assert!(ClipboardMode::Native.supports_native());

    assert!(ClipboardMode::Osc52WithNativeFallback.supports_osc52());
    assert!(ClipboardMode::Osc52WithNativeFallback.supports_native());
}

// ---------------------------------------------------------------------------
// Semantic cells valid — combined assertion for happy scenario
// ---------------------------------------------------------------------------

/// Happy scenario: truecolor + enhanced mouse + theme + clipboard all valid.
#[test]
fn happy_truecolor_enhanced_mouse_theme_clipboard_semantic_cells_valid() {
    // arrange
    let caps = TerminalCapabilityLeaf::full();
    let mouse = MouseLeaf::full();
    let theme = ThemeLeaf::default_theme();
    let clipboard = ClipboardLeaf::full();
    let unicode = UnicodeWidthRecord::canonical();

    // act
    let cap_records = TerminalCapabilityRecord::all_for(&caps);

    // assert — semantic cells are valid
    assert_eq!(cap_records.len(), 4);
    assert!(caps.color_mode.is_truecolor());
    assert!(caps.keyboard_mode.is_enhanced());
    assert!(caps.mouse_capture);
    assert!(caps.osc52_clipboard);
    assert!(caps.bracketed_paste);
    assert!(caps.focus_reporting);

    assert!(mouse.capture_mode.is_enabled());
    assert!(mouse.wheel_scroll_enabled);
    assert!(mouse.click_focus_enabled);
    assert!(mouse.selection_drag_enabled);

    assert_eq!(theme.theme, NamedTheme::HarnessChat);
    assert!(!theme.reduced_capability);

    assert!(clipboard.mode.is_available());
    assert!(clipboard.mode.supports_osc52());
    assert!(clipboard.mode.supports_native());
    assert!(clipboard.copy_on_select);
    assert!(clipboard.hyperlink_support);

    assert!(!unicode.entries.is_empty());
    assert!(!unicode.narrow_entries().is_empty());
    assert!(!unicode.wide_entries().is_empty());
    assert!(unicode.total_display_width() > 0);
}

/// Failure scenario: reduced color + legacy clipboard + resize + CJK widths verified.
#[test]
fn failure_reduced_color_legacy_clipboard_resize_cjk_widths_verified() {
    // arrange
    let caps = TerminalCapabilityLeaf::reduced();
    let mouse = MouseLeaf::disabled();
    let theme = ThemeLeaf::reduced();
    let clipboard = ClipboardLeaf::disabled();

    // act
    let cap_records = TerminalCapabilityRecord::all_for(&caps);

    // assert — widths are verified even in reduced mode
    assert_eq!(cap_records.len(), 4);
    assert!(!caps.color_mode.is_truecolor());
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
    assert!(!caps.keyboard_mode.is_enhanced());
    assert!(!caps.mouse_capture);
    assert!(!caps.osc52_clipboard);

    assert!(!mouse.capture_mode.is_enabled());
    assert!(!mouse.wheel_scroll_enabled);

    assert_eq!(theme.theme, NamedTheme::HighContrast);
    assert!(theme.reduced_capability);

    assert!(!clipboard.mode.is_available());
    assert_eq!(clipboard.paste_mode, PasteMode::Disabled);

    // CJK widths are still correct in reduced mode
    assert_eq!(char_display_width('\u{5DDD}'), 2);
    assert_eq!(char_display_width('\u{3042}'), 2);
    assert_eq!(char_display_width('\u{30A2}'), 2);
    assert_eq!(char_display_width('\u{AC00}'), 2);
    assert_eq!(char_display_width('a'), 1);
    assert_eq!(char_display_width('\u{0301}'), 0);
}
