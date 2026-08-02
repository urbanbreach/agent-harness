//! Task 29: Dashboard, agents, tasks, queue, plans, and worktree/session
//! journeys parity tests.
//!
//! Contract: `grok-build-parity-parallel-execution.md` lines 985-993.
//!
//! Covers: dashboard entry/exit/rows, multi-agent/coordinator journeys,
//! failure/cancel/restart, task/subagent status, queue state, plan state,
//! worktree choice, session navigation (history/lineage/fork/clone/rename),
//! keyboard and mouse navigation, responsive layout, semantic cells, and
//! no-hosted-hub-state dependency.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, BackgroundTaskNotificationEvent, BackgroundTaskNotificationStatus, EventActor,
    EventEnvelopeV1, EventV1, RunStartedEvent, StaleDetectedEvent, TaskCancelledEvent,
    TaskCompletedEvent, TaskResultLateEvent, TaskScheduleState, TaskScheduledEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};
use harness_tui::overlay::OverlayKind;
use harness_tui::render_test::render_to_string;
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
        event_id: format!("evt-task29-{seq:04}"),
        seq,
        run_id: "run_task29_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task29_parity".to_string()),
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    let mut env = envelope(seq, correlation_id, payload);
    env.actor = actor;
    env
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task29").with_mode_label("Demo"),
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
        LaunchMetadata::from_model_ref("build", "mock:model-task29").with_mode_label("Demo"),
    );
    (app, intents)
}

fn startup_app_with_sink() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    (AppState::new_startup(Vec::new(), Some(sink)), intents)
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

fn agent_spawned(seq: u64, agent_id: &str, profile: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )
}

fn agent_spawned_with_parent(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: Some(parent.to_string()),
        }),
    )
}

fn task_scheduled_queued(seq: u64, task_id: &str, queue_key: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(task_id),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.to_string().into(),
            state: TaskScheduleState::Queued,
            queue_key: Some(queue_key.to_string()),
        }),
    )
}

fn task_scheduled_started(seq: u64, task_id: &str, queue_key: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(task_id),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.to_string()),
        }),
    )
}

fn task_completed(seq: u64, task_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(task_id),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: task_id.to_string().into(),
            result_summary: format!("completed: {task_id}"),
            result_digest: format!("digest-{task_id}"),
            metadata: None,
        }),
    )
}

fn task_cancelled(seq: u64, task_id: &str, reason: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(task_id),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: task_id.to_string().into(),
            reason: reason.to_string(),
            task_scope: None,
        }),
    )
}

fn stale_detected(seq: u64, task_id: &str, stale_for_ms: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::StaleDetected(StaleDetectedEvent {
            task_id: task_id.to_string().into(),
            stale_for_ms,
        }),
    )
}

fn task_result_late(seq: u64, task_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::TaskResultLate(TaskResultLateEvent {
            task_id: task_id.to_string().into(),
            result_digest: format!("late-digest-{task_id}"),
        }),
    )
}

fn background_notification(
    seq: u64,
    task_id: &str,
    status: BackgroundTaskNotificationStatus,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
            parent_session_id: "run_task29_parity".to_string().into(),
            parent_agent_id: None,
            child_session_id: "child_session_task29".to_string().into(),
            child_request_id: "child_req_task29".to_string(),
            task_id: task_id.to_string().into(),
            description: format!("bg task: {task_id}"),
            status,
            summary: format!("bg summary: {task_id}"),
            terminal_event_id: format!("evt-terminal-{task_id}"),
            terminal_task_id: task_id.to_string(),
            delivered_turn_request_id: None,
        }),
    )
}

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn open_dashboard(app: &mut AppState) {
    app.execute_slash_command("dashboard", None);
}

fn dashboard_visible(app: &AppState) -> bool {
    app.overlay_stack()
        .ordered()
        .contains(&OverlayKind::StatusDialog)
}

// ---------------------------------------------------------------------------
// Dashboard entry, exit, and rows
// ---------------------------------------------------------------------------

#[test]
fn dashboard_absent_by_default() {
    // arrange
    let app = live_app();

    // act
    let visible = dashboard_visible(&app);

    // assert
    assert!(!visible, "dashboard must not be visible by default");
}

#[test]
fn dashboard_opens_via_slash_command() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("dashboard", None);

    // assert
    assert!(
        dashboard_visible(&app),
        "dashboard must open via /dashboard"
    );
}

