//! Task 29: Plan, vim, minimal, inline, fullscreen, rewind, and mode transitions.
//!
//! Differential TDD contract tests for:
//! - plan enter/edit/view/approve/exit (writes only `.agent-harness/plans/<run>.md`)
//! - vim/simple toggle + navigation
//! - compact/minimal/native scrollback
//! - inline/fullscreen switch
//! - fold/raw/manual preferences
//! - rewind restore draft
//! - prompt stash
//! - unauthorized plan-path rejection
//! - cancel/reject approval
//! - unsupported terminal mode
//! - child suspend failure
//! - replay-mode mutation rejection
//!
//! Compile-repair note (Todo 3): the AppState integration tests drive the
//! *current* public input surface (`handle_key` via `apply_keybindings`) for
//! behaviors that already have a keybinding/plan-view path (plan enter/exit,
//! prompt stash push/pop/list). The Plan Mode mutation contracts that have no
//! current entrypoint (`plan_edit`/`plan_approve`/`plan_reject`/`plan_cancel`/
//! `plan_validate_path`/`plan_child_suspend`/`plan_is_replay_mutation_blocked`)
//! call the `pub` contract stubs on `AppState` defined in `plan_view.rs`,
//! which `unimplemented!()` so these tests run as honest *red* evidence for
//! Todo 21. No assertion is weakened, ignored, or deleted.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "differential parity tests use fail-fast asserts"
)]

#[path = "../src/app/screen_mode.rs"]
mod screen_mode;

#[path = "../src/app/vim_mode.rs"]
mod vim_mode;

#[path = "../src/app/rewind_view.rs"]
mod rewind_view;

use std::collections::BTreeMap;
use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use tempfile::tempdir;

use rewind_view::{PromptDraftSnapshot, RewindView};
use screen_mode::{
    ScreenModeError, ScreenModeState, ScrollbackStyle, TerminalMode, ViewPreference,
};
use vim_mode::{VimEditorMode, VimState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_ref(
        "build",
        "mock:model-task29-modes",
    ));
    app
}

fn live_app_with_workspace() -> (AppState, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let mut app = live_app();
    app.set_file_mention_workspace_root_for_test(dir.path().to_path_buf());
    (app, dir)
}

fn ingest_run_started(app: &mut AppState, dir: &tempfile::TempDir) {
    app.ingest_event(envelope(
        1,
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "run_task29_modes".to_string().into(),
            workspace_root: dir.path().to_string_lossy().to_string(),
        }),
    ));
}

fn replay_app() -> AppState {
    let dir = tempdir().expect("tempdir");
    let events = vec![envelope(
        1,
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "run-replay-modes".to_string().into(),
            workspace_root: dir.path().to_string_lossy().to_string(),
        }),
    )];
    let mut app = AppState::new_replay(dir.path().to_path_buf(), events);
    app.set_launch_metadata(LaunchMetadata::from_model_ref(
        "build",
        "mock:model-task29-replay",
    ));
    app.set_file_mention_workspace_root_for_test(dir.path().to_path_buf());
    app
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-modes-{seq:04}"),
        seq,
        run_id: "run_task29_modes".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task29_modes".to_string()),
        payload,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

/// Bind `action_name` to `key` (via the public `apply_keybindings` surface)
/// and dispatch it through the public `handle_key` input path. This is the
/// "public input behavior" route for actions that have no direct public
/// AppState method (e.g. `OpenViewPlan`, `PromptStash`).
fn trigger_action(app: &mut AppState, action_name: &str, key_str: &str, key: KeyEvent) {
    let mut bindings = BTreeMap::new();
    bindings.insert(action_name.to_string(), key_str.to_string());
    app.apply_keybindings(bindings);
    app.handle_key(key);
}

/// Open the plan view via the current `OpenViewPlan` keybinding path.
fn open_plan_view_via_key(app: &mut AppState) {
    trigger_action(
        app,
        "open_view_plan",
        "f5",
        KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
    );
}

/// Close the plan view via the current Escape key path (plan-view overlay).
fn close_plan_view_via_key(app: &mut AppState) {
    app.handle_key(key(KeyCode::Esc));
}

/// Stash the current prompt via the current `PromptStash` keybinding path.
fn stash_push_via_key(app: &mut AppState) {
    trigger_action(
        app,
        "prompt_stash",
        "f6",
        KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
    );
}

