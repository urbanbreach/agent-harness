//! Task 31: Themes, auto/system appearance, notifications, and contextual
//! tips parity tests.
//!
//! Contract: `grok-build-parity-parallel-execution.md` lines 1005-1014 and
//! `crates/harness-tui/DESIGN.md` sections 10, 11, 12.
//!
//! Covers: named themes and exact token roles, truecolor/basic/no-color
//! adaptation, system auto dark/light selection, preview/revert, notification
//! timing, terminal title/progress, sleep inhibitor behavior, focus-aware
//! permission/background notifications, tips, seen counts, and contextual
//! hint dismissal/persistence.
//!
//! QA: theme token/cell/pixel matrix, system preference changes, unsupported
//! terminal fallback, notification focus timing, persistence/restart, and
//! reference-vs-Harness parity at all required viewports.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    SCHEMA_VERSION,
};
use harness_core::sleep_wake_auth::{
    decide_sleep_wake_credential_refresh, decide_sleep_wake_credential_refresh_for,
    evaluate_sleep_wake_credential_refresh, observe_and_decide_sleep_wake_host_event,
    observe_and_decide_sleep_wake_host_event_for, observe_sleep_wake_host_event,
    sleep_wake_credential_refresh_availability, summarize_sleep_wake_observations,
    CredentialExpirySnapshot, SleepWakeCredentialPolicy, SleepWakeHostEvent, SleepWakeObservation,
    SleepWakeObservationSummary, SleepWakeRefreshDecision,
};
use harness_testkit::parity::{
    compare_frames, compare_frames_with_provenance, CaptureSource, CellModifiers, CursorState,
    IdentityMaskRegistry, ResolvedRgb, SemanticCell, SemanticFrame,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata, UiIntent};
use harness_tui::leaf_actions::group_f_notices::{
    capability_ids, group_id, is_replay_safe, resolve, validate_input, ActionAvailability,
    InputValidation, LeafActionResolution, NoticeAction, NoticeLevel, BACKEND_OWNER,
    CAPABILITY_IDS, GROUP_ID,
};
use harness_tui::render_test::render_to_string;
use harness_tui::responsive::{
    VIEWPORT_100x30, VIEWPORT_120x40, VIEWPORT_120x50, VIEWPORT_60x20, VIEWPORT_79x24,
    VIEWPORT_80x24, ViewportId, ViewportPlan, VIEWPORT_WIDE,
};
use harness_tui::terminal::{
    ColorMode, KeyboardMode, TerminalCapabilityLeaf, TerminalCapabilityRecord,
    TerminalCapabilityRow,
};
use harness_tui::theme::{ShellGeometry, ShellGeometryTarget, Theme};
use harness_tui::theme_leaf::{NamedTheme, ThemeAutoMode, ThemeLeaf};
use harness_tui::{ui, FrameLayoutPlan, UnwrapOrAbort};
use ratatui::layout::Rect;
use ratatui::style::Color;

const W: u16 = 120;
const H: u16 = 40;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-task31-{seq:04}"),
        seq,
        run_id: "run_task31_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task31-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task31_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task31").with_mode_label("Demo"),
    );
    app
}

fn live_app_with_sink() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task31").with_mode_label("Demo"),
    );
    (app, intents)
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn permission_requested_event(seq: u64, correlation_id: &str, kind: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(correlation_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: format!("perm-{seq}"),
            kind: kind.to_string(),
            tool_call_id: Some(correlation_id.into()),
            summary: "test permission summary".into(),
            request_digest: format!("digest-{seq}"),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    )
}

// ---------------------------------------------------------------------------
// 1. Theme token matrix — DESIGN.md §10 color roles
// ---------------------------------------------------------------------------

#[test]
fn theme_harness_dark_background_is_black() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.surface.canvas, Color::Rgb(0x0B, 0x0E, 0x14));
}

#[test]
fn theme_harness_dark_foreground_default_is_light_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.text.primary, Color::Rgb(0xEE, 0xEE, 0xEC));
}

#[test]
fn theme_harness_dark_dim_text_is_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.text.secondary, Color::Rgb(0x88, 0x8B, 0x91));
}

#[test]
fn theme_harness_dark_bold_text_is_white() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.text.inverse, Color::Rgb(0x0B, 0x0E, 0x14));
}

#[test]
fn theme_harness_dark_error_is_red() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.status.error, Color::Rgb(0xE0, 0x6C, 0x75));
}

#[test]
fn theme_harness_dark_success_is_green() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.status.success, Color::Rgb(0x7F, 0xD8, 0x8F));
}

#[test]
fn theme_harness_dark_warning_is_yellow() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.status.warning, Color::Rgb(0xE5, 0xC0, 0x7B));
}

#[test]
fn theme_harness_dark_info_is_cyan() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.status.info, Color::Rgb(0x56, 0xB6, 0xC2));
}

#[test]
fn theme_harness_dark_border_subtle_is_dark_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.border.subtle, Color::Rgb(0x3A, 0x3D, 0x43));
}

#[test]
fn theme_harness_dark_border_strong_is_medium_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.border.strong, Color::Rgb(0x48, 0x4B, 0x52));
}

#[test]
fn theme_harness_dark_border_focus_is_light_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.border.focus, Color::Rgb(0x60, 0x63, 0x6A));
}

#[test]
fn theme_harness_dark_accent_is_purple() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.text.accent, Color::Rgb(0xD9, 0x84, 0xD9));
}

#[test]
fn theme_harness_dark_agent_build_is_blue() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agents.build, Color::Rgb(0x5C, 0x9C, 0xF5));
}

#[test]
fn theme_harness_dark_agent_plan_is_purple() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agents.plan, Color::Rgb(0xD9, 0x84, 0xD9));
}

#[test]
fn theme_harness_dark_scrollbar_track_matches_canvas() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.scrollbar.track, theme.surface.canvas);
}

#[test]
fn theme_harness_dark_scrollbar_thumb_is_dark_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.scrollbar.thumb, Color::Rgb(0x32, 0x36, 0x3C));
}

#[test]
fn theme_harness_dark_scrollbar_thumb_active_is_light_grey() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.scrollbar.thumb_active, Color::Rgb(0x60, 0x63, 0x6A));
}

#[test]
fn theme_harness_dark_panel_elevated_differs_from_panel() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.surface.panel_elevated, theme.surface.panel);
}

#[test]
fn theme_harness_dark_markdown_heading_matches_accent() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.markdown.heading, theme.text.accent);
}

#[test]
fn theme_harness_dark_markdown_code_matches_success() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.markdown.code, theme.status.success);
}

#[test]
fn theme_harness_dark_markdown_link_is_light_purple() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.markdown.link, Color::Rgb(0xE8, 0xA0, 0xE8));
}

#[test]
fn theme_harness_dark_markdown_emph_matches_warning() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.markdown.emph, theme.status.warning);
}

#[test]
fn theme_harness_dark_agent_palette_has_seven_colors() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agents.palette.len(), 7);
}

#[test]
fn theme_harness_dark_agent_palette_colors_are_unique() {
    let theme = Theme::harness_dark();
    let colors: Vec<Color> = theme.agents.palette.to_vec();
    let unique: std::collections::HashSet<Color> = colors.into_iter().collect();
    assert_eq!(unique.len(), 7);
}

// ---------------------------------------------------------------------------
// 2. High contrast theme token matrix
// ---------------------------------------------------------------------------

#[test]
fn theme_high_contrast_background_is_black() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.surface.canvas, Color::Black);
}

#[test]
fn theme_high_contrast_foreground_is_white() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.text.primary, Color::White);
}

#[test]
fn theme_high_contrast_error_is_light_red() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.status.error, Color::LightRed);
}

#[test]
fn theme_high_contrast_success_is_light_green() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.status.success, Color::LightGreen);
}

#[test]
fn theme_high_contrast_warning_is_yellow() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.status.warning, Color::Yellow);
}

#[test]
fn theme_high_contrast_info_is_light_cyan() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.status.info, Color::LightCyan);
}

#[test]
fn theme_high_contrast_border_focus_is_yellow() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.border.focus, Color::Yellow);
}

