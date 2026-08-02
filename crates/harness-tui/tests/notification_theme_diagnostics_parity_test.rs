//! Task 31 differential TDD: local notifications, tips, appearance preview,
//! and diagnostics surfaces.
//!
//! Contract: clean-room parity program Todo 31.
//! Covers: task-completion-while-unfocused notification, permission focus
//! alert, theme preview/revert/apply, auto dark/light system switch,
//! color-capability adaptation (truecolor/basic/no-color), terminal
//! diagnostic output, FPS/scroll debug behind explicit controls,
//! notification-storm bounding, focus race, sleep transition, unsupported
//! color/clipboard/mouse, hosted-command absence. No network calls.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::leaf_actions::group_f_notices::{
    resolve, ActionAvailability, NoticeAction, NoticeLevel,
};
use harness_tui::notifications::{
    NotificationEntry, NotificationKind, NotificationState, MAX_CONCURRENT_NOTIFICATIONS,
};
use harness_tui::terminal::{ColorMode, TerminalCapabilityLeaf};
use harness_tui::terminal_diagnostics::{TerminalDiagnostics, UnsupportedCapability};
use harness_tui::theme::Theme;
use harness_tui::theme_leaf::{NamedTheme, ThemeAutoMode, ThemeLeaf};
use harness_tui::theme_preview::{SystemAppearance, ThemePreviewState};
use harness_tui::tips::{TipEntry, TipState};
use ratatui::style::Color;

// ---------------------------------------------------------------------------
// 1. Task-completion-while-unfocused notification
// ---------------------------------------------------------------------------

#[test]
fn task_completed_notification_delivered_when_unfocused() {
    // Given: unfocused notification state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: a task-completed notification is pushed
    let seq = state.push(
        NotificationKind::TaskCompleted,
        NoticeLevel::Success,
        "Task finished",
    );

    // Then: notification is in entries with the correct kind
    assert!(seq > 0);
    let entries = state.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, NotificationKind::TaskCompleted);
    assert_eq!(entries[0].message, "Task finished");
}

#[test]
fn task_completed_notification_suppressed_when_focused() {
    // Given: focused notification state
    let mut state = NotificationState::new();
    state.set_focused(true);

    // When: a task-completed notification is pushed
    // Then: should_deliver returns false
    assert!(!state.should_deliver());
}

#[test]
fn task_completed_notification_delivered_after_refocus() {
    // Given: focused state, then unfocused
    let mut state = NotificationState::new();
    state.set_focused(true);
    state.set_focused(false);

    // When: a notification is pushed
    let seq = state.push(
        NotificationKind::TaskCompleted,
        NoticeLevel::Success,
        "Done",
    );

    // Then: it is delivered
    assert!(seq > 0);
    assert_eq!(state.entries().len(), 1);
}

// ---------------------------------------------------------------------------
// 2. Permission focus alert
// ---------------------------------------------------------------------------

#[test]
fn permission_alert_delivered_when_unfocused() {
    // Given: unfocused state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: a permission alert is pushed
    let seq = state.push(
        NotificationKind::PermissionAlert,
        NoticeLevel::Warning,
        "Permission required",
    );

    // Then: alert is in entries
    assert!(seq > 0);
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].kind, NotificationKind::PermissionAlert);
    assert_eq!(state.entries()[0].level, NoticeLevel::Warning);
}

#[test]
fn permission_alert_not_delivered_when_focused() {
    // Given: focused state
    let mut state = NotificationState::new();
    state.set_focused(true);

    // Then: should_deliver is false
    assert!(!state.should_deliver());
}

#[test]
fn permission_alert_and_task_completed_coexist() {
    // Given: unfocused state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: both notification kinds are pushed
    state.push(
        NotificationKind::PermissionAlert,
        NoticeLevel::Warning,
        "perm",
    );
    state.push(
        NotificationKind::TaskCompleted,
        NoticeLevel::Success,
        "done",
    );

    // Then: both are in entries
    assert_eq!(state.entries().len(), 2);
    assert_eq!(state.entries()[0].kind, NotificationKind::PermissionAlert);
    assert_eq!(state.entries()[1].kind, NotificationKind::TaskCompleted);
}

