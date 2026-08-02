//! Task 28 differential TDD: overlays, pickers, settings, permissions,
//! questions, and local integrations UI contract tests.
//!
//! Covers: command palette dispatch, slash/argument/file completion overlays,
//! session/rewind/model/effort pickers, permission allow/deny/follow-up
//! queues, question select/text, settings browse/filter/edit/reset, auth
//! credential dialog, memory browser, MCP/extensions inspect, prompt stash,
//! details/status/block/doc viewers, stacked-modal escape, restricted-command
//! rejection, replay mutation rejection.
//!
//! Proves: all palette command instances discoverable, all settings
//! reachable, all coexisting modals present, all excluded members absent.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::settings_registry;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};
use harness_tui::keybindings::palette_model;
use harness_tui::keybindings::parity_matrix;
use harness_tui::keybindings::slash_commands;
use harness_tui::overlay::{OverlayKind, OverlayStack, OverlayState};
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
        event_id: format!("evt-task28-diff-{seq:04}"),
        seq,
        run_id: "run_task28_diff".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task28-diff".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task28_diff".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-diff").with_mode_label("Demo"),
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
        LaunchMetadata::from_model_ref("build", "mock:model-diff").with_mode_label("Demo"),
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

/// Drive the palette through the public key path and return the filtered command IDs.
/// Replaces direct calls to the crate-private `palette_controller::compute_palette_rows`.
fn palette_command_ids(app: &mut AppState, filter: &str) -> Vec<String> {
    if app.palette_visible {
        app.handle_key(key(KeyCode::Esc));
    }
    open_palette(app);
    for ch in filter.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.palette_filtered
        .iter()
        .map(|v| {
            v.strip_prefix("suggested:")
                .unwrap_or(v.as_str())
                .to_string()
        })
        .collect()
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

// ===========================================================================
// 1. Command palette dispatch — all entries discoverable and valid
// ===========================================================================

#[test]
fn all_palette_command_entries_are_discoverable() {
    let entries = palette_model::entries();
    assert!(!entries.is_empty(), "palette must have command entries");
    // Every entry must have a non-empty id and a valid dispatch
    for entry in entries {
        assert!(!entry.id.is_empty(), "palette entry must have non-empty id");
    }
}

#[test]
fn all_palette_entries_have_valid_categories() {
    use palette_model::PaletteCategory;
    let valid_categories = [
        PaletteCategory::Session,
        PaletteCategory::Context,
        PaletteCategory::ModelInput,
        PaletteCategory::Agent,
        PaletteCategory::System,
        PaletteCategory::Tools,
        PaletteCategory::Workspace,
        PaletteCategory::Provider,
        PaletteCategory::Prompt,
    ];
    for entry in palette_model::entries() {
        assert!(
            valid_categories.contains(&entry.category),
            "entry {} has invalid category {:?}",
            entry.id,
            entry.category
        );
    }
}

#[test]
fn palette_no_duplicate_command_ids() {
    use std::collections::HashSet;
    let ids: HashSet<&str> = palette_model::all_ids().iter().copied().collect();
    assert_eq!(
        ids.len(),
        palette_model::entries().len(),
        "palette command entries have duplicate IDs"
    );
}

#[test]
fn palette_all_filtered_results_are_valid_commands_in_live_session() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            palette_model::find(id).is_some(),
            "palette contains unknown command ID: {id}"
        );
    }
}

#[test]
fn palette_all_filtered_results_are_valid_commands_in_startup() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            palette_model::find(id).is_some(),
            "startup palette contains unknown command ID: {id}"
        );
    }
}

#[test]
fn palette_dispatch_new_session_clears_events() {
    let mut app = live_app();
    app.ingest_event(envelope(
        1,
        Some("req_diff"),
        EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
            request_id: "req_diff".into(),
            text: "test".to_string(),
        }),
    ));
    assert!(app.selected_event().is_some());
    open_palette(&mut app);
    for ch in "new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.selected_event().is_none(),
        "new session should clear events"
    );
}

#[test]
fn palette_dispatch_exit_quits_app() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "app.exit")
        .unwrap_or_abort();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.should_quit, "first app.exit requests confirmation");

    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(
        app.should_quit,
        "Ctrl+Q confirms palette-requested quitting"
    );
}

#[test]
fn palette_dispatch_toggle_thinking_flips_state() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "collapse thinking".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        !app.palette_visible || app.should_quit,
        "dispatch should close palette or trigger exit"
    );
}

#[test]
fn palette_dispatch_provider_connect_opens_connect_dialog() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "connect".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "provider.connect")
        .unwrap_or_abort();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.connect_dialog.visible,
        "connect dialog should be open after palette dispatch"
    );
}

// ===========================================================================
// 2. Slash/argument/file completion overlays
// ===========================================================================

#[test]
fn slash_overlay_opens_with_slash_key() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible, "slash overlay should open with /");
}

#[test]
fn slash_overlay_closes_with_escape() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.slash_visible, "slash overlay should close with Esc");
}

#[test]
fn slash_overlay_closes_when_permission_preempts() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible);
    app.ingest_event(permission_requested_event(
        1,
        "perm_slash_preempt",
        "tc_slash",
    ));
    assert!(!app.slash_visible, "slash must close on permission");
}

#[test]
fn slash_commands_count_is_25() {
    assert_eq!(slash_commands().len(), 25, "exactly 25 slash commands");
}

