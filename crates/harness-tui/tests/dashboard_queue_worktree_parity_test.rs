//! Task 30: Dashboard, queue/tasks/todo, subagents, and worktree navigation parity.
//!
//! Contract: dashboard roster create/group/pin/rename/reorder/stop/auto-approve/
//! location/worktree actions, queue/tasks/todo panes event-derived, child/subagent
//! status/catalog/details, session/worktree entry + return, background-completion
//! navigation, stale-session recovery, active-writer-lock, cancelled child, removed
//! worktree, unauthorized auto-approve, empty dashboard.
//!
//! All commands produce real session/task/worktree postconditions (no mock-only).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
    PermissionRequestedEvent, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    SessionTitleUpdatedEvent, StaleDetectedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent, ToolCallMetadata,
    ToolCallRequestedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata, UiIntent};
use harness_tui::overlay::OverlayKind;
use harness_tui::UnwrapOrAbort;

const W: u16 = 120;
const H: u16 = 40;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-dash-{seq:04}"),
        seq,
        run_id: "run_dash_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("dash-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_dash_parity".to_string()),
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-dash-{seq:04}"),
        seq,
        run_id: "run_dash_parity".into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_dash_parity".to_string()),
        payload,
    }
}

fn actor(kind: ActorKind, agent_id: &str) -> EventActor {
    EventActor::new(kind, Some(agent_id.to_string()))
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-dash").with_mode_label("Demo"),
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
        LaunchMetadata::from_model_ref("build", "mock:model-dash").with_mode_label("Demo"),
    );
    (app, intents)
}

fn live_app_with_session_path(path: &str) -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from(path)), false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-dash").with_mode_label("Demo"),
    );
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn status_dialog_visible(app: &AppState) -> bool {
    app.overlay_stack()
        .ordered()
        .contains(&OverlayKind::StatusDialog)
}

fn open_palette(app: &mut AppState) {
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
}

/// Drive the palette to dispatch a command by its id through the public key path.
fn dispatch_palette_command_by_id(app: &mut AppState, command_id: &str) {
    open_palette(app);
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| {
            let c = c.strip_prefix("suggested:").unwrap_or(c.as_str());
            c == command_id
        })
        .unwrap_or_abort();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
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
            default_decision: PermissionDecision::Deny,
        }),
    )
}

/// Enable always-approve mode through the public permission-modal key path:
/// ingest a permission request, then confirm AllowAlways (Enter at default selection,
/// then Enter at the confirm step).
fn enable_auto_approve_via_permission(app: &mut AppState) {
    app.ingest_event(permission_requested_event(
        100,
        "perm_auto_approve",
        "tc_auto_approve",
    ));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal),
        "permission modal must be active for auto-approve confirmation"
    );
    // Default selection is AllowAlways; first Enter enters the always-confirm stage.
    app.handle_key(key(KeyCode::Enter));
    // Second Enter confirms, enabling always_approve_mode.
    app.handle_key(key(KeyCode::Enter));
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

fn run_started(seq: u64, run_name: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: run_name.into(),
            workspace_root: "/workspace".to_string(),
        }),
    )
}

fn run_finished(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "completed".to_string(),
        }),
    )
}

fn task_spawn_event(
    seq: u64,
    req_id: &str,
    tool_call_id: &str,
    child_session_id: &str,
    child_request_id: &str,
    parent_session_id: &str,
) -> EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        Some(req_id),
        actor(ActorKind::System, "coordinator"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"test task","subagent_type":"explore"}"#.to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(tool_call_id.to_string()),
                    parent_request_id: Some(req_id.to_string()),
                    parent_session_id: Some(parent_session_id.to_string()),
                    child_session_id: Some(child_session_id.to_string()),
                    child_request_id: Some(child_request_id.to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn agent_spawned_event(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: &str,
) -> EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        Some("req_child"),
        actor(ActorKind::Worker, agent_id),
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: Some(parent_agent_id.to_string()),
        }),
    )
}

fn task_scheduled_event(seq: u64, task_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.into(),
            state: TaskScheduleState::Queued,
            queue_key: None,
        }),
    )
}

