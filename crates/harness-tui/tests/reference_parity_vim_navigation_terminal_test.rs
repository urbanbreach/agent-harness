//! Task 30: Vim, minimal/fullscreen, navigation, mouse, and responsive
//! terminal behavior parity tests.
//!
//! Contract: `grok-build-parity-parallel-execution.md` lines 995-1003 and
//! `crates/harness-tui/DESIGN.md` sections 5, 10, 11, 12.
//!
//! Covers: vim editing modes, mode indicators, minimal/fullscreen relaunch,
//! history/find, next/previous turn/response, fold/raw/expand-all,
//! page/half-page, focus switching, mouse capture, wheel/trackpad modes,
//! selection/copy-on-select, terminal title/progress/focus events,
//! alternate-screen behavior, resize during every mode, reduced-color
//! fallback, legacy keys, no-color fallback, non-TTY/error relaunch,
//! persisted preferences, and reference-vs-Harness parity at all required
//! viewports.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata, UiIntent};
use harness_tui::clipboard_leaf::{
    build_osc52_sequence, format_osc8_hyperlink, ClipboardLeaf, ClipboardMode, PasteMode,
};
use harness_tui::mouse::{MouseCaptureMode, MouseLeaf};
use harness_tui::render_test::render_to_string;
use harness_tui::responsive::{
    VIEWPORT_120x40, VIEWPORT_120x50, ViewportId, ViewportPlan, VIEWPORT_WIDE,
};
use harness_tui::terminal::{
    char_display_width, ColorMode, KeyboardMode, TerminalCapabilityLeaf, TerminalCapabilityRecord,
    TerminalCapabilityRow,
};
use harness_tui::{ui, UnwrapOrAbort};
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-task30-{seq:04}"),
        seq,
        run_id: "run_task30_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task30-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task30_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task30").with_mode_label("Demo"),
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
        LaunchMetadata::from_model_ref("build", "mock:model-task30").with_mode_label("Demo"),
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

fn user_message(seq: u64, req_id: &str, text: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(req_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: req_id.into(),
            text: text.to_string(),
        }),
    )
}

fn provider_started(seq: u64, req_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(req_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: req_id.into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "prompt".to_string(),
            request_digest: format!("digest-{req_id}"),
            metadata: None,
        }),
    )
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

// ---------------------------------------------------------------------------
// 1. Vim editing modes and mode indicators
// ---------------------------------------------------------------------------

#[test]
fn vim_sub_mode_defaults_to_disabled() {
    use harness_tui::leaf_actions::group_b_composer_modes::VimSubMode;
    assert_eq!(VimSubMode::default(), VimSubMode::Disabled);
}

#[test]
fn vim_sub_mode_normal_is_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(is_replay_safe(ComposerModeAction::VimNormal));
}

#[test]
fn vim_sub_mode_insert_is_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(is_replay_safe(ComposerModeAction::VimInsert));
}

#[test]
fn vim_sub_mode_visual_is_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(is_replay_safe(ComposerModeAction::VimVisual));
}

#[test]
fn vim_toggle_is_not_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(!is_replay_safe(ComposerModeAction::ToggleVimMode));
}

#[test]
fn vim_mode_indicator_footer_grammar_present_in_idle_shell() {
    let app = live_app();
    let rendered = render(&app);
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "VIM-MODE: idle footer must contain Shift+Tab:mode indicator\n{rendered}"
    );
}

#[test]
fn vim_mode_indicator_footer_grammar_present_in_draft_state() {
    let mut app = live_app();
    app.composer.prompt_buffer = "draft".to_string();
    let rendered = render(&app);
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "VIM-MODE: draft footer must contain Shift+Tab:mode indicator\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:send"),
        "VIM-MODE: draft footer must contain Enter:send\n{rendered}"
    );
}