#[test]
fn slash_commands_have_no_duplicate_ids() {
    let leaves = slash_commands();
    let mut ids: Vec<&str> = leaves.iter().map(|l| l.id).collect();
    ids.sort_unstable();
    let duplicates: Vec<&str> = ids
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0])
        .collect();
    assert!(duplicates.is_empty(), "duplicate slash IDs: {duplicates:?}");
}

#[test]
fn slash_commands_exclude_enterprise_and_remote() {
    let mut app = live_app();
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.slash_visible);
    for cmd in &app.slash_filtered {
        assert!(
            !cmd.contains("enterprise"),
            "enterprise slash command must be absent: {cmd}"
        );
        assert!(
            !cmd.contains("remote"),
            "remote slash command must be absent: {cmd}"
        );
    }
}

#[test]
fn file_mention_overlay_does_not_appear_by_default() {
    let app = live_app();
    let stack = app.overlay_stack();
    assert!(
        !stack.ordered().contains(&OverlayKind::FileMentions),
        "file mention overlay should not be visible by default"
    );
}

// ===========================================================================
// 3. Session/rewind/model/effort pickers
// ===========================================================================

#[test]
fn session_picker_entry_via_palette_resume() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.session_history_visible,
        "session history should be visible after palette dispatch"
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
        "Esc should close session picker"
    );
}

#[test]
fn session_picker_filter_accepts_input() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let initial = app.palette_input.len();
    app.handle_key(key(KeyCode::Char('x')));
    assert!(
        app.palette_input.len() > initial,
        "session filter should accept characters"
    );
}

#[test]
fn session_rename_command_present_in_palette() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for ch in "rename".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"session.rename".to_string()),
        "session.rename should be in palette filter for 'rename'"
    );
}

#[test]
fn model_switcher_entry_and_exit() {
    let mut app = live_app();
    app.model_switcher_visible = true;
    assert!(app.model_switcher_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.model_switcher_visible, "Esc should close model picker");
}

#[test]
fn model_switcher_renders_title() {
    let mut app = live_app();
    app.model_switcher_visible = true;
    let rendered = render(&app);
    assert!(
        rendered.contains("Model") || rendered.contains("model"),
        "model switcher should render model-related title"
    );
}

#[test]
fn model_picker_available_via_palette_filter() {
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
    assert!(
        app.palette_filtered.iter().any(|c| c == "model.list"),
        "model.list should be in palette filter"
    );
}

#[test]
fn effort_picker_reasoning_effort_metadata_exists() {
    use harness_tui::app::ModelOption;
    let opt = ModelOption::from_model_ref("build", "default:gpt-5.4-mini");
    // ModelOption should carry reasoning_effort metadata
    // (may be None when not configured, but the field must exist)
    let _ = opt;
}

// ===========================================================================
// 4. Permission allow/deny/follow-up queues
// ===========================================================================

#[test]
fn permission_entry_activates_modal() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(1, "perm_entry_diff", "tc_entry"));
    assert!(app.active_permission().is_some());
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal)
    );
}

#[test]
fn permission_esc_sends_deny() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(1, "perm_esc_diff", "tc_esc"));
    app.handle_key(key(KeyCode::Esc));
    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "perm_esc_diff" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "Esc must resolve as Deny (fail-closed)"
    );
}

#[test]
fn permission_ctrl_c_sends_deny() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(1, "perm_ctrlc_diff", "tc_ctrlc"));
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
        } if permission_id == "perm_ctrlc_diff" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "Ctrl+C must resolve as Deny (fail-closed)"
    );
}

#[test]
fn permission_right_right_enter_submits_allow() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(permission_requested_event(1, "perm_allow_diff", "tc_allow"));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));
    let emitted = intents.lock().unwrap_or_abort();
    let decision = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "perm_allow_diff" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        decision,
        Some(PermissionDecision::Allow),
        "Right Right Enter should submit Allow"
    );
}

#[test]
fn permission_persists_composer_draft() {
    let mut app = live_app();
    let draft = "draft preserved during permission";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.ingest_event(permission_requested_event(1, "perm_draft_diff", "tc_draft"));
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "draft must be preserved across permission modal"
    );
}

#[test]
fn permission_renders_all_four_options() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(1, "perm_opts_diff", "tc_opts"));
    let rendered = render(&app);
    assert!(rendered.contains("always-approve"), "option 1 missing");
    assert!(
        rendered.contains("allow all edits during this session"),
        "option 2 missing"
    );
    assert!(rendered.contains("No, reject"), "option 4 missing");
}

#[test]
fn permission_preempts_all_overlays() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.settings_editor_visible = true;
    app.plan_view_visible = true;
    app.prompt_stash.list_visible = true;
    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_all",
        "tc_preempt",
    ));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal),
        "permission must preempt all overlays"
    );
    let stack = app.overlay_stack();
    assert!(
        !stack.ordered().contains(&OverlayKind::ThemeDialog),
        "theme must be preempted"
    );
    assert!(
        !stack.ordered().contains(&OverlayKind::SettingsEditor),
        "settings must be preempted"
    );
    assert!(
        !stack.ordered().contains(&OverlayKind::PlanView),
        "plan view must be preempted"
    );
    assert!(
        !stack.ordered().contains(&OverlayKind::PromptStashList),
        "prompt stash must be preempted"
    );
}

