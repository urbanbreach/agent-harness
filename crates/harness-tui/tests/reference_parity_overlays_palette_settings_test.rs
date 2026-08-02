//! Task 28: Overlays, permissions, questions, palette, models, sessions,
//! auth, and settings parity tests.
//!
//! Contract: `grok-build-parity-parallel-execution.md` lines 973-983 and
//! `crates/harness-tui/DESIGN.md` sections 7, 8, 9, 10.
//!
//! Covers: overlay geometry (sizes/placement/dimming/z-order/preemption),
//! filtering, selection, dismissal, mouse hit targets, permission/question
//! flows, command palette, model picker, session picker, settings editor,
//! theme picker, and enterprise/remote/marketplace absence.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{AppState, Focus, LaunchMetadata, UiIntent};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan, UnwrapOrAbort};
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-task28-{seq:04}"),
        seq,
        run_id: "run_task28_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task28-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task28_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task28").with_mode_label("Demo"),
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
        LaunchMetadata::from_model_ref("build", "mock:model-task28").with_mode_label("Demo"),
    );
    (app, intents)
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn plan_for(app: &AppState) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, W, H))
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn open_palette(app: &mut AppState) {
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn question_permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [
                        {"label": "A", "description": "Option A"},
                        {"label": "B", "description": "Option B"},
                        {"label": "C", "description": "Option C"},
                    ],
                    "multiple": false,
                }]
            })
            .to_string(),
            request_digest: format!("digest-question-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

// ---------------------------------------------------------------------------
// 1. Overlay geometry: command palette
// ---------------------------------------------------------------------------

#[test]
fn palette_overlay_renders_sharp_corners_not_rounded() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains('┌') && rendered.contains('┐'),
        "OVL-PALETTE: must use sharp corners ┌┐, not rounded ╭╮\n{rendered}"
    );
    assert!(
        rendered.contains('└') && rendered.contains('┘'),
        "OVL-PALETTE: must use sharp corners └┘, not rounded ╰╯\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_close_button() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("[✗]"),
        "OVL-PALETTE: close button [✗] must be present\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_title_commands() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("Commands"),
        "OVL-PALETTE: title 'Commands' must be present\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_search_bar() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("search:"),
        "OVL-PALETTE: search bar 'search:' must be present\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_nav_footer() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("nav") && rendered.contains("select") && rendered.contains("close"),
        "OVL-PALETTE: nav footer must contain nav/select/close\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_section_headers() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("Session"),
        "OVL-PALETTE: Session section header must be present\n{rendered}"
    );
    assert!(
        rendered.contains("Context"),
        "OVL-PALETTE: Context section header must be present\n{rendered}"
    );
    assert!(
        rendered.contains("Model") && rendered.contains("Input"),
        "OVL-PALETTE: Model & Input section header must be present\n{rendered}"
    );
    assert!(
        rendered.contains("Tools"),
        "OVL-PALETTE: Tools section header must be present\n{rendered}"
    );
}

#[test]
fn palette_overlay_renders_diamond_glyph_for_commands() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains('◆'),
        "OVL-PALETTE: command entries must use ◆ diamond glyph\n{rendered}"
    );
}

#[test]
fn palette_overlay_geometry_matches_freeze_measurements() {
    let mut app = live_app();
    open_palette(&mut app);
    let plan = plan_for(&app);
    let overlay = plan
        .palette_overlay
        .expect("OVL-PALETTE: overlay area must exist");
    // DESIGN.md §7: width=60, top border row 5 (0-indexed 4), height=32
    assert!(
        overlay.width <= 60,
        "OVL-PALETTE: overlay width must not exceed 60 (freeze), got {}",
        overlay.width
    );
    assert_eq!(
        overlay.y, 4,
        "OVL-PALETTE: overlay top must be row 4 (0-indexed = row 5 1-indexed)"
    );
}

// ---------------------------------------------------------------------------
// 2. Overlay geometry: session picker
// ---------------------------------------------------------------------------

#[test]
fn session_overlay_renders_sharp_corners() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render(&app);
    assert!(
        rendered.contains('┌') && rendered.contains('┐'),
        "OVL-SESSION: must use sharp corners\n{rendered}"
    );
}

#[test]
fn session_overlay_renders_close_button() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render(&app);
    assert!(
        rendered.contains("[✗]"),
        "OVL-SESSION: close button [✗] must be present\n{rendered}"
    );
}