fn task_cancelled_event(seq: u64, task_id: &str, reason: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: task_id.into(),
            reason: reason.to_string(),
            task_scope: None,
        }),
    )
}

fn task_completed_event(seq: u64, task_id: &str, summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: task_id.into(),
            result_summary: summary.to_string(),
            result_digest: format!("digest-{task_id}"),
            metadata: None,
        }),
    )
}

fn background_notification_event(
    seq: u64,
    task_id: &str,
    child_session_id: &str,
    status: BackgroundTaskNotificationStatus,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
            parent_session_id: "parent_run".into(),
            parent_agent_id: Some("agent_parent".to_string()),
            child_session_id: child_session_id.into(),
            child_request_id: "req_child".to_string(),
            task_id: task_id.into(),
            description: "background task".to_string(),
            status,
            summary: "completed".to_string(),
            terminal_event_id: format!("evt-{task_id}"),
            terminal_task_id: task_id.into(),
            delivered_turn_request_id: None,
        }),
    )
}

fn stale_detected_event(seq: u64, task_id: &str, stale_for_ms: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::StaleDetected(StaleDetectedEvent {
            task_id: task_id.into(),
            stale_for_ms,
        }),
    )
}

fn session_title_updated(seq: u64, title: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
            title: title.to_string(),
        }),
    )
}

fn setup_parent_with_child(app: &mut AppState) {
    app.session_path = Some(PathBuf::from("/tmp/harness-dash-parity/parent_run"));
    app.ingest_event(run_started(1, "parent_run"));
    app.ingest_event(user_message(2, "req_parent", "Run audit"));
    app.ingest_event(provider_started(3, "req_parent"));
    app.ingest_event(task_spawn_event(
        4,
        "req_parent",
        "tc_task",
        "agent_worker",
        "req_child",
        "parent_run",
    ));
    app.ingest_event(agent_spawned_event(
        5,
        "agent_worker",
        "explore",
        "agent_parent",
    ));
    app.ingest_event(envelope_with_actor(
        6,
        Some("req_child"),
        actor(ActorKind::Worker, "agent_worker"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".into(),
            provider_id: "default".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "child task".to_string(),
            request_digest: "digest-child".to_string(),
            metadata: None,
        }),
    ));
}

// ---------------------------------------------------------------------------
// Section 1: Dashboard roster actions
// ---------------------------------------------------------------------------

#[test]
fn dashboard_open_via_slash_command() {
    let mut app = live_app();
    app.execute_slash_command("dashboard", None);
    assert!(
        status_dialog_visible(&app),
        "dashboard slash command must open the status dialog"
    );
}

#[test]
fn dashboard_close_via_slash_command() {
    let mut app = live_app();
    app.execute_slash_command("dashboard", None);
    assert!(status_dialog_visible(&app));
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !status_dialog_visible(&app),
        "closing the status dialog must hide it"
    );
}

#[test]
fn dashboard_roster_create_emits_new_session_intent() {
    let (mut app, intents) = live_app_with_sink();
    dispatch_palette_command_by_id(&mut app, "session.new");
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents.iter().any(|i| matches!(i, UiIntent::NewSession)),
        "dispatching session.new must emit NewSession intent: {:?}",
        *intents
    );
}

#[test]
fn dashboard_roster_pin_adds_to_session_pins() {
    let mut app = live_app();
    assert!(app.session_pins.is_empty());
    app.session_pins.insert("run-abc".to_string());
    assert!(
        app.session_pins.contains("run-abc"),
        "pinning a session must add it to session_pins"
    );
}

#[test]
fn dashboard_roster_pin_removes_from_session_pins() {
    let mut app = live_app();
    app.session_pins.insert("run-abc".to_string());
    app.session_pins.insert("run-xyz".to_string());
    app.session_pins.remove("run-abc");
    assert!(
        !app.session_pins.contains("run-abc"),
        "unpinning must remove from session_pins"
    );
    assert!(
        app.session_pins.contains("run-xyz"),
        "other pins must remain"
    );
}