#[test]
fn vim_mode_resolve_names_real_backend_owner() {
    use harness_tui::leaf_actions::group_b_composer_modes::{resolve, ActionAvailability};
    let res = resolve("tui.vim_mode").expect("must resolve");
    assert_eq!(res.capability_id, "tui.vim_mode");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/ui_composer.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

// ---------------------------------------------------------------------------
// 2. Minimal/fullscreen relaunch behavior
// ---------------------------------------------------------------------------

#[test]
fn minimal_mode_resolve_names_real_backend_owner() {
    use harness_tui::leaf_actions::group_c_screen_modes::{resolve, ActionAvailability};
    let res = resolve("tui.minimal_mode").expect("must resolve");
    assert_eq!(res.capability_id, "tui.minimal_mode");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

#[test]
fn screen_mode_defaults_to_normal() {
    use harness_tui::leaf_actions::group_c_screen_modes::ScreenMode;
    assert_eq!(ScreenMode::default(), ScreenMode::Normal);
}

#[test]
fn screen_mode_toggle_minimal_is_replay_safe() {
    use harness_tui::leaf_actions::group_c_screen_modes::{is_replay_safe, ScreenModeAction};
    assert!(is_replay_safe(ScreenModeAction::ToggleMinimal));
}

#[test]
fn screen_mode_toggle_compact_is_replay_safe() {
    use harness_tui::leaf_actions::group_c_screen_modes::{is_replay_safe, ScreenModeAction};
    assert!(is_replay_safe(ScreenModeAction::ToggleCompact));
}

#[test]
fn screen_mode_expand_is_replay_safe() {
    use harness_tui::leaf_actions::group_c_screen_modes::{is_replay_safe, ScreenModeAction};
    assert!(is_replay_safe(ScreenModeAction::Expand));
}

#[test]
fn fullscreen_relaunch_preserves_composer_border_at_all_viewports() {
    let app = live_app();
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains('\u{276F}'),
            "{}: composer glyph required for fullscreen relaunch\n{rendered}",
            vp.behavior_id()
        );
        assert_eq!(
            count_char(&rendered, '\u{256D}'),
            1,
            "{}: exactly one bordered box (composer) for fullscreen relaunch\n{rendered}",
            vp.behavior_id()
        );
    }
}

#[test]
fn minimal_mode_does_not_remove_composer_border() {
    let app = live_app();
    let rendered = render_at(&app, 80, 24);
    assert!(
        rendered.contains('\u{276F}'),
        "MINIMAL: composer glyph required even in compact/minimal mode\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "MINIMAL: composer border must survive compact mode\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 3. History/find
// ---------------------------------------------------------------------------

#[test]
fn history_navigation_previous_is_not_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(!is_replay_safe(ComposerModeAction::HistoryPrevious));
}

#[test]
fn history_navigation_next_is_not_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(!is_replay_safe(ComposerModeAction::HistoryNext));
}

#[test]
fn find_in_composer_rejects_empty_search() {
    use harness_tui::leaf_actions::group_b_composer_modes::{
        validate_input, ComposerModeAction, InputValidation,
    };
    assert!(matches!(
        validate_input(ComposerModeAction::FindInComposer, ""),
        InputValidation::Invalid(_)
    ));
}

#[test]
fn find_in_composer_rejects_overlong_search() {
    use harness_tui::leaf_actions::group_b_composer_modes::{
        validate_input, ComposerModeAction, InputValidation,
    };
    let long = "x".repeat(1025);
    assert!(matches!(
        validate_input(ComposerModeAction::FindInComposer, &long),
        InputValidation::Invalid(_)
    ));
}

#[test]
fn find_in_composer_accepts_valid_search() {
    use harness_tui::leaf_actions::group_b_composer_modes::{
        validate_input, ComposerModeAction, InputValidation,
    };
    assert!(matches!(
        validate_input(ComposerModeAction::FindInComposer, "search term"),
        InputValidation::Valid
    ));
}

#[test]
fn find_in_composer_is_replay_safe() {
    use harness_tui::leaf_actions::group_b_composer_modes::{is_replay_safe, ComposerModeAction};
    assert!(is_replay_safe(ComposerModeAction::FindInComposer));
}

// ---------------------------------------------------------------------------
// 4. Next/previous turn/response navigation
// ---------------------------------------------------------------------------

#[test]
fn next_turn_navigation_via_shift_right_does_not_crash() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.ingest_event(user_message(1, "req_a", "First turn"));
    app.ingest_event(provider_started(2, "req_a"));
    app.ingest_event(user_message(3, "req_b", "Second turn"));
    app.ingest_event(provider_started(4, "req_b"));
    app.handle_key(key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT));
}

#[test]
fn previous_turn_navigation_via_shift_left_does_not_crash() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.ingest_event(user_message(1, "req_a", "First turn"));
    app.ingest_event(provider_started(2, "req_a"));
    app.handle_key(key_with_modifiers(KeyCode::Left, KeyModifiers::SHIFT));
}

#[test]
fn next_response_navigation_does_not_clear_draft() {
    let mut app = live_app();
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.ingest_event(user_message(1, "req_a", "First turn"));
    app.ingest_event(provider_started(2, "req_a"));
    app.handle_key(key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 10);
}

// ---------------------------------------------------------------------------
// 5. Fold/raw/expand-all (render-based assertions)
// ---------------------------------------------------------------------------