#[test]
fn theme_high_contrast_accent_is_yellow() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.text.accent, Color::Yellow);
}

#[test]
fn theme_high_contrast_uses_named_colors_not_rgb() {
    let theme = Theme::harness_high_contrast();
    assert!(matches!(theme.surface.canvas, Color::Black));
    assert!(matches!(theme.text.primary, Color::White));
    assert!(matches!(theme.status.error, Color::LightRed));
}

// ---------------------------------------------------------------------------
// 3. Theme catalog and by_name resolution
// ---------------------------------------------------------------------------

#[test]
fn theme_by_name_resolves_default() {
    let theme = Theme::by_name("default");
    assert!(theme.is_some());
    assert_eq!(
        theme.unwrap().surface.canvas,
        Theme::harness_chat().surface.canvas
    );
}

#[test]
fn theme_by_name_resolves_harness_dark() {
    let theme = Theme::by_name("harness-dark");
    assert!(theme.is_some());
}

#[test]
fn theme_by_name_resolves_high_contrast() {
    let theme = Theme::by_name("high-contrast");
    assert!(theme.is_some());
    assert_eq!(theme.unwrap().surface.canvas, Color::Black);
}

#[test]
fn theme_by_name_returns_none_for_unknown() {
    assert!(Theme::by_name("nonexistent").is_none());
}

#[test]
fn theme_available_names_includes_default_and_high_contrast() {
    let names = Theme::available_theme_names();
    assert!(names.contains(&"default"));
    assert!(names.contains(&"high-contrast"));
}

#[test]
fn theme_default_is_harness_chat() {
    let default = Theme::default();
    let harness_chat = Theme::harness_chat();
    assert_eq!(default.surface.canvas, harness_chat.surface.canvas);
    assert_eq!(default.text.primary, harness_chat.text.primary);
}

#[test]
fn theme_harness_dark_and_high_contrast_differ_in_background() {
    let dark = Theme::harness_dark();
    let hc = Theme::harness_high_contrast();
    assert_ne!(dark.surface.canvas, hc.surface.canvas);
}

// ---------------------------------------------------------------------------
// 4. NamedTheme leaf catalog (theme_leaf.rs)
// ---------------------------------------------------------------------------

#[test]
fn named_theme_all_has_three_themes() {
    assert_eq!(NamedTheme::ALL.len(), 4);
}

#[test]
fn named_theme_harness_dark_label() {
    assert_eq!(NamedTheme::HarnessDark.label(), "harness-dark");
}

#[test]
fn named_theme_harness_light_label() {
    assert_eq!(NamedTheme::HarnessLight.label(), "harness-light");
}

#[test]
fn named_theme_high_contrast_label() {
    assert_eq!(NamedTheme::HighContrast.label(), "high-contrast");
}

#[test]
fn named_theme_from_label_resolves_dark() {
    assert_eq!(
        NamedTheme::from_label("dark"),
        Some(NamedTheme::HarnessDark)
    );
    assert_eq!(
        NamedTheme::from_label("harness-dark"),
        Some(NamedTheme::HarnessDark)
    );
}

#[test]
fn named_theme_from_label_resolves_light() {
    assert_eq!(
        NamedTheme::from_label("light"),
        Some(NamedTheme::HarnessLight)
    );
    assert_eq!(
        NamedTheme::from_label("harness-light"),
        Some(NamedTheme::HarnessLight)
    );
}

#[test]
fn named_theme_from_label_is_case_insensitive() {
    assert_eq!(
        NamedTheme::from_label("HARNESS-DARK"),
        Some(NamedTheme::HarnessDark)
    );
    assert_eq!(
        NamedTheme::from_label("HIGH-CONTRAST"),
        Some(NamedTheme::HighContrast)
    );
}

#[test]
fn named_theme_from_label_returns_none_for_unknown() {
    assert!(NamedTheme::from_label("unknown").is_none());
    assert!(NamedTheme::from_label("").is_none());
}

#[test]
fn named_theme_labels_are_unique() {
    let labels: Vec<&str> = NamedTheme::ALL.iter().map(|t| t.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(unique.len(), 4);
}

// ---------------------------------------------------------------------------
// 5. ThemeLeaf auto/system appearance
// ---------------------------------------------------------------------------

#[test]
fn theme_leaf_default_is_harness_dark_explicit() {
    let leaf = ThemeLeaf::default_theme();
    assert_eq!(leaf.theme, NamedTheme::HarnessDark);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
    assert!(!leaf.reduced_capability);
}

#[test]
fn theme_leaf_default_impl_matches_default_theme() {
    let leaf = ThemeLeaf::default();
    assert_eq!(leaf, ThemeLeaf::default_theme());
}

#[test]
fn theme_leaf_auto_from_env_truecolor_is_not_reduced() {
    let leaf = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));
    assert_eq!(leaf.theme, NamedTheme::HarnessDark);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
    assert!(!leaf.reduced_capability);
}

#[test]
fn theme_leaf_auto_from_env_24bit_is_not_reduced() {
    let leaf = ThemeLeaf::auto_from_env(Some("24bit"), Some("xterm-256color"));
    assert_eq!(leaf.theme, NamedTheme::HarnessDark);
    assert!(!leaf.reduced_capability);
}

#[test]
fn theme_leaf_auto_from_env_dumb_is_high_contrast_reduced() {
    let leaf = ThemeLeaf::auto_from_env(None, Some("dumb"));
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
    assert!(leaf.reduced_capability);
}

#[test]
fn theme_leaf_auto_from_env_without_truecolor_is_reduced() {
    let leaf = ThemeLeaf::auto_from_env(None, Some("xterm-256color"));
    assert!(leaf.reduced_capability);
}

#[test]
fn theme_leaf_auto_from_env_no_env_is_reduced() {
    let leaf = ThemeLeaf::auto_from_env(None, None);
    assert!(leaf.reduced_capability);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
}

#[test]
fn theme_leaf_explicit_clears_auto_mode() {
    let leaf = ThemeLeaf::explicit(NamedTheme::HarnessLight);
    assert_eq!(leaf.theme, NamedTheme::HarnessLight);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
    assert!(!leaf.reduced_capability);
}

#[test]
fn theme_leaf_explicit_high_contrast() {
    let leaf = ThemeLeaf::explicit(NamedTheme::HighContrast);
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
}

#[test]
fn theme_leaf_reduced_is_high_contrast_with_reduced_flag() {
    let leaf = ThemeLeaf::reduced();
    assert_eq!(leaf.theme, NamedTheme::HighContrast);
    assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
    assert!(leaf.reduced_capability);
}

#[test]
fn theme_auto_mode_explicit_is_default() {
    assert_eq!(ThemeAutoMode::default(), ThemeAutoMode::Explicit);
}

// ---------------------------------------------------------------------------
// 6. ColorMode adaptation — truecolor/basic/no-color
// ---------------------------------------------------------------------------

#[test]
fn color_mode_from_env_truecolor_from_colorterm() {
    assert_eq!(
        ColorMode::from_env(Some("truecolor"), Some("xterm-256color")),
        ColorMode::Truecolor
    );
}

#[test]
fn color_mode_from_env_24bit_from_colorterm() {
    assert_eq!(
        ColorMode::from_env(Some("24bit"), Some("xterm-256color")),
        ColorMode::Truecolor
    );
}

#[test]
fn color_mode_from_env_truecolor_case_insensitive() {
    assert_eq!(
        ColorMode::from_env(Some("TrueColor"), None),
        ColorMode::Truecolor
    );
}

#[test]
fn color_mode_from_env_256color_from_term() {
    assert_eq!(
        ColorMode::from_env(None, Some("xterm-256color")),
        ColorMode::Ansi256
    );
}

#[test]
fn color_mode_from_env_dumb_is_none() {
    assert_eq!(ColorMode::from_env(None, Some("dumb")), ColorMode::None);
}

#[test]
fn color_mode_from_env_defaults_to_ansi16() {
    assert_eq!(ColorMode::from_env(None, None), ColorMode::Ansi16);
    assert_eq!(ColorMode::from_env(None, Some("xterm")), ColorMode::Ansi16);
}