/// Pop the last stashed prompt via the current `PromptStashPop` keybinding path.
fn stash_pop_via_key(app: &mut AppState) {
    trigger_action(
        app,
        "prompt_stash_pop",
        "f7",
        KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
    );
}

/// Open the prompt stash list via the current `PromptStashList` keybinding path.
fn open_stash_list_via_key(app: &mut AppState) {
    trigger_action(
        app,
        "prompt_stash_list",
        "f8",
        KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE),
    );
}

// ---------------------------------------------------------------------------
// Plan enter / edit / view / approve / exit
// ---------------------------------------------------------------------------

#[test]
fn plan_enter_opens_plan_view() {
    // arrange
    let mut app = live_app();

    // act
    open_plan_view_via_key(&mut app);

    // assert
    assert!(app.plan_view_is_visible(), "plan_enter must open plan view");
}

#[test]
fn plan_enter_sets_status_banner() {
    // arrange
    let mut app = live_app();

    // act
    open_plan_view_via_key(&mut app);

    // assert
    assert!(
        app.status_banner.is_some(),
        "plan_enter must set a status banner"
    );
}

#[test]
fn plan_edit_writes_to_canonical_plan_path() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    let plan_body = "# Plan\n\nStep 1: analyze\nStep 2: implement\n";

    // act
    let result = app.plan_edit(plan_body.to_string());

    // assert
    assert!(result.is_ok(), "plan_edit must succeed");
    let plan_path = dir
        .path()
        .join(".agent-harness")
        .join("plans")
        .join("run_task29_modes.md");
    assert!(
        plan_path.is_file(),
        "plan file must exist at canonical path"
    );
    let written = fs::read_to_string(&plan_path).expect("read plan file");
    assert_eq!(
        written, plan_body,
        "plan file content must match edited body"
    );
}

#[test]
fn plan_edit_creates_plans_directory() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    let plans_dir = dir.path().join(".agent-harness").join("plans");
    assert!(!plans_dir.exists(), "plans dir must not exist yet");

    // act
    app.plan_edit("# Plan\n".to_string()).expect("edit");

    // assert
    assert!(
        plans_dir.is_dir(),
        "plan_edit must create the plans directory"
    );
}

#[test]
fn plan_edit_overwrites_existing_plan() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    app.plan_edit("# Original Plan\n".to_string())
        .expect("first edit");
    let plan_path = dir
        .path()
        .join(".agent-harness")
        .join("plans")
        .join("run_task29_modes.md");

    // act
    app.plan_edit("# Updated Plan\n".to_string())
        .expect("second edit");

    // assert
    let written = fs::read_to_string(&plan_path).expect("read plan file");
    assert_eq!(
        written, "# Updated Plan\n",
        "plan_edit must overwrite existing plan"
    );
}

#[test]
fn plan_edit_without_run_id_returns_error() {
    // arrange
    let (mut app, _dir) = live_app_with_workspace();
    // no events ingested → no run_id

    // act
    let result = app.plan_edit("# Plan\n".to_string());

    // assert
    assert!(
        result.is_err(),
        "plan_edit without run_id must return error"
    );
}

#[test]
fn plan_view_shows_edited_plan_in_rows() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    app.plan_edit("# Plan for task 29\n".to_string())
        .expect("edit");

    // act
    let rows = app.plan_view_rows();

    // assert
    assert!(
        rows.iter()
            .any(|r| r.slug == "run_task29_modes" && r.exists),
        "plan_view rows must include the edited plan"
    );
}

#[test]
fn plan_approve_sets_banner_and_closes_view() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    app.plan_edit("# Plan\n".to_string()).expect("edit");
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());

    // act
    app.plan_approve();

    // assert
    assert!(
        app.status_banner.is_some(),
        "plan_approve must set status banner"
    );
    assert!(
        !app.plan_view_is_visible(),
        "plan_approve must close plan view"
    );
}

#[test]
fn plan_exit_closes_plan_view() {
    // arrange
    let (mut app, _dir) = live_app_with_workspace();
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());

    // act
    close_plan_view_via_key(&mut app);

    // assert
    assert!(
        !app.plan_view_is_visible(),
        "plan_exit must close plan view"
    );
}