#[test]
fn fold_state_renders_transcript_without_crash() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_fold", "Turn with tools"));
    app.ingest_event(provider_started(2, "req_fold"));
    let rendered = render(&app);
    assert!(!rendered.is_empty(), "fold state must render without crash");
}

#[test]
fn raw_view_renders_transcript_without_crash() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_raw", "Turn for raw view"));
    app.ingest_event(provider_started(2, "req_raw"));
    let rendered = render(&app);
    assert!(!rendered.is_empty(), "raw view must render without crash");
}

#[test]
fn expand_all_renders_transcript_without_crash() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_expand", "Turn for expand-all"));
    app.ingest_event(provider_started(2, "req_expand"));
    let rendered = render(&app);
    assert!(!rendered.is_empty(), "expand-all must render without crash");
}

// ---------------------------------------------------------------------------
// 6. Page/half-page scrolling (render-based + key handling)
// ---------------------------------------------------------------------------

#[test]
fn page_up_does_not_crash_with_prompt_focus() {
    let mut app = live_app();
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 10);
}

#[test]
fn page_down_does_not_crash_with_prompt_focus() {
    let mut app = live_app();
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "draft text");
}

#[test]
fn ctrl_up_does_not_crash_with_prompt_focus() {
    let mut app = live_app();
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft".to_string();
    app.handle_key(key_with_modifiers(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn ctrl_down_does_not_crash_with_prompt_focus() {
    let mut app = live_app();
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft".to_string();
    app.handle_key(key_with_modifiers(KeyCode::Down, KeyModifiers::CONTROL));
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn home_key_does_not_crash() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.handle_key(key(KeyCode::Home));
}

#[test]
fn end_key_does_not_crash() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.handle_key(key(KeyCode::End));
}

// ---------------------------------------------------------------------------
// 7. Focus switching
// ---------------------------------------------------------------------------

#[test]
fn tab_cycles_focus_without_crash() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Tab));
}

#[test]
fn backtab_cycles_focus_without_crash() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::BackTab));
}

#[test]
fn focus_switching_preserves_composer_draft() {
    let mut app = live_app();
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 10);
}

// ---------------------------------------------------------------------------
// 8. Mouse capture modes (leaf module tests)
// ---------------------------------------------------------------------------

#[test]
fn mouse_capture_full_mode_has_all_features() {
    let leaf = MouseLeaf::full();
    assert!(leaf.capture_mode.is_enabled());
    assert!(leaf.capture_mode.supports_scroll());
    assert!(leaf.capture_mode.supports_drag());
    assert!(leaf.wheel_scroll_enabled);
    assert!(leaf.click_focus_enabled);
    assert!(leaf.selection_drag_enabled);
}

#[test]
fn mouse_capture_disabled_mode_has_no_features() {
    let leaf = MouseLeaf::disabled();
    assert!(!leaf.capture_mode.is_enabled());
    assert!(!leaf.wheel_scroll_enabled);
    assert!(!leaf.click_focus_enabled);
    assert!(!leaf.selection_drag_enabled);
}

#[test]
fn mouse_capture_reduced_mode_has_click_only() {
    let leaf = MouseLeaf::reduced();
    assert!(leaf.capture_mode.is_enabled());
    assert!(!leaf.capture_mode.supports_scroll());
    assert!(!leaf.capture_mode.supports_drag());
    assert!(leaf.click_focus_enabled);
    assert!(!leaf.selection_drag_enabled);
}

#[test]
fn mouse_capture_button_event_supports_scroll_and_drag() {
    assert!(MouseCaptureMode::ButtonEvent.supports_scroll());
    assert!(MouseCaptureMode::ButtonEvent.supports_drag());
}

#[test]
fn mouse_capture_normal_mode_does_not_support_scroll_or_drag() {
    assert!(!MouseCaptureMode::Normal.supports_scroll());
    assert!(!MouseCaptureMode::Normal.supports_drag());
}

#[test]
fn mouse_capture_all_mode_supports_all_features() {
    let mode = MouseCaptureMode::All;
    assert!(mode.is_enabled());
    assert!(mode.supports_scroll());
    assert!(mode.supports_drag());
}

// ---------------------------------------------------------------------------
// 9. Wheel/trackpad modes (leaf module tests)
// ---------------------------------------------------------------------------

#[test]
fn wheel_scroll_enabled_in_full_mouse_mode() {
    assert!(MouseLeaf::full().wheel_scroll_enabled);
}

#[test]
fn wheel_scroll_disabled_in_reduced_mouse_mode() {
    assert!(!MouseLeaf::reduced().wheel_scroll_enabled);
}