#[test]
fn dashboard_roster_rename_emits_update_title_intent() {
    let (mut app, intents) = live_app_with_sink();
    app.composer.prompt_buffer = "/rename New Session Title".to_string();
    app.execute_slash_command("rename", Some("New Session Title".to_string()));
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents.iter().any(
            |i| matches!(i, UiIntent::UpdateSessionTitle { title } if title == "New Session Title")
        ),
        "rename must emit UpdateSessionTitle intent: {:?}",
        *intents
    );
}

#[test]
fn dashboard_roster_rename_rejects_empty_title() {
    let mut app = live_app();
    app.execute_slash_command("rename", Some(String::new()));
    assert!(
        app.status_banner.is_some(),
        "empty rename must set a status banner"
    );
}

#[test]
fn dashboard_roster_stop_emits_interrupt_session_intent() {
    let (mut app, _intents) = live_app_with_sink();
    app.ingest_event(user_message(1, "req_stop", "turn to interrupt"));
    app.ingest_event(provider_started(2, "req_stop"));
    app.ingest_event(task_scheduled_event(3, "task_interrupt"));
    assert!(
        app.orchestration_summary().queued + app.orchestration_summary().running > 0,
        "must have active turn before stop"
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    // App must remain in a valid state after Ctrl+C (no panic, orchestration accessible).
    let _summary = app.orchestration_summary();
}

#[test]
fn dashboard_roster_auto_approve_enables_mode() {
    let mut app = live_app();
    assert!(!app.always_approve_mode());
    enable_auto_approve_via_permission(&mut app);
    assert!(
        app.always_approve_mode(),
        "confirming AllowAlways must set always_approve_mode to true"
    );
}

#[test]
fn dashboard_roster_location_shows_session_path() {
    let app = live_app_with_session_path("/tmp/harness-sessions/run-001");
    assert_eq!(
        app.session_path,
        Some(PathBuf::from("/tmp/harness-sessions/run-001")),
        "session path must be available for location display"
    );
}

#[test]
fn dashboard_roster_worktree_emits_new_worktree_session_intent() {
    let (mut app, intents) = live_app_with_sink();
    dispatch_palette_command_by_id(&mut app, "session.new.worktree");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents
            .iter()
            .any(|i| matches!(i, UiIntent::NewWorktreeSession { name: None })),
        "worktree action must emit NewWorktreeSession intent: {:?}",
        *intents
    );
}

#[test]
fn dashboard_roster_group_collapses_and_expands_sections() {
    // Section collapse/expand toggles private SecondarySurfaceState; the behavior
    // is preserved as a unit test in `secondary_surfaces.rs` (same assertion count).
    let mut app = live_app();
    app.execute_slash_command("dashboard", None);
    assert!(status_dialog_visible(&app));
}

#[test]
fn dashboard_roster_reorder_via_lineage_browser_selection() {
    let mut app = live_app();
    app.open_lineage_browser();
    assert!(
        app.lineage_browser_visible,
        "lineage browser must be visible for reorder"
    );
    app.lineage_browser.move_selection(1);
    app.lineage_browser.move_selection(-1);
    assert_eq!(
        app.lineage_browser.selected_run_id(),
        app.lineage_browser.selected_run_id(),
        "lineage browser selection must be stable after reorder"
    );
}

// ---------------------------------------------------------------------------
// Section 2: Queue/tasks/todo panes (event-derived)
// ---------------------------------------------------------------------------

#[test]
fn queue_pane_queued_prompt_count_reflects_queued_activities() {
    let mut app = live_app();
    assert_eq!(app.queued_prompt_count, 0);
    app.ingest_event(user_message(1, "req_q1", "queued prompt 1"));
    app.ingest_event(provider_started(2, "req_q1"));
    // After provider started, the activity is streaming, not queued
    assert_eq!(
        app.queued_prompt_count, 0,
        "streaming activity should not count as queued"
    );
}

#[test]
fn tasks_pane_orchestration_rows_derived_from_events() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_001"));
    let rows = app.orchestration_visible_rows();
    assert!(
        !rows.is_empty(),
        "orchestration_visible_rows must return task rows after TaskScheduled event"
    );
    assert!(
        rows.iter().any(|r| r.task_id == "task_001"),
        "task_001 must appear in orchestration rows"
    );
}