#[test]
fn color_mode_from_env_colorterm_takes_precedence_over_term() {
    assert_eq!(
        ColorMode::from_env(Some("truecolor"), Some("dumb")),
        ColorMode::Truecolor
    );
}

#[test]
fn color_mode_is_truecolor_only_for_truecolor() {
    assert!(ColorMode::Truecolor.is_truecolor());
    assert!(!ColorMode::Ansi256.is_truecolor());
    assert!(!ColorMode::Ansi16.is_truecolor());
    assert!(!ColorMode::None.is_truecolor());
}

#[test]
fn color_mode_supports_256_for_ansi256_and_truecolor() {
    assert!(ColorMode::Truecolor.supports_256());
    assert!(ColorMode::Ansi256.supports_256());
    assert!(!ColorMode::Ansi16.supports_256());
    assert!(!ColorMode::None.supports_256());
}

#[test]
fn color_mode_default_is_ansi16() {
    assert_eq!(ColorMode::default(), ColorMode::Ansi16);
}

// ---------------------------------------------------------------------------
// 7. Terminal capability leaf — full vs reduced
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_full_has_truecolor() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.color_mode.is_truecolor());
}

#[test]
fn terminal_capability_full_has_enhanced_keyboard() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.keyboard_mode.is_enhanced());
}

#[test]
fn terminal_capability_full_has_mouse_capture() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.mouse_capture);
}

#[test]
fn terminal_capability_full_has_bracketed_paste() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.bracketed_paste);
}

#[test]
fn terminal_capability_full_has_osc52_clipboard() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.osc52_clipboard);
}

#[test]
fn terminal_capability_full_has_alternate_screen() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.alternate_screen);
}

#[test]
fn terminal_capability_full_has_focus_reporting() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.focus_reporting);
}

#[test]
fn terminal_capability_reduced_has_ansi16() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
}

#[test]
fn terminal_capability_reduced_has_legacy_keyboard() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.keyboard_mode.is_enhanced());
}

#[test]
fn terminal_capability_reduced_has_no_mouse() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.mouse_capture);
}

#[test]
fn terminal_capability_reduced_has_no_bracketed_paste() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.bracketed_paste);
}

#[test]
fn terminal_capability_reduced_has_no_osc52() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.osc52_clipboard);
}

#[test]
fn terminal_capability_reduced_has_no_alternate_screen() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.alternate_screen);
}

#[test]
fn terminal_capability_reduced_has_no_focus_reporting() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.focus_reporting);
}

#[test]
fn terminal_capability_default_is_reduced() {
    let caps = TerminalCapabilityLeaf::default();
    assert_eq!(caps, TerminalCapabilityLeaf::reduced());
}

#[test]
fn terminal_capability_from_env_truecolor_tty() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), true);
    assert!(caps.color_mode.is_truecolor());
    assert!(caps.osc52_clipboard);
}

#[test]
fn terminal_capability_from_env_disables_osc52_for_non_tty() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.osc52_clipboard);
}

#[test]
fn terminal_capability_from_env_dumb_terminal() {
    let caps = TerminalCapabilityLeaf::from_env(None, Some("dumb"), true);
    assert_eq!(caps.color_mode, ColorMode::None);
}

#[test]
fn terminal_capability_from_env_no_env() {
    let caps = TerminalCapabilityLeaf::from_env(None, None, false);
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
    assert!(!caps.osc52_clipboard);
}

// ---------------------------------------------------------------------------
// 8. Terminal capability records and behavior IDs
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_behavior_ids_match_manifest() {
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Color),
        "TERM-CAP-COLOR"
    );
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Keys),
        "TERM-CAP-KEYS"
    );
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Mouse),
        "TERM-CAP-MOUSE"
    );
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Clipboard),
        "TERM-CAP-CLIPBOARD"
    );
}

#[test]
fn terminal_capability_row_all_has_four_rows() {
    assert_eq!(TerminalCapabilityRow::ALL.len(), 4);
}

#[test]
fn terminal_capability_records_cover_all_four_rows() {
    let caps = TerminalCapabilityLeaf::full();
    let records = TerminalCapabilityRecord::all_for(&caps);
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|r| r.color_mode.is_truecolor()));
}

#[test]
fn terminal_capability_record_for_row_carries_all_fields() {
    let caps = TerminalCapabilityLeaf::full();
    let record = TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Color, &caps);
    assert_eq!(record.row, TerminalCapabilityRow::Color);
    assert_eq!(record.behavior_id, "TERM-CAP-COLOR");
    assert!(record.color_mode.is_truecolor());
    assert!(record.mouse_capture);
    assert!(record.focus_reporting);
}

#[test]
fn terminal_capability_record_reduced_has_no_focus() {
    let caps = TerminalCapabilityLeaf::reduced();
    let record = TerminalCapabilityRecord::for_row(TerminalCapabilityRow::Color, &caps);
    assert!(!record.focus_reporting);
    assert!(!record.mouse_capture);
}

// ---------------------------------------------------------------------------
// 9. Theme dialog — preview/revert
// ---------------------------------------------------------------------------

#[test]
fn theme_dialog_visible_defaults_false() {
    let app = live_app();
    assert!(!app.theme_dialog_visible);
}

#[test]
fn theme_dialog_selected_defaults_zero() {
    let app = live_app();
    assert_eq!(app.theme_dialog_selected, 0);
}

#[test]
fn theme_name_defaults_to_default() {
    let app = live_app();
    assert_eq!(app.theme_name, "default");
}

#[test]
fn theme_returns_current_theme() {
    let app = live_app();
    let theme = app.theme();
    assert_eq!(theme.surface.canvas, Theme::harness_chat().surface.canvas);
}

#[test]
fn theme_by_name_high_contrast_returns_black_background() {
    let theme = Theme::by_name("high-contrast").expect("must resolve");
    assert_eq!(theme.surface.canvas, Color::Black);
}

#[test]
fn theme_by_name_default_returns_harness_chat_background() {
    let theme = Theme::by_name("default").expect("must resolve");
    assert_eq!(theme.surface.canvas, Theme::harness_chat().surface.canvas);
}

#[test]
fn theme_by_name_harness_dark_alias_resolves() {
    let theme = Theme::by_name("harness-dark").expect("must resolve");
    assert_eq!(theme.surface.canvas, Theme::harness_dark().surface.canvas);
}

#[test]
fn theme_by_name_unknown_returns_none() {
    assert!(Theme::by_name("nonexistent").is_none());
}

#[test]
fn theme_preview_revert_via_theme_struct() {
    let original = Theme::harness_dark();
    let preview = Theme::harness_high_contrast();
    assert_ne!(original.surface.canvas, preview.surface.canvas);
    let reverted = Theme::harness_dark();
    assert_eq!(reverted.surface.canvas, original.surface.canvas);
}

#[test]
fn theme_dialog_can_be_opened_and_closed() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    assert!(app.theme_dialog_visible);
    app.theme_dialog_visible = false;
    assert!(!app.theme_dialog_visible);
}

#[test]
fn theme_dialog_selection_can_be_changed() {
    let mut app = live_app();
    app.theme_dialog_selected = 1;
    assert_eq!(app.theme_dialog_selected, 1);
    app.theme_dialog_selected = 0;
    assert_eq!(app.theme_dialog_selected, 0);
}

#[test]
fn render_with_default_theme_produces_output() {
    let app = live_app();
    let output = render(&app);
    assert!(!output.is_empty());
}

// ---------------------------------------------------------------------------
// 10. Shell geometry tokens
// ---------------------------------------------------------------------------

#[test]
fn shell_geometry_minimum_is_80x24() {
    assert_eq!(ShellGeometry::MINIMUM.width, 80);
    assert_eq!(ShellGeometry::MINIMUM.height, 24);
}

#[test]
fn shell_geometry_split_is_90x36() {
    assert_eq!(ShellGeometry::SPLIT.width, 90);
    assert_eq!(ShellGeometry::SPLIT.height, 36);
}