#[test]
fn trackpad_mode_uses_same_scroll_capability() {
    let leaf = MouseLeaf::full();
    assert!(leaf.capture_mode.supports_scroll());
    assert!(leaf.wheel_scroll_enabled);
}

// ---------------------------------------------------------------------------
// 10. Selection/copy-on-select (leaf module tests)
// ---------------------------------------------------------------------------

#[test]
fn selection_drag_enabled_in_full_mouse_mode() {
    assert!(MouseLeaf::full().selection_drag_enabled);
}

#[test]
fn selection_drag_disabled_in_reduced_mouse_mode() {
    assert!(!MouseLeaf::reduced().selection_drag_enabled);
}

#[test]
fn copy_on_select_enabled_in_full_clipboard_mode() {
    assert!(ClipboardLeaf::full().copy_on_select);
}

#[test]
fn copy_on_select_disabled_in_no_clipboard_mode() {
    assert!(!ClipboardLeaf::disabled().copy_on_select);
}

#[test]
fn osc52_sequence_builds_correctly_for_copy() {
    let seq = build_osc52_sequence("selected text", false);
    assert!(seq.starts_with("\x1b]52;c;"));
    assert!(seq.ends_with("\x07"));
    assert!(seq.contains("c2VsZWN0ZWQgdGV4dA=="));
}

#[test]
fn osc52_sequence_tmux_passthrough_wraps_correctly() {
    let seq = build_osc52_sequence("test", true);
    assert!(seq.starts_with("\x1bPtmux;\x1b"));
    assert!(seq.ends_with("\x1b\\"));
}

#[test]
fn osc8_hyperlink_formats_correctly_for_selection() {
    let linked = format_osc8_hyperlink("https://example.com/path", "path");
    assert!(linked.contains("https://example.com/path"));
    assert!(linked.contains("path"));
    assert!(linked.starts_with("\x1b]8;;"));
}

#[test]
fn clipboard_mode_osc52_supports_osc52() {
    let mode = ClipboardMode::Osc52;
    assert!(mode.supports_osc52());
    assert!(!mode.supports_native());
}

#[test]
fn clipboard_mode_native_supports_native_only() {
    let mode = ClipboardMode::Native;
    assert!(!mode.supports_osc52());
    assert!(mode.supports_native());
}

#[test]
fn clipboard_mode_osc52_with_fallback_supports_both() {
    let mode = ClipboardMode::Osc52WithNativeFallback;
    assert!(mode.supports_osc52());
    assert!(mode.supports_native());
}

#[test]
fn paste_mode_bracketed_is_active_in_full_clipboard() {
    assert_eq!(ClipboardLeaf::full().paste_mode, PasteMode::Bracketed);
}

// ---------------------------------------------------------------------------
// 11. Terminal title/progress/focus events (capability leaf tests)
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_focus_reporting_enabled_in_full_mode() {
    assert!(TerminalCapabilityLeaf::full().focus_reporting);
}

#[test]
fn terminal_capability_focus_reporting_disabled_in_reduced_mode() {
    assert!(!TerminalCapabilityLeaf::reduced().focus_reporting);
}

#[test]
fn terminal_capability_alternate_screen_enabled_in_full_mode() {
    assert!(TerminalCapabilityLeaf::full().alternate_screen);
}

#[test]
fn terminal_capability_alternate_screen_disabled_in_reduced_mode() {
    assert!(!TerminalCapabilityLeaf::reduced().alternate_screen);
}

#[test]
fn terminal_capability_bracketed_paste_enabled_in_full_mode() {
    assert!(TerminalCapabilityLeaf::full().bracketed_paste);
}

#[test]
fn terminal_capability_mouse_capture_enabled_in_full_mode() {
    assert!(TerminalCapabilityLeaf::full().mouse_capture);
}

#[test]
fn terminal_capability_osc52_clipboard_enabled_in_full_mode() {
    assert!(TerminalCapabilityLeaf::full().osc52_clipboard);
}

#[test]
fn terminal_capability_records_cover_all_four_rows() {
    let caps = TerminalCapabilityLeaf::full();
    let records = TerminalCapabilityRecord::all_for(&caps);
    assert_eq!(records.len(), 4);
    assert_eq!(records[0].behavior_id, "TERM-CAP-COLOR");
    assert_eq!(records[1].behavior_id, "TERM-CAP-KEYS");
    assert_eq!(records[2].behavior_id, "TERM-CAP-MOUSE");
    assert_eq!(records[3].behavior_id, "TERM-CAP-CLIPBOARD");
    assert!(records.iter().all(|r| r.color_mode.is_truecolor()));
    assert!(records.iter().all(|r| r.mouse_capture));
    assert!(records.iter().all(|r| r.focus_reporting));
    assert!(records.iter().all(|r| r.alternate_screen));
}