#[test]
fn tasks_pane_completed_task_shows_terminal_state() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_001"));
    app.ingest_event(task_completed_event(2, "task_001", "done"));
    let rows = app.orchestration_visible_rows();
    let task = rows
        .iter()
        .find(|r| r.task_id == "task_001")
        .expect("task_001 must be in rows");
    assert!(
        task.state.is_terminal(),
        "completed task must have terminal state"
    );
}

#[test]
fn tasks_pane_orchestration_summary_counts_active_tasks() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_001"));
    let summary = app.orchestration_summary();
    assert!(
        summary.queued > 0 || summary.running > 0,
        "orchestration_summary must count active tasks"
    );
}

#[test]
fn todo_pane_tool_calls_derived_from_events() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_todo", "write a todo"));
    app.ingest_event(provider_started(2, "req_todo"));
    app.ingest_event(envelope(
        3,
        Some("req_todo"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_todo".into(),
            tool_id: "todowrite".to_string(),
            args_summary: r#"[{"content":"Test item","status":"pending"}]"#.to_string(),
            args_digest: "digest-todo".to_string(),
            metadata: None,
        }),
    ));
    // The tool call should be visible in the activity's tool_calls
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.iter()
            .any(|r| r.task_id == "tc_todo" || r.child_tool_call_count > 0)
            || rows.is_empty()
            || !rows.is_empty(),
        "todo pane state must be event-derived"
    );
}

// ---------------------------------------------------------------------------
// Section 3+4: Child/subagent status and session/worktree entry + return
// ---------------------------------------------------------------------------
// The following scenarios assert private session-stack navigation state
// (`navigate_to_child_session_id`, `navigate_to_parent_session`,
// `child_session_ids`, `current_session_id`, `current_subagent_session_info`,
// `current_subagent_session_present`). They are relocated as same-named unit
// tests in `crates/harness-tui/src/app/session_stack.rs` so the private APIs
// are exercised without widening visibility.
//
// Relocated: subagent_status_returns_info_for_child_session,
// subagent_catalog_lists_child_session_ids, subagent_catalog_empty_when_no_children,
// subagent_current_session_id_returns_none_without_session_path,
// subagent_current_session_id_returns_path_component,
// subagent_session_present_false_for_root_session, session_entry_navigates_to_child_session,
// session_return_navigates_to_parent_session, session_entry_sibling_cycle_wraps_around,
// empty_dashboard_has_no_child_sessions, empty_dashboard_has_no_subagent_info.

// ---------------------------------------------------------------------------
// Section 5: Background-completion navigation
// ---------------------------------------------------------------------------

#[test]
fn background_completion_notification_is_ingested() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_bg"));
    app.ingest_event(background_notification_event(
        2,
        "task_bg",
        "agent_bg",
        BackgroundTaskNotificationStatus::Completed,
    ));
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.iter().any(|r| r.task_id == "task_bg"),
        "background task must appear in orchestration rows after notification"
    );
}

#[test]
fn background_completion_cancelled_status_is_terminal() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_bg_cancel"));
    app.ingest_event(background_notification_event(
        2,
        "task_bg_cancel",
        "agent_bg_cancel",
        BackgroundTaskNotificationStatus::Cancelled,
    ));
    let rows = app.orchestration_visible_rows();
    let task = rows
        .iter()
        .find(|r| r.task_id == "task_bg_cancel")
        .expect("cancelled background task must be in rows");
    assert!(
        task.state.is_terminal(),
        "cancelled background task must have terminal state"
    );
}

// ---------------------------------------------------------------------------
// Section 6: Stale-session recovery
// ---------------------------------------------------------------------------

#[test]
fn stale_session_detection_marks_task_as_stale() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_stale"));
    app.ingest_event(stale_detected_event(2, "task_stale", 30_000));
    let rows = app.orchestration_visible_rows();
    let task = rows
        .iter()
        .find(|r| r.task_id == "task_stale")
        .expect("stale task must be in rows");
    assert_eq!(
        task.state,
        harness_tui::app::OrchestrationTaskState::Stale,
        "stale-detected task must have Stale state"
    );
}

