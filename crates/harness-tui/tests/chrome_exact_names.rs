const UI_CHROME: &str = include_str!("../src/ui_chrome.rs");
const UI_LIFECYCLE: &str = include_str!("../src/ui_lifecycle.rs");
const UI_SECONDARY: &str = include_str!("../src/ui_secondary.rs");
const UI_OVERLAYS: &str = include_str!("../src/ui_overlays.rs");
const UI_RS: &str = include_str!("../src/ui.rs");

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn assert_no_ad_hoc_borders(file: &str) {
    for raw_border in [
        "Borders::ALL",
        "Borders::TOP | Borders::BOTTOM",
        "Borders::TOP | Borders::LEFT | Borders::RIGHT",
    ] {
        assert!(
            !file.contains(raw_border),
            "expected helper-routed chrome, found raw border contract {raw_border}"
        );
    }
}

#[test]
fn chrome_helper_inventory_is_exhaustive() {
    assert!(UI_CHROME.contains("pub(super) enum ChromeFrame"));
    assert!(UI_CHROME.contains("pub(super) fn chromeless_shell_section"));
    assert!(UI_CHROME.contains("pub(super) fn divided_shell_section"));
    assert!(UI_CHROME.contains("pub(super) fn secondary_pane_block"));
    assert!(UI_CHROME.contains("pub(super) fn elevated_card_block"));
    assert!(UI_CHROME.contains("pub(super) fn interruptive_modal_block"));

    assert!(count_occurrences(UI_SECONDARY, "ui_chrome::secondary_pane_block(") >= 1);
    assert!(
        UI_SECONDARY.contains("Block::default().style(Style::default().bg(theme.surface.shell))")
    );
    assert!(count_occurrences(UI_OVERLAYS, "render_overlay_surface(") >= 2);
    assert_eq!(
        count_occurrences(UI_OVERLAYS, "ui_chrome::interruptive_modal_block("),
        1
    );
    assert_eq!(
        count_occurrences(UI_OVERLAYS, "ui_chrome::elevated_card_block("),
        1
    );
    assert!(count_occurrences(UI_RS, "chromeless_shell_section(theme)") >= 3);
    assert!(count_occurrences(UI_RS, "interruptive_modal_block(") >= 1);

    for file in [UI_SECONDARY, UI_OVERLAYS, UI_RS] {
        assert_no_ad_hoc_borders(file);
    }
}

#[test]
fn primary_surfaces_use_semantic_chrome_helpers() {
    assert!(UI_LIFECYCLE.contains("crate::layout::startup_shell_area(area, theme)"));
    assert!(UI_LIFECYCLE.contains("render_lifecycle_copy_line("));
    assert!(UI_LIFECYCLE.contains("&startup_card.metadata"));
    assert!(!UI_LIFECYCLE.contains("ui_chrome::elevated_card_block("));
    assert!(!UI_LIFECYCLE.contains("ui_chrome::divided_shell_section("));

    assert!(UI_SECONDARY.contains("ui_chrome::divided_shell_surface(theme)"));
    assert!(UI_SECONDARY
        .contains("ui_chrome::secondary_pane_block(theme, Line::default(), is_focused, surface)"));
    assert!(
        UI_SECONDARY.contains("Block::default().style(Style::default().bg(theme.surface.shell))")
    );

    assert!(UI_RS.contains("chromeless_shell_section(theme)"));
    assert!(UI_RS.contains("let surface = elevated_card_surface(theme);"));
    assert!(UI_RS.contains("let block = interruptive_modal_block("));
}

#[test]
fn interruptive_overlays_keep_elevated_card_contract() {
    assert_eq!(
        count_occurrences(UI_OVERLAYS, "render_quiet_overlay_card("),
        0
    );
    assert!(
        count_occurrences(UI_OVERLAYS, "render_overlay_surface(") >= 2,
        "command palette/session history and the shared helper should route through the compatibility overlay surface helper"
    );
    assert_eq!(
        count_occurrences(UI_OVERLAYS, "ui_chrome::interruptive_modal_block("),
        1,
        "permission modal should stay on the elevated interruptive contract"
    );
    assert_eq!(
        count_occurrences(UI_OVERLAYS, "ui_chrome::elevated_card_block("),
        1
    );
    assert_eq!(
        count_occurrences(UI_RS, "interruptive_modal_block("),
        1,
        "runtime overlay should stay on the elevated interruptive contract"
    );
    assert!(!UI_OVERLAYS.contains("ui_chrome::quiet_overlay_title("));
    assert!(UI_OVERLAYS.contains("theme.surface.overlay"));
    assert!(!UI_OVERLAYS.contains("ui_chrome::quiet_overlay_block("));
    assert!(!UI_OVERLAYS.contains("ui_chrome::secondary_pane_block("));
    assert!(!UI_OVERLAYS.contains("ui_chrome::divided_shell_section("));
}