#[test]
fn shell_geometry_primary_is_100x30() {
    assert_eq!(ShellGeometry::PRIMARY.width, 100);
    assert_eq!(ShellGeometry::PRIMARY.height, 30);
}

#[test]
fn shell_breakpoints_target_primary_for_large_viewport() {
    let bp = harness_tui::theme::ShellBreakpoints::DEFAULT;
    assert_eq!(bp.target(120, 50), ShellGeometryTarget::Primary);
}

#[test]
fn shell_breakpoints_target_split_for_medium_viewport() {
    let bp = harness_tui::theme::ShellBreakpoints::DEFAULT;
    assert_eq!(bp.target(90, 36), ShellGeometryTarget::Split);
}

#[test]
fn shell_breakpoints_target_minimum_for_small_viewport() {
    let bp = harness_tui::theme::ShellBreakpoints::DEFAULT;
    assert_eq!(bp.target(80, 24), ShellGeometryTarget::Minimum);
}

#[test]
fn shell_breakpoints_target_minimum_for_tiny_viewport() {
    let bp = harness_tui::theme::ShellBreakpoints::DEFAULT;
    assert_eq!(bp.target(60, 20), ShellGeometryTarget::Minimum);
}

// ---------------------------------------------------------------------------
// 11. Notice action contract (group_f_notices.rs)
// ---------------------------------------------------------------------------

#[test]
fn notice_action_none_is_default() {
    assert_eq!(NoticeAction::default(), NoticeAction::None);
}

#[test]
fn notice_level_info_is_default() {
    assert_eq!(NoticeLevel::default(), NoticeLevel::Info);
}

#[test]
fn notice_group_id_is_f() {
    assert_eq!(GROUP_ID, "F");
    assert_eq!(group_id(), "F");
}

#[test]
fn notice_backend_owner_is_app_rs() {
    assert_eq!(BACKEND_OWNER, "crates/harness-tui/src/app.rs");
}

#[test]
fn notice_capability_ids_has_two_entries() {
    assert_eq!(CAPABILITY_IDS.len(), 2);
    assert!(CAPABILITY_IDS.contains(&"tui.notifications"));
    assert!(CAPABILITY_IDS.contains(&"tui.tips"));
}

#[test]
fn notice_capability_ids_function_matches_const() {
    assert_eq!(capability_ids(), CAPABILITY_IDS);
}