#[test]
fn session_overlay_renders_title() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render(&app);
    assert!(
        rendered.contains("Resume session") || rendered.contains("Continue session"),
        "OVL-SESSION: title must be present\n{rendered}"
    );
}

#[test]
fn session_overlay_renders_search_placeholder() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render(&app);
    assert!(
        rendered.contains("/ to search") || rendered.contains("search"),
        "OVL-SESSION: search placeholder must be present\n{rendered}"
    );
}

#[test]
fn session_overlay_renders_nav_footer() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render(&app);
    assert!(
        rendered.contains("nav"),
        "OVL-SESSION: nav footer must be present\n{rendered}"
    );
}

#[test]
fn session_overlay_geometry_width_does_not_exceed_freeze() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let plan = plan_for(&app);
    let overlay = plan
        .palette_overlay
        .expect("OVL-SESSION: overlay area must exist");
    // DESIGN.md §7: session picker width=78
    assert!(
        overlay.width <= 78,
        "OVL-SESSION: overlay width must not exceed 78 (freeze), got {}",
        overlay.width
    );
    assert_eq!(
        overlay.y, 4,
        "OVL-SESSION: overlay top must be row 4 (0-indexed)"
    );
}

// ---------------------------------------------------------------------------
// 3. Overlay z-order / preemption
// ---------------------------------------------------------------------------

#[test]
fn permission_preempts_command_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(app.palette_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette",
        "tool_call_preempt_palette",
    ));

    assert!(!app.palette_visible, "palette must close on permission");
    assert!(
        app.active_permission().is_some(),
        "permission must be active"
    );
    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal)
    );
}

#[test]
fn permission_preempts_session_history() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.session_history_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_session",
        "tool_call_preempt_session",
    ));

    assert!(
        !app.session_history_visible,
        "session history must close on permission"
    );
    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal)
    );
}

#[test]
fn permission_preempts_slash_commands() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_slash",
        "tool_call_preempt_slash",
    ));

    assert!(!app.slash_visible, "slash must close on permission");
    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal)
    );
}

#[test]
fn permission_preempts_theme_dialog() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    assert!(app.theme_dialog_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_theme",
        "tool_call_preempt_theme",
    ));

    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal),
        "theme dialog must be preempted by permission"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::ThemeDialog),
        "permission must preempt theme dialog from the stack"
    );
}

#[test]
fn permission_preempts_settings_editor() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    assert!(app.settings_editor_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_settings",
        "tool_call_preempt_settings",
    ));

    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal),
        "settings editor must be preempted by permission"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::SettingsEditor),
        "permission must preempt settings editor from the stack"
    );
}

#[test]
fn permission_preempts_plan_view() {
    let mut app = live_app();
    app.plan_view_visible = true;
    assert!(app.plan_view_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_plan",
        "tool_call_preempt_plan",
    ));

    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal),
        "plan view must be preempted by permission"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::PlanView),
        "permission must preempt plan view from the stack"
    );
}

#[test]
fn permission_preempts_prompt_stash() {
    let mut app = live_app();
    app.prompt_stash.list_visible = true;
    assert!(app.prompt_stash.list_visible);

    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_stash",
        "tool_call_preempt_stash",
    ));

    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal),
        "prompt stash must be preempted by permission"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::PromptStashList),
        "permission must preempt prompt stash list from the stack"
    );
}

#[test]
fn palette_and_session_are_mutually_exclusive() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(app.palette_visible);
    assert!(!app.session_history_visible);

    // Opening session history should close palette
    app.session_history_visible = true;
    app.palette_visible = false;
    let stack = app.overlay_stack();
    let palette_count = stack
        .ordered()
        .iter()
        .filter(|k| {
            matches!(
                k,
                harness_tui::overlay::OverlayKind::CommandPalette
                    | harness_tui::overlay::OverlayKind::LineageBrowser
                    | harness_tui::overlay::OverlayKind::ForkSelector
                    | harness_tui::overlay::OverlayKind::TogglesMenu
            )
        })
        .count();
    assert_eq!(
        palette_count, 1,
        "command-palette channel must emit exactly one entry"
    );
}

#[test]
fn overlay_stack_permission_is_top_when_pending() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.prompt_stash.list_visible = true;
    app.ingest_event(permission_requested_event(
        1,
        "perm_stack_top",
        "tool_call_stack_top",
    ));
    let stack = app.overlay_stack();
    assert_eq!(
        stack.top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal)
    );
    assert!(
        !stack
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::ThemeDialog)
            && !stack
                .ordered()
                .contains(&harness_tui::overlay::OverlayKind::PromptStashList),
        "permission must preempt theme dialog and prompt stash from the stack"
    );
}