#[test]
fn plan_edit_writes_only_to_plans_directory() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    let root = dir.path().to_path_buf();

    // act
    app.plan_edit("# Plan\n".to_string()).expect("edit");

    // assert — only .agent-harness/plans/run_task29_modes.md should exist
    let plan_file = root
        .join(".agent-harness")
        .join("plans")
        .join("run_task29_modes.md");
    assert!(plan_file.is_file(), "canonical plan file must exist");

    // no stray files outside .agent-harness/
    let agent_harness_dir = root.join(".agent-harness");
    let plans_dir = agent_harness_dir.join("plans");
    let plan_files: Vec<_> = fs::read_dir(&plans_dir)
        .expect("read plans dir")
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(
        plan_files.len(),
        1,
        "exactly one plan file must exist in plans dir"
    );
}

// ---------------------------------------------------------------------------
// Unauthorized plan-path rejection
// ---------------------------------------------------------------------------

#[test]
fn plan_validate_path_accepts_canonical_plan_path() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_validate_path(".agent-harness/plans/run_task29_modes.md");

    // assert
    assert!(result.is_ok(), "canonical plan path must be accepted");
}

#[test]
fn plan_validate_path_rejects_path_outside_plans_dir() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_validate_path(".agent-harness/agents/build.md");

    // assert
    assert!(result.is_err(), "path outside plans dir must be rejected");
}

#[test]
fn plan_validate_path_rejects_path_traversal() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_validate_path(".agent-harness/plans/../../../etc/passwd");

    // assert
    assert!(result.is_err(), "path traversal must be rejected");
}

#[test]
fn plan_validate_path_rejects_absolute_path() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_validate_path("/etc/passwd");

    // assert
    assert!(result.is_err(), "absolute path must be rejected");
}

#[test]
fn plan_validate_path_rejects_non_md_extension() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_validate_path(".agent-harness/plans/run.txt");

    // assert
    assert!(result.is_err(), "non-.md path must be rejected");
}

// ---------------------------------------------------------------------------
// Cancel / reject approval
// ---------------------------------------------------------------------------

#[test]
fn plan_reject_sets_banner_and_closes_view() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    app.plan_edit("# Plan\n".to_string()).expect("edit");
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());

    // act
    app.plan_reject();

    // assert
    assert!(
        app.status_banner.is_some(),
        "plan_reject must set status banner"
    );
    assert!(
        !app.plan_view_is_visible(),
        "plan_reject must close plan view"
    );
}

#[test]
fn plan_cancel_closes_plan_view_without_writing() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());
    let plan_path = dir
        .path()
        .join(".agent-harness")
        .join("plans")
        .join("run_task29_modes.md");

    // act
    app.plan_cancel();

    // assert
    assert!(
        !app.plan_view_is_visible(),
        "plan_cancel must close plan view"
    );
    assert!(
        !plan_path.exists(),
        "plan_cancel must not write a plan file"
    );
}

// ---------------------------------------------------------------------------
// Child suspend failure
// ---------------------------------------------------------------------------

#[test]
fn plan_child_suspend_returns_error_when_no_active_child() {
    // arrange
    let app = live_app();

    // act
    let result = app.plan_child_suspend();

    // assert
    assert!(
        result.is_err(),
        "child suspend with no active child must return error"
    );
}

// ---------------------------------------------------------------------------
// Replay-mode mutation rejection
// ---------------------------------------------------------------------------

#[test]
fn plan_enter_rejected_in_replay_mode() {
    // arrange
    let mut app = replay_app();
    assert!(app.replay_mode);

    // act
    open_plan_view_via_key(&mut app);

    // assert
    assert!(
        !app.plan_view_is_visible(),
        "plan_enter must be rejected in replay mode"
    );
}

#[test]
fn plan_edit_rejected_in_replay_mode() {
    // arrange
    let mut app = replay_app();
    assert!(app.replay_mode);

    // act
    let result = app.plan_edit("# Plan\n".to_string());

    // assert
    assert!(result.is_err(), "plan_edit must be rejected in replay mode");
}

#[test]
fn plan_approve_rejected_in_replay_mode() {
    // arrange
    let mut app = replay_app();
    assert!(app.replay_mode);

    // act
    app.plan_approve();

    // assert
    assert!(
        !app.plan_view_is_visible(),
        "plan_approve must be rejected in replay mode"
    );
}

#[test]
fn plan_is_replay_mutation_blocked_true_in_replay() {
    // arrange
    let app = replay_app();

    // act
    let blocked = app.plan_is_replay_mutation_blocked();

    // assert
    assert!(blocked, "replay mode must block plan mutations");
}