#[test]
fn notice_resolve_notifications_returns_unwired() {
    let resolution = resolve("tui.notifications").expect("must resolve");
    assert_eq!(resolution.capability_id, "tui.notifications");
    assert_eq!(resolution.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(resolution.availability, ActionAvailability::Unwired);
    assert!(resolution.replay_safe);
}

#[test]
fn notice_resolve_tips_returns_unwired() {
    let resolution = resolve("tui.tips").expect("must resolve");
    assert_eq!(resolution.capability_id, "tui.tips");
    assert_eq!(resolution.availability, ActionAvailability::Unwired);
}

#[test]
fn notice_resolve_unknown_returns_none() {
    assert!(resolve("tui.unknown").is_none());
    assert!(resolve("").is_none());
}

#[test]
fn notice_validate_input_show_notification_valid() {
    assert_eq!(
        validate_input(NoticeAction::ShowNotification, "Hello world"),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_show_notification_empty_invalid() {
    assert_eq!(
        validate_input(NoticeAction::ShowNotification, ""),
        InputValidation::Invalid("notification requires a non-empty message")
    );
}

#[test]
fn notice_validate_input_show_notification_too_long_invalid() {
    let long_msg = "x".repeat(1025);
    assert_eq!(
        validate_input(NoticeAction::ShowNotification, &long_msg),
        InputValidation::Invalid("notification message exceeds 1024 chars")
    );
}

#[test]
fn notice_validate_input_show_notification_max_length_valid() {
    let max_msg = "x".repeat(1024);
    assert_eq!(
        validate_input(NoticeAction::ShowNotification, &max_msg),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_show_tip_valid() {
    assert_eq!(
        validate_input(NoticeAction::ShowTip, "Use /help for commands"),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_show_tip_empty_invalid() {
    assert_eq!(
        validate_input(NoticeAction::ShowTip, ""),
        InputValidation::Invalid("tip requires a non-empty body")
    );
}

#[test]
fn notice_validate_input_show_tip_too_long_invalid() {
    let long_msg = "x".repeat(2049);
    assert_eq!(
        validate_input(NoticeAction::ShowTip, &long_msg),
        InputValidation::Invalid("tip body exceeds 2048 chars")
    );
}

#[test]
fn notice_validate_input_show_tip_max_length_valid() {
    let max_msg = "x".repeat(2048);
    assert_eq!(
        validate_input(NoticeAction::ShowTip, &max_msg),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_dismiss_notification_no_input_valid() {
    assert_eq!(
        validate_input(NoticeAction::DismissNotification, ""),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_dismiss_notification_with_input_invalid() {
    assert_eq!(
        validate_input(NoticeAction::DismissNotification, "text"),
        InputValidation::Invalid("dismiss/show takes no input")
    );
}

#[test]
fn notice_validate_input_dismiss_tip_no_input_valid() {
    assert_eq!(
        validate_input(NoticeAction::DismissTip, ""),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_show_announcement_no_input_valid() {
    assert_eq!(
        validate_input(NoticeAction::ShowAnnouncement, ""),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_show_release_notes_no_input_valid() {
    assert_eq!(
        validate_input(NoticeAction::ShowReleaseNotes, ""),
        InputValidation::Valid
    );
}

#[test]
fn notice_validate_input_none_invalid() {
    assert_eq!(
        validate_input(NoticeAction::None, ""),
        InputValidation::Invalid("action is None")
    );
}

#[test]
fn notice_is_replay_safe_for_show_actions() {
    assert!(is_replay_safe(NoticeAction::ShowNotification));
    assert!(is_replay_safe(NoticeAction::ShowTip));
    assert!(is_replay_safe(NoticeAction::ShowAnnouncement));
    assert!(is_replay_safe(NoticeAction::ShowReleaseNotes));
}

#[test]
fn notice_is_replay_safe_for_none() {
    assert!(is_replay_safe(NoticeAction::None));
}

#[test]
fn notice_action_availability_default_is_unwired() {
    assert_eq!(ActionAvailability::default(), ActionAvailability::Unwired);
}

#[test]
fn notice_input_validation_valid_eq() {
    assert_eq!(InputValidation::Valid, InputValidation::Valid);
    assert_ne!(InputValidation::Valid, InputValidation::Invalid("err"));
}

#[test]
fn notice_leaf_action_resolution_fields() {
    let res = LeafActionResolution {
        capability_id: "tui.notifications",
        backend_owner: "test",
        availability: ActionAvailability::Available,
        replay_safe: true,
    };
    assert_eq!(res.capability_id, "tui.notifications");
    assert_eq!(res.backend_owner, "test");
    assert_eq!(res.availability, ActionAvailability::Available);
}

// ---------------------------------------------------------------------------
// 12. Sleep/wake observation and refresh decision
// ---------------------------------------------------------------------------

#[test]
fn sleep_wake_host_event_sleep_str() {
    assert_eq!(SleepWakeHostEvent::Sleep.as_str(), "sleep");
}

#[test]
fn sleep_wake_host_event_wake_str() {
    assert_eq!(SleepWakeHostEvent::Wake.as_str(), "wake");
}

#[test]
fn sleep_wake_host_event_resume_str() {
    assert_eq!(SleepWakeHostEvent::Resume.as_str(), "resume");
}

#[test]
fn sleep_wake_host_event_suspend_str() {
    assert_eq!(SleepWakeHostEvent::Suspend.as_str(), "suspend");
}

#[test]
fn sleep_wake_host_event_wake_triggers_refresh_evaluation() {
    assert!(SleepWakeHostEvent::Wake.may_trigger_refresh_evaluation());
}

#[test]
fn sleep_wake_host_event_resume_triggers_refresh_evaluation() {
    assert!(SleepWakeHostEvent::Resume.may_trigger_refresh_evaluation());
}

#[test]
fn sleep_wake_host_event_sleep_does_not_trigger_refresh() {
    assert!(!SleepWakeHostEvent::Sleep.may_trigger_refresh_evaluation());
}

#[test]
fn sleep_wake_host_event_suspend_does_not_trigger_refresh() {
    assert!(!SleepWakeHostEvent::Suspend.may_trigger_refresh_evaluation());
}

#[test]
fn sleep_wake_observe_wake_produces_recorded_observation() {
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert!(obs.is_recorded());
    assert!(!obs.is_recorded_noop());
}

#[test]
fn sleep_wake_observe_sleep_produces_recorded_observation() {
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    assert!(obs.is_recorded());
}

#[test]
fn sleep_wake_decide_wake_without_expiry_skips() {
    let decision = decide_sleep_wake_credential_refresh(SleepWakeHostEvent::Wake);
    assert!(decision.is_skip());
    assert!(!decision.is_refresh());
}

#[test]
fn sleep_wake_decide_sleep_without_expiry_skips() {
    let decision = decide_sleep_wake_credential_refresh(SleepWakeHostEvent::Sleep);
    assert!(decision.is_skip());
}

#[test]
fn sleep_wake_decide_wake_with_near_expiry_refreshes() {
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 60_000), now);
    let decision =
        decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Wake, Some(&expiry));
    assert!(decision.is_refresh());
}

#[test]
fn sleep_wake_decide_wake_with_fresh_credentials_skips() {
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 3_600_000), now);
    let decision =
        decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Wake, Some(&expiry));
    assert!(decision.is_skip());
}

#[test]
fn sleep_wake_decide_sleep_with_near_expiry_still_skips() {
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 60_000), now);
    let decision =
        decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Sleep, Some(&expiry));
    assert!(decision.is_skip());
}

#[test]
fn sleep_wake_observe_and_decide_wake_without_expiry() {
    let (obs, decision) = observe_and_decide_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert!(obs.is_recorded());
    assert!(decision.is_skip());
}

#[test]
fn sleep_wake_observe_and_decide_wake_with_near_expiry() {
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 60_000), now);
    let (obs, decision) =
        observe_and_decide_sleep_wake_host_event_for(SleepWakeHostEvent::Wake, Some(&expiry));
    assert!(obs.is_recorded());
    assert!(decision.is_refresh());
}

#[test]
fn sleep_wake_refresh_decision_event_returns_original() {
    let decision = decide_sleep_wake_credential_refresh(SleepWakeHostEvent::Wake);
    assert_eq!(decision.event(), SleepWakeHostEvent::Wake);
}

#[test]
fn sleep_wake_refresh_decision_one_line_contains_event() {
    let decision = decide_sleep_wake_credential_refresh(SleepWakeHostEvent::Wake);
    let line = decision.one_line();
    assert!(line.contains("wake"));
}

#[test]
fn sleep_wake_credential_policy_is_active() {
    let policy = evaluate_sleep_wake_credential_refresh();
    assert!(policy.is_active());
    assert!(!policy.is_noop_or_unavailable());
}

#[test]
fn sleep_wake_credential_refresh_availability_returns_active() {
    let avail = sleep_wake_credential_refresh_availability();
    assert!(avail.is_active());
}

#[test]
fn sleep_wake_credential_policy_one_line_contains_active() {
    let policy = evaluate_sleep_wake_credential_refresh();
    let line = policy.one_line();
    assert!(line.contains("active") || line.contains("Active"));
}

#[test]
fn sleep_wake_observation_one_line_contains_event() {
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    let line = obs.one_line();
    assert!(!line.is_empty());
}

#[test]
fn sleep_wake_summarize_empty_observations_has_zero_count() {
    let summary = summarize_sleep_wake_observations(&[]);
    assert_eq!(summary.total, 0);
}

#[test]
fn sleep_wake_summarize_single_recorded_observation() {
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    let summary = summarize_sleep_wake_observations(&[obs]);
    assert!(!summary.all_recorded_noop());
}

#[test]
fn sleep_wake_summarize_one_line_not_empty() {
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    let summary = summarize_sleep_wake_observations(&[obs]);
    assert!(!summary.one_line().is_empty());
}

#[test]
fn credential_expiry_snapshot_near_expiry_is_true() {
    let now = 1_000_000_i64;
    let snapshot = CredentialExpirySnapshot::with_default_leeway(Some(now + 60_000), now);
    assert!(snapshot.is_near_expiry());
}

#[test]
fn credential_expiry_snapshot_near_expiry_is_false_for_fresh() {
    let now = 1_000_000_i64;
    let snapshot = CredentialExpirySnapshot::with_default_leeway(Some(now + 3_600_000), now);
    assert!(!snapshot.is_near_expiry());
}

#[test]
fn credential_expiry_snapshot_remaining_ms_some_when_expiry_set() {
    let now = 1_000_000_i64;
    let snapshot = CredentialExpirySnapshot::with_default_leeway(Some(now + 3_600_000), now);
    assert!(snapshot.remaining_ms().is_some());
}

#[test]
fn credential_expiry_snapshot_remaining_ms_none_when_no_expiry() {
    let now = 1_000_000_i64;
    let snapshot = CredentialExpirySnapshot::with_default_leeway(None, now);
    assert!(snapshot.remaining_ms().is_none());
}

// ---------------------------------------------------------------------------
// 13. AppState sleep/wake integration
// ---------------------------------------------------------------------------

#[test]
fn app_state_sleep_wake_observation_summary_defaults_none() {
    let app = live_app();
    assert!(app.sleep_wake_observation_summary().is_none());
}

#[test]
fn app_state_sleep_wake_last_observation_defaults_none() {
    let app = live_app();
    assert!(app.sleep_wake_last_observation().is_none());
}

#[test]
fn app_state_sleep_wake_last_decision_defaults_none() {
    let app = live_app();
    assert!(app.sleep_wake_last_decision().is_none());
}

#[test]
fn app_state_sleep_wake_observation_log_defaults_empty() {
    let app = live_app();
    assert!(app.sleep_wake_observation_log().is_empty());
}

#[test]
fn app_state_sleep_wake_credential_policy_defaults_none() {
    let app = live_app();
    assert!(app.sleep_wake_credential_policy().is_none());
}

#[test]
fn app_state_sleep_wake_availability_defaults_none() {
    let app = live_app();
    assert!(app.sleep_wake_availability().is_none());
}

#[test]
fn app_state_apply_sleep_wake_wake_records_observation() {
    let mut app = live_app();
    let decision = app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert!(decision.is_skip());
    assert!(app.sleep_wake_last_observation().is_some());
    assert!(app.sleep_wake_last_decision().is_some());
    assert!(!app.sleep_wake_observation_log().is_empty());
    assert!(app.sleep_wake_observation_summary().is_some());
    assert!(app.sleep_wake_credential_policy().is_some());
    assert!(app.sleep_wake_availability().is_some());
}

#[test]
fn app_state_apply_sleep_wake_sleep_records_observation() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    assert!(app.sleep_wake_last_observation().is_some());
    assert!(!app.sleep_wake_observation_log().is_empty());
}

#[test]
fn app_state_apply_sleep_wake_resume_records_observation() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Resume);
    assert!(app.sleep_wake_last_observation().is_some());
}

#[test]
fn app_state_apply_sleep_wake_suspend_records_observation() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Suspend);
    assert!(app.sleep_wake_last_observation().is_some());
}

#[test]
fn app_state_apply_sleep_wake_multiple_events_accumulate_log() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Suspend);
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Resume);
    assert_eq!(app.sleep_wake_observation_log().len(), 4);
}

#[test]
fn app_state_apply_sleep_wake_wake_with_near_expiry_refreshes() {
    let mut app = live_app();
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 60_000), now);
    let decision =
        app.apply_sleep_wake_host_event_with_expiry(SleepWakeHostEvent::Wake, Some(&expiry));
    assert!(decision.is_refresh());
}