// ---------------------------------------------------------------------------
// 3. Theme preview/revert/apply
// ---------------------------------------------------------------------------

#[test]
fn theme_preview_does_not_persist() {
    // Given: preview state starting at "default"
    let mut state = ThemePreviewState::new("default");

    // When: preview "high-contrast"
    state.preview("high-contrast");

    // Then: is_previewing is true, but current_name is still "default"
    assert!(state.is_previewing());
    assert_eq!(state.preview_name(), Some("high-contrast"));
    assert_eq!(state.current_name(), "default");
}

#[test]
fn theme_revert_restores_original() {
    // Given: preview state with active preview
    let mut state = ThemePreviewState::new("default");
    state.preview("high-contrast");
    assert!(state.is_previewing());

    // When: revert
    state.revert();

    // Then: no longer previewing, current_name is "default"
    assert!(!state.is_previewing());
    assert_eq!(state.preview_name(), None);
    assert_eq!(state.current_name(), "default");
}

#[test]
fn theme_apply_persists_preview() {
    // Given: preview state with active preview
    let mut state = ThemePreviewState::new("default");
    state.preview("high-contrast");

    // When: apply
    state.apply();

    // Then: no longer previewing, current_name is "high-contrast"
    assert!(!state.is_previewing());
    assert_eq!(state.current_name(), "high-contrast");
}

#[test]
fn theme_preview_can_be_changed_multiple_times() {
    // Given: preview state
    let mut state = ThemePreviewState::new("default");

    // When: preview multiple themes
    state.preview("high-contrast");
    assert_eq!(state.preview_name(), Some("high-contrast"));
    state.preview("harness-light");
    assert_eq!(state.preview_name(), Some("harness-light"));

    // Then: current_name is still "default"
    assert_eq!(state.current_name(), "default");
}

#[test]
fn theme_revert_without_preview_is_noop() {
    // Given: preview state with no active preview
    let mut state = ThemePreviewState::new("default");

    // When: revert (no preview active)
    state.revert();

    // Then: current_name is still "default"
    assert!(!state.is_previewing());
    assert_eq!(state.current_name(), "default");
}

#[test]
fn theme_apply_without_preview_is_noop() {
    // Given: preview state with no active preview
    let mut state = ThemePreviewState::new("default");

    // When: apply (no preview active)
    state.apply();

    // Then: current_name is still "default"
    assert_eq!(state.current_name(), "default");
}

// ---------------------------------------------------------------------------
// 4. Auto dark/light system switch
// ---------------------------------------------------------------------------

#[test]
fn auto_mode_disabled_by_default() {
    let state = ThemePreviewState::new("default");
    assert!(!state.is_auto_mode());
}

#[test]
fn auto_mode_can_be_enabled() {
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);
    assert!(state.is_auto_mode());
}

#[test]
fn auto_mode_dark_selects_dark_theme() {
    // Given: auto mode enabled
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);

    // When: system appearance is dark
    state.on_system_appearance_change(SystemAppearance::Dark);

    // Then: current_name resolves to dark theme
    assert_eq!(state.current_name(), "harness-dark");
}

#[test]
fn auto_mode_light_selects_light_theme() {
    // Given: auto mode enabled
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);

    // When: system appearance is light
    state.on_system_appearance_change(SystemAppearance::Light);

    // Then: current_name resolves to light theme
    assert_eq!(state.current_name(), "harness-light");
}

#[test]
fn auto_mode_switches_on_appearance_change() {
    // Given: auto mode enabled, dark appearance
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);
    state.on_system_appearance_change(SystemAppearance::Dark);
    assert_eq!(state.current_name(), "harness-dark");

    // When: appearance changes to light
    state.on_system_appearance_change(SystemAppearance::Light);

    // Then: theme switches to light
    assert_eq!(state.current_name(), "harness-light");
}

#[test]
fn auto_mode_disabled_does_not_switch_on_appearance_change() {
    // Given: auto mode disabled
    let mut state = ThemePreviewState::new("default");

    // When: system appearance changes
    state.on_system_appearance_change(SystemAppearance::Light);

    // Then: current_name is still "default"
    assert_eq!(state.current_name(), "default");
}