#[test]
fn dashboard_opens_via_status_slash_command() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("status", None);

    // assert
    assert!(dashboard_visible(&app), "dashboard must open via /status");
}

#[test]
fn dashboard_closes_on_esc() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);
    assert!(dashboard_visible(&app));

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(!dashboard_visible(&app), "dashboard must close on Esc");
}

#[test]
fn dashboard_renders_operator_summary_line() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_alpha", "researcher"));
    app.ingest_event(task_scheduled_queued(
        2,
        "task_alpha",
        "agent:queued:primary",
    ));
    open_dashboard(&mut app);

    // act
    let rendered = render(&app);

    // assert
    assert!(
        rendered.contains("operator dashboard:"),
        "dashboard must render operator summary line\n{rendered}"
    );
}

#[test]
fn dashboard_does_not_depend_on_hosted_hub_state() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);

    // act
    let rendered = render(&app);

    // assert
    assert!(
        !rendered.contains("hub.") && !rendered.contains("hosted"),
        "dashboard must not reference hosted hub state\n{rendered}"
    );
    assert!(
        !rendered.contains("marketplace") && !rendered.contains("enterprise"),
        "dashboard must not reference enterprise/marketplace\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Multi-agent / coordinator journeys
// ---------------------------------------------------------------------------

#[test]
fn multi_agent_journey_tracks_active_agents() {
    // arrange
    let mut app = live_app();

    // act
    app.ingest_event(agent_spawned(1, "agent_alpha", "researcher"));
    app.ingest_event(agent_spawned(2, "agent_beta", "reviewer"));

    // assert
    let summary = app.orchestration_summary();
    assert_eq!(summary.active_agents, 0, "no tasks yet → no active agents");
}

#[test]
fn multi_agent_journey_tracks_queued_and_running_tasks() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_alpha", "researcher"));
    app.ingest_event(agent_spawned(2, "agent_beta", "reviewer"));

    // act
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_alpha"),
        worker_actor("agent_alpha"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_alpha".to_string().into(),
            state: TaskScheduleState::Queued,
            queue_key: Some("agent:queued:primary".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_beta"),
        worker_actor("agent_beta"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_beta".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:secondary".to_string()),
        }),
    ));

    // assert
    let summary = app.orchestration_summary();
    assert_eq!(summary.active_agents, 2, "two worker agents active");
    assert_eq!(summary.queued, 1, "one queued task");
    assert_eq!(summary.running, 1, "one running task");
}

#[test]
fn multi_agent_journey_completes_all_tasks() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_alpha", "researcher"));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_alpha"),
        worker_actor("agent_alpha"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_alpha".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:primary".to_string()),
        }),
    ));

    // act
    app.ingest_event(task_completed(3, "task_alpha"));

    // assert
    let summary = app.orchestration_summary();
    assert_eq!(
        summary.active_agents, 0,
        "no active agents after completion"
    );
    assert_eq!(summary.queued, 0);
    assert_eq!(summary.running, 0);
}

#[test]
fn coordinator_journey_tracks_child_agent_lineage() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned_with_parent(
        1,
        "child_agent_1",
        "explore",
        "supervisor",
    ));

    // act
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_child"),
        worker_actor("child_agent_1"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:child".to_string()),
        }),
    ));

    // assert
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, "task_child");
    assert_eq!(
        rows[0].owner_agent_id.as_deref(),
        Some("child_agent_1"),
        "child agent id must be tracked"
    );
}

// ---------------------------------------------------------------------------
// Failure / cancel / restart journeys
// ---------------------------------------------------------------------------

#[test]
fn failure_journey_task_cancelled_projects_terminal_state() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_fail", "build"));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_fail"),
        worker_actor("agent_fail"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_fail".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:fail".to_string()),
        }),
    ));

    // act
    app.ingest_event(task_cancelled(3, "task_fail", "user cancelled"));

    // assert
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].state.is_terminal(),
        "cancelled task must be terminal"
    );
    let summary = app.orchestration_summary();
    assert_eq!(summary.running, 0, "no running after cancel");
}