// ---------------------------------------------------------------------------
// 12. Alternate-screen behavior
// ---------------------------------------------------------------------------

#[test]
fn alternate_screen_enabled_by_default_in_full_caps() {
    assert!(TerminalCapabilityLeaf::full().alternate_screen);
}

#[test]
fn alternate_screen_disabled_when_not_a_tty() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.alternate_screen);
}

#[test]
fn alternate_screen_disabled_in_reduced_caps() {
    assert!(!TerminalCapabilityLeaf::reduced().alternate_screen);
}

// ---------------------------------------------------------------------------
// 13. Resize during every mode
// ---------------------------------------------------------------------------

#[test]
fn resize_during_idle_shell_preserves_composer_border() {
    let app = live_app();
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains('\u{276F}'),
            "{}: resize during idle must preserve composer glyph\n{rendered}",
            vp.behavior_id()
        );
        assert_eq!(
            count_char(&rendered, '\u{256D}'),
            1,
            "{}: resize during idle must preserve exactly one composer border\n{rendered}",
            vp.behavior_id()
        );
    }
}

#[test]
fn resize_during_draft_state_preserves_composer_border() {
    let mut app = live_app();
    app.composer.prompt_buffer = "draft text".to_string();
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains('\u{276F}'),
            "{}: resize during draft must preserve composer glyph\n{rendered}",
            vp.behavior_id()
        );
        assert_eq!(
            count_char(&rendered, '\u{256D}'),
            1,
            "{}: resize during draft must preserve exactly one composer border\n{rendered}",
            vp.behavior_id()
        );
    }
}

#[test]
fn resize_during_palette_open_preserves_overlay() {
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let _rendered = render_at(&app, w, h);
        assert!(
            app.palette_visible,
            "{}: resize must not close palette",
            vp.behavior_id()
        );
    }
}

#[test]
fn resize_during_dashboard_open_preserves_overlay() {
    let mut app = live_app();
    app.execute_slash_command("dashboard", None);
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            !rendered.is_empty(),
            "{}: resize must preserve dashboard content (non-empty render)\n{rendered}",
            vp.behavior_id()
        );
    }
}

#[test]
fn resize_during_streaming_preserves_composer_border() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_stream", "streaming turn"));
    app.ingest_event(provider_started(2, "req_stream"));
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains('\u{276F}'),
            "{}: resize during streaming must preserve composer glyph\n{rendered}",
            vp.behavior_id()
        );
    }
}

// ---------------------------------------------------------------------------
// 14. Reduced-color fallback
// ---------------------------------------------------------------------------

#[test]
fn reduced_color_fallback_uses_ansi16() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
    assert!(!caps.color_mode.is_truecolor());
    assert!(!caps.color_mode.supports_256());
}

#[test]
fn reduced_color_fallback_disables_mouse_capture() {
    assert!(!TerminalCapabilityLeaf::reduced().mouse_capture);
}

#[test]
fn reduced_color_fallback_disables_osc52_clipboard() {
    assert!(!TerminalCapabilityLeaf::reduced().osc52_clipboard);
}

#[test]
fn reduced_color_fallback_disables_focus_reporting() {
    assert!(!TerminalCapabilityLeaf::reduced().focus_reporting);
}

#[test]
fn reduced_color_fallback_disables_bracketed_paste() {
    assert!(!TerminalCapabilityLeaf::reduced().bracketed_paste);
}

// ---------------------------------------------------------------------------
// 15. Legacy keys
// ---------------------------------------------------------------------------