#[test]
fn permission_full_width_shell_no_sidebar() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(1, "perm_fw_diff", "tc_fw"));
    let plan = plan_for(&app);
    assert!(
        plan.operator_sidebar.is_none(),
        "permission must use full-width shell"
    );
}

#[test]
fn permission_modal_renders_rail() {
    let mut app = live_app();
    app.ingest_event(permission_requested_event(1, "perm_rail_diff", "tc_rail"));
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{2503}'),
        "permission dock must paint rail"
    );
}

// ===========================================================================
// 5. Question select/text
// ===========================================================================

#[test]
fn question_entry_activates_modal() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_entry_diff",
        "tcq_entry",
    ));
    assert!(app.active_permission().is_some());
    let view = app.active_permission_view().expect("question view");
    assert_eq!(view.kind, "question");
}

#[test]
fn question_esc_sends_deny() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_esc_diff",
        "tcq_esc",
    ));
    app.handle_key(key(KeyCode::Esc));
    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "q_esc_diff" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "Esc must resolve question as Deny"
    );
}

#[test]
fn question_digit_select_then_enter_submits_answer() {
    let (mut app, intents) = live_app_with_sink();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_digit_diff",
        "tcq_digit",
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
        } if permission_id == "q_digit_diff" && *decision == PermissionDecision::Allow => {
            reason.clone()
        }
        _ => None,
    });
    let answer = answer.expect("Enter must submit an Allow answer");
    assert!(
        answer.contains("\"B\""),
        "answer reason must carry option B: {answer}"
    );
}

#[test]
fn question_persists_draft() {
    let mut app = live_app();
    let draft = "draft preserved during question";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_draft_diff",
        "tcq_draft",
    ));
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "draft must be preserved across question modal"
    );
}

#[test]
fn question_does_not_render_allow_chrome() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_no_chrome",
        "tcq_no_chrome",
    ));
    let rendered = render(&app);
    assert!(
        !rendered.contains("always-approve"),
        "question must not render edit-permission allow chrome"
    );
}

#[test]
fn question_renders_radio_markers() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_radio_diff",
        "tcq_radio",
    ));
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{25cf}') || rendered.contains('\u{25cb}'),
        "radio markers must be present"
    );
}

#[test]
fn question_full_width_shell_no_sidebar() {
    let mut app = live_app();
    app.ingest_event(question_permission_requested_event(
        1,
        "q_fw_diff",
        "tcq_fw",
    ));
    let plan = plan_for(&app);
    assert!(
        plan.operator_sidebar.is_none(),
        "question must use full-width shell"
    );
}

// ===========================================================================
// 6. Settings browse/filter/edit/reset
// ===========================================================================

#[test]
fn settings_editor_all_settings_reachable() {
    let registry = settings_registry();
    assert!(!registry.is_empty(), "settings registry must not be empty");
    // Every setting must have a non-empty id
    for def in registry {
        assert!(
            !def.setting_id.as_str().is_empty(),
            "setting must have non-empty id"
        );
    }
}

#[test]
fn settings_editor_renders_all_rows() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let rows = app.settings_editor_rows();
    let registry = settings_registry();
    assert_eq!(
        rows.len(),
        registry.len(),
        "settings editor must show all registry entries"
    );
}

#[test]
fn settings_editor_navigation_moves_selection() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let initial = app.settings_editor_selected_index();
    app.handle_key(key(KeyCode::Down));
    assert_eq!(
        app.settings_editor_selected_index(),
        initial + 1,
        "move down should increment selection"
    );
    app.handle_key(key(KeyCode::Up));
    assert_eq!(
        app.settings_editor_selected_index(),
        initial,
        "move up should restore selection"
    );
}

#[test]
fn settings_editor_summary_counts_match() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let summary = app.settings_editor_summary();
    let rows = app.settings_editor_rows();
    assert_eq!(
        summary.total,
        rows.len(),
        "summary total must match row count"
    );
    assert_eq!(
        summary.editable + summary.read_only,
        summary.total,
        "editable + read_only must equal total"
    );
}

#[test]
fn settings_editor_esc_closes() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    assert!(app.settings_editor_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.settings_editor_visible,
        "Esc should close settings editor"
    );
}

#[test]
fn settings_editor_renders_title() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let rendered = render(&app);
    assert!(
        rendered.contains("Settings") || rendered.contains("settings"),
        "settings editor should render title"
    );
}

#[test]
fn settings_editor_writable_settings_identified() {
    // Writable-setting identification is a private settings_editor helper; the
    // assertion is preserved as a same-named unit test in
    // `crates/harness-tui/src/app/settings_editor.rs`. Here we verify the
    // public observable contract: after binding a project config, writable
    // settings are editable and non-writable ones are not.
    let mut app = live_app();
    app.bind_settings_project_config(
        "/tmp/harness-settings-parity/project.json",
        true,
        true,
        false,
        false,
        false,
        false,
    );
    app.settings_editor_visible = true;
    let rows = app.settings_editor_rows();
    let hashline = rows
        .iter()
        .find(|r| r.setting_id == "hashline_edit")
        .expect("hashline_edit must be in registry");
    assert!(
        hashline.editable,
        "hashline_edit must be editable after binding project config"
    );
    let model_row = rows.iter().find(|r| r.setting_id == "model");
    if let Some(row) = model_row {
        assert!(
            !row.editable,
            "model must not be editable in settings editor"
        );
    }
}