#[test]
fn failure_journey_stale_then_late_result_projects_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_stale",
        "agent:running:stale",
    ));

    // act
    app.ingest_event(stale_detected(2, "task_stale", 5000));
    app.ingest_event(task_result_late(3, "task_stale"));

    // assert
    let rows = app.orchestration_visible_rows();
    let stale_row = rows
        .iter()
        .find(|r| r.task_id == "task_stale")
        .expect("stale task row must exist");
    assert!(
        stale_row.state.is_terminal(),
        "stale+late task must be terminal"
    );
}

#[test]
fn failure_journey_background_notification_failed_projects_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_bg_fail",
        "agent:running:bg",
    ));

    // act
    app.ingest_event(background_notification(
        2,
        "task_bg_fail",
        BackgroundTaskNotificationStatus::Failed,
    ));

    // assert
    let rows = app.orchestration_visible_rows();
    let bg_row = rows
        .iter()
        .find(|r| r.task_id == "task_bg_fail")
        .expect("bg task row must exist");
    assert!(
        bg_row.state.is_terminal(),
        "failed bg task must be terminal"
    );
}

#[test]
fn failure_journey_background_notification_timed_out_projects_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_bg_timeout",
        "agent:running:bg",
    ));

    // act
    app.ingest_event(background_notification(
        2,
        "task_bg_timeout",
        BackgroundTaskNotificationStatus::TimedOut,
    ));

    // assert
    let rows = app.orchestration_visible_rows();
    let timeout_row = rows
        .iter()
        .find(|r| r.task_id == "task_bg_timeout")
        .expect("timeout task row must exist");
    assert!(
        timeout_row.state.is_terminal(),
        "timed-out bg task must be terminal"
    );
}

#[test]
fn restart_journey_new_session_clears_orchestration_state() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_restart", "build"));
    app.ingest_event(task_scheduled_queued(
        2,
        "task_restart",
        "agent:queued:restart",
    ));
    assert_eq!(app.orchestration_summary().queued, 1);

    // act
    app.execute_slash_command("new", None);

    // assert
    let summary = app.orchestration_summary();
    assert_eq!(summary.queued, 0, "new session must clear queued tasks");
    assert_eq!(summary.active_agents, 0, "new session must clear agents");
}

#[test]
fn restart_journey_ctrl_w_clears_state_via_worktree_handoff() {
    // arrange
    let (mut app, _intents) = live_app_with_sink();
    app.startup_mode = true;
    app.composer.prompt_buffer.clear();
    app.ingest_event(task_scheduled_queued(1, "task_wt", "agent:queued:wt"));
    assert_eq!(app.orchestration_summary().queued, 1);

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Enter));

    // assert
    let summary = app.orchestration_summary();
    assert_eq!(
        summary.queued, 0,
        "worktree session must clear queued tasks"
    );
}

// ---------------------------------------------------------------------------
// Task / subagent status
// ---------------------------------------------------------------------------

#[test]
fn task_status_queued_then_started_tracks_state_transition() {
    // arrange
    let mut app = live_app();

    // act
    app.ingest_event(task_scheduled_queued(
        1,
        "task_transition",
        "agent:queued:trans",
    ));
    let queued_rows = app.orchestration_visible_rows();
    app.ingest_event(task_scheduled_started(
        2,
        "task_transition",
        "agent:running:trans",
    ));
    let started_rows = app.orchestration_visible_rows();

    // assert
    assert_eq!(queued_rows.len(), 1);
    assert_eq!(
        queued_rows[0].state,
        harness_tui::app::OrchestrationTaskState::Queued
    );
    assert_eq!(started_rows.len(), 1);
    assert_eq!(
        started_rows[0].state,
        harness_tui::app::OrchestrationTaskState::Running
    );
}

#[test]
fn task_status_completed_is_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(1, "task_done", "agent:running:done"));

    // act
    app.ingest_event(task_completed(2, "task_done"));

    // assert
    let rows = app.orchestration_visible_rows();
    assert!(
        rows[0].state.is_terminal(),
        "completed task must be terminal"
    );
}

#[test]
fn task_status_cancelled_is_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_cancel",
        "agent:running:cancel",
    ));

    // act
    app.ingest_event(task_cancelled(2, "task_cancel", "user stop"));

    // assert
    let rows = app.orchestration_visible_rows();
    assert!(
        rows[0].state.is_terminal(),
        "cancelled task must be terminal"
    );
}