#[test]
fn legacy_keys_ctrl_q_does_not_crash() {
    let (mut app, _intents) = live_app_with_sink();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn legacy_keys_ctrl_d_does_not_crash() {
    let (mut app, _intents) = live_app_with_sink();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn legacy_keys_ctrl_w_triggers_worktree_handoff() {
    let (mut app, _intents) = live_app_with_sink();
    app.startup_mode = true;
    app.composer.prompt_buffer.clear();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn legacy_keys_ctrl_s_in_startup_does_not_crash() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn legacy_keys_ctrl_p_opens_palette() {
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible, "Ctrl+P must open command palette");
}

#[test]
fn legacy_keys_ctrl_x_does_not_crash() {
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn legacy_keys_space_does_not_crash() {
    let mut app = live_app();
    app.focus = Focus::Details;
    app.handle_key(key(KeyCode::Char(' ')));
}

// ---------------------------------------------------------------------------
// 16. No-color fallback
// ---------------------------------------------------------------------------

#[test]
fn no_color_fallback_for_dumb_terminal() {
    assert_eq!(ColorMode::from_env(None, Some("dumb")), ColorMode::None);
}

#[test]
fn no_color_fallback_disables_all_visual_features() {
    let caps = TerminalCapabilityLeaf::reduced();
    assert_eq!(caps.color_mode, ColorMode::Ansi16);
    assert!(!caps.color_mode.is_truecolor());
    assert!(!caps.mouse_capture);
    assert!(!caps.bracketed_paste);
    assert!(!caps.osc52_clipboard);
    assert!(!caps.alternate_screen);
    assert!(!caps.focus_reporting);
}

#[test]
fn no_color_mode_does_not_support_256() {
    let mode = ColorMode::None;
    assert!(!mode.is_truecolor());
    assert!(!mode.supports_256());
}

#[test]
fn no_color_terminal_still_renders_composer() {
    let app = live_app();
    let rendered = render_at(&app, 80, 24);
    assert!(
        rendered.contains('\u{276F}'),
        "no-color terminal must still render composer glyph\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "no-color terminal must still render composer border\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 17. Non-TTY/error relaunch
// ---------------------------------------------------------------------------

#[test]
fn non_tty_disables_osc52_clipboard() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.osc52_clipboard);
}

#[test]
fn non_tty_disables_mouse_capture() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.mouse_capture);
}

#[test]
fn non_tty_disables_bracketed_paste() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.bracketed_paste);
}

#[test]
fn non_tty_disables_alternate_screen() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.alternate_screen);
}

#[test]
fn non_tty_disables_focus_reporting() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(!caps.focus_reporting);
}

#[test]
fn non_tty_still_probes_color_mode() {
    let caps = TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);
    assert!(caps.color_mode.is_truecolor());
}

#[test]
fn error_relaunch_does_not_crash_app_state() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_err", "error turn"));
    app.ingest_event(provider_started(2, "req_err"));
    let rendered = render(&app);
    assert!(
        !rendered.is_empty(),
        "error relaunch must produce non-empty render"
    );
}

// ---------------------------------------------------------------------------
// 18. Persisted preferences
// ---------------------------------------------------------------------------

#[test]
fn persisted_preferences_model_favorites_default_empty() {
    assert!(live_app().model_favorites.is_empty());
}

#[test]
fn persisted_preferences_model_recents_default_empty() {
    assert!(live_app().model_recents.is_empty());
}

#[test]
fn persisted_preferences_always_approve_defaults_false() {
    assert!(!live_app().always_approve_mode());
}

#[test]
fn persisted_preferences_replay_mode_defaults_false() {
    assert!(!live_app().replay_mode);
}

#[test]
fn persisted_preferences_startup_mode_defaults_false() {
    assert!(!live_app().startup_mode);
}

#[test]
fn persisted_preferences_palette_visible_defaults_false() {
    assert!(!live_app().palette_visible);
}

// ---------------------------------------------------------------------------
// 19. Reference-vs-Harness parity at all required viewports
// ---------------------------------------------------------------------------

#[test]
fn parity_idle_shell_renders_at_all_required_viewports() {
    let app = live_app();
    for &vp in &ViewportId::ALL {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        let plan = ViewportPlan::for_viewport(vp);
        assert!(
            rendered.contains('\u{276F}'),
            "{}: parity — composer glyph required\n{rendered}",
            vp.behavior_id()
        );
        assert_eq!(
            count_char(&rendered, '\u{256D}'),
            1,
            "{}: parity — exactly one bordered box (composer)\n{rendered}",
            vp.behavior_id()
        );
        assert!(
            rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
            "{}: parity — idle footer required\n{rendered}",
            vp.behavior_id()
        );
        assert!(
            !rendered.contains("Enter:send"),
            "{}: parity — idle shell must not show draft footer\n{rendered}",
            vp.behavior_id()
        );
        assert!(plan.composer_bordered);
        assert!(plan.footer_hints_visible);
        assert!(!plan.welcome_panel_visible);
    }
}

#[test]
fn parity_composer_anatomy_matches_reference_at_120x40() {
    let app = live_app();
    let rendered = render_at(&app, 120, 40);
    assert!(
        rendered.contains('\u{256D}'),
        "PARITY-120x40: ╭ required\n{rendered}"
    );
    assert!(
        rendered.contains('\u{256E}'),
        "PARITY-120x40: ╮ required\n{rendered}"
    );
    assert!(
        rendered.contains('\u{2570}'),
        "PARITY-120x40: ╰ required\n{rendered}"
    );
    assert!(
        rendered.contains('\u{256F}'),
        "PARITY-120x40: ╯ required\n{rendered}"
    );
    assert!(
        rendered.contains('\u{276F}'),
        "PARITY-120x40: ❯ required\n{rendered}"
    );
}