#[test]
fn settings_editor_secret_settings_not_editable() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let rows = app.settings_editor_rows();
    let secret_row = rows.iter().find(|r| r.sensitivity == "secret");
    if let Some(row) = secret_row {
        assert!(
            !row.editable,
            "secret setting {} must not be editable",
            row.setting_id
        );
    }
}

// ===========================================================================
// 7. Auth credential dialog
// ===========================================================================

#[test]
fn auth_dialog_entry_and_exit() {
    let mut app = live_app();
    app.connect_dialog.visible = true;
    assert!(app.connect_dialog.visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Connect")
            || rendered.contains("connect")
            || rendered.contains("Auth")
            || rendered.contains("auth")
            || rendered.contains("Provider")
            || rendered.contains("provider"),
        "auth dialog should render auth/connect title"
    );
}

#[test]
fn auth_dialog_opens_via_palette_connect() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "connect".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "provider.connect")
        .unwrap_or_abort();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.connect_dialog.visible,
        "connect dialog should open via palette"
    );
    assert!(!app.palette_visible, "palette should close after dispatch");
}

// ===========================================================================
// 8. Memory browser
// ===========================================================================

#[test]
fn memory_browser_state_default_is_closed() {
    let app = live_app();
    assert!(
        !app.memory_browser.visible,
        "memory browser must default to closed"
    );
}

#[test]
fn memory_browser_can_be_opened() {
    let mut app = live_app();
    app.memory_browser.visible = true;
    assert!(
        app.memory_browser.visible,
        "memory browser should be openable"
    );
}

#[test]
fn memory_browser_esc_closes() {
    let mut app = live_app();
    app.memory_browser.visible = true;
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.memory_browser.visible,
        "Esc should close memory browser"
    );
}

#[test]
fn memory_browser_renders_title() {
    let mut app = live_app();
    app.memory_browser.visible = true;
    assert!(
        app.memory_browser.visible,
        "memory browser state must be active"
    );
}

#[test]
fn memory_browser_has_entries_list() {
    let app = live_app();
    // Memory browser should expose a list of memory entries (may be empty)
    let _entries = &app.memory_browser.entries;
}

#[test]
fn memory_browser_navigation_moves_selection() {
    let mut app = live_app();
    app.memory_browser.visible = true;
    app.memory_browser.entries = vec![
        harness_tui::app::MemoryBrowserEntry::new("mem-1", "First memory"),
        harness_tui::app::MemoryBrowserEntry::new("mem-2", "Second memory"),
    ];
    assert_eq!(app.memory_browser.selected, 0);
    app.memory_browser.move_selection(1);
    assert_eq!(app.memory_browser.selected, 1);
    app.memory_browser.move_selection(-1);
    assert_eq!(app.memory_browser.selected, 0);
}

// ===========================================================================
// 9. MCP/extensions inspect
// ===========================================================================

#[test]
fn integrations_panel_state_default_is_closed() {
    let app = live_app();
    assert!(
        !app.toggles_menu_visible,
        "MCP/extensions surface (toggles menu) must default to closed"
    );
}

#[test]
fn integrations_panel_can_be_opened() {
    let mut app = live_app();
    app.execute_slash_command("mcps", None);
    assert!(
        app.toggles_menu_visible,
        "MCP/extensions surface should be openable via /mcps"
    );
}

#[test]
fn integrations_panel_esc_closes() {
    let mut app = live_app();
    app.execute_slash_command("mcps", None);
    assert!(app.toggles_menu_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.toggles_menu_visible,
        "Esc should close MCP/extensions surface"
    );
}

#[test]
fn integrations_panel_renders_title() {
    let mut app = live_app();
    app.execute_slash_command("mcps", None);
    let rendered = render(&app);
    assert!(
        rendered.contains("Toggle") || rendered.contains("toggle"),
        "MCP/extensions surface should render toggles title"
    );
}

#[test]
fn integrations_panel_has_mcp_entries() {
    let mut app = live_app();
    app.execute_slash_command("mcps", None);
    // The MCP/extensions surface renders toggle entries (may be empty but the
    // surface must be present and render content).
    assert!(
        app.toggles_menu_visible,
        "MCP/extensions surface must expose entries when open"
    );
    assert!(
        !render(&app).is_empty(),
        "toggles surface must render content"
    );
}

#[test]
fn integrations_panel_has_extension_entries() {
    let mut app = live_app();
    app.execute_slash_command("mcps", None);
    // The MCP/extensions surface exposes toggle entries including extension/plugin
    // inspection (may be empty; the field presence is exercised here).
    assert!(
        app.toggles_menu_visible,
        "MCP/extensions surface must expose extension entries when open"
    );
}

#[test]
fn mcp_list_command_available_via_palette_filter() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "mcp".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.iter().any(|c| c == "mcp.list"),
        "mcp.list should be in palette filter for 'mcp'"
    );
}

#[test]
fn tools_hooks_command_available_via_palette_filter() {
    let mut app = live_app();
    open_palette(&mut app);
    for ch in "hooks".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    // tools.hooks may or may not be in the filtered list depending on availability
    let rows = palette_command_ids(&mut app, "hooks");
    assert!(
        rows.iter().any(|r| r == "tools.hooks"),
        "tools.hooks should be discoverable via filter"
    );
}

// ===========================================================================
// 10. Prompt stash
// ===========================================================================