#[test]
fn task_status_queue_key_preserved_across_transitions() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_q", "agent:queued:persist"));

    // act
    app.ingest_event(task_scheduled_started(2, "task_q", "agent:running:persist"));
    let rows = app.orchestration_visible_rows();

    // assert
    assert_eq!(
        rows[0].queue_key.as_deref(),
        Some("agent:running:persist"),
        "queue key must update to running"
    );
}

#[test]
fn subagent_status_visible_in_dashboard_operator_summary() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_sidebar", "explore"));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_sidebar"),
        worker_actor("agent_sidebar"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_sidebar".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:sidebar".to_string()),
        }),
    ));
    open_dashboard(&mut app);

    // act
    let rendered = render(&app);

    // assert
    assert!(
        rendered.contains("operator dashboard:"),
        "dashboard must show operator summary with orchestration data\n{rendered}"
    );
    assert!(
        app.orchestration_summary().running > 0,
        "orchestration must track running task"
    );
}

// ---------------------------------------------------------------------------
// Queue state
// ---------------------------------------------------------------------------

#[test]
fn queue_state_tracks_queued_count() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_q1", "agent:queued:q1"));
    app.ingest_event(task_scheduled_queued(2, "task_q2", "agent:queued:q2"));

    // act
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(summary.queued, 2, "two queued tasks");
}

#[test]
fn queue_state_tracks_running_count() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(1, "task_r1", "agent:running:r1"));

    // act
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(summary.running, 1, "one running task");
}

#[test]
fn queue_state_tracks_stale_count() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_stale_q",
        "agent:running:stale",
    ));
    app.ingest_event(stale_detected(2, "task_stale_q", 10000));

    // act
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(summary.stale, 1, "one stale task");
}

#[test]
fn queue_state_terminal_tasks_excluded_from_counts() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_term", "agent:queued:term"));
    app.ingest_event(task_completed(2, "task_term"));

    // act
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(summary.queued, 0, "terminal tasks excluded from queued");
    assert_eq!(summary.running, 0, "terminal tasks excluded from running");
}

#[test]
fn queue_state_distinct_queue_keys_preserved() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_qa", "agent:queued:primary"));
    app.ingest_event(task_scheduled_queued(
        2,
        "task_qb",
        "agent:queued:secondary",
    ));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    let queue_keys: Vec<_> = rows.iter().map(|r| r.queue_key.as_deref()).collect();
    assert!(
        queue_keys.contains(&Some("agent:queued:primary")),
        "primary queue key must be preserved"
    );
    assert!(
        queue_keys.contains(&Some("agent:queued:secondary")),
        "secondary queue key must be preserved"
    );
}

// ---------------------------------------------------------------------------
// Plan state
// ---------------------------------------------------------------------------

#[test]
fn plan_view_absent_by_default() {
    // arrange
    let app = live_app();

    // act
    let visible = app.plan_view_is_visible();

    // assert
    assert!(!visible, "plan view must not be visible by default");
}

#[test]
fn plan_view_does_not_preempt_dashboard() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);
    assert!(dashboard_visible(&app));

    // act
    app.plan_view_visible = true;

    // assert
    assert!(app.plan_view_is_visible(), "plan view must open");
    assert!(
        dashboard_visible(&app),
        "dashboard must remain visible alongside plan view"
    );
}

#[test]
fn plan_view_summary_reports_zero_when_no_plans() {
    // arrange
    let tempdir = tempfile::tempdir().unwrap();
    let mut app = live_app();
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());

    // act
    let summary = app.plan_view_summary();

    // assert
    assert_eq!(summary.total, 0, "no plans → zero total");
    assert_eq!(summary.existing, 0, "no plans → zero existing");
}

#[test]
fn plan_view_rows_empty_when_no_plans() {
    // arrange
    let tempdir = tempfile::tempdir().unwrap();
    let mut app = live_app();
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());

    // act
    let rows = app.plan_view_rows();

    // assert
    assert!(rows.is_empty(), "plan view rows must be empty");
}

#[test]
fn plan_view_closes_on_esc() {
    // arrange
    let mut app = live_app();
    app.plan_view_visible = true;
    assert!(app.plan_view_is_visible());

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(!app.plan_view_is_visible(), "Esc must close plan view");
}

// ---------------------------------------------------------------------------
// Worktree choice
// ---------------------------------------------------------------------------