#[test]
fn plan_is_replay_mutation_blocked_false_in_live() {
    // arrange
    let app = live_app();

    // act
    let blocked = app.plan_is_replay_mutation_blocked();

    // assert
    assert!(!blocked, "live mode must not block plan mutations");
}

#[test]
fn prompt_stash_push_rejected_in_replay_mode() {
    // arrange
    let mut app = replay_app();
    app.composer.prompt_buffer = "stashed text".to_string();

    // act
    stash_push_via_key(&mut app);

    // assert
    assert_eq!(
        app.composer.prompt_buffer, "stashed text",
        "replay mode must not clear prompt on stash push"
    );
    assert!(
        app.prompt_stash.entries.is_empty(),
        "replay mode must not add stash entries"
    );
}

#[test]
fn prompt_stash_pop_rejected_in_replay_mode() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "pre-stashed".to_string();
    stash_push_via_key(&mut app);
    assert_eq!(app.prompt_stash.entries.len(), 1);
    app.composer.prompt_buffer.clear();
    app.replay_mode = true;

    // act
    stash_pop_via_key(&mut app);

    // assert
    assert!(
        app.composer.prompt_buffer.is_empty(),
        "replay mode must not restore prompt on stash pop"
    );
    assert_eq!(
        app.prompt_stash.entries.len(),
        1,
        "replay mode must not remove stash entries on pop"
    );
}

// ---------------------------------------------------------------------------
// Vim / simple toggle + navigation
// ---------------------------------------------------------------------------

#[test]
fn vim_state_defaults_to_disabled() {
    // arrange
    let state = VimState::new();

    // act
    let mode = state.mode();

    // assert
    assert_eq!(
        mode,
        VimEditorMode::Disabled,
        "vim must default to disabled"
    );
    assert!(!state.is_enabled(), "vim must not be enabled by default");
}

#[test]
fn vim_toggle_enables_normal_mode() {
    // arrange
    let mut state = VimState::new();

    // act
    state.toggle_vim();

    // assert
    assert_eq!(state.mode(), VimEditorMode::Normal);
    assert!(state.is_enabled(), "toggle must enable vim");
}

#[test]
fn vim_toggle_disables_from_normal_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    assert_eq!(state.mode(), VimEditorMode::Normal);

    // act
    state.toggle_vim();

    // assert
    assert_eq!(state.mode(), VimEditorMode::Disabled);
    assert!(!state.is_enabled());
}

#[test]
fn vim_toggle_disables_from_insert_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    state.enter_insert();
    assert_eq!(state.mode(), VimEditorMode::Insert);

    // act
    state.toggle_vim();

    // assert
    assert_eq!(state.mode(), VimEditorMode::Disabled);
}

#[test]
fn vim_enter_insert_from_disabled_is_noop() {
    // arrange
    let mut state = VimState::new();

    // act
    state.enter_insert();

    // assert
    assert_eq!(
        state.mode(),
        VimEditorMode::Disabled,
        "enter_insert from disabled must be noop"
    );
}

#[test]
fn vim_enter_normal_from_insert() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    state.enter_insert();

    // act
    state.enter_normal();

    // assert
    assert_eq!(state.mode(), VimEditorMode::Normal);
}

#[test]
fn vim_move_up_decrements_line_in_normal_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    // position at line 5
    for _ in 0..4 {
        state.move_down(100);
    }
    assert_eq!(state.cursor_line(), 4);

    // act
    state.move_up();

    // assert
    assert_eq!(state.cursor_line(), 3);
}

#[test]
fn vim_move_up_clamps_at_zero() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();

    // act
    state.move_up();
    state.move_up();

    // assert
    assert_eq!(state.cursor_line(), 0, "move_up must clamp at 0");
}

#[test]
fn vim_move_down_increments_line_in_normal_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    assert_eq!(state.cursor_line(), 0);

    // act
    state.move_down(100);

    // assert
    assert_eq!(state.cursor_line(), 1);
}

#[test]
fn vim_move_down_clamps_at_max() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();

    // act
    state.move_down(2);
    state.move_down(2);
    state.move_down(2);

    // assert
    assert_eq!(state.cursor_line(), 2, "move_down must clamp at max_line");
}