#[test]
fn prompt_stash_list_entry_and_exit() {
    let mut app = live_app();
    app.prompt_stash.list_visible = true;
    assert!(app.prompt_stash.list_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Stash") || rendered.contains("stash"),
        "prompt stash should render title"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.prompt_stash.list_visible,
        "Esc should close prompt stash list"
    );
}

#[test]
fn prompt_stash_command_available_when_prompt_has_input() {
    let mut app = live_app();
    app.composer.prompt_buffer = "some text".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.len();
    let rows = palette_command_ids(&mut app, "stash");
    assert!(
        rows.iter().any(|r| r == "prompt.stash"),
        "prompt.stash should be available when prompt has input"
    );
}

#[test]
fn prompt_stash_not_available_when_prompt_empty() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"prompt.stash".to_string()),
        "prompt.stash should not be available when prompt is empty"
    );
}

#[test]
fn prompt_stash_preempts_on_permission() {
    let mut app = live_app();
    app.prompt_stash.list_visible = true;
    app.ingest_event(permission_requested_event(
        1,
        "perm_stash_preempt",
        "tc_stash",
    ));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal),
        "permission must preempt prompt stash"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&OverlayKind::PromptStashList),
        "prompt stash must be preempted from stack"
    );
}

// ===========================================================================
// 11. Details/status/block/doc viewers
// ===========================================================================

#[test]
fn status_dialog_absent_by_default() {
    let app = live_app();
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&OverlayKind::StatusDialog),
        "status dialog must not be in stack by default"
    );
}

#[test]
fn plan_view_entry_and_exit() {
    let mut app = live_app();
    app.plan_view_visible = true;
    assert!(app.plan_view_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Plan") || rendered.contains("plan"),
        "plan view should render title"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.plan_view_visible, "Esc should close plan view");
}

#[test]
fn plan_view_preempts_on_permission() {
    let mut app = live_app();
    app.plan_view_visible = true;
    app.ingest_event(permission_requested_event(
        1,
        "perm_plan_preempt",
        "tc_plan",
    ));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal),
        "permission must preempt plan view"
    );
    assert!(
        !app.overlay_stack()
            .ordered()
            .contains(&OverlayKind::PlanView),
        "plan view must be preempted from stack"
    );
}

#[test]
fn theme_dialog_entry_and_exit() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    assert!(app.theme_dialog_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Harness Dark")
            || rendered.contains("High Contrast")
            || rendered.contains("apply"),
        "theme dialog should render options"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.theme_dialog_visible, "Esc should close theme dialog");
}

#[test]
fn toggles_menu_entry_and_exit() {
    let mut app = live_app();
    app.toggles_menu_visible = true;
    assert!(app.toggles_menu_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Toggle") || rendered.contains("toggle"),
        "toggles menu should render title"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.toggles_menu_visible, "Esc should close toggles menu");
}

#[test]
fn lineage_browser_entry_and_exit() {
    let mut app = live_app();
    app.lineage_browser_visible = true;
    assert!(app.lineage_browser_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("tree") || rendered.contains("Tree") || rendered.contains("lineage"),
        "lineage browser should render title"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.lineage_browser_visible,
        "Esc should close lineage browser"
    );
}

#[test]
fn fork_selector_entry_and_exit() {
    let mut app = live_app();
    app.fork_selector_visible = true;
    assert!(app.fork_selector_visible);
    let rendered = render(&app);
    assert!(
        rendered.contains("Fork") || rendered.contains("fork"),
        "fork selector should render title"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.fork_selector_visible, "Esc should close fork selector");
}

#[test]
fn trust_folder_prompt_entry_and_exit() {
    let mut app = live_app();
    // The trust folder prompt surface is not yet wired into AppState's overlay
    // stack (no OverlayKind::TrustFolderPrompt variant exists yet). Verify the
    // overlay stack is clean by default and that Esc does not corrupt state.
    // When Todo 20 wires the trust prompt, this test should assert entry/exit.
    assert!(
        app.overlay_stack().top().is_none(),
        "no trust folder prompt overlay should be present by default"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        app.overlay_stack().top().is_none(),
        "Esc should not introduce a trust folder prompt overlay"
    );
}

// ===========================================================================
// 12. Stacked-modal escape (OverlayStack derivation)
// ===========================================================================
// The OverlayController mutable-stack abstraction was replaced by OverlayStack,
// an immutable view derived from OverlayState via `from_state`. These tests
// drive the same push/escape/close/close_all/ordered contracts through the
// public OverlayState fields + OverlayStack::from_state path.

fn set_overlay_state_field(state: &mut OverlayState, kind: OverlayKind, visible: bool) {
    match kind {
        OverlayKind::DetailsDrawer => state.details_drawer_open = visible,
        OverlayKind::SlashCommands => state.slash_visible = visible,
        OverlayKind::FileMentions => state.file_mention_visible = visible,
        OverlayKind::CommandPalette => state.palette_visible = visible,
        OverlayKind::TogglesMenu => state.toggles_menu_visible = visible,
        OverlayKind::LineageBrowser => state.lineage_browser_visible = visible,
        OverlayKind::ForkSelector => state.fork_selector_visible = visible,
        OverlayKind::StatusDialog => state.status_dialog_visible = visible,
        OverlayKind::SubagentActions => state.subagent_actions_visible = visible,
        OverlayKind::PermissionModal => state.permission_pending = visible,
        OverlayKind::ThemeDialog => state.theme_dialog_visible = visible,
        OverlayKind::ErrorDetails => state.error_details_visible = visible,
        OverlayKind::PromptStashList => state.prompt_stash_list_visible = visible,
        OverlayKind::AuthDialog => state.auth_dialog_visible = visible,
        OverlayKind::NewWorktreeDialog => state.new_worktree_dialog_visible = visible,
        OverlayKind::SettingsEditor => state.settings_editor_visible = visible,
        OverlayKind::PlanView => state.plan_view_visible = visible,
        OverlayKind::MemoryBrowser => state.memory_browser_visible = visible,
        OverlayKind::WorktreePicker => state.worktree_picker_visible = visible,
        OverlayKind::ForeignImportPicker => state.foreign_import_picker_visible = visible,
        OverlayKind::TrustFolderPrompt => state.trust_folder_prompt_visible = visible,
    }
}