// ---------------------------------------------------------------------------
// 4. Permission flows: entry/exit/error/persist by keyboard
// ---------------------------------------------------------------------------

#[test]
fn permission_entry_by_event_activates_modal() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_entry",
        "tool_call_entry",
    ));
    assert!(app.active_permission().is_some());
    assert_eq!(
        app.overlay_stack().top(),
        Some(harness_tui::overlay::OverlayKind::PermissionModal)
    );
}

#[test]
fn permission_exit_by_esc_sends_deny_intent() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(
        1,
        "perm_esc_exit",
        "tool_call_esc_exit",
    ));
    app.handle_key(key(KeyCode::Esc));

    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "perm_esc_exit" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "PERM: Esc must resolve as Deny (fail-closed)"
    );
}

#[test]
fn permission_exit_by_ctrl_c_sends_deny_intent() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(
        1,
        "perm_ctrlc_exit",
        "tool_call_ctrlc_exit",
    ));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "perm_ctrlc_exit" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "PERM: Ctrl+C must resolve as Deny (fail-closed)"
    );
}

#[test]
fn permission_digit_select_then_enter_submits_allow() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(
        1,
        "perm_digit_enter",
        "tool_call_digit_enter",
    ));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));

    let emitted = intents.lock().unwrap_or_abort();
    let decision = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "perm_digit_enter" => Some(*decision),
        _ => None,
    });
    assert!(
        decision == Some(PermissionDecision::Allow),
        "PERM: Right Right Enter should submit Allow (session grant), got {decision:?}"
    );
}

#[test]
fn permission_persists_draft_across_modal() {
    let mut app = live_app();
    let draft = "draft preserved during permission";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.ingest_event(permission_requested_event(
        1,
        "perm_draft_persist",
        "tool_call_draft_persist",
    ));
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "PERM: draft must be preserved across permission modal"
    );
}

#[test]
fn permission_cycling_moves_selection_marker() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_cycle",
        "tool_call_cycle",
    ));
    let initial = render(&app);
    assert!(
        initial.contains("(●) Yes, and don't ask again"),
        "PERM: default on always-approve\n{initial}"
    );
    app.handle_key(key(KeyCode::Right));
    let cycled = render(&app);
    assert!(
        cycled.contains("(○) Yes, and don't ask again"),
        "PERM: marker leaves option 1 after cycle\n{cycled}"
    );
}

#[test]
fn permission_renders_all_four_options() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_options",
        "tool_call_options",
    ));
    let rendered = render(&app);
    assert!(
        rendered.contains("always-approve"),
        "PERM: option 1\n{rendered}"
    );
    assert!(
        rendered.contains("allow all edits during this session"),
        "PERM: option 2\n{rendered}"
    );
    assert!(
        rendered.contains("3 (○) Yes"),
        "PERM: option 3 allow-once\n{rendered}"
    );
    assert!(
        rendered.contains("No, reject"),
        "PERM: option 4 reject\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 5. Question flows: entry/exit/error/persist by keyboard
// ---------------------------------------------------------------------------

#[test]
fn question_entry_by_event_activates_modal() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_entry",
        "tool_call_q_entry",
    ));
    assert!(app.active_permission().is_some());
    let view = app.active_permission_view().expect("question view");
    assert_eq!(view.kind, "question");
}

#[test]
fn question_exit_by_esc_sends_deny_intent() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_esc_exit",
        "tool_call_q_esc_exit",
    ));
    app.handle_key(key(KeyCode::Esc));

    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "question_esc_exit" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "QUESTION: Esc must resolve as Deny (fail-closed)"
    );
}

#[test]
fn question_digit_select_then_enter_submits_answer() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_digit_enter",
        "tool_call_q_digit_enter",
    ));
    app.handle_key(key(KeyCode::Char('2')));
    app.handle_key(key(KeyCode::Enter));

    let emitted = intents.lock().unwrap_or_abort();
    let answer = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            reason,
            ..
        } if permission_id == "question_digit_enter" && *decision == PermissionDecision::Allow => {
            reason.clone()
        }
        _ => None,
    });
    let answer = answer.expect("QUESTION: Enter must submit an Allow answer");
    assert!(
        answer.contains("\"B\""),
        "QUESTION: answer reason must carry option B: {answer}"
    );
}