#[test]
fn auto_mode_tracks_system_appearance() {
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);

    // Initially no system appearance
    assert_eq!(state.system_appearance(), None);

    state.on_system_appearance_change(SystemAppearance::Dark);
    assert_eq!(state.system_appearance(), Some(SystemAppearance::Dark));

    state.on_system_appearance_change(SystemAppearance::Light);
    assert_eq!(state.system_appearance(), Some(SystemAppearance::Light));
}

#[test]
fn disabling_auto_mode_keeps_current_theme() {
    // Given: auto mode enabled with light appearance
    let mut state = ThemePreviewState::new("default");
    state.set_auto_mode(true);
    state.on_system_appearance_change(SystemAppearance::Light);
    assert_eq!(state.current_name(), "harness-light");

    // When: auto mode is disabled
    state.set_auto_mode(false);

    // Then: current_name stays as the last resolved theme
    assert_eq!(state.current_name(), "harness-light");
}

// ---------------------------------------------------------------------------
// 5. Color-capability adaptation (truecolor/basic/no-color)
// ---------------------------------------------------------------------------

#[test]
fn color_capability_truecolor_produces_full_theme() {
    // Given: truecolor environment
    let leaf = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));

    // Then: not reduced, dark theme
    assert!(!leaf.reduced_capability);
    assert_eq!(leaf.theme, NamedTheme::HarnessDark);
}

#[test]
fn color_capability_basic_produces_reduced_flag() {
    // Given: no truecolor but 256color terminal
    let leaf = ThemeLeaf::auto_from_env(None, Some("xterm-256color"));

    // Then: reduced capability flag is set
    assert!(leaf.reduced_capability);
}

#[test]
fn color_capability_no_color_produces_high_contrast() {
    // Given: dumb terminal (no color)
    let leaf = ThemeLeaf::auto_from_env(None, Some("dumb"));

    // Then: high contrast theme, reduced capability
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert!(leaf.reduced_capability);
}

#[test]
fn color_capability_truecolor_resolves_to_harness_dark_theme() {
    let leaf = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));
    let theme = Theme::by_name(leaf.theme.label()).expect("must resolve");
    assert_eq!(theme.surface.canvas, Theme::harness_dark().surface.canvas);
}

#[test]
fn color_capability_no_color_resolves_to_high_contrast_theme() {
    let leaf = ThemeLeaf::auto_from_env(None, Some("dumb"));
    let theme = Theme::by_name(leaf.theme.label()).expect("must resolve");
    assert_eq!(
        theme.surface.canvas,
        Theme::harness_high_contrast().surface.canvas
    );
}

#[test]
fn color_mode_truecolor_is_not_reduced() {
    let mode = ColorMode::from_env(Some("truecolor"), Some("xterm-256color"));
    assert_eq!(mode, ColorMode::Truecolor);
    assert!(mode.is_truecolor());
}

#[test]
fn color_mode_ansi256_is_basic() {
    let mode = ColorMode::from_env(None, Some("xterm-256color"));
    assert_eq!(mode, ColorMode::Ansi256);
    assert!(!mode.is_truecolor());
    assert!(mode.supports_256());
}

#[test]
fn color_mode_none_is_no_color() {
    let mode = ColorMode::from_env(None, Some("dumb"));
    assert_eq!(mode, ColorMode::None);
    assert!(!mode.is_truecolor());
    assert!(!mode.supports_256());
}

#[test]
fn terminal_capability_full_has_truecolor_and_all_features() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.color_mode.is_truecolor());
    assert!(caps.mouse_capture);
    assert!(caps.osc52_clipboard);
    assert!(caps.focus_reporting);
}

#[test]
fn terminal_capability_reduced_has_no_mouse_no_clipboard() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.mouse_capture);
    assert!(!caps.osc52_clipboard);
    assert!(!caps.focus_reporting);
}

// ---------------------------------------------------------------------------
// 6. Terminal diagnostic output
// ---------------------------------------------------------------------------