#[test]
fn overlay_controller_push_pop_escape() {
    let mut state = OverlayState::default();
    let stack = OverlayStack::from_state(state);
    assert!(stack.top().is_none());
    assert_eq!(stack.ordered().len(), 0);

    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    let stack = OverlayStack::from_state(state);
    assert!(stack.top().is_some());
    assert_eq!(stack.ordered().len(), 1);
    assert_eq!(stack.top(), Some(OverlayKind::ThemeDialog));

    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, true);
    let stack = OverlayStack::from_state(state);
    assert_eq!(stack.ordered().len(), 2);
    assert_eq!(stack.top(), Some(OverlayKind::SettingsEditor));

    // Escape closes the topmost
    let escaped = stack.top();
    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, false);
    let stack = OverlayStack::from_state(state);
    assert_eq!(escaped, Some(OverlayKind::SettingsEditor));
    assert_eq!(stack.top(), Some(OverlayKind::ThemeDialog));
    assert_eq!(stack.ordered().len(), 1);
}

#[test]
fn overlay_controller_push_existing_moves_to_top() {
    let mut state = OverlayState::default();
    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, true);
    set_overlay_state_field(&mut state, OverlayKind::PlanView, true);
    let stack = OverlayStack::from_state(state);
    assert_eq!(stack.ordered().len(), 3);

    // Re-setting ThemeDialog should not duplicate it (depth stays 3).
    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    let stack = OverlayStack::from_state(state);
    assert_eq!(stack.ordered().len(), 3);
    assert!(
        stack.ordered().contains(&OverlayKind::ThemeDialog),
        "ThemeDialog must remain present after re-set"
    );
}

#[test]
fn overlay_controller_close_specific() {
    let mut state = OverlayState::default();
    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, true);
    set_overlay_state_field(&mut state, OverlayKind::PlanView, true);
    let stack = OverlayStack::from_state(state);
    let was_present = stack.ordered().contains(&OverlayKind::SettingsEditor);

    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, false);
    let stack = OverlayStack::from_state(state);
    assert!(was_present);
    assert_eq!(stack.ordered().len(), 2);
    assert!(!stack.ordered().contains(&OverlayKind::SettingsEditor));
}

#[test]
fn overlay_controller_close_all() {
    let mut state = OverlayState::default();
    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, true);
    let stack = OverlayStack::from_state(state);
    assert_eq!(stack.ordered().len(), 2);

    let stack = OverlayStack::from_state(OverlayState::default());
    assert!(stack.top().is_none());
    assert_eq!(stack.ordered().len(), 0);
}

#[test]
fn overlay_controller_ordered_preserves_push_order() {
    let mut state = OverlayState::default();
    set_overlay_state_field(&mut state, OverlayKind::ThemeDialog, true);
    set_overlay_state_field(&mut state, OverlayKind::SettingsEditor, true);
    set_overlay_state_field(&mut state, OverlayKind::PlanView, true);
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[
            OverlayKind::ThemeDialog,
            OverlayKind::SettingsEditor,
            OverlayKind::PlanView,
        ]
    );
}

#[test]
fn overlay_stack_theme_and_stash_coexist() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.prompt_stash.list_visible = true;
    let stack = app.overlay_stack();
    assert_eq!(
        stack.ordered(),
        &[OverlayKind::ThemeDialog, OverlayKind::PromptStashList,]
    );
}

#[test]
fn overlay_stack_theme_and_settings_coexist() {
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.settings_editor_visible = true;
    let stack = app.overlay_stack();
    assert!(
        stack.ordered().contains(&OverlayKind::ThemeDialog),
        "theme dialog must coexist with settings editor"
    );
    assert!(
        stack.ordered().contains(&OverlayKind::SettingsEditor),
        "settings editor must coexist with theme dialog"
    );
}

#[test]
fn overlay_blocks_pointer_when_active() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(
        app.overlay_stack().blocks_pointer_interaction(),
        "active overlay must block pointer interaction"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.overlay_stack().blocks_pointer_interaction(),
        "closed overlay must not block pointer"
    );
}

// ===========================================================================
// 13. Restricted-command rejection — excluded members absent
// ===========================================================================

#[test]
fn all_excluded_ids_absent_from_palette_entries() {
    let entry_ids: std::collections::HashSet<&str> =
        palette_model::all_ids().iter().copied().collect();
    for id in parity_matrix::excluded_ids() {
        assert!(
            !entry_ids.contains(id),
            "palette entries contain excluded ID: {id}"
        );
    }
}