#[test]
fn vim_move_left_decrements_col_in_normal_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    for _ in 0..3 {
        state.move_right(100);
    }
    assert_eq!(state.cursor_col(), 3);

    // act
    state.move_left();

    // assert
    assert_eq!(state.cursor_col(), 2);
}

#[test]
fn vim_move_left_clamps_at_zero() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();

    // act
    state.move_left();
    state.move_left();

    // assert
    assert_eq!(state.cursor_col(), 0, "move_left must clamp at 0");
}

#[test]
fn vim_move_right_clamps_at_max() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();

    // act
    state.move_right(2);
    state.move_right(2);
    state.move_right(2);

    // assert
    assert_eq!(state.cursor_col(), 2, "move_right must clamp at max_col");
}

#[test]
fn vim_navigation_noop_in_insert_mode() {
    // arrange
    let mut state = VimState::new();
    state.toggle_vim();
    state.enter_insert();

    // act
    state.move_down(100);
    state.move_right(100);

    // assert
    assert_eq!(state.cursor_line(), 0, "navigation must be noop in insert");
    assert_eq!(state.cursor_col(), 0, "navigation must be noop in insert");
}

#[test]
fn vim_navigation_noop_when_disabled() {
    // arrange
    let mut state = VimState::new();

    // act
    state.move_down(100);
    state.move_right(100);
    state.move_up();
    state.move_left();

    // assert
    assert_eq!(
        state.cursor_line(),
        0,
        "navigation must be noop when disabled"
    );
    assert_eq!(
        state.cursor_col(),
        0,
        "navigation must be noop when disabled"
    );
}

// ---------------------------------------------------------------------------
// Compact / minimal / native scrollback
// ---------------------------------------------------------------------------

#[test]
fn screen_mode_defaults_to_compact_scrollback() {
    // arrange
    let state = ScreenModeState::new(true);

    // act
    let style = state.scrollback_style();

    // assert
    assert_eq!(
        style,
        ScrollbackStyle::Compact,
        "default scrollback must be compact"
    );
}

#[test]
fn screen_mode_cycle_scrollback_compact_to_minimal() {
    // arrange
    let mut state = ScreenModeState::new(true);
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Compact);

    // act
    state.cycle_scrollback_style();

    // assert
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Minimal);
}

#[test]
fn screen_mode_cycle_scrollback_minimal_to_native() {
    // arrange
    let mut state = ScreenModeState::new(true);
    state.cycle_scrollback_style(); // compact → minimal
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Minimal);

    // act
    state.cycle_scrollback_style();

    // assert
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Native);
}

#[test]
fn screen_mode_cycle_scrollback_native_back_to_compact() {
    // arrange
    let mut state = ScreenModeState::new(true);
    state.cycle_scrollback_style(); // → minimal
    state.cycle_scrollback_style(); // → native
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Native);

    // act
    state.cycle_scrollback_style();

    // assert
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Compact);
}

#[test]
fn screen_mode_set_scrollback_style_explicit() {
    // arrange
    let mut state = ScreenModeState::new(true);

    // act
    state.set_scrollback_style(ScrollbackStyle::Native);

    // assert
    assert_eq!(state.scrollback_style(), ScrollbackStyle::Native);
}

// ---------------------------------------------------------------------------
// Inline / fullscreen switch
// ---------------------------------------------------------------------------

#[test]
fn screen_mode_defaults_to_inline() {
    // arrange
    let state = ScreenModeState::new(true);

    // act
    let mode = state.terminal_mode();

    // assert
    assert_eq!(
        mode,
        TerminalMode::Inline,
        "default terminal mode must be inline"
    );
}

#[test]
fn screen_mode_toggle_fullscreen_switches_to_fullscreen() {
    // arrange
    let mut state = ScreenModeState::new(true);
    assert_eq!(state.terminal_mode(), TerminalMode::Inline);

    // act
    let result = state.toggle_fullscreen();

    // assert
    assert!(result.is_ok(), "toggle to fullscreen must succeed");
    assert_eq!(state.terminal_mode(), TerminalMode::Fullscreen);
}

#[test]
fn screen_mode_toggle_fullscreen_back_to_inline() {
    // arrange
    let mut state = ScreenModeState::new(true);
    state.toggle_fullscreen().expect("to fullscreen");
    assert_eq!(state.terminal_mode(), TerminalMode::Fullscreen);

    // act
    let result = state.toggle_fullscreen();

    // assert
    assert!(result.is_ok(), "toggle back to inline must succeed");
    assert_eq!(state.terminal_mode(), TerminalMode::Inline);
}