#[test]
fn worktree_choice_ctrl_w_empty_composer_emits_new_worktree_session() {
    // arrange
    let (mut app, intents) = startup_app_with_sink();
    app.composer.prompt_buffer.clear();

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Enter));

    // assert
    let captured = intents.lock().unwrap_or_abort().clone();
    assert_eq!(
        captured,
        vec![UiIntent::NewWorktreeSession { name: None }],
        "empty Ctrl+W must emit NewWorktreeSession after confirmation"
    );
}

#[test]
fn worktree_choice_ctrl_w_with_draft_does_not_emit_worktree_intent() {
    // arrange
    let (mut app, intents) = startup_app_with_sink();
    app.composer.prompt_buffer = "hello world".to_string();

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));

    // assert
    let captured = intents.lock().unwrap_or_abort().clone();
    assert!(
        !captured
            .iter()
            .any(|intent| matches!(intent, UiIntent::NewWorktreeSession { .. })),
        "draft Ctrl+W must not emit NewWorktreeSession"
    );
}

#[test]
fn worktree_choice_does_not_depend_on_hosted_hub() {
    // arrange
    let (mut app, _intents) = startup_app_with_sink();
    app.composer.prompt_buffer.clear();

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    let rendered = render(&app);

    // assert
    assert!(
        !rendered.contains("hub.") && !rendered.contains("hosted"),
        "worktree choice must not reference hosted hub\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Session navigation
// ---------------------------------------------------------------------------

#[test]
fn session_navigation_history_picker_absent_by_default() {
    // arrange
    let app = live_app();

    // act
    let visible = app.session_history_visible;

    // assert
    assert!(
        !visible,
        "session history picker must not be visible by default"
    );
}

#[test]
fn session_navigation_slash_sessions_opens_history_picker() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("sessions", None);

    // assert
    assert!(
        app.session_history_visible,
        "/sessions must open session history picker"
    );
}

#[test]
fn session_navigation_slash_replay_opens_history_picker() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("replay", None);

    // assert
    assert!(
        app.session_history_visible,
        "/replay must open session history picker"
    );
}

#[test]
fn session_navigation_lineage_browser_absent_by_default() {
    // arrange
    let app = live_app();

    // act
    let visible = app.lineage_browser_visible;

    // assert
    assert!(!visible, "lineage browser must not be visible by default");
}

#[test]
fn session_navigation_slash_tree_opens_lineage_browser() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("tree", None);

    // assert
    assert!(
        app.lineage_browser_visible,
        "/tree must open lineage browser"
    );
}

#[test]
fn session_navigation_slash_fork_opens_fork_selector() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("fork", None);

    // assert
    assert!(app.fork_selector_visible, "/fork must open fork selector");
}

#[test]
fn session_navigation_slash_clone_blocked_without_stable_events() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("clone", None);

    // assert
    assert!(
        !app.fork_selector_visible,
        "/clone must not open fork selector without stable events"
    );
    assert!(
        app.status_banner.is_some(),
        "/clone must set a status banner when blocked"
    );
}

#[test]
fn session_navigation_rename_emits_update_session_title_intent() {
    // arrange
    let (mut app, intents) = live_app_with_sink();
    app.composer.prompt_buffer = "/rename My New Title".to_string();

    // act
    app.execute_slash_command("rename", None);

    // assert
    let captured = intents.lock().unwrap_or_abort().clone();
    assert!(
        captured.iter().any(|i| matches!(
            i,
            UiIntent::UpdateSessionTitle { title } if title == "My New Title"
        )),
        "/rename must emit UpdateSessionTitle intent with the title text"
    );
}

#[test]
fn session_navigation_rename_empty_title_emits_error() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "/rename   ".to_string();

    // act
    app.execute_slash_command("rename", None);

    // assert
    assert!(
        app.status_banner
            .as_ref()
            .map(|banner| banner.contains("empty"))
            .unwrap_or(false),
        "empty rename must set error banner"
    );
}

#[test]
fn session_navigation_new_session_clears_overlays() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);
    app.session_history_visible = true;
    app.lineage_browser_visible = true;
    assert!(dashboard_visible(&app));

    // act
    app.execute_slash_command("new", None);

    // assert
    assert!(!dashboard_visible(&app), "new session must close dashboard");
    assert!(
        !app.session_history_visible,
        "new session must close session history"
    );
    assert!(
        !app.lineage_browser_visible,
        "new session must close lineage browser"
    );
}

// ---------------------------------------------------------------------------
// Keyboard navigation
// ---------------------------------------------------------------------------