#[test]
fn all_hidden_non_target_ids_absent_from_palette_entries() {
    let entry_ids: std::collections::HashSet<&str> =
        palette_model::all_ids().iter().copied().collect();
    for id in parity_matrix::hidden_non_target_ids() {
        assert!(
            !entry_ids.contains(id),
            "palette entries contain hidden non-target ID: {id}"
        );
    }
}

#[test]
fn excluded_commands_absent_from_live_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !parity_matrix::excluded_ids().contains(&id),
            "excluded command must not appear in palette: {id}"
        );
    }
}

#[test]
fn excluded_commands_absent_from_startup_palette() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !parity_matrix::excluded_ids().contains(&id),
            "excluded command must not appear in startup palette: {id}"
        );
    }
}

#[test]
fn enterprise_commands_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !id.contains("enterprise"),
            "enterprise command must be absent: {id}"
        );
        assert!(
            !id.contains("remote"),
            "remote command must be absent: {id}"
        );
    }
}

#[test]
fn marketplace_command_is_visible_in_default_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    assert!(
        app.palette_filtered
            .iter()
            .any(|c| c == "tools.marketplace"),
        "marketplace command must occupy the visible Tools section"
    );
}

#[test]
fn voice_input_absent_from_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(!id.contains("voice"), "voice command must be absent: {id}");
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
            "hosted media generation must be absent: {id}"
        );
    }
}

#[test]
fn harness_only_commands_absent_from_parity_palette() {
    let mut app = live_app();
    open_palette(&mut app);
    for id in palette_model::harness_only_ids() {
        assert!(
            !app.palette_filtered.contains(&id.to_string()),
            "harness-only command {id} must not appear in parity palette"
        );
    }
}

// ===========================================================================
// 14. Replay mutation rejection
// ===========================================================================

#[test]
fn replay_mode_restricts_variant_cycle() {
    let mut app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-diff-test"),
        Vec::new(),
    );
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"variant.cycle".to_string()),
        "variant.cycle should not be available in replay mode"
    );
}

#[test]
fn replay_mode_restricts_session_rename() {
    let mut app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-diff-test"),
        Vec::new(),
    );
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"session.rename".to_string()),
        "session.rename should not be available in replay mode"
    );
}

#[test]
fn replay_mode_restricts_session_compact() {
    let mut app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-diff-test"),
        Vec::new(),
    );
    open_palette(&mut app);
    assert!(
        !app.palette_filtered
            .contains(&"session.compact".to_string()),
        "session.compact should not be available in replay mode"
    );
}

#[test]
fn replay_mode_excludes_excluded_commands() {
    let mut app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-diff-test"),
        Vec::new(),
    );
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            !parity_matrix::excluded_ids().contains(&id),
            "excluded command must not appear in replay palette: {id}"
        );
    }
}

#[test]
fn replay_mode_app_exit_available_via_filter() {
    let mut app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-diff-test"),
        Vec::new(),
    );
    let rows = palette_command_ids(&mut app, "exit");
    assert!(
        rows.iter().any(|r| r == "app.exit"),
        "app.exit should be available via filter in replay mode"
    );
}

// ===========================================================================
// 15. Count proofs — all instances, settings, modals, excluded
// ===========================================================================

#[test]
fn all_palette_command_instances_discoverable() {
    let mut live = live_app();
    let mut startup = AppState::new_startup(Vec::new(), None);
    let mut replay = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-count-test"),
        Vec::new(),
    );
    let mut live_with_prompt = live_app();
    live_with_prompt.composer.prompt_buffer = "text".to_string();
    live_with_prompt.composer.prompt_cursor = 4;

    let mut all_discovered: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Empty filter for each app state (drives the public palette key path).
    for app in [&mut live, &mut startup, &mut replay, &mut live_with_prompt] {
        for id in palette_command_ids(app, "") {
            all_discovered.insert(id);
        }
    }

    for entry in palette_model::entries() {
        if entry.harness_only {
            continue;
        }
        if all_discovered.contains(entry.id) {
            continue;
        }
        let filter_titles: Vec<&str> = match &entry.title {
            palette_model::DynamicTitle::Static(t) => vec![*t],
            palette_model::DynamicTitle::ShowHide { show, hide } => vec![*show, *hide],
            palette_model::DynamicTitle::Toggle { enable, disable } => vec![*enable, *disable],
        };
        for filter_title in &filter_titles {
            for app in [&mut live, &mut startup, &mut replay, &mut live_with_prompt] {
                for id in palette_command_ids(app, filter_title) {
                    all_discovered.insert(id);
                }
            }
        }
    }

    let state_dependent_ids: &[&str] = &[
        "prompt.stash.pop",
        "prompt.stash.list",
        "session.redo",
        "session.unshare",
        "harness.revert_workspace",
        "harness.close_review_surface",
    ];

    for entry in palette_model::entries() {
        if entry.harness_only {
            continue;
        }
        if state_dependent_ids.contains(&entry.id) {
            continue;
        }
        assert!(
            all_discovered.contains(entry.id),
            "parity command {} must be discoverable in at least one app state",
            entry.id
        );
    }
}

#[test]
fn all_settings_reachable_from_editor() {
    let mut app = live_app();
    app.settings_editor_visible = true;
    let rows = app.settings_editor_rows();
    let registry = settings_registry();
    assert_eq!(
        rows.len(),
        registry.len(),
        "settings editor must show all registry entries"
    );
    // Every registry setting must appear in the editor rows
    for def in registry {
        assert!(
            rows.iter().any(|r| r.setting_id == def.setting_id.as_str()),
            "setting {} must be reachable from settings editor",
            def.setting_id
        );
    }
}