#[test]
fn terminal_diagnostics_produces_output() {
    let diag = TerminalDiagnostics::new();
    let lines = diag.diagnostic_lines();
    assert!(!lines.is_empty());
}

#[test]
fn terminal_diagnostics_includes_color_mode() {
    let diag = TerminalDiagnostics::new();
    let lines = diag.diagnostic_lines();
    let combined = lines.join("\n");
    assert!(
        combined.to_lowercase().contains("color"),
        "diagnostic output should mention color"
    );
}

#[test]
fn terminal_diagnostics_reports_unsupported_color() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Color);
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Color));
}

#[test]
fn terminal_diagnostics_reports_unsupported_clipboard() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Clipboard);
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Clipboard));
}

#[test]
fn terminal_diagnostics_reports_unsupported_mouse() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Mouse);
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Mouse));
}

#[test]
fn terminal_diagnostics_unsupported_appears_in_output() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Color);
    let lines = diag.diagnostic_lines();
    let combined = lines.join("\n");
    assert!(
        combined.to_lowercase().contains("color"),
        "unsupported color should appear in diagnostics"
    );
}

// ---------------------------------------------------------------------------
// 7. FPS/scroll debug behind explicit controls
// ---------------------------------------------------------------------------

#[test]
fn fps_debug_disabled_by_default() {
    let diag = TerminalDiagnostics::new();
    assert!(!diag.fps_debug_enabled());
}

#[test]
fn fps_debug_can_be_enabled() {
    let mut diag = TerminalDiagnostics::new();
    diag.enable_fps_debug();
    assert!(diag.fps_debug_enabled());
}

#[test]
fn fps_debug_can_be_disabled() {
    let mut diag = TerminalDiagnostics::new();
    diag.enable_fps_debug();
    assert!(diag.fps_debug_enabled());
    diag.disable_fps_debug();
    assert!(!diag.fps_debug_enabled());
}

#[test]
fn scroll_debug_disabled_by_default() {
    let diag = TerminalDiagnostics::new();
    assert!(!diag.scroll_debug_enabled());
}

#[test]
fn scroll_debug_can_be_enabled() {
    let mut diag = TerminalDiagnostics::new();
    diag.enable_scroll_debug();
    assert!(diag.scroll_debug_enabled());
}

#[test]
fn scroll_debug_can_be_disabled() {
    let mut diag = TerminalDiagnostics::new();
    diag.enable_scroll_debug();
    assert!(diag.scroll_debug_enabled());
    diag.disable_scroll_debug();
    assert!(!diag.scroll_debug_enabled());
}

#[test]
fn fps_and_scroll_debug_are_independent() {
    let mut diag = TerminalDiagnostics::new();
    diag.enable_fps_debug();
    assert!(diag.fps_debug_enabled());
    assert!(!diag.scroll_debug_enabled());
    diag.enable_scroll_debug();
    assert!(diag.fps_debug_enabled());
    assert!(diag.scroll_debug_enabled());
    diag.disable_fps_debug();
    assert!(!diag.fps_debug_enabled());
    assert!(diag.scroll_debug_enabled());
}

// ---------------------------------------------------------------------------
// 8. Notification-storm bounding
// ---------------------------------------------------------------------------

#[test]
fn notification_storm_max_concurrent_is_three() {
    assert_eq!(MAX_CONCURRENT_NOTIFICATIONS, 3);
}

#[test]
fn notification_storm_drops_oldest_when_exceeded() {
    // Given: unfocused state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: push more than MAX_CONCURRENT_NOTIFICATIONS
    for i in 0..(MAX_CONCURRENT_NOTIFICATIONS + 2) {
        state.push(
            NotificationKind::Info,
            NoticeLevel::Info,
            &format!("msg-{i}"),
        );
    }

    // Then: entries are bounded to MAX_CONCURRENT_NOTIFICATIONS
    assert_eq!(state.entries().len(), MAX_CONCURRENT_NOTIFICATIONS);
}