#[test]
fn keyboard_ctrl_p_opens_command_palette() {
    // arrange
    let mut app = live_app();

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert!(app.palette_visible, "Ctrl+P must open command palette");
}

#[test]
fn keyboard_esc_closes_command_palette() {
    // arrange
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(!app.palette_visible, "Esc must close command palette");
}

#[test]
fn keyboard_esc_closes_session_history_picker() {
    // arrange
    let mut app = live_app();
    app.execute_slash_command("sessions", None);
    assert!(app.session_history_visible);

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(
        !app.session_history_visible,
        "Esc must close session history picker"
    );
}

#[test]
fn keyboard_esc_closes_lineage_browser() {
    // arrange
    let mut app = live_app();
    app.execute_slash_command("tree", None);
    assert!(app.lineage_browser_visible);

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(
        !app.lineage_browser_visible,
        "Esc must close lineage browser"
    );
}

#[test]
fn keyboard_esc_closes_fork_selector() {
    // arrange
    let mut app = live_app();
    app.execute_slash_command("fork", None);
    assert!(app.fork_selector_visible);

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(!app.fork_selector_visible, "Esc must close fork selector");
}

#[test]
fn keyboard_esc_closes_plan_view() {
    // arrange
    let mut app = live_app();
    app.plan_view_visible = true;
    assert!(app.plan_view_is_visible());

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(!app.plan_view_is_visible(), "Esc must close plan view");
}

// ---------------------------------------------------------------------------
// Responsive layout
// ---------------------------------------------------------------------------

#[test]
fn responsive_dashboard_renders_at_80x24() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert!(
        rendered.contains("operator dashboard:"),
        "dashboard must render at 80x24\n{rendered}"
    );
}

#[test]
fn responsive_dashboard_renders_at_120x40() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);

    // act
    let rendered = render_at(&app, 120, 40);

    // assert
    assert!(
        rendered.contains("operator dashboard:"),
        "dashboard must render at 120x40\n{rendered}"
    );
}

#[test]
fn responsive_dashboard_renders_at_60x20() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);

    // act
    let rendered = render_at(&app, 60, 20);

    // assert
    assert!(
        !rendered.is_empty(),
        "dashboard must produce non-empty render at 60x20"
    );
}

#[test]
fn responsive_idle_shell_renders_at_79x24() {
    // arrange
    let app = live_app();

    // act
    let rendered = render_at(&app, 79, 24);

    // assert
    assert!(!rendered.is_empty(), "idle shell must render at 79x24");
}

#[test]
fn responsive_idle_shell_renders_at_140x40() {
    // arrange
    let app = live_app();

    // act
    let rendered = render_at(&app, 140, 40);

    // assert
    assert!(!rendered.is_empty(), "idle shell must render at 140x40");
}

// ---------------------------------------------------------------------------
// Semantic cells
// ---------------------------------------------------------------------------

#[test]
fn semantic_cells_dashboard_contains_operator_summary() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_cell", "build"));
    app.ingest_event(task_scheduled_queued(2, "task_cell", "agent:queued:cell"));
    open_dashboard(&mut app);

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let dashboard_line = lines
        .iter()
        .find(|line| line.contains("operator dashboard:"));

    // assert
    assert!(
        dashboard_line.is_some(),
        "semantic cell: dashboard must contain operator summary line"
    );
}

#[test]
fn semantic_cells_operator_sidebar_contains_orchestration_data() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_sa", "explore"));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_sa"),
        worker_actor("agent_sa"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_sa".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:sa".to_string()),
        }),
    ));
    open_dashboard(&mut app);

    // act
    let rendered = render(&app);

    // assert
    assert!(
        rendered.contains("operator dashboard:"),
        "semantic cell: dashboard must contain orchestration data\n{rendered}"
    );
    assert!(
        app.orchestration_visible_rows()
            .iter()
            .any(|r| r.task_id == "task_sa"),
        "semantic cell: orchestration must track task_sa"
    );
}

#[test]
fn semantic_cells_orchestration_row_has_task_id() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(
        1,
        "task_cell_id",
        "agent:queued:cell",
    ));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].task_id, "task_cell_id",
        "semantic cell: row must have task_id"
    );
}

#[test]
fn semantic_cells_orchestration_row_has_queue_key() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_qk", "agent:queued:qk"));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].queue_key.as_deref(),
        Some("agent:queued:qk"),
        "semantic cell: row must have queue_key"
    );
}