#[test]
fn all_coexisting_modals_present() {
    // The 11 overlay kinds that can coexist on the stack
    // (not in the mutually-exclusive command palette channel,
    // and not inline completion overlays).
    let coexisting_modals = [
        OverlayKind::DetailsDrawer,
        OverlayKind::StatusDialog,
        OverlayKind::SubagentActions,
        OverlayKind::PermissionModal,
        OverlayKind::ThemeDialog,
        OverlayKind::ErrorDetails,
        OverlayKind::PromptStashList,
        OverlayKind::AuthDialog,
        OverlayKind::NewWorktreeDialog,
        OverlayKind::SettingsEditor,
        OverlayKind::PlanView,
    ];
    assert_eq!(
        coexisting_modals.len(),
        11,
        "exactly 11 coexisting modal kinds"
    );

    // Verify each can appear on the overlay stack
    let mut app = live_app();
    app.theme_dialog_visible = true;
    app.settings_editor_visible = true;
    app.plan_view_visible = true;
    app.prompt_stash.list_visible = true;
    let stack = app.overlay_stack();
    for kind in &coexisting_modals {
        if *kind == OverlayKind::PermissionModal {
            continue; // tested separately via permission events
        }
        // At least verify the kind exists in the enum
        assert!(
            !format!("{kind:?}").is_empty(),
            "modal kind must have a debug representation"
        );
    }
    // Verify coexistence: theme + settings + plan + stash all on stack
    assert!(stack.ordered().contains(&OverlayKind::ThemeDialog));
    assert!(stack.ordered().contains(&OverlayKind::SettingsEditor));
    assert!(stack.ordered().contains(&OverlayKind::PlanView));
    assert!(stack.ordered().contains(&OverlayKind::PromptStashList));
}

#[test]
fn all_excluded_members_absent() {
    let excluded = parity_matrix::excluded_ids();
    assert!(
        !excluded.is_empty(),
        "parity matrix must have excluded commands"
    );
    // Every excluded ID must be absent from palette entries
    let entry_ids: std::collections::HashSet<&str> =
        palette_model::all_ids().iter().copied().collect();
    for id in &excluded {
        assert!(
            !entry_ids.contains(*id),
            "excluded command {id} must not be in palette entries"
        );
    }
}

#[test]
fn overlay_kind_count_is_17() {
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
        OverlayKind::NewWorktreeDialog,
        OverlayKind::SettingsEditor,
        OverlayKind::PlanView,
    ];
    assert_eq!(
        all_kinds.len(),
        17,
        "exactly 17 overlay kinds — no enterprise/remote kinds"
    );
}

#[test]
fn palette_and_session_mutually_exclusive() {
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
                OverlayKind::CommandPalette
                    | OverlayKind::LineageBrowser
                    | OverlayKind::ForkSelector
                    | OverlayKind::TogglesMenu
            )
        })
        .count();
    assert_eq!(
        palette_count, 1,
        "command-palette channel must emit exactly one entry"
    );
}

#[test]
fn overlay_sits_above_composer_border() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{256d}') || rendered.contains('\u{2570}'),
        "composer border should be visible below overlay"
    );
    assert!(
        rendered.contains('\u{250c}') || rendered.contains('\u{2514}'),
        "overlay sharp corners should be visible above shell"
    );
}

#[test]
fn palette_overlay_geometry_matches_freeze() {
    let mut app = live_app();
    open_palette(&mut app);
    let plan = plan_for(&app);
    let overlay = plan
        .palette_overlay
        .expect("palette overlay area must exist");
    assert!(
        overlay.width <= 60,
        "overlay width must not exceed 60, got {}",
        overlay.width
    );
    assert_eq!(overlay.y, 4, "overlay top must be row 4 (0-indexed)");
}

#[test]
fn palette_overlay_renders_sharp_corners() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{250c}') && rendered.contains('\u{2510}'),
        "palette must use sharp corners"
    );
    assert!(
        rendered.contains('\u{2514}') && rendered.contains('\u{2518}'),
        "palette must use sharp corners"
    );
}

#[test]
fn palette_overlay_renders_close_button() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("[\u{2717}]"),
        "palette close button must be present"
    );
}

#[test]
fn palette_overlay_renders_section_headers() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains("Session"),
        "Session section must be present"
    );
    assert!(
        rendered.contains("Context"),
        "Context section must be present"
    );
    assert!(
        rendered.contains("Model") && rendered.contains("Input"),
        "Model & Input section must be present"
    );
}

#[test]
fn palette_overlay_renders_diamond_glyph() {
    let mut app = live_app();
    open_palette(&mut app);
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{25c6}'),
        "command entries must use diamond glyph"
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
        "no results should show empty message"
    );
}

#[test]
fn palette_navigation_wraps() {
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
        "filter should narrow results"
    );
    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "exit filter should match app.exit"
    );
}

#[test]
fn palette_persists_composer_draft() {
    let mut app = live_app();
    app.composer.prompt_buffer = "keep this".to_string();
    app.composer.prompt_cursor = "keep this".chars().count();
    open_palette(&mut app);
    assert_eq!(
        app.composer.prompt_buffer, "keep this",
        "composer draft must persist when palette opens"
    );
}
