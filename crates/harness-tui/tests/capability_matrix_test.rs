use harness_tui::capability_matrix::*;
use harness_tui::theme::ColorLevel;

#[test]
fn axes_have_stable_labels_and_mappings() {
    assert_eq!(ColorCapability::TrueColor.label(), "true_color");
    assert_eq!(
        ColorCapability::Ansi256.to_color_level(),
        ColorLevel::Ansi256
    );
    assert_eq!(ColorCapability::Basic16.to_color_level(), ColorLevel::Basic);
    assert_eq!(ColorCapability::NoColor.to_color_level(), ColorLevel::None);
    assert!(GraphicsCapability::Kitty.supports_inline());
    assert!(GraphicsCapability::ITerm2.supports_inline());
    assert!(GraphicsCapability::Sixel.supports_inline());
    assert!(WidthCapability::Unicode11.handles_cjk());
    assert!(WidthCapability::Unicode9.handles_cjk());
    assert_eq!(ViewportCapability::all().len(), 7);
    assert_eq!(ViewportCapability::Default80x24.dimensions(), (80, 24));
    assert_eq!(ViewportCapability::Maximum200x60.dimensions(), (200, 60));
}

#[test]
fn known_profiles_classify_expected_capabilities() {
    let profiles = harness_tui::capability_matrix::well_known_profiles();
    let wez = &profiles[0].1;
    assert_eq!(wez.color(), ColorCapability::TrueColor);
    assert_eq!(wez.graphics(), GraphicsCapability::Kitty);
    assert_eq!(wez.keyboard(), KeyboardCapability::Modern);
    assert_eq!(wez.focus(), FocusCapability::Reported);
    assert_eq!(wez.notification(), NotificationCapability::Osc99);
    assert_eq!(wez.clipboard(), ClipboardCapability::Osc52);
    assert_eq!(wez.title(), TitleCapability::Supported);
    let kitty = &profiles[1].1;
    assert_eq!(kitty.color(), ColorCapability::TrueColor);
    assert_eq!(kitty.graphics(), GraphicsCapability::Kitty);
    assert_eq!(kitty.keyboard(), KeyboardCapability::Modern);
    assert_eq!(kitty.focus(), FocusCapability::Reported);
    assert_eq!(kitty.notification(), NotificationCapability::Bell);
    let xterm = &profiles[3].1;
    assert_eq!(xterm.color(), ColorCapability::Ansi256);
    assert_eq!(xterm.graphics(), GraphicsCapability::None);
    assert_eq!(xterm.keyboard(), KeyboardCapability::Legacy);
    assert_eq!(xterm.focus(), FocusCapability::Unknown);
    let dumb = &profiles[4].1;
    assert_eq!(dumb.color(), ColorCapability::NoColor);
    assert_eq!(dumb.graphics(), GraphicsCapability::None);
    assert_eq!(dumb.keyboard(), KeyboardCapability::Minimal);
    assert_eq!(dumb.focus(), FocusCapability::Unknown);
    assert_eq!(dumb.clipboard(), ClipboardCapability::None);
    assert_eq!(dumb.title(), TitleCapability::Unsupported);
    let tmux = &profiles[5].1;
    assert_eq!(tmux.multiplexer(), MultiplexerCapability::Tmux);
    assert_eq!(tmux.notification(), NotificationCapability::Osc9);
}

#[test]
fn matrix_contains_classified_cell_for_every_viewport() {
    let classifier = harness_tui::capability_matrix::well_known_profiles()
        .remove(0)
        .1;
    let matrix = CapabilityMatrix::new(classifier);
    assert_eq!(matrix.len(), 7);
    assert!(!matrix.is_empty());
    assert!(matrix
        .for_viewport(ViewportCapability::Default80x24)
        .is_some());
    assert!(matrix.all_classified());
    assert!(matrix.unclassified_combinations().is_empty());
    assert!(matrix.cells().iter().all(|cell| !cell.label().is_empty()));
}