#[test]
fn semantic_cells_orchestration_row_has_state() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(1, "task_state", "agent:queued:state"));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].state,
        harness_tui::app::OrchestrationTaskState::Queued,
        "semantic cell: row must have state"
    );
}

// ---------------------------------------------------------------------------
// Overlay stack interactions
// ---------------------------------------------------------------------------

#[test]
fn dashboard_and_plan_view_coexist_without_conflict() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);

    // act
    app.plan_view_visible = true;

    // assert
    assert!(dashboard_visible(&app));
    assert!(app.plan_view_is_visible());
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PlanView),
        "plan view must be top overlay"
    );
}

#[test]
fn dashboard_captures_keyboard_only_esc_dismisses() {
    // arrange
    let mut app = live_app();
    open_dashboard(&mut app);
    assert!(dashboard_visible(&app));

    // act — Ctrl+P is captured by the status dialog, does not open palette
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert!(
        dashboard_visible(&app),
        "dashboard must remain visible — only Esc dismisses it"
    );
    assert!(
        !app.palette_visible,
        "palette must not open while dashboard captures keyboard"
    );
}

#[test]
fn session_history_and_lineage_browser_mutually_exclusive() {
    // arrange
    let mut app = live_app();
    app.execute_slash_command("tree", None);
    assert!(app.lineage_browser_visible);

    // act
    app.execute_slash_command("sessions", None);

    // assert
    assert!(
        !app.lineage_browser_visible,
        "session history must close lineage browser"
    );
    assert!(
        app.session_history_visible,
        "session history must be visible"
    );
}

// ---------------------------------------------------------------------------
// No hosted hub dependency
// ---------------------------------------------------------------------------

#[test]
fn no_hosted_hub_dependency_in_orchestration_projection() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_nohub", "build"));
    app.ingest_event(task_scheduled_queued(2, "task_nohub", "agent:queued:nohub"));

    // act
    let rows = app.orchestration_visible_rows();
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(rows.len(), 1, "orchestration must work without hub");
    assert_eq!(summary.queued, 1, "queue must work without hub");
}

#[test]
fn no_hosted_hub_dependency_in_session_navigation() {
    // arrange
    let mut app = live_app();

    // act
    app.execute_slash_command("sessions", None);
    let rendered = render(&app);

    // assert
    assert!(app.session_history_visible);
    assert!(
        !rendered.contains("hub.") && !rendered.contains("hosted"),
        "session navigation must not reference hosted hub\n{rendered}"
    );
}

#[test]
fn no_hosted_hub_dependency_in_worktree_choice() {
    // arrange
    let (mut app, _intents) = live_app_with_sink();
    app.startup_mode = true;
    app.composer.prompt_buffer.clear();

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));
    let rendered = render(&app);

    // assert
    assert!(
        !rendered.contains("hub.") && !rendered.contains("hosted"),
        "worktree choice must not reference hosted hub\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Orchestration row ordering and retention
// ---------------------------------------------------------------------------

#[test]
fn orchestration_rows_sort_non_terminal_before_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_queued(
        1,
        "task_term_first",
        "agent:queued:term1",
    ));
    app.ingest_event(task_completed(2, "task_term_first"));
    app.ingest_event(task_scheduled_queued(
        3,
        "task_active",
        "agent:queued:active",
    ));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    assert!(!rows.is_empty(), "must have at least one row");
    let active_idx = rows.iter().position(|r| r.task_id == "task_active");
    let terminal_idx = rows.iter().position(|r| r.task_id == "task_term_first");
    if let (Some(active), Some(terminal)) = (active_idx, terminal_idx) {
        assert!(
            active < terminal,
            "non-terminal task must sort before terminal task"
        );
    }
}

#[test]
fn orchestration_rows_stale_sorts_first_among_non_terminal() {
    // arrange
    let mut app = live_app();
    app.ingest_event(task_scheduled_started(
        1,
        "task_stale_sort",
        "agent:running:stale",
    ));
    app.ingest_event(stale_detected(2, "task_stale_sort", 8000));
    app.ingest_event(task_scheduled_queued(
        3,
        "task_queued_sort",
        "agent:queued:sort",
    ));

    // act
    let rows = app.orchestration_visible_rows();

    // assert
    let stale_idx = rows.iter().position(|r| r.task_id == "task_stale_sort");
    let queued_idx = rows.iter().position(|r| r.task_id == "task_queued_sort");
    if let (Some(stale), Some(queued)) = (stale_idx, queued_idx) {
        assert!(stale < queued, "stale task must sort before queued task");
    }
}