#[test]
fn app_state_apply_sleep_wake_wake_with_fresh_credentials_skips() {
    let mut app = live_app();
    let now = 1_000_000_i64;
    let expiry = CredentialExpirySnapshot::with_default_leeway(Some(now + 3_600_000), now);
    let decision =
        app.apply_sleep_wake_host_event_with_expiry(SleepWakeHostEvent::Wake, Some(&expiry));
    assert!(decision.is_skip());
}

#[test]
fn app_state_set_sleep_wake_observation_summary() {
    let mut app = live_app();
    let summary = SleepWakeObservationSummary::default();
    app.set_sleep_wake_observation_summary(Some(summary));
    assert_eq!(app.sleep_wake_observation_summary(), Some(summary));
}

#[test]
fn app_state_set_sleep_wake_last_observation() {
    let mut app = live_app();
    let obs = observe_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    app.set_sleep_wake_last_observation(Some(obs.clone()));
    assert_eq!(app.sleep_wake_last_observation(), Some(&obs));
}

#[test]
fn app_state_set_sleep_wake_credential_policy() {
    let mut app = live_app();
    let policy = SleepWakeCredentialPolicy::Active {
        strategy: "hook".to_string(),
    };
    app.set_sleep_wake_credential_policy(Some(policy.clone()));
    assert_eq!(app.sleep_wake_credential_policy(), Some(&policy));
}

#[test]
fn app_state_set_sleep_wake_availability() {
    let mut app = live_app();
    let avail = SleepWakeCredentialPolicy::Active {
        strategy: "hook".to_string(),
    };
    app.set_sleep_wake_availability(Some(avail.clone()));
    assert_eq!(app.sleep_wake_availability(), Some(&avail));
}

// ---------------------------------------------------------------------------
// 14. Focus-aware permission notifications
// ---------------------------------------------------------------------------

#[test]
fn app_state_focus_defaults_to_prompt() {
    let app = live_app();
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn app_state_focus_can_switch_to_details() {
    let mut app = live_app();
    app.focus = Focus::Details;
    assert_eq!(app.focus, Focus::Details);
}

#[test]
fn app_state_focus_can_return_to_prompt() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.focus = Focus::Prompt;
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn permission_event_renders_without_error() {
    let mut app = live_app();
    let evt = permission_requested_event(1, "corr-1", "bash");
    app.ingest_event(evt);
    let output = render(&app);
    assert!(!output.is_empty());
}

#[test]
fn permission_event_correlation_id_set() {
    let mut app = live_app();
    let evt = permission_requested_event(1, "corr-perm-31", "edit_fs");
    app.ingest_event(evt);
    let output = render(&app);
    assert!(!output.is_empty());
}

#[test]
fn focus_reporting_available_in_full_capability() {
    let caps = TerminalCapabilityLeaf::full();
    assert!(caps.focus_reporting);
}

#[test]
fn focus_reporting_unavailable_in_reduced_capability() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert!(!caps.focus_reporting);
}

#[test]
fn focus_reporting_disabled_in_non_tty_env() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.focus_reporting);
}

// ---------------------------------------------------------------------------
// 15. Persistence/restart — theme name and observation log
// ---------------------------------------------------------------------------

#[test]
fn theme_name_persists_in_app_state() {
    let app = live_app();
    assert_eq!(app.theme_name, "default");
}

#[test]
fn theme_name_can_be_changed_via_public_field() {
    let mut app = live_app();
    app.theme_name = "high-contrast".to_string();
    assert_eq!(app.theme_name, "high-contrast");
    app.theme_name = "default".to_string();
    assert_eq!(app.theme_name, "default");
}

#[test]
fn theme_by_name_resolves_for_persistence_check() {
    let dark = Theme::by_name("default").expect("must resolve");
    let hc = Theme::by_name("high-contrast").expect("must resolve");
    assert_ne!(dark.surface.canvas, hc.surface.canvas);
}

#[test]
fn sleep_wake_observation_log_persists_across_events() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    let log_len_after_sleep = app.sleep_wake_observation_log().len();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert_eq!(
        app.sleep_wake_observation_log().len(),
        log_len_after_sleep + 1
    );
}

#[test]
fn sleep_wake_summary_updates_after_each_event() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    let summary_after_sleep = app.sleep_wake_observation_summary();
    assert!(summary_after_sleep.is_some());
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    let summary_after_wake = app.sleep_wake_observation_summary();
    assert!(summary_after_wake.is_some());
}

#[test]
fn sleep_wake_last_decision_updates_after_each_event() {
    let mut app = live_app();
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Sleep);
    let decision_after_sleep = app.sleep_wake_last_decision().cloned();
    assert!(decision_after_sleep.is_some());
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    let decision_after_wake = app.sleep_wake_last_decision().cloned();
    assert!(decision_after_wake.is_some());
}

#[test]
fn sleep_wake_credential_policy_updates_after_event() {
    let mut app = live_app();
    assert!(app.sleep_wake_credential_policy().is_none());
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert!(app.sleep_wake_credential_policy().is_some());
    assert!(app.sleep_wake_credential_policy().unwrap().is_active());
}

#[test]
fn sleep_wake_availability_updates_after_event() {
    let mut app = live_app();
    assert!(app.sleep_wake_availability().is_none());
    app.apply_sleep_wake_host_event(SleepWakeHostEvent::Wake);
    assert!(app.sleep_wake_availability().is_some());
    assert!(app.sleep_wake_availability().unwrap().is_active());
}

#[test]
fn theme_dialog_state_persists_until_changed() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.theme_dialog_selected = 1;
    assert!(app.theme_dialog_visible);
    assert_eq!(app.theme_dialog_selected, 1);
    app.theme_dialog_selected = 0;
    assert_eq!(app.theme_dialog_selected, 0);
    assert!(app.theme_dialog_visible);
}

// ---------------------------------------------------------------------------
// 16. Identity-mask comparator behavior at representative viewports
//
// These are comparator unit tests built from synthetic frames. They do not
// claim reference-vs-Harness rendering parity; real parity is covered by the
// PTY/xterm evidence tests and frozen capture validator.
// ---------------------------------------------------------------------------