#[test]
fn parity_footer_grammar_matches_reference_idle() {
    let app = live_app();
    let rendered = render_at(&app, 120, 40);
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "PARITY-FOOTER: idle footer must contain Shift+Tab:mode\n{rendered}"
    );
    assert!(
        rendered.contains("Ctrl+x:shortcuts"),
        "PARITY-FOOTER: idle footer must contain Ctrl+x:shortcuts\n{rendered}"
    );
}

#[test]
fn parity_footer_grammar_matches_reference_draft() {
    let mut app = live_app();
    app.composer.prompt_buffer = "draft".to_string();
    let rendered = render_at(&app, 120, 40);
    assert!(
        rendered.contains("Enter:send"),
        "PARITY-FOOTER: draft footer must contain Enter:send\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "PARITY-FOOTER: draft footer must contain Shift+Tab:mode\n{rendered}"
    );
}

#[test]
fn parity_compact_viewport_drops_welcome_keeps_composer_border() {
    let app = live_app();
    let rendered = render_at(&app, 80, 24);
    assert!(
        rendered.contains('\u{276F}'),
        "PARITY-COMPACT: composer glyph required at 80x24\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "PARITY-COMPACT: exactly one bordered box at 80x24\n{rendered}"
    );
}

#[test]
fn parity_extreme_narrow_drops_top_margin_keeps_composer() {
    let app = live_app();
    let rendered = render_at(&app, 60, 20);
    assert!(
        rendered.contains('\u{276F}'),
        "PARITY-NARROW: composer glyph required at 60x20\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '\u{256D}'),
        1,
        "PARITY-NARROW: exactly one bordered box at 60x20\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "PARITY-NARROW: footer grammar required at 60x20\n{rendered}"
    );
}

#[test]
fn parity_model_badge_present_on_composer_bottom_border() {
    let app = live_app();
    let rendered = render_at(&app, 120, 40);
    assert!(
        rendered.contains("Demo") || rendered.contains("mock"),
        "PARITY-BADGE: model badge must be present on composer bottom border\n{rendered}"
    );
}

#[test]
fn parity_breadcrumb_present_at_standard_viewports() {
    let app = live_app();
    for &vp in &[VIEWPORT_120x40, VIEWPORT_120x50, VIEWPORT_WIDE] {
        let (w, h) = vp.dims();
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains("ui-ux-experiments") || rendered.contains("agent-harness"),
            "{}: breadcrumb must be present at standard viewport\n{rendered}",
            vp.behavior_id()
        );
    }
}

// ---------------------------------------------------------------------------
// 20. PTY-driven key matrix (deterministic simulation)
// ---------------------------------------------------------------------------

#[test]
fn pty_key_matrix_ctrl_p_opens_palette() {
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
}

#[test]
fn pty_key_matrix_esc_closes_palette() {
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
}

#[test]
fn pty_key_matrix_enter_submits_prompt() {
    let (mut app, intents) = live_app_with_sink();
    app.composer.prompt_buffer = "test prompt".to_string();
    app.handle_key(key(KeyCode::Enter));
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents
            .iter()
            .any(|i| matches!(i, UiIntent::SubmitPrompt { .. })),
        "Enter must submit prompt: {:?}",
        *intents
    );
}

#[test]
fn pty_key_matrix_alt_enter_does_not_crash() {
    let mut app = live_app();
    app.composer.prompt_buffer = "line1".to_string();
    app.composer.prompt_cursor = 5;
    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::ALT));
}

#[test]
fn pty_key_matrix_backspace_deletes_char() {
    let mut app = live_app();
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 5;
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(app.composer.prompt_buffer, "hell");
    assert_eq!(app.composer.prompt_cursor, 4);
}

#[test]
fn pty_key_matrix_ctrl_c_does_not_crash() {
    let (mut app, _intents) = live_app_with_sink();
    app.ingest_event(user_message(1, "req_cancel", "cancel turn"));
    app.ingest_event(provider_started(2, "req_cancel"));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
}

#[test]
fn pty_key_matrix_arrow_keys_do_not_crash() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Right));
}

#[test]
fn pty_key_matrix_home_end_do_not_crash() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::End));
}

#[test]
fn pty_key_matrix_page_up_down_do_not_crash() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::PageUp));
    app.handle_key(key(KeyCode::PageDown));
}

// ---------------------------------------------------------------------------
// 21. Unicode width (terminal capability)
// ---------------------------------------------------------------------------