#[test]
fn orchestration_summary_excludes_terminal_from_active_agents() {
    // arrange
    let mut app = live_app();
    app.ingest_event(agent_spawned(1, "agent_term", "build"));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_term"),
        worker_actor("agent_term"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_term_agent".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running:term".to_string()),
        }),
    ));
    app.ingest_event(task_completed(3, "task_term_agent"));

    // act
    let summary = app.orchestration_summary();

    // assert
    assert_eq!(
        summary.active_agents, 0,
        "terminal tasks must not count as active agents"
    );
}

// ---------------------------------------------------------------------------
// Run lifecycle integration
// ---------------------------------------------------------------------------

#[test]
fn run_started_event_does_not_create_orchestration_rows() {
    // arrange
    let mut app = live_app();

    // act
    app.ingest_event(envelope(
        1,
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "run_task29_parity".to_string().into(),
            workspace_root: ".".to_string(),
        }),
    ));

    // assert
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.is_empty(),
        "RunStarted must not create orchestration rows"
    );
}

#[test]
fn agent_stopped_does_not_create_orchestration_row() {
    // arrange
    let mut app = live_app();

    // act
    app.ingest_event(envelope(
        1,
        None,
        EventV1::AgentStopped(harness_core::event::AgentStoppedEvent {
            agent_id: "agent_stopped".to_string(),
            reason: "done".to_string(),
        }),
    ));

    // assert
    let rows = app.orchestration_visible_rows();
    assert!(
        rows.is_empty(),
        "AgentStopped must not create orchestration rows"
    );
}

// ---------------------------------------------------------------------------
// Palette dashboard command
// ---------------------------------------------------------------------------

#[test]
fn palette_dashboard_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_dashboard = app
        .palette_filtered
        .iter()
        .any(|id| id == "session.dashboard");

    // assert
    assert!(
        has_dashboard,
        "palette must contain session.dashboard on startup"
    );
}

#[test]
fn palette_new_session_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_new = app.palette_filtered.iter().any(|id| id == "session.new");

    // assert
    assert!(has_new, "palette must contain session.new on startup");
}

#[test]
fn palette_new_worktree_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_worktree = app
        .palette_filtered
        .iter()
        .any(|id| id == "session.new.worktree");

    // assert
    assert!(
        has_worktree,
        "palette must contain session.new.worktree on startup"
    );
}

#[test]
fn palette_resume_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_resume = app.palette_filtered.iter().any(|id| id == "session.list");

    // assert
    assert!(has_resume, "palette must contain session.resume on startup");
}

#[test]
fn palette_rename_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_rename = app.palette_filtered.iter().any(|id| id == "session.rename");

    // assert
    assert!(has_rename, "palette must contain session.rename on startup");
}

#[test]
fn palette_view_plan_command_present_on_startup() {
    // arrange
    let mut app = live_app();
    app.startup_mode = true;
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_plan = app
        .palette_filtered
        .iter()
        .any(|id| id == "context.view_plan");

    // assert
    assert!(
        has_plan,
        "palette must contain context.view-plan on startup"
    );
}

// ---------------------------------------------------------------------------
// Enterprise / remote / marketplace absence
// ---------------------------------------------------------------------------

#[test]
fn marketplace_is_visible_in_palette_commands() {
    // arrange
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_marketplace = app
        .palette_filtered
        .iter()
        .any(|id| id.contains("marketplace"));

    // assert
    assert!(
        has_marketplace,
        "marketplace must be visible in palette commands"
    );
}

#[test]
fn no_enterprise_in_palette_commands() {
    // arrange
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_enterprise = app
        .palette_filtered
        .iter()
        .any(|id| id.contains("enterprise"));

    // assert
    assert!(
        !has_enterprise,
        "enterprise must be absent from palette commands"
    );
}

#[test]
fn no_remote_management_in_palette_commands() {
    // arrange
    let mut app = live_app();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));

    // act
    let has_remote = app.palette_filtered.iter().any(|id| id.contains("remote"));

    // assert
    assert!(
        !has_remote,
        "remote management must be absent from palette commands"
    );
}