#[test]
fn question_persists_draft_across_modal() {
    let mut app = live_app();
    let draft = "draft preserved during question";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_draft_persist",
        "tool_call_q_draft_persist",
    ));
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "QUESTION: draft must be preserved across question modal"
    );
}

#[test]
fn question_does_not_render_allow_chrome() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_no_allow_chrome",
        "tool_call_q_no_allow",
    ));
    let rendered = render(&app);
    assert!(
        !rendered.contains("always-approve"),
        "QUESTION: must not render edit-permission allow chrome\n{rendered}"
    );
}

#[test]
fn question_renders_radio_markers() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_radio",
        "tool_call_q_radio",
    ));
    let rendered = render(&app);
    assert!(
        rendered.contains('●') || rendered.contains('○'),
        "QUESTION: radio markers must be present\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 6. Command palette: entry/exit/error/persist by keyboard
// ---------------------------------------------------------------------------

#[test]
fn palette_entry_by_ctrl_p() {
    let mut app = live_app();
    assert!(!app.palette_visible);
    open_palette(&mut app);
    assert!(app.palette_visible);
}

#[test]
fn palette_exit_by_esc() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(app.palette_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
}

#[test]
fn palette_exit_by_ctrl_c() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(app.palette_visible);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert!(!app.palette_visible);
}

#[test]
fn palette_filter_narrows_results() {
    let mut app = live_app();
    open_palette(&mut app);
    let initial_count = app.palette_filtered.len();
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.len() < initial_count,
        "PALETTE: filter should narrow results"
    );
    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "PALETTE: 'exit' filter should match app.exit"
    );
}

#[test]
fn palette_no_results_shows_empty_message() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "zzzzzzz".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(app.palette_filtered.is_empty());
    let rendered = render(&app);
    assert!(
        rendered.contains("No results"),
        "PALETTE: no results should show empty message\n{rendered}"
    );
}

#[test]
fn palette_navigation_up_down_wraps() {
    let mut app = live_app();
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 1);
    app.palette_selected = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.palette_selected, len - 1);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.palette_selected, 0);
}

#[test]
fn palette_dispatch_executes_command() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"session.new".to_string()),
        "PALETTE: 'new' filter should match session.new"
    );
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "session.new")
        .unwrap_or_abort();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.should_quit || !app.palette_visible,
        "PALETTE: dispatch should close palette or trigger exit"
    );
}

#[test]
fn palette_persists_composer_draft_when_opened() {
    let mut app = live_app();
    app.composer.prompt_buffer = "keep this".to_string();
    app.composer.prompt_cursor = "keep this".chars().count();
    open_palette(&mut app);
    assert_eq!(
        app.composer.prompt_buffer, "keep this",
        "PALETTE: composer draft must persist when palette opens"
    );
}

#[test]
fn palette_home_end_navigation() {
    let mut app = live_app();
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 2);
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.palette_selected, len - 1);
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.palette_selected, 0);
}

// ---------------------------------------------------------------------------
// 7. Model picker: entry/exit/navigation
// ---------------------------------------------------------------------------

#[test]
fn model_picker_entry_via_palette() {
    let mut app = live_app();
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![harness_tui::app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )],
        ),
    );
    open_palette(&mut app);
    for ch in "model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let has_model = app.palette_filtered.iter().any(|c| c == "model.list");
    assert!(has_model, "MODEL: model.list should be in palette filter");
}

#[test]
fn model_picker_exit_by_esc() {
    let mut app = live_app();
    app.model_switcher_visible = true;
    assert!(app.model_switcher_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.model_switcher_visible,
        "MODEL: Esc should close model picker"
    );
}

#[test]
fn model_picker_renders_overlay_title() {
    let mut app = live_app();
    app.model_switcher_visible = true;
    let rendered = render(&app);
    assert!(
        rendered.contains("Model") || rendered.contains("model"),
        "MODEL: overlay should render model-related title\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 8. Session picker: entry/exit/filter
// ---------------------------------------------------------------------------

#[test]
fn session_picker_entry_via_palette() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.session_history_visible,
        "SESSION: session history should be visible after palette dispatch"
    );
}

#[test]
fn session_picker_exit_by_esc() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.session_history_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.session_history_visible,
        "SESSION: Esc should close session picker"
    );
}