// ---------------------------------------------------------------------------
// Unsupported terminal mode
// ---------------------------------------------------------------------------

#[test]
fn screen_mode_toggle_fullscreen_unsupported_returns_error() {
    // arrange
    let mut state = ScreenModeState::new(false); // fullscreen not supported

    // act
    let result = state.toggle_fullscreen();

    // assert
    assert!(
        result.is_err(),
        "toggle_fullscreen must error when unsupported"
    );
    assert_eq!(
        state.terminal_mode(),
        TerminalMode::Inline,
        "mode must remain inline when unsupported"
    );
}

#[test]
fn screen_mode_fullscreen_error_is_fullscreen_not_supported() {
    // arrange
    let mut state = ScreenModeState::new(false);

    // act
    let result = state.toggle_fullscreen();

    // assert
    assert!(matches!(
        result,
        Err(ScreenModeError::FullscreenNotSupported)
    ));
}

// ---------------------------------------------------------------------------
// Fold / raw / manual preferences
// ---------------------------------------------------------------------------

#[test]
fn screen_mode_defaults_to_fold_view_preference() {
    // arrange
    let state = ScreenModeState::new(true);

    // act
    let pref = state.view_preference();

    // assert
    assert_eq!(
        pref,
        ViewPreference::Fold,
        "default view preference must be fold"
    );
}

#[test]
fn screen_mode_cycle_view_preference_fold_to_raw() {
    // arrange
    let mut state = ScreenModeState::new(true);
    assert_eq!(state.view_preference(), ViewPreference::Fold);

    // act
    state.cycle_view_preference();

    // assert
    assert_eq!(state.view_preference(), ViewPreference::Raw);
}

#[test]
fn screen_mode_cycle_view_preference_raw_to_manual() {
    // arrange
    let mut state = ScreenModeState::new(true);
    state.cycle_view_preference(); // fold → raw
    assert_eq!(state.view_preference(), ViewPreference::Raw);

    // act
    state.cycle_view_preference();

    // assert
    assert_eq!(state.view_preference(), ViewPreference::Manual);
}

#[test]
fn screen_mode_cycle_view_preference_manual_back_to_fold() {
    // arrange
    let mut state = ScreenModeState::new(true);
    state.cycle_view_preference(); // → raw
    state.cycle_view_preference(); // → manual
    assert_eq!(state.view_preference(), ViewPreference::Manual);

    // act
    state.cycle_view_preference();

    // assert
    assert_eq!(state.view_preference(), ViewPreference::Fold);
}

#[test]
fn screen_mode_set_view_preference_explicit() {
    // arrange
    let mut state = ScreenModeState::new(true);

    // act
    state.set_view_preference(ViewPreference::Manual);

    // assert
    assert_eq!(state.view_preference(), ViewPreference::Manual);
}

// ---------------------------------------------------------------------------
// Rewind restore draft
// ---------------------------------------------------------------------------

#[test]
fn rewind_view_starts_empty() {
    // arrange
    let view = RewindView::new();

    // act
    let len = view.len();
    let is_empty = view.is_empty();

    // assert
    assert_eq!(len, 0, "rewind view must start empty");
    assert!(is_empty, "rewind view must be empty");
}

#[test]
fn rewind_view_snapshot_captures_draft() {
    // arrange
    let mut view = RewindView::new();

    // act
    view.snapshot("hello world".to_string(), 5, Some(2));

    // assert
    assert_eq!(view.len(), 1, "snapshot must add one entry");
    assert!(!view.is_empty());
}

#[test]
fn rewind_view_restore_returns_latest_snapshot() {
    // arrange
    let mut view = RewindView::new();
    view.snapshot("first draft".to_string(), 3, None);
    view.snapshot("second draft".to_string(), 7, Some(1));

    // act
    let restored = view.restore();

    // assert
    let snapshot = restored.expect("must restore a snapshot");
    assert_eq!(snapshot.text, "second draft");
    assert_eq!(snapshot.cursor, 7);
    assert_eq!(snapshot.selection_anchor, Some(1));
}

