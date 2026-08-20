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