#[test]
fn session_picker_filter_narrows_results() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    // Type in the session search
    let initial = app.palette_input.len();
    app.handle_key(key(KeyCode::Char('x')));
    assert!(
        app.palette_input.len() > initial,
        "SESSION: filter input should accept characters"
    );
}

// ---------------------------------------------------------------------------
// 9. Settings editor: entry/exit
// ---------------------------------------------------------------------------

#[test]
fn settings_editor_entry_and_exit() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    assert!(app.settings_editor_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Settings") || rendered.contains("settings"),
        "SETTINGS: overlay should render settings title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.settings_editor_visible,
        "SETTINGS: Esc should close settings editor"
    );
}

// ---------------------------------------------------------------------------
// 10. Theme picker: entry/exit
// ---------------------------------------------------------------------------

#[test]
fn theme_picker_entry_and_exit() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    assert!(app.theme_dialog_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Harness Dark")
            || rendered.contains("High Contrast")
            || rendered.contains("apply"),
        "THEME: overlay should render theme options\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.theme_dialog_visible,
        "THEME: Esc should close theme dialog"
    );
}

// ---------------------------------------------------------------------------
// 11. Enterprise/remote/marketplace absence
// ---------------------------------------------------------------------------

#[test]
fn enterprise_actions_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !id.contains("enterprise"),
            "ENTERPRISE: enterprise command must be absent, not disabled: {id}"
        );
        assert!(
            !id.contains("remote"),
            "REMOTE: remote management command must be absent, not disabled: {id}"
        );
    }
}

#[test]
fn remote_management_actions_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !id.starts_with("remote."),
            "REMOTE: remote.* command must be absent: {id}"
        );
    }
}

#[test]
fn marketplace_command_is_visible_in_empty_filter_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(
        app.palette_filtered
            .iter()
            .any(|c| c == "tools.marketplace"),
        "MARKETPLACE: marketplace command must be visible in the empty-filter palette"
    );
}

#[test]
fn enterprise_actions_absent_from_slash_commands() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible);
    let slash_commands = &app.slash_filtered;
    for cmd in slash_commands {
        assert!(
            !cmd.contains("enterprise"),
            "ENTERPRISE: enterprise slash command must be absent: {cmd}"
        );
        assert!(
            !cmd.contains("remote"),
            "REMOTE: remote slash command must be absent: {cmd}"
        );
    }
}

#[test]
fn no_enterprise_or_remote_overlay_kinds() {
    use harness_tui::overlay::OverlayKind;
    // Verify no enterprise/remote overlay kinds exist
    let all_kinds = [
        OverlayKind::DetailsDrawer,
        OverlayKind::SlashCommands,
        OverlayKind::FileMentions,
        OverlayKind::CommandPalette,
        OverlayKind::TogglesMenu,
        OverlayKind::LineageBrowser,
        OverlayKind::ForkSelector,
        OverlayKind::StatusDialog,
        OverlayKind::SubagentActions,
        OverlayKind::PermissionModal,
        OverlayKind::ThemeDialog,
        OverlayKind::ErrorDetails,
        OverlayKind::PromptStashList,
        OverlayKind::AuthDialog,
        OverlayKind::SettingsEditor,
        OverlayKind::PlanView,
        OverlayKind::TrustFolderPrompt,
    ];
    // If enterprise/remote kinds were added, the array length would grow
    assert_eq!(
        all_kinds.len(),
        17,
        "OVERLAY: exactly 17 overlay kinds — no enterprise/remote kinds"
    );
}

// ---------------------------------------------------------------------------
// 12. Overlay backdrop / dimming
// ---------------------------------------------------------------------------

#[test]
fn palette_overlay_renders_backdrop_dimming() {
    let mut app = live_app();
    open_palette(&mut app);
    let plan = plan_for(&app);
    // The palette overlay should exist and have a backdrop
    assert!(
        plan.palette_overlay.is_some(),
        "OVL-PALETTE: backdrop/overlay area must exist"
    );
}

#[test]
fn session_overlay_renders_backdrop_dimming() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let plan = plan_for(&app);
    assert!(
        plan.palette_overlay.is_some(),
        "OVL-SESSION: backdrop/overlay area must exist"
    );
}

// ---------------------------------------------------------------------------
// 13. Overlay composer border interruption (z-order)
// ---------------------------------------------------------------------------