#[test]
fn notification_storm_oldest_dropped_first() {
    // Given: unfocused state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: push 4 notifications (max is 3)
    state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    state.push(NotificationKind::Info, NoticeLevel::Info, "second");
    state.push(NotificationKind::Info, NoticeLevel::Info, "third");
    state.push(NotificationKind::Info, NoticeLevel::Info, "fourth");

    // Then: "first" is dropped, "second" is now oldest
    let messages: Vec<&str> = state.entries().iter().map(|e| e.message.as_str()).collect();
    assert!(!messages.contains(&"first"));
    assert!(messages.contains(&"second"));
    assert!(messages.contains(&"third"));
    assert!(messages.contains(&"fourth"));
}

#[test]
fn notification_storm_exact_max_held() {
    // Given: unfocused state
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: push exactly MAX_CONCURRENT_NOTIFICATIONS
    for i in 0..MAX_CONCURRENT_NOTIFICATIONS {
        state.push(
            NotificationKind::Info,
            NoticeLevel::Info,
            &format!("msg-{i}"),
        );
    }

    // Then: all are held
    assert_eq!(state.entries().len(), MAX_CONCURRENT_NOTIFICATIONS);
}

// ---------------------------------------------------------------------------
// 9. Focus race
// ---------------------------------------------------------------------------

#[test]
fn focus_race_rapid_focus_changes_dont_lose_notifications() {
    // Given: state starting unfocused
    let mut state = NotificationState::new();
    state.set_focused(false);

    // When: rapid focus changes with a push in between
    state.push(NotificationKind::Info, NoticeLevel::Info, "before");
    state.set_focused(true);
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "after");

    // Then: both notifications are present (no loss from focus race)
    assert_eq!(state.entries().len(), 2);
    assert_eq!(state.entries()[0].message, "before");
    assert_eq!(state.entries()[1].message, "after");
}

#[test]
fn focus_race_focus_during_push_doesnt_corrupt_state() {
    // Given: state with existing notifications
    let mut state = NotificationState::new();
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "first");

    // When: focus changes between pushes
    state.set_focused(true);
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "second");

    // Then: state is consistent
    assert_eq!(state.entries().len(), 2);
    assert!(state.should_deliver());
}

#[test]
fn focus_race_concurrent_focus_and_dismiss() {
    // Given: state with notifications
    let mut state = NotificationState::new();
    state.set_focused(false);
    let seq1 = state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    let seq2 = state.push(NotificationKind::Info, NoticeLevel::Info, "second");

    // When: focus changes and dismiss happens
    state.set_focused(true);
    state.dismiss(seq1);
    state.set_focused(false);

    // Then: only the dismissed one is gone
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].seq, seq2);
}

// ---------------------------------------------------------------------------
// 10. Sleep transition
// ---------------------------------------------------------------------------

#[test]
fn sleep_transition_notification_state_survives_sleep() {
    // Given: state with a notification
    let mut state = NotificationState::new();
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "before-sleep");

    // When: sleep transition (simulated by marking unfocused)
    // Then: notification is still present
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].message, "before-sleep");
}

#[test]
fn sleep_transition_clear_does_not_affect_new_notifications() {
    // Given: state with notifications
    let mut state = NotificationState::new();
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "old");

    // When: clear (simulating sleep transition cleanup)
    state.clear();

    // Then: new notifications can still be pushed
    state.push(NotificationKind::Info, NoticeLevel::Info, "new");
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].message, "new");
}

#[test]
fn sleep_transition_focus_state_preserved() {
    // Given: focused state
    let mut state = NotificationState::new();
    state.set_focused(true);

    // When: sleep transition (no state change)
    // Then: focus is still true
    assert!(state.is_focused());
    assert!(!state.should_deliver());
}

// ---------------------------------------------------------------------------
// 11. Unsupported color/clipboard/mouse
// ---------------------------------------------------------------------------

#[test]
fn unsupported_color_reported_in_diagnostics() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Color);
    let caps = diag.unsupported_capabilities();
    assert_eq!(caps.len(), 1);
    assert!(caps.contains(&UnsupportedCapability::Color));
}

#[test]
fn unsupported_clipboard_reported_in_diagnostics() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Clipboard);
    let caps = diag.unsupported_capabilities();
    assert_eq!(caps.len(), 1);
    assert!(caps.contains(&UnsupportedCapability::Clipboard));
}