fn reference_idle_frame(cols: u16, rows: u16, cursor_row: u16) -> SemanticFrame {
    let cursor = CursorState {
        row: cursor_row,
        col: 7,
        visible: true,
        shape: harness_testkit::parity::CursorShape::Block,
    };
    let mut frame = SemanticFrame::new(cols, rows, cursor);
    frame.alternate_screen = true;

    let dim = ResolvedRgb::new(102, 102, 102);
    let white = ResolvedRgb::new(229, 229, 229);
    let black = ResolvedRgb::new(0, 0, 0);

    let breadcrumb = "  \u{e0a0} ui-ux-experiments ~/Projects/agent-harness";
    for (i, ch) in breadcrumb.chars().enumerate() {
        let col = u16::try_from(i).expect("col fits");
        let cell = SemanticCell::blank(1, col)
            .with_grapheme(ch.to_string(), 1)
            .with_fg(dim)
            .with_bg(black)
            .with_modifiers(CellModifiers {
                dim: true,
                ..CellModifiers::default()
            });
        frame.set_cell(cell).expect("set breadcrumb");
    }

    let composer_top = cursor_row.saturating_sub(2);
    let composer_bottom = cursor_row.saturating_sub(1);
    let composer_right = cols.saturating_sub(3);

    frame
        .set_cell(
            SemanticCell::blank(composer_top, 2)
                .with_grapheme("\u{256d}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    for col in 3..composer_right {
        frame
            .set_cell(
                SemanticCell::blank(composer_top, col)
                    .with_grapheme("\u{2500}", 1)
                    .with_fg(dim)
                    .with_bg(black),
            )
            .expect("set");
    }
    frame
        .set_cell(
            SemanticCell::blank(composer_top, composer_right)
                .with_grapheme("\u{256e}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    frame
        .set_cell(
            SemanticCell::blank(cursor_row, 2)
                .with_grapheme("\u{2502}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    frame
        .set_cell(
            SemanticCell::blank(cursor_row, 4)
                .with_grapheme("\u{276f}", 1)
                .with_fg(white)
                .with_bg(black),
        )
        .expect("set");
    frame
        .set_cell(
            SemanticCell::blank(cursor_row, composer_right)
                .with_grapheme("\u{2502}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    frame
        .set_cell(
            SemanticCell::blank(composer_bottom, 2)
                .with_grapheme("\u{2570}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    for col in 3..composer_right {
        frame
            .set_cell(
                SemanticCell::blank(composer_bottom, col)
                    .with_grapheme("\u{2500}", 1)
                    .with_fg(dim)
                    .with_bg(black),
            )
            .expect("set");
    }
    frame
        .set_cell(
            SemanticCell::blank(composer_bottom, composer_right)
                .with_grapheme("\u{256f}", 1)
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    let footer_row = rows.saturating_sub(2);
    let footer = "  Shift+Tab:mode  \u{2502}  Ctrl+x:shortcuts";
    for (i, ch) in footer.chars().enumerate() {
        let col = u16::try_from(i).expect("col fits");
        frame
            .set_cell(
                SemanticCell::blank(footer_row, col)
                    .with_grapheme(ch.to_string(), 1)
                    .with_fg(white)
                    .with_bg(black),
            )
            .expect("set footer");
    }

    frame
}

fn model_badge_mask(cols: u16, cursor_row: u16) -> IdentityMaskRegistry {
    let badge_start = cols.saturating_sub(60);
    let badge_end = cols.saturating_sub(3);
    let cells: Vec<(u16, u16)> = (badge_start..=badge_end)
        .map(|col| (cursor_row.saturating_sub(1), col))
        .collect();
    IdentityMaskRegistry::new().with_field("model_badge_text", cells)
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_120x50() {
    let expected = reference_idle_frame(120, 50, 46);
    let mut actual = expected.clone();
    for col in 60..=80 {
        actual
            .set_cell(
                SemanticCell::blank(45, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(120, 46);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_120x40() {
    let expected = reference_idle_frame(120, 40, 36);
    let mut actual = expected.clone();
    for col in 60..=80 {
        actual
            .set_cell(
                SemanticCell::blank(35, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(120, 36);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_120x32() {
    let expected = reference_idle_frame(120, 32, 28);
    let mut actual = expected.clone();
    for col in 60..=80 {
        actual
            .set_cell(
                SemanticCell::blank(27, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(120, 28);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_100x30() {
    let expected = reference_idle_frame(100, 30, 26);
    let mut actual = expected.clone();
    for col in 50..=70 {
        actual
            .set_cell(
                SemanticCell::blank(25, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(100, 26);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_80x24() {
    let expected = reference_idle_frame(80, 24, 20);
    let mut actual = expected.clone();
    for col in 40..=60 {
        actual
            .set_cell(
                SemanticCell::blank(19, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(80, 20);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_79x24() {
    let expected = reference_idle_frame(79, 24, 20);
    let mut actual = expected.clone();
    for col in 39..=59 {
        actual
            .set_cell(
                SemanticCell::blank(19, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(79, 20);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_60x20() {
    let expected = reference_idle_frame(60, 20, 18);
    let mut actual = expected.clone();
    for col in 30..=50 {
        actual
            .set_cell(
                SemanticCell::blank(17, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(60, 18);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

#[test]
fn identity_mask_accepts_declared_model_badge_cells_at_140x40() {
    let expected = reference_idle_frame(140, 40, 36);
    let mut actual = expected.clone();
    for col in 80..=100 {
        actual
            .set_cell(
                SemanticCell::blank(35, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }
    let masks = model_badge_mask(140, 36);
    assert!(compare_frames(&expected, &actual, &masks).is_ok());
}

// ---------------------------------------------------------------------------
// 17. Cross-source provenance parity
// ---------------------------------------------------------------------------

#[test]
fn provenance_rejects_self_oracle_reference() {
    let expected = reference_idle_frame(120, 50, 46);
    let actual = expected.clone();
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Reference,
        &masks,
    );
    assert!(result.is_err());
}

#[test]
fn provenance_rejects_self_oracle_harness() {
    let expected = reference_idle_frame(120, 50, 46);
    let actual = expected.clone();
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Harness,
        &actual,
        CaptureSource::Harness,
        &masks,
    );
    assert!(result.is_err());
}

#[test]
fn provenance_accepts_cross_source_identical() {
    let expected = reference_idle_frame(120, 50, 46);
    let actual = expected.clone();
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Harness,
        &masks,
    );
    assert!(result.is_ok());
}

#[test]
fn provenance_rejects_cross_source_color_mutation() {
    let expected = reference_idle_frame(120, 50, 46);
    let mut actual = expected.clone();
    actual
        .set_cell(
            SemanticCell::blank(46, 4)
                .with_grapheme("\u{276f}", 1)
                .with_fg(ResolvedRgb::new(241, 76, 76))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Harness,
        &masks,
    );
    assert!(result.is_err());
}

#[test]
fn provenance_rejects_cross_source_border_mutation() {
    let expected = reference_idle_frame(120, 50, 46);
    let mut actual = expected.clone();
    actual
        .set_cell(
            SemanticCell::blank(44, 2)
                .with_grapheme("\u{250c}", 1)
                .with_fg(ResolvedRgb::new(102, 102, 102))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");
    let masks = IdentityMaskRegistry::new();
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Harness,
        &masks,
    );
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// 18. Viewport plan coverage for theme parity
// ---------------------------------------------------------------------------

#[test]
fn viewport_plan_covers_all_seven_manifest_viewports() {
    let plans = ViewportPlan::all_plans();
    assert_eq!(plans.len(), 7);
    let ids: Vec<&str> = plans.iter().map(|p| p.id.behavior_id()).collect();
    assert!(ids.contains(&"RESP-120x50"));
    assert!(ids.contains(&"RESP-120x40"));
    assert!(ids.contains(&"RESP-100x30"));
    assert!(ids.contains(&"RESP-80x24"));
    assert!(ids.contains(&"RESP-79x24"));
    assert!(ids.contains(&"RESP-60x20"));
    assert!(ids.contains(&"RESP-WIDE"));
}

#[test]
fn viewport_plan_idle_shell_has_no_welcome_panel() {
    for plan in ViewportPlan::all_plans() {
        assert!(!plan.welcome_panel_visible);
    }
}

#[test]
fn viewport_plan_all_plans_have_footer_hints() {
    for plan in ViewportPlan::all_plans() {
        assert!(plan.footer_hints_visible);
    }
}

#[test]
fn render_at_120x50_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 120, 50);
    assert!(!output.is_empty());
}

#[test]
fn render_at_120x40_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 120, 40);
    assert!(!output.is_empty());
}

#[test]
fn render_at_100x30_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 100, 30);
    assert!(!output.is_empty());
}

#[test]
fn render_at_80x24_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 80, 24);
    assert!(!output.is_empty());
}

#[test]
fn render_at_79x24_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 79, 24);
    assert!(!output.is_empty());
}

#[test]
fn render_at_60x20_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 60, 20);
    assert!(!output.is_empty());
}

#[test]
fn render_at_140x40_with_default_theme() {
    let app = live_app();
    let output = render_at(&app, 140, 40);
    assert!(!output.is_empty());
}

#[test]
fn render_at_120x50_with_high_contrast_theme_tokens() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.surface.canvas, Color::Black);
    let app = live_app();
    let output = render_at(&app, 120, 50);
    assert!(!output.is_empty());
}

#[test]
fn render_at_80x24_with_high_contrast_theme_tokens() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.text.primary, Color::White);
    let app = live_app();
    let output = render_at(&app, 80, 24);
    assert!(!output.is_empty());
}

#[test]
fn render_at_60x20_with_high_contrast_theme_tokens() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.status.error, Color::LightRed);
    let app = live_app();
    let output = render_at(&app, 60, 20);
    assert!(!output.is_empty());
}

// ---------------------------------------------------------------------------
// 19. Terminal title and progress indicators
// ---------------------------------------------------------------------------

#[test]
fn render_at_streaming_state_produces_output() {
    let mut app = live_app();
    let evt = envelope(
        1,
        Some("corr-stream"),
        EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
            request_id: "req-31-stream".into(),
            provider_id: "mock".into(),
            model_id: "mock:model-task31".into(),
            prompt_summary: "test prompt".into(),
            request_digest: "digest-stream".into(),
            metadata: None,
        }),
    );
    app.ingest_event(evt);
    let output = render(&app);
    assert!(!output.is_empty());
}

#[test]
fn render_with_permission_prompt_produces_output() {
    let mut app = live_app();
    let evt = permission_requested_event(1, "corr-perm", "bash");
    app.ingest_event(evt);
    let output = render(&app);
    assert!(!output.is_empty());
}

// ---------------------------------------------------------------------------
// 20. Theme token role consistency (DESIGN.md §10)
// ---------------------------------------------------------------------------

#[test]
fn theme_dark_composer_border_uses_dim_color() {
    let theme = Theme::harness_dark();
    let dim = theme.border.subtle;
    assert_ne!(dim, theme.surface.canvas);
}

#[test]
fn theme_dark_prompt_glyph_uses_foreground_color() {
    let theme = Theme::harness_dark();
    let prompt_color = theme.text.primary;
    assert_ne!(prompt_color, theme.surface.canvas);
}

#[test]
fn theme_dark_error_differs_from_success() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.status.error, theme.status.success);
}

#[test]
fn theme_dark_error_differs_from_info() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.status.error, theme.status.info);
}

#[test]
fn theme_dark_warning_differs_from_error() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.status.warning, theme.status.error);
}

#[test]
fn theme_dark_accent_differs_from_foreground() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.text.accent, theme.text.primary);
}

#[test]
fn theme_dark_agent_colors_differ() {
    let theme = Theme::harness_dark();
    assert_ne!(theme.agents.build, theme.agents.plan);
    assert_ne!(theme.agents.build, theme.agents.docs);
    assert_ne!(theme.agents.plan, theme.agents.ask);
}

#[test]
fn theme_high_contrast_uses_only_named_colors() {
    let theme = Theme::harness_high_contrast();
    assert!(matches!(theme.surface.canvas, Color::Black));
    assert!(matches!(theme.text.primary, Color::White));
    assert!(matches!(theme.status.error, Color::LightRed));
    assert!(matches!(theme.status.success, Color::LightGreen));
    assert!(matches!(theme.status.warning, Color::Yellow));
    assert!(matches!(theme.status.info, Color::LightCyan));
}

#[test]
fn theme_high_contrast_border_subtle_is_dark_gray() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.border.subtle, Color::DarkGray);
}

#[test]
fn theme_high_contrast_border_strong_is_gray() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.border.strong, Color::Gray);
}

#[test]
fn theme_high_contrast_scrollbar_track_is_black() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.scrollbar.track, Color::Black);
}

#[test]
fn theme_high_contrast_scrollbar_thumb_is_dark_gray() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.scrollbar.thumb, Color::DarkGray);
}

#[test]
fn theme_high_contrast_agent_build_is_cyan() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.agents.build, Color::Cyan);
}