#[test]
fn rewind_view_restore_empty_returns_none() {
    // arrange
    let view = RewindView::new();

    // act
    let restored = view.restore();

    // assert
    assert!(restored.is_none(), "restore on empty must return None");
}

#[test]
fn rewind_view_rewind_goes_back_in_history() {
    // arrange
    let mut view = RewindView::new();
    view.snapshot("v1".to_string(), 0, None);
    view.snapshot("v2".to_string(), 0, None);
    view.snapshot("v3".to_string(), 0, None);
    assert_eq!(view.selected_index(), 2);

    // act
    let rewound = view.rewind(1);

    // assert
    assert!(rewound, "rewind must succeed");
    assert_eq!(view.selected_index(), 1);
    let restored = view.restore().expect("restore");
    assert_eq!(restored.text, "v2");
}

#[test]
fn rewind_view_rewind_clamps_at_zero() {
    // arrange
    let mut view = RewindView::new();
    view.snapshot("v1".to_string(), 0, None);
    view.snapshot("v2".to_string(), 0, None);

    // act
    let rewound = view.rewind(10);

    // assert
    assert!(rewound, "rewind must succeed");
    assert_eq!(view.selected_index(), 0, "rewind must clamp at 0");
}

#[test]
fn rewind_view_forward_advances_in_history() {
    // arrange
    let mut view = RewindView::new();
    view.snapshot("v1".to_string(), 0, None);
    view.snapshot("v2".to_string(), 0, None);
    view.snapshot("v3".to_string(), 0, None);
    view.rewind(2);
    assert_eq!(view.selected_index(), 0);

    // act
    let forwarded = view.forward(1);

    // assert
    assert!(forwarded, "forward must succeed");
    assert_eq!(view.selected_index(), 1);
    let restored = view.restore().expect("restore");
    assert_eq!(restored.text, "v2");
}

#[test]
fn rewind_view_forward_clamps_at_last() {
    // arrange
    let mut view = RewindView::new();
    view.snapshot("v1".to_string(), 0, None);
    view.snapshot("v2".to_string(), 0, None);
    view.rewind(1);
    assert_eq!(view.selected_index(), 0);

    // act
    let forwarded = view.forward(10);

    // assert
    assert!(forwarded, "forward must succeed");
    assert_eq!(view.selected_index(), 1, "forward must clamp at last");
}

#[test]
fn rewind_view_rewind_empty_returns_false() {
    // arrange
    let mut view = RewindView::new();

    // act
    let result = view.rewind(1);

    // assert
    assert!(!result, "rewind on empty must return false");
}

#[test]
fn rewind_view_forward_empty_returns_false() {
    // arrange
    let mut view = RewindView::new();

    // act
    let result = view.forward(1);

    // assert
    assert!(!result, "forward on empty must return false");
}

#[test]
fn rewind_view_snapshot_preserves_cursor_and_anchor() {
    // arrange
    let mut view = RewindView::new();

    // act
    view.snapshot("text".to_string(), 42, Some(10));

    // assert
    let snapshot = view.restore().expect("restore");
    assert_eq!(snapshot.cursor, 42);
    assert_eq!(snapshot.selection_anchor, Some(10));
}

// ---------------------------------------------------------------------------
// Prompt stash integration
// ---------------------------------------------------------------------------

#[test]
fn prompt_stash_push_stashes_current_prompt() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "stash me".to_string();
    app.composer.prompt_cursor = 4;

    // act
    stash_push_via_key(&mut app);

    // assert
    assert_eq!(app.prompt_stash.entries.len(), 1);
    assert_eq!(app.prompt_stash.entries[0].text, "stash me");
    assert_eq!(app.prompt_stash.entries[0].cursor, 4);
    assert!(
        app.composer.prompt_buffer.is_empty(),
        "buffer must be cleared"
    );
}

#[test]
fn prompt_stash_pop_restores_last_stashed() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "first".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer = "second".to_string();
    stash_push_via_key(&mut app);
    assert_eq!(app.prompt_stash.entries.len(), 2);

    // act
    stash_pop_via_key(&mut app);

    // assert
    assert_eq!(app.composer.prompt_buffer, "second");
    assert_eq!(app.prompt_stash.entries.len(), 1);
}

#[test]
fn prompt_stash_push_empty_buffer_is_noop() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer.clear();

    // act
    stash_push_via_key(&mut app);

    // assert
    assert!(
        app.prompt_stash.entries.is_empty(),
        "empty buffer push must be noop"
    );
}