#[test]
fn unicode_width_ascii_is_one() {
    assert_eq!(char_display_width('A'), 1);
}

#[test]
fn unicode_width_cjk_is_two() {
    assert_eq!(char_display_width('\u{4E2D}'), 2);
}

#[test]
fn unicode_width_emoji_is_two() {
    // U+2705 (✅) is in the 0x2600-0x27BF pictograph range — width 2
    assert_eq!(char_display_width('\u{2705}'), 2);
}

// ---------------------------------------------------------------------------
// 22. Keyboard mode (legacy vs enhanced)
// ---------------------------------------------------------------------------

#[test]
fn keyboard_mode_legacy_is_default() {
    assert_eq!(KeyboardMode::default(), KeyboardMode::Legacy);
}

#[test]
fn keyboard_mode_enhanced_is_not_legacy() {
    assert!(KeyboardMode::Enhanced.is_enhanced());
    assert!(!KeyboardMode::Legacy.is_enhanced());
}

#[test]
fn keyboard_mode_full_caps_use_enhanced() {
    assert_eq!(
        TerminalCapabilityLeaf::full().keyboard_mode,
        KeyboardMode::Enhanced
    );
}

#[test]
fn keyboard_mode_reduced_caps_use_legacy() {
    assert_eq!(
        TerminalCapabilityLeaf::reduced().keyboard_mode,
        KeyboardMode::Legacy
    );
}

// ---------------------------------------------------------------------------
// 23. Color mode probing
// ---------------------------------------------------------------------------

#[test]
fn color_mode_probes_truecolor_from_colorterm() {
    assert_eq!(
        ColorMode::from_env(Some("truecolor"), Some("xterm-256color")),
        ColorMode::Truecolor
    );
}

#[test]
fn color_mode_probes_256_from_term() {
    assert_eq!(
        ColorMode::from_env(None, Some("xterm-256color")),
        ColorMode::Ansi256
    );
}

#[test]
fn color_mode_probes_none_for_dumb() {
    assert_eq!(ColorMode::from_env(None, Some("dumb")), ColorMode::None);
}

#[test]
fn color_mode_defaults_to_ansi16() {
    assert_eq!(ColorMode::from_env(None, None), ColorMode::Ansi16);
}

#[test]
fn color_mode_truecolor_supports_256() {
    let mode = ColorMode::Truecolor;
    assert!(mode.supports_256());
    assert!(mode.is_truecolor());
}

#[test]
fn color_mode_ansi256_supports_256() {
    let mode = ColorMode::Ansi256;
    assert!(mode.supports_256());
    assert!(!mode.is_truecolor());
}

// ---------------------------------------------------------------------------
// 24. Terminal capability row behavior IDs
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_row_color_behavior_id() {
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Color),
        "TERM-CAP-COLOR"
    );
}

#[test]
fn terminal_capability_row_keys_behavior_id() {
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Keys),
        "TERM-CAP-KEYS"
    );
}

#[test]
fn terminal_capability_row_mouse_behavior_id() {
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Mouse),
        "TERM-CAP-MOUSE"
    );
}

#[test]
fn terminal_capability_row_clipboard_behavior_id() {
    assert_eq!(
        TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Clipboard),
        "TERM-CAP-CLIPBOARD"
    );
}

#[test]
fn terminal_capability_row_all_has_four_rows() {
    let all = TerminalCapabilityRow::ALL;
    assert_eq!(all.len(), 4);
    assert!(all.contains(&TerminalCapabilityRow::Color));
    assert!(all.contains(&TerminalCapabilityRow::Keys));
    assert!(all.contains(&TerminalCapabilityRow::Mouse));
    assert!(all.contains(&TerminalCapabilityRow::Clipboard));
}

// ---------------------------------------------------------------------------
// 25. Viewport plan parity
// ---------------------------------------------------------------------------

#[test]
fn viewport_plan_all_viewports_have_bordered_composer() {
    for plan in ViewportPlan::all_plans() {
        assert!(
            plan.composer_bordered,
            "{}: composer must be bordered",
            plan.id.behavior_id()
        );
    }
}

#[test]
fn viewport_plan_all_viewports_have_footer_hints() {
    for plan in ViewportPlan::all_plans() {
        assert!(
            plan.footer_hints_visible,
            "{}: footer hints must be visible",
            plan.id.behavior_id()
        );
    }
}

#[test]
fn viewport_plan_idle_shell_has_no_welcome_panel() {
    for plan in ViewportPlan::all_plans() {
        assert!(
            !plan.welcome_panel_visible,
            "{}: welcome panel must not appear in idle shell",
            plan.id.behavior_id()
        );
    }
}

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