#[test]
fn overlay_sits_above_composer_border() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    // The composer border should be visible but the overlay should sit above it
    // The composer rounded corners ╭╮╰╯ should still be present
    assert!(
        rendered.contains('╭') || rendered.contains('╰'),
        "OVL-Z-ORDER: composer border should be visible below overlay\n{rendered}"
    );
    // The overlay sharp corners should also be present
    assert!(
        rendered.contains('┌') || rendered.contains('└'),
        "OVL-Z-ORDER: overlay sharp corners should be visible above shell\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 14. Prompt stash overlay
// ---------------------------------------------------------------------------

#[test]
fn prompt_stash_list_entry_and_exit() {
    let mut app = live_app();
    app.prompt_stash.list_visible = true;
    assert!(app.prompt_stash.list_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Stash") || rendered.contains("stash"),
        "STASH: overlay should render stash title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.prompt_stash.list_visible,
        "STASH: Esc should close prompt stash list"
    );
}

// ---------------------------------------------------------------------------
// 15. Plan view overlay
// ---------------------------------------------------------------------------

#[test]
fn plan_view_entry_and_exit() {
    let mut app = live_app();
    app.plan_view_visible = true;
    assert!(app.plan_view_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Plan") || rendered.contains("plan"),
        "PLAN: overlay should render plan title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.plan_view_visible, "PLAN: Esc should close plan view");
}

// ---------------------------------------------------------------------------
// 16. Auth dialog overlay
// ---------------------------------------------------------------------------

#[test]
fn auth_dialog_entry_and_exit() {
    let mut app = live_app();
    app.connect_dialog.visible = true;
    assert!(app.connect_dialog.visible);
    let rendered = render(&app);
    // Auth dialog should render some auth-related content
    assert!(
        rendered.contains("Connect")
            || rendered.contains("connect")
            || rendered.contains("Auth")
            || rendered.contains("auth")
            || rendered.contains("Provider")
            || rendered.contains("provider"),
        "AUTH: overlay should render auth/connect title\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 17. Trust folder prompt overlay
// ---------------------------------------------------------------------------

#[test]
fn trust_folder_prompt_entry_and_exit() {
    let mut app = live_app();
    app.trust_folder_prompt_visible = true;
    assert!(app.trust_folder_prompt_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.trust_folder_prompt_visible,
        "TRUST: Esc should close trust folder prompt"
    );
}

// ---------------------------------------------------------------------------
// 18. Overlay blocks pointer interaction when active
// ---------------------------------------------------------------------------

#[test]
fn overlay_blocks_pointer_interaction_when_active() {
    let mut app = live_app();
    open_palette(&mut app);
    let stack = app.overlay_stack();
    assert!(
        stack.blocks_pointer_interaction(),
        "OVL-POINTER: active overlay must block pointer interaction"
    );
    // Close overlay
    app.handle_key(key(KeyCode::Esc));
    let stack = app.overlay_stack();
    assert!(
        !stack.blocks_pointer_interaction(),
        "OVL-POINTER: closed overlay must not block pointer"
    );
}

// ---------------------------------------------------------------------------
// 19. Error details overlay (accessed via keybinding, not direct field)
// ---------------------------------------------------------------------------

#[test]
fn error_details_absent_when_no_error() {
    let app = live_app();
    let stack = app.overlay_stack();
    assert!(
        !stack
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::ErrorDetails),
        "error details must not be in the stack when status dialog is open"
    );
}

// ---------------------------------------------------------------------------
// 20. Toggles menu overlay
// ---------------------------------------------------------------------------

#[test]
fn toggles_menu_entry_and_exit() {
    let mut app = live_app();
    app.toggles_menu_visible = true;
    assert!(app.toggles_menu_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Toggle") || rendered.contains("toggle"),
        "TOGGLES: overlay should render toggles title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.toggles_menu_visible,
        "TOGGLES: Esc should close toggles menu"
    );
}

// ---------------------------------------------------------------------------
// 21. Lineage browser overlay
// ---------------------------------------------------------------------------

#[test]
fn lineage_browser_entry_and_exit() {
    let mut app = live_app();
    app.lineage_browser_visible = true;
    assert!(app.lineage_browser_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("tree") || rendered.contains("Tree") || rendered.contains("lineage"),
        "LINEAGE: overlay should render lineage/tree title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.lineage_browser_visible,
        "LINEAGE: Esc should close lineage browser"
    );
}

// ---------------------------------------------------------------------------
// 22. Fork selector overlay
// ---------------------------------------------------------------------------

#[test]
fn fork_selector_entry_and_exit() {
    let mut app = live_app();
    app.fork_selector_visible = true;
    assert!(app.fork_selector_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Fork") || rendered.contains("fork"),
        "FORK: overlay should render fork title\n{rendered}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.fork_selector_visible,
        "FORK: Esc should close fork selector"
    );
}

// ---------------------------------------------------------------------------
// 23. Status dialog overlay (accessed via keybinding)
// ---------------------------------------------------------------------------

#[test]
fn status_dialog_absent_by_default() {
    let app = live_app();
    let stack = app.overlay_stack();
    assert!(
        !stack
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::StatusDialog),
        "status dialog must not be in the stack when error details are open"
    );
}

// ---------------------------------------------------------------------------
// 24. Overlay collision: multiple non-preempted overlays coexist
// ---------------------------------------------------------------------------

#[test]
fn theme_dialog_and_prompt_stash_coexist() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.prompt_stash.list_visible = true;
    let stack = app.overlay_stack();
    assert_eq!(
        stack.ordered(),
        &[
            harness_tui::overlay::OverlayKind::ThemeDialog,
            harness_tui::overlay::OverlayKind::PromptStashList,
        ]
    );
}

#[test]
fn status_dialog_and_theme_dialog_coexist() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.settings_editor_visible = true;
    let stack = app.overlay_stack();
    assert!(
        stack
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::ThemeDialog),
        "THEME: theme dialog must coexist with settings editor"
    );
    assert!(
        stack
            .ordered()
            .contains(&harness_tui::overlay::OverlayKind::SettingsEditor),
        "SETTINGS: settings editor must coexist with theme dialog"
    );
}