#[test]
fn prompt_stash_list_restore_selected_restores_entry() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "entry one".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer = "entry two".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer.clear();
    open_stash_list_via_key(&mut app);
    assert!(app.prompt_stash.list_visible);

    // act — restore the selected entry via the list overlay Enter key
    app.handle_key(key(KeyCode::Enter));

    // assert
    assert_eq!(
        app.composer.prompt_buffer, "entry two",
        "must restore the selected entry"
    );
    assert!(
        !app.prompt_stash.list_visible,
        "list must close after restore"
    );
}

#[test]
fn prompt_stash_list_move_navigates_entries() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "a".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer = "b".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer = "c".to_string();
    stash_push_via_key(&mut app);
    open_stash_list_via_key(&mut app);
    assert_eq!(app.prompt_stash.list_selected, 2);

    // act — navigate up twice via the list overlay Up key
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Up));

    // assert
    assert_eq!(app.prompt_stash.list_selected, 0);
}

#[test]
fn prompt_stash_list_delete_selected_removes_entry() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "keep".to_string();
    stash_push_via_key(&mut app);
    app.composer.prompt_buffer = "delete".to_string();
    stash_push_via_key(&mut app);
    open_stash_list_via_key(&mut app);
    assert_eq!(app.prompt_stash.entries.len(), 2);

    // act — delete the selected entry via the list overlay Ctrl+D key
    app.handle_key(key_with_modifiers(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert_eq!(app.prompt_stash.entries.len(), 1);
    assert_eq!(app.prompt_stash.entries[0].text, "keep");
}

// ---------------------------------------------------------------------------
// Mode transition integration
// ---------------------------------------------------------------------------

#[test]
fn plan_enter_then_exit_then_reenter_cycle() {
    // arrange
    let (mut app, _dir) = live_app_with_workspace();

    // act
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());
    close_plan_view_via_key(&mut app);
    assert!(!app.plan_view_is_visible());
    open_plan_view_via_key(&mut app);

    // assert
    assert!(
        app.plan_view_is_visible(),
        "plan view must be visible after re-enter"
    );
}

#[test]
fn plan_full_lifecycle_enter_edit_approve_exit() {
    // arrange
    let (mut app, dir) = live_app_with_workspace();
    ingest_run_started(&mut app, &dir);

    // act
    open_plan_view_via_key(&mut app);
    assert!(app.plan_view_is_visible());
    app.plan_edit("# Full Lifecycle Plan\n".to_string())
        .expect("edit");
    app.plan_approve();
    assert!(!app.plan_view_is_visible());
    close_plan_view_via_key(&mut app);

    // assert
    let plan_path = dir
        .path()
        .join(".agent-harness")
        .join("plans")
        .join("run_task29_modes.md");
    assert!(plan_path.is_file(), "plan file must persist after exit");
    let body = fs::read_to_string(&plan_path).expect("read");
    assert_eq!(body, "# Full Lifecycle Plan\n");
}

#[test]
fn screen_mode_and_vim_state_independent() {
    // arrange
    let mut screen = ScreenModeState::new(true);
    let mut vim = VimState::new();

    // act
    screen.toggle_fullscreen().expect("fullscreen");
    vim.toggle_vim();
    screen.cycle_scrollback_style();
    vim.move_down(10);

    // assert
    assert_eq!(screen.terminal_mode(), TerminalMode::Fullscreen);
    assert_eq!(screen.scrollback_style(), ScrollbackStyle::Minimal);
    assert_eq!(vim.mode(), VimEditorMode::Normal);
    assert_eq!(vim.cursor_line(), 1);
}

#[test]
fn rewind_and_stash_can_coexist() {
    // arrange
    let mut app = live_app();
    let mut rewind = RewindView::new();

    // act
    app.composer.prompt_buffer = "draft one".to_string();
    stash_push_via_key(&mut app);
    rewind.snapshot("draft one".to_string(), 0, None);
    app.composer.prompt_buffer = "draft two".to_string();
    stash_push_via_key(&mut app);
    rewind.snapshot("draft two".to_string(), 0, None);

    // assert
    assert_eq!(app.prompt_stash.entries.len(), 2);
    assert_eq!(rewind.len(), 2);
    let restored = rewind.restore().expect("restore");
    assert_eq!(restored.text, "draft two");
}