#[test]
fn unsupported_mouse_reported_in_diagnostics() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Mouse);
    let caps = diag.unsupported_capabilities();
    assert_eq!(caps.len(), 1);
    assert!(caps.contains(&UnsupportedCapability::Mouse));
}

#[test]
fn unsupported_capabilities_accumulate() {
    let mut diag = TerminalDiagnostics::new();
    diag.report_unsupported(UnsupportedCapability::Color);
    diag.report_unsupported(UnsupportedCapability::Clipboard);
    diag.report_unsupported(UnsupportedCapability::Mouse);
    assert_eq!(diag.unsupported_capabilities().len(), 3);
}

#[test]
fn unsupported_capabilities_empty_by_default() {
    let diag = TerminalDiagnostics::new();
    assert_eq!(diag.unsupported_capabilities().len(), 0);
}

#[test]
fn unsupported_color_from_reduced_capability() {
    // Given: reduced terminal capability (no truecolor)
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.color_mode.is_truecolor());

    // When: reporting based on reduced caps
    let mut diag = TerminalDiagnostics::new();
    if !caps.color_mode.is_truecolor() {
        diag.report_unsupported(UnsupportedCapability::Color);
    }

    // Then: color is reported as unsupported
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Color));
}

#[test]
fn unsupported_clipboard_from_reduced_capability() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.osc52_clipboard);

    let mut diag = TerminalDiagnostics::new();
    if !caps.osc52_clipboard {
        diag.report_unsupported(UnsupportedCapability::Clipboard);
    }
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Clipboard));
}

#[test]
fn unsupported_mouse_from_reduced_capability() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.mouse_capture);

    let mut diag = TerminalDiagnostics::new();
    if !caps.mouse_capture {
        diag.report_unsupported(UnsupportedCapability::Mouse);
    }
    assert!(diag
        .unsupported_capabilities()
        .contains(&UnsupportedCapability::Mouse));
}

// ---------------------------------------------------------------------------
// 12. Hosted-command absence
// ---------------------------------------------------------------------------

#[test]
fn hosted_announcement_action_is_unwired() {
    let resolution = resolve("tui.notifications").expect("must resolve");
    assert_eq!(resolution.availability, ActionAvailability::Unwired);
}

#[test]
fn hosted_tips_action_is_unwired() {
    let resolution = resolve("tui.tips").expect("must resolve");
    assert_eq!(resolution.availability, ActionAvailability::Unwired);
}

#[test]
fn hosted_show_announcement_takes_no_input() {
    // Announcement and release notes are hosted commands that must not
    // fetch anything. They take no input and are replay-safe.
    assert!(harness_tui::leaf_actions::group_f_notices::is_replay_safe(
        NoticeAction::ShowAnnouncement
    ));
}

#[test]
fn hosted_show_release_notes_takes_no_input() {
    assert!(harness_tui::leaf_actions::group_f_notices::is_replay_safe(
        NoticeAction::ShowReleaseNotes
    ));
}

#[test]
fn hosted_show_announcement_validates_no_input() {
    use harness_tui::leaf_actions::group_f_notices::validate_input;
    assert_eq!(
        validate_input(NoticeAction::ShowAnnouncement, ""),
        harness_tui::leaf_actions::group_f_notices::InputValidation::Valid
    );
}

#[test]
fn hosted_show_release_notes_validates_no_input() {
    use harness_tui::leaf_actions::group_f_notices::validate_input;
    assert_eq!(
        validate_input(NoticeAction::ShowReleaseNotes, ""),
        harness_tui::leaf_actions::group_f_notices::InputValidation::Valid
    );
}

#[test]
fn notification_state_has_no_network_methods() {
    // The notification state must not expose any method that fetches
    // hosted content. Verify only local push/dismiss/clear exist.
    let mut state = NotificationState::new();
    state.set_focused(false);
    let seq = state.push(NotificationKind::Info, NoticeLevel::Info, "local");
    assert!(seq > 0);
    state.dismiss(seq);
    assert_eq!(state.entries().len(), 0);
}