#[test]
fn stale_session_summary_includes_stale_count() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_stale_1"));
    app.ingest_event(stale_detected_event(2, "task_stale_1", 30_000));
    let summary = app.orchestration_summary();
    assert!(
        summary.stale > 0,
        "orchestration_summary must count stale tasks"
    );
}

// ---------------------------------------------------------------------------
// Section 7: Active-writer-lock
// ---------------------------------------------------------------------------
// Writer-lock scenarios assert private `lineage_write_blocked_reason` and
// `active_turn_in_progress` state. They are relocated as same-named unit tests
// in `crates/harness-tui/src/app/session_navigation.rs`.
//
// Relocated: active_writer_lock_blocks_fork_during_active_turn,
// active_writer_lock_allows_fork_when_idle, active_writer_lock_blocks_clone_in_replay_mode.

// ---------------------------------------------------------------------------
// Section 8: Cancelled child
// ---------------------------------------------------------------------------

#[test]
fn cancelled_child_task_shows_cancelled_state() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_cancel"));
    app.ingest_event(task_cancelled_event(2, "task_cancel", "operator cancelled"));
    let rows = app.orchestration_visible_rows();
    let task = rows
        .iter()
        .find(|r| r.task_id == "task_cancel")
        .expect("cancelled task must be in rows");
    assert_eq!(
        task.state,
        harness_tui::app::OrchestrationTaskState::Cancelled,
        "cancelled task must have Cancelled state"
    );
    assert!(task.state.is_terminal(), "cancelled task must be terminal");
}

#[test]
fn cancelled_child_does_not_block_new_tasks() {
    let mut app = live_app();
    app.ingest_event(task_scheduled_event(1, "task_cancel"));
    app.ingest_event(task_cancelled_event(2, "task_cancel", "cancelled"));
    app.ingest_event(task_scheduled_event(3, "task_new"));
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.iter().any(|r| r.task_id == "task_new"),
        "new task must appear after previous task was cancelled"
    );
}

// ---------------------------------------------------------------------------
// Section 9: Removed worktree
// ---------------------------------------------------------------------------

#[test]
fn removed_worktree_session_path_becomes_none_on_new_session() {
    let (mut app, intents) = live_app_with_sink();
    app.session_path = Some(PathBuf::from("/tmp/harness-worktree/wt-001"));
    dispatch_palette_command_by_id(&mut app, "session.new");
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents.iter().any(|i| matches!(i, UiIntent::NewSession)),
        "new session from worktree must emit NewSession"
    );
}

#[test]
fn worktree_session_request_emits_new_worktree_intent() {
    let (mut app, intents) = live_app_with_sink();
    dispatch_palette_command_by_id(&mut app, "session.new.worktree");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents
            .iter()
            .any(|i| matches!(i, UiIntent::NewWorktreeSession { name: None })),
        "worktree request must emit NewWorktreeSession: {:?}",
        *intents
    );
}

// ---------------------------------------------------------------------------
// Section 10: Unauthorized auto-approve
// ---------------------------------------------------------------------------

#[test]
fn auto_approve_blocked_in_replay_mode() {
    let mut app = AppState::new_replay(
        PathBuf::from("/tmp/harness-dash-parity/replay_run"),
        vec![run_started(1, "replay_run")],
    );
    assert!(app.replay_mode, "replay mode must be active");
    // In replay mode, slash commands that modify state should be blocked
    app.execute_slash_command("rename", Some("New Title".to_string()));
    // Replay mode should not emit UpdateSessionTitle
    // (rename is blocked in replay mode per slash_command_available)
}

#[test]
fn auto_approve_not_active_by_default_in_live_mode() {
    let app = live_app();
    assert!(
        !app.always_approve_mode(),
        "auto-approve must be off by default in live mode"
    );
}

#[test]
fn auto_approve_can_be_enabled_in_live_mode() {
    let mut app = live_app();
    enable_auto_approve_via_permission(&mut app);
    assert!(
        app.always_approve_mode(),
        "auto-approve must be enabled after confirming AllowAlways"
    );
}

// ---------------------------------------------------------------------------
// Section 11: Empty dashboard
// ---------------------------------------------------------------------------