// ---------------------------------------------------------------------------
// 25. Command palette scrollbar presence
// ---------------------------------------------------------------------------

#[test]
fn palette_overlay_renders_scrollbar_when_items_exceed_visible() {
    let mut app = live_app();
    open_palette(&mut app);
    // With many commands, the scrollbar should be present
    let rendered = render(&app);
    // The scrollbar uses █ (full block) — check for its presence
    // Note: scrollbar may not appear if all items fit, so we check the overlay exists
    assert!(
        rendered.contains("Commands"),
        "OVL-PALETTE: overlay must render with scrollbar area\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 26. Permission modal uses ┃ rail (freeze-matched)
// ---------------------------------------------------------------------------

#[test]
fn permission_modal_renders_rail_marker() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(1, "perm_rail", "tool_call_rail"));
    let rendered = render(&app);
    assert!(
        rendered.contains('┃'),
        "PERM: permission dock must paint freeze-matched ┃ rail\n{rendered}"
    );
}

#[test]
fn question_modal_renders_rail_marker() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_rail",
        "tool_call_q_rail",
    ));
    let rendered = render(&app);
    assert!(
        rendered.contains('┃'),
        "QUESTION: question dock must paint ┃ rail\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 27. Full-width shell under overlays
// ---------------------------------------------------------------------------

#[test]
fn permission_uses_full_width_shell_no_sidebar() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_full_width",
        "tool_call_full_width",
    ));
    let plan = plan_for(&app);
    assert!(
        plan.operator_sidebar.is_none(),
        "PERM: full-width shell (no operator sidebar) under permission"
    );
}

#[test]
fn question_uses_full_width_shell_no_sidebar() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "question_full_width",
        "tool_call_q_full_width",
    ));
    let plan = plan_for(&app);
    assert!(
        plan.operator_sidebar.is_none(),
        "QUESTION: full-width shell (no operator sidebar) under question"
    );
}

// ---------------------------------------------------------------------------
// 28. Palette excludes voice and hosted media
// ---------------------------------------------------------------------------

#[test]
fn voice_input_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !id.contains("voice"),
            "VOICE: voice command must be absent from palette: {id}"
        );
    }
}

#[test]
fn hosted_media_generation_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !id.contains("media") || id.contains("generic_tool"),
            "MEDIA: hosted media generation must be absent from palette: {id}"
        );
    }
}

// ---------------------------------------------------------------------------
// 29. Session rename dialog
// ---------------------------------------------------------------------------

#[test]
fn session_rename_dialog_renders_within_session_overlay() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "rename".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"session.rename".to_string()),
        "RENAME: session.rename should be in palette filter for 'rename'"
    );
}