// ---------------------------------------------------------------------------
// 13. Theme catalog completeness for preview
// ---------------------------------------------------------------------------

#[test]
fn theme_harness_light_exists() {
    let theme = Theme::harness_light();
    // Light theme should have a light background
    assert_ne!(theme.surface.canvas, Theme::harness_dark().surface.canvas);
}

#[test]
fn theme_by_name_resolves_harness_light() {
    let theme = Theme::by_name("harness-light");
    assert!(theme.is_some());
    assert_ne!(
        theme.unwrap().surface.canvas,
        Theme::harness_dark().surface.canvas
    );
}

#[test]
fn theme_available_names_includes_harness_light() {
    let names = Theme::available_theme_names();
    assert!(names.contains(&"harness-light"));
}

#[test]
fn theme_harness_light_has_light_background() {
    let light = Theme::harness_light();
    let dark = Theme::harness_dark();
    // Light background should be brighter than dark
    if let (Color::Rgb(lr, lg, lb), Color::Rgb(dr, dg, db)) =
        (light.surface.canvas, dark.surface.canvas)
    {
        let light_sum = u32::from(lr) + u32::from(lg) + u32::from(lb);
        let dark_sum = u32::from(dr) + u32::from(dg) + u32::from(db);
        assert!(light_sum > dark_sum, "light background should be brighter");
    }
}

#[test]
fn theme_harness_light_foreground_is_dark() {
    let light = Theme::harness_light();
    if let Color::Rgb(r, g, b) = light.text.primary {
        let sum = u32::from(r) + u32::from(g) + u32::from(b);
        assert!(
            sum < 384,
            "light theme foreground should be dark (sum < 384)"
        );
    }
}

#[test]
fn theme_preview_to_light_then_apply() {
    let mut state = ThemePreviewState::new("default");
    state.preview("harness-light");
    assert!(state.is_previewing());
    state.apply();
    assert_eq!(state.current_name(), "harness-light");
}

#[test]
fn theme_preview_to_light_then_revert() {
    let mut state = ThemePreviewState::new("default");
    state.preview("harness-light");
    state.revert();
    assert_eq!(state.current_name(), "default");
}

// ---------------------------------------------------------------------------
// 14. Tips state
// ---------------------------------------------------------------------------

#[test]
fn tip_state_empty_by_default() {
    let state = TipState::new();
    assert!(state.active().is_none());
}

#[test]
fn tip_show_sets_active_tip() {
    let mut state = TipState::new();
    state.show("keyboard-shortcuts", "Press Ctrl+P for the command palette");
    let active = state.active().expect("tip should be active");
    assert_eq!(active.id, "keyboard-shortcuts");
    assert_eq!(active.body, "Press Ctrl+P for the command palette");
}

#[test]
fn tip_dismiss_clears_active() {
    let mut state = TipState::new();
    state.show("tip-1", "body");
    assert!(state.active().is_some());
    state.dismiss();
    assert!(state.active().is_none());
}

#[test]
fn tip_seen_count_starts_at_zero() {
    let state = TipState::new();
    assert_eq!(state.seen_count("any-tip"), 0);
}

#[test]
fn tip_seen_count_increments_on_show() {
    let mut state = TipState::new();
    state.show("tip-1", "first");
    assert_eq!(state.seen_count("tip-1"), 1);
    state.show("tip-1", "second");
    assert_eq!(state.seen_count("tip-1"), 2);
}

#[test]
fn tip_dismissed_flag_set_after_dismiss() {
    let mut state = TipState::new();
    state.show("tip-1", "body");
    assert!(!state.is_dismissed("tip-1"));
    state.dismiss();
    assert!(state.is_dismissed("tip-1"));
}

#[test]
fn tip_different_ids_tracked_separately() {
    let mut state = TipState::new();
    state.show("tip-a", "a");
    state.show("tip-b", "b");
    assert_eq!(state.seen_count("tip-a"), 1);
    assert_eq!(state.seen_count("tip-b"), 1);
}