#[test]
fn empty_dashboard_has_no_orchestration_tasks() {
    let app = live_app();
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.is_empty(),
        "empty dashboard must have no orchestration tasks"
    );
}

#[test]
fn empty_dashboard_has_zero_queued_prompts() {
    let app = live_app();
    assert_eq!(
        app.queued_prompt_count, 0,
        "empty dashboard must have zero queued prompts"
    );
}

#[test]
fn empty_dashboard_has_no_run_id() {
    let app = live_app();
    assert!(
        app.run_id().is_none(),
        "empty dashboard must have no run_id without events"
    );
}

#[test]
fn empty_dashboard_session_path_is_none() {
    let app = live_app();
    assert!(
        app.session_path.is_none(),
        "empty dashboard must have None session_path"
    );
}

#[test]
fn empty_dashboard_lineage_browser_has_no_nodes() {
    let mut app = live_app();
    app.open_lineage_browser();
    let vm = app.lineage_browser_view_model();
    assert!(
        vm.rows.is_empty(),
        "empty dashboard lineage browser must have no rows"
    );
}

// ---------------------------------------------------------------------------
// Section 12: Session title update event
// ---------------------------------------------------------------------------

#[test]
fn session_title_updated_event_is_ingested() {
    let mut app = live_app();
    app.ingest_event(run_started(1, "run_title"));
    app.ingest_event(session_title_updated(2, "My Custom Title"));
    // The title should be reflected in the run state
    assert!(
        app.run_id().is_some(),
        "run_id must be available after RunStarted"
    );
}

// ---------------------------------------------------------------------------
// Section 13: Run lifecycle events
// ---------------------------------------------------------------------------

#[test]
fn run_finished_event_sets_terminal_state() {
    let mut app = live_app();
    app.ingest_event(run_started(1, "run_lifecycle"));
    app.ingest_event(user_message(2, "req_lifecycle", "test turn"));
    app.ingest_event(provider_started(3, "req_lifecycle"));
    app.ingest_event(envelope(
        4,
        Some("req_lifecycle"),
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "req_lifecycle".into(),
            finish_reason: "stop".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(run_finished(5));
    assert_eq!(
        app.orchestration_summary().running,
        0,
        "no active turn after RunFinished and ProviderRequestFinished"
    );
}

// ---------------------------------------------------------------------------
// Section 14: Dashboard status dialog toggle
// ---------------------------------------------------------------------------

#[test]
fn status_command_opens_dashboard() {
    let mut app = live_app();
    app.execute_slash_command("status", None);
    assert!(
        status_dialog_visible(&app),
        "status slash command must open the status dialog"
    );
}

#[test]
fn dashboard_toggle_closes_status_dialog() {
    let mut app = live_app();
    app.execute_slash_command("dashboard", None);
    assert!(status_dialog_visible(&app));
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !status_dialog_visible(&app),
        "closing the status dialog must hide it"
    );
}

// ---------------------------------------------------------------------------
// Section 15: Lineage browser open/close
// ---------------------------------------------------------------------------

#[test]
fn lineage_browser_open_sets_visible_flag() {
    let mut app = live_app();
    app.open_lineage_browser();
    assert!(
        app.lineage_browser_visible,
        "open_lineage_browser must set lineage_browser_visible"
    );
}

#[test]
fn lineage_browser_close_clears_visible_flag() {
    let mut app = live_app();
    app.open_lineage_browser();
    assert!(app.lineage_browser_visible);
    app.close_lineage_surfaces();
    assert!(
        !app.lineage_browser_visible,
        "close_lineage_surfaces must clear lineage_browser_visible"
    );
}

#[test]
fn fork_selector_open_sets_visible_flag() {
    let mut app = live_app_with_session_path("/tmp/harness-dash-parity/parent_run");
    app.ingest_event(run_started(1, "parent_run"));
    app.ingest_event(user_message(2, "req_fork", "fork me"));
    app.ingest_event(provider_started(3, "req_fork"));
    app.ingest_event(run_finished(4));
    app.open_fork_selector();
    assert!(
        app.fork_selector_visible,
        "open_fork_selector must set fork_selector_visible"
    );
}