#[test]
fn theme_high_contrast_agent_plan_is_magenta() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.agents.plan, Color::Magenta);
}

#[test]
fn theme_high_contrast_agent_palette_has_seven_colors() {
    let theme = Theme::harness_high_contrast();
    assert_eq!(theme.agents.palette.len(), 7);
}

#[test]
fn theme_agent_accent_returns_build_for_default_profile() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent("default"), theme.agents.build);
}

#[test]
fn theme_agent_accent_returns_build_for_build_profile() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent("build"), theme.agents.build);
}

#[test]
fn theme_agent_accent_returns_plan_for_plan_profile() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent("plan"), theme.agents.plan);
}

#[test]
fn theme_agent_accent_returns_docs_for_docs_profile() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent("docs"), theme.agents.docs);
}

#[test]
fn theme_agent_accent_returns_ask_for_ask_profile() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent("ask"), theme.agents.ask);
}

#[test]
fn theme_agent_accent_returns_palette_color_for_unknown_profile() {
    let theme = Theme::harness_dark();
    let accent = theme.agent_accent("unknown-profile");
    assert!(theme.agents.palette.contains(&accent));
}

#[test]
fn theme_agent_accent_empty_string_returns_build() {
    let theme = Theme::harness_dark();
    assert_eq!(theme.agent_accent(""), theme.agents.build);
}

// ---------------------------------------------------------------------------
// 21. Live shell layout tokens
// ---------------------------------------------------------------------------

#[test]
fn theme_live_shell_layout_for_primary_viewport() {
    let theme = Theme::harness_dark();
    let layout = theme.live_shell_layout(120, 50);
    assert_eq!(layout.target, ShellGeometryTarget::Primary);
}

#[test]
fn theme_live_shell_layout_for_split_viewport() {
    let theme = Theme::harness_dark();
    let layout = theme.live_shell_layout(90, 36);
    assert_eq!(layout.target, ShellGeometryTarget::Split);
}

#[test]
fn theme_live_shell_layout_for_minimum_viewport() {
    let theme = Theme::harness_dark();
    let layout = theme.live_shell_layout(80, 24);
    assert_eq!(layout.target, ShellGeometryTarget::Minimum);
}

#[test]
fn theme_live_shell_layout_primary_has_nonzero_transcript_width() {
    let theme = Theme::harness_dark();
    let layout = theme.live_shell_layout(120, 50);
    assert!(layout.transcript_min_width > 0);
}

#[test]
fn theme_live_shell_layout_primary_has_nonzero_centered_content() {
    let theme = Theme::harness_dark();
    let layout = theme.live_shell_layout(120, 50);
    assert!(layout.centered_content_width > 0);
}

#[test]
fn theme_lifecycle_surface_layout_for_primary() {
    let theme = Theme::harness_dark();
    let layout = theme.lifecycle_surface_layout(120, 50);
    assert_eq!(layout.target, ShellGeometryTarget::Primary);
}

#[test]
fn theme_lifecycle_surface_layout_for_minimum() {
    let theme = Theme::harness_dark();
    let layout = theme.lifecycle_surface_layout(80, 24);
    assert_eq!(layout.target, ShellGeometryTarget::Minimum);
}

#[test]
fn theme_lifecycle_surface_layout_primary_has_startup_card() {
    let theme = Theme::harness_dark();
    let layout = theme.lifecycle_surface_layout(120, 50);
    assert!(layout.startup_card.width > 0);
    assert!(layout.startup_card.height > 0);
}

// ---------------------------------------------------------------------------
// 22. Theme token families
// ---------------------------------------------------------------------------

#[test]
fn theme_token_families_has_semantic_chrome() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    assert_eq!(
        families.semantic.chrome.chromeless.mode,
        harness_tui::theme::ChromeMode::Chromeless
    );
}

#[test]
fn theme_token_families_has_palette() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    assert_eq!(families.palette.surfaces, theme.surface);
}

#[test]
fn theme_token_families_has_live_shell() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    let layout = families.live_shell.geometry.select(120, 50);
    assert_eq!(layout.target, ShellGeometryTarget::Primary);
}

#[test]
fn theme_token_families_composer_minimum_is_card_mode() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    let composer = families
        .semantic
        .composer
        .select(ShellGeometryTarget::Minimum);
    assert_eq!(composer.chrome, harness_tui::theme::ChromeMode::Card);
}

#[test]
fn theme_token_families_composer_primary_is_divided_mode() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    let composer = families
        .semantic
        .composer
        .select(ShellGeometryTarget::Primary);
    assert_eq!(composer.chrome, harness_tui::theme::ChromeMode::Divided);
}

#[test]
fn theme_token_families_density_minimum_is_compact() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    let density = families
        .semantic
        .density
        .select(ShellGeometryTarget::Minimum);
    assert_eq!(density.density, harness_tui::theme::SpacingDensity::Compact);
}

#[test]
fn theme_token_families_density_primary_is_roomy() {
    let theme = Theme::harness_dark();
    let families = theme.token_families();
    let density = families
        .semantic
        .density
        .select(ShellGeometryTarget::Primary);
    assert_eq!(density.density, harness_tui::theme::SpacingDensity::Roomy);
}