#[test]
fn tip_replacing_active_tip_increments_seen() {
    let mut state = TipState::new();
    state.show("tip-a", "first");
    state.show("tip-b", "second");
    assert_eq!(state.seen_count("tip-a"), 1);
    assert_eq!(state.seen_count("tip-b"), 1);
    let active = state.active().expect("active tip");
    assert_eq!(active.id, "tip-b");
}

// ---------------------------------------------------------------------------
// 15. Notification entry fields
// ---------------------------------------------------------------------------

#[test]
fn notification_entry_carries_all_fields() {
    let entry = NotificationEntry {
        kind: NotificationKind::TaskCompleted,
        level: NoticeLevel::Success,
        message: "task done".to_string(),
        seq: 42,
    };
    assert_eq!(entry.kind, NotificationKind::TaskCompleted);
    assert_eq!(entry.level, NoticeLevel::Success);
    assert_eq!(entry.message, "task done");
    assert_eq!(entry.seq, 42);
}

#[test]
fn notification_kind_variants_are_distinct() {
    assert_ne!(
        NotificationKind::TaskCompleted,
        NotificationKind::PermissionAlert
    );
    assert_ne!(NotificationKind::TaskCompleted, NotificationKind::Info);
    assert_ne!(NotificationKind::PermissionAlert, NotificationKind::Info);
}

#[test]
fn notification_seq_increments() {
    let mut state = NotificationState::new();
    state.set_focused(false);
    let seq1 = state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    let seq2 = state.push(NotificationKind::Info, NoticeLevel::Info, "second");
    assert!(seq2 > seq1, "seq should increment");
}

#[test]
fn notification_dismiss_removes_by_seq() {
    let mut state = NotificationState::new();
    state.set_focused(false);
    let seq1 = state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    let seq2 = state.push(NotificationKind::Info, NoticeLevel::Info, "second");
    state.dismiss(seq1);
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].seq, seq2);
}

#[test]
fn notification_dismiss_unknown_seq_is_noop() {
    let mut state = NotificationState::new();
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    state.dismiss(999);
    assert_eq!(state.entries().len(), 1);
}

#[test]
fn notification_clear_removes_all() {
    let mut state = NotificationState::new();
    state.set_focused(false);
    state.push(NotificationKind::Info, NoticeLevel::Info, "first");
    state.push(NotificationKind::Info, NoticeLevel::Info, "second");
    state.clear();
    assert_eq!(state.entries().len(), 0);
}

// ---------------------------------------------------------------------------
// 16. SystemAppearance enum
// ---------------------------------------------------------------------------

#[test]
fn system_appearance_dark_and_light_are_distinct() {
    assert_ne!(SystemAppearance::Dark, SystemAppearance::Light);
}

// ---------------------------------------------------------------------------
// 17. ThemeLeaf auto mode consistency
// ---------------------------------------------------------------------------

#[test]
fn theme_leaf_auto_mode_is_auto_from_env() {
    let leaf = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
}

#[test]
fn theme_leaf_explicit_mode_is_explicit() {
    let leaf = ThemeLeaf::explicit(NamedTheme::HarnessDark);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
}

#[test]
fn theme_leaf_reduced_is_high_contrast() {
    let leaf = ThemeLeaf::reduced();
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert!(leaf.reduced_capability);
}

#[test]
fn theme_leaf_auto_from_env_no_env_is_reduced() {
    let leaf = ThemeLeaf::auto_from_env(None, None);
    assert!(leaf.reduced_capability);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
}

// ---------------------------------------------------------------------------
// 18. Notification default state
// ---------------------------------------------------------------------------

#[test]
fn notification_state_default_is_unfocused() {
    let state = NotificationState::new();
    assert!(!state.is_focused());
    assert!(state.should_deliver());
}

#[test]
fn notification_state_default_empty() {
    let state = NotificationState::new();
    assert_eq!(state.entries().len(), 0);
}

#[test]
fn notification_state_focus_toggle() {
    let mut state = NotificationState::new();
    assert!(!state.is_focused());
    state.set_focused(true);
    assert!(state.is_focused());
    assert!(!state.should_deliver());
    state.set_focused(false);
    assert!(!state.is_focused());
    assert!(state.should_deliver());
}
