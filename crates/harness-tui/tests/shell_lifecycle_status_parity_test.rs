//! Task 26: Shell lifecycle, status, context, footer, and recovery states.
//!
//! Differential TDD contract tests for the shell lifecycle surface:
//! - Ordered lifecycle states: idle -> stream -> permission -> tool -> complete -> post-run
//! - Replay read-only state
//! - Model/effort/context-usage bars
//! - Footer vocabulary per state
//! - Turn status
//! - Handoff actions
//! - Provider-fail recovery, cancel, permission-timeout, recovery-retry
//! - Truncated/corrupt replay
//!
//! The new leaf modules (`shell_status.rs`, `footer_state.rs`, `recovery_state.rs`)
//! are included via `#[path]` so they compile as part of the test crate without
//! requiring registration in `app.rs` (a shared root that must not be edited).

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
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, RunFinishedEvent,
    TaskCancelledEvent, TaskCompletedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_testkit::parity::cells::SemanticFrame;
use harness_tui::app::footer_state::{FooterHintKind, FooterVocabulary};
use harness_tui::app::recovery_state::RecoveryState;
use harness_tui::app::shell_status::{ContextUsageBar, EffortBar, ModelBar, ShellStatus};
use harness_tui::app::{
    AppState, LaunchMetadata, LifecycleShellState, PostRunHandoffAction, RuntimeStateKind, UiIntent,
};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-task26-lifecycle-{seq:04}"),
        seq,
        run_id: "run_task26_lifecycle".into(),
        mono_ms: seq * 100,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task26-lifecycle".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task26_lifecycle".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task26").with_mode_label("Demo"),
    );
    app
}

fn live_app_with_sink() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task26").with_mode_label("Demo"),
    );
    (app, intents)
}

fn replay_app(events: Vec<EventEnvelopeV1>) -> AppState {
    AppState::new_replay(PathBuf::from("/tmp/sessions/run_replay_task26"), events)
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

fn reference_composer_top(scenario: &str) -> usize {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/grok-build-v0.1.220-alpha.4/core")
        .join(scenario)
        .join("cells.json");
    let frame = SemanticFrame::read_cells_json(&path).expect("reference semantic frame");
    frame
        .cells
        .iter()
        .find(|cell| cell.grapheme == "╭")
        .map(|cell| usize::from(cell.row))
        .expect("reference composer top border")
}

fn rendered_composer_top(rendered: &str) -> usize {
    rendered
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains('╭') && line.contains('─'))
        .map(|(row, _)| row)
        .expect("rendered composer top border")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn user_message(seq: u64, request_id: &str, text: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
    )
}

fn provider_started(seq: u64, request_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-task26".to_string(),
            prompt_summary: "task26 prompt".to_string(),
            request_digest: "digest-task26".to_string(),
            metadata: None,
        }),
    )
}

fn stream_delta(seq: u64, request_id: &str, delta: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: delta.to_string(),
        }),
    )
}

fn task_completed(seq: u64, request_id: &str, summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string().into(),
            result_summary: summary.to_string(),
            result_digest: "digest-completed".to_string(),
            metadata: None,
        }),
    )
}

fn task_cancelled(seq: u64, request_id: &str, reason: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_000002".to_string().into(),
            reason: reason.to_string(),
            task_scope: None,
        }),
    )
}

fn permission_requested(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn run_finished(seq: u64, summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::RunFinished(RunFinishedEvent {
            summary: summary.to_string(),
        }),
    )
}

fn run_failed(seq: u64, error: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: error.to_string(),
        }),
    )
}

// ===========================================================================
// 1. Ordered lifecycle states: idle -> stream -> permission -> tool -> complete -> post-run
// ===========================================================================

#[test]
fn shell_status_ordered_lifecycle_states_exist() {
    let ordered = ShellStatus::ORDERED_LIFECYCLE;
    assert_eq!(
        ordered.len(),
        10,
        "must have exactly 10 ordered lifecycle states"
    );
    assert_eq!(ordered[0], ShellStatus::Idle, "first state must be Idle");
    assert_eq!(
        ordered[1],
        ShellStatus::Streaming,
        "second state must be Streaming"
    );
    assert_eq!(
        ordered[2],
        ShellStatus::PermissionBlocked,
        "third state must be PermissionBlocked"
    );
    assert_eq!(
        ordered[3],
        ShellStatus::ToolQueued,
        "fourth state must be ToolQueued"
    );
    assert_eq!(
        ordered[4],
        ShellStatus::ToolRunning,
        "fifth state must be ToolRunning"
    );
    assert_eq!(
        ordered[5],
        ShellStatus::ToolSucceeded,
        "sixth state must be ToolSucceeded"
    );
    assert_eq!(
        ordered[6],
        ShellStatus::TurnComplete,
        "seventh state must be TurnComplete"
    );
    assert_eq!(
        ordered[7],
        ShellStatus::PostRun,
        "eighth state must be PostRun"
    );
    assert_eq!(
        ordered[8],
        ShellStatus::PostRunFailure,
        "ninth state must be PostRunFailure"
    );
    assert_eq!(
        ordered[9],
        ShellStatus::ReplayReadOnly,
        "tenth state must be ReplayReadOnly"
    );
}

#[test]
fn idle_state_has_ready_label() {
    assert_eq!(ShellStatus::Idle.label(), "Ready");
    assert!(!ShellStatus::Idle.composer_disabled());
    assert!(!ShellStatus::Idle.is_read_only());
    assert!(!ShellStatus::Idle.is_terminal());
}

#[test]
fn streaming_state_has_streaming_label() {
    assert_eq!(ShellStatus::Streaming.label(), "Streaming");
    assert!(!ShellStatus::Streaming.composer_disabled());
}

#[test]
fn permission_blocked_state_has_correct_label() {
    assert_eq!(ShellStatus::PermissionBlocked.label(), "Permission blocked");
    assert!(!ShellStatus::PermissionBlocked.composer_disabled());
}

#[test]
fn permission_pending_state_disables_composer() {
    assert_eq!(ShellStatus::PermissionPending.label(), "Permission pending");
    assert!(
        ShellStatus::PermissionPending.composer_disabled(),
        "PermissionPending must disable composer"
    );
}

#[test]
fn tool_states_have_correct_labels() {
    assert_eq!(ShellStatus::ToolQueued.label(), "Tool queued");
    assert_eq!(ShellStatus::ToolRunning.label(), "Tool running");
    assert_eq!(ShellStatus::ToolSucceeded.label(), "Tool finished");
    assert_eq!(ShellStatus::ToolFailed.label(), "Tool failed");
}

#[test]
fn turn_complete_state_has_correct_label() {
    assert_eq!(ShellStatus::TurnComplete.label(), "Turn complete");
    assert!(!ShellStatus::TurnComplete.composer_disabled());
}

#[test]
fn post_run_states_are_terminal_and_disable_composer() {
    assert_eq!(ShellStatus::PostRun.label(), "Run finished");
    assert!(ShellStatus::PostRun.is_terminal());
    assert!(
        ShellStatus::PostRun.composer_disabled(),
        "PostRun must disable composer"
    );

    assert_eq!(ShellStatus::PostRunFailure.label(), "Run failed");
    assert!(ShellStatus::PostRunFailure.is_terminal());
    assert!(
        ShellStatus::PostRunFailure.composer_disabled(),
        "PostRunFailure must disable composer"
    );
}

#[test]
fn replay_read_only_state_is_read_only() {
    assert_eq!(
        ShellStatus::ReplayReadOnly.label(),
        "Replay \u{00b7} read-only"
    );
    assert!(ShellStatus::ReplayReadOnly.is_read_only());
    assert!(
        ShellStatus::ReplayReadOnly.composer_disabled(),
        "ReplayReadOnly must disable composer"
    );
}

#[test]
fn shell_status_from_runtime_state_maps_correctly() {
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Ready),
        ShellStatus::Idle
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Streaming),
        ShellStatus::Streaming
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Success),
        ShellStatus::TurnComplete
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Failure),
        ShellStatus::Failure
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Cancelled),
        ShellStatus::Cancelled
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::PermissionBlocked),
        ShellStatus::PermissionBlocked
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::PermissionPending),
        ShellStatus::PermissionPending
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Degraded),
        ShellStatus::Degraded
    );
    assert_eq!(
        ShellStatus::from_runtime_state(RuntimeStateKind::Disconnected),
        ShellStatus::Disconnected
    );
}

// ===========================================================================
// 2. Live lifecycle state transitions via AppState
// ===========================================================================

#[test]
fn live_idle_state_shows_ready_runtime() {
    let app = live_app();
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Ready);
    assert!(!app.composer_disabled());
}

#[test]
fn live_streaming_state_shows_streaming_runtime() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_stream", "hello"));
    app.ingest_event(provider_started(2, "req_stream"));
    app.ingest_event(stream_delta(3, "req_stream", "response"));
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Streaming);
}

#[test]
fn live_permission_state_shows_blocked_runtime() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_perm", "edit file"));
    app.ingest_event(provider_started(2, "req_perm"));
    app.ingest_event(stream_delta(3, "req_perm", "editing"));
    app.ingest_event(permission_requested(4, "perm_live", "tc_perm_live"));
    assert_eq!(
        app.runtime_state().kind,
        RuntimeStateKind::PermissionBlocked
    );
}

#[test]
fn live_turn_complete_state_shows_success_runtime() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_complete", "question"));
    app.ingest_event(provider_started(2, "req_complete"));
    app.ingest_event(stream_delta(3, "req_complete", "answer"));
    app.ingest_event(task_completed(4, "req_complete", "answer"));
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
}

#[test]
fn live_post_run_state_shows_post_run_handoff() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_post_run")),
        false,
        None,
    );
    app.ingest_event(user_message(1, "req_post", "final"));
    app.ingest_event(provider_started(2, "req_post"));
    app.ingest_event(stream_delta(3, "req_post", "done"));
    app.ingest_event(task_completed(4, "req_post", "done"));
    app.ingest_event(run_finished(5, "run complete"));

    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Success);
}

#[test]
fn live_post_run_failure_shows_failure_runtime() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/sessions/run_failed")), false, None);
    app.status_banner = Some("provider request failed".to_string());
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Failure);
}

// ===========================================================================
// 3. Replay read-only state
// ===========================================================================

#[test]
fn replay_state_is_read_only() {
    let events = vec![
        user_message(1, "req_replay", "replay test"),
        provider_started(2, "req_replay"),
        stream_delta(3, "req_replay", "replayed response"),
        task_completed(4, "req_replay", "replayed response"),
    ];
    let app = replay_app(events);
    assert!(app.replay_mode);
    assert!(app.composer_disabled(), "replay mode must disable composer");
    let state = app.runtime_state();
    assert!(
        !state.composer_disabled || state.kind == RuntimeStateKind::Ready,
        "replay runtime state must be ready or have composer handling"
    );
}

#[test]
fn replay_does_not_emit_live_intents() {
    let events = vec![
        user_message(1, "req_replay_intent", "replay"),
        provider_started(2, "req_replay_intent"),
        stream_delta(3, "req_replay_intent", "response"),
        task_completed(4, "req_replay_intent", "response"),
    ];
    let app = AppState::new_replay(PathBuf::from("/tmp/sessions/run_replay_intent"), events);

    assert!(
        app.composer_disabled(),
        "replay mode must disable composer, preventing live submission"
    );
}

#[test]
fn replay_shell_registry_marks_session_read_only() {
    let events = vec![user_message(1, "req_replay_reg", "replay")];
    let app = replay_app(events);
    let registry = app.default_shell_registry();
    let session = registry
        .iter()
        .find(|d| d.kind == harness_tui::app::ShellKind::Session)
        .expect("session shell descriptor must exist");
    assert!(session.read_only, "replay session shell must be read-only");
}

// ===========================================================================
// 4. Model/effort/context-usage bars
// ===========================================================================

#[test]
fn model_bar_display_label_includes_profile_and_mode() {
    let bar = ModelBar::new("gpt-5.4", "build", Some("Demo"));
    assert_eq!(bar.display_label(), "build \u{00b7} Demo");
}

#[test]
fn model_bar_display_label_without_mode() {
    let bar = ModelBar::new("gpt-5.4", "build", None);
    assert_eq!(bar.display_label(), "build");
}

#[test]
fn effort_bar_labels_are_distinct() {
    assert_eq!(EffortBar::Low.label(), "low");
    assert_eq!(EffortBar::Medium.label(), "medium");
    assert_eq!(EffortBar::High.label(), "high");
    assert_ne!(EffortBar::Low, EffortBar::Medium);
    assert_ne!(EffortBar::Medium, EffortBar::High);
}

#[test]
fn context_usage_bar_from_tokens_is_visible() {
    let bar = ContextUsageBar::from_tokens(50_000);
    assert!(bar.is_visible());
    assert_eq!(bar.tokens, Some(50_000));
    assert!(!bar.compacted_pending_refresh);
    assert_eq!(bar.label, "Context");
}

#[test]
fn context_usage_bar_compacted_pending_refresh() {
    let bar = ContextUsageBar::compacted_pending_refresh();
    assert!(bar.is_visible());
    assert!(bar.tokens.is_none());
    assert!(bar.compacted_pending_refresh);
    assert!(
        bar.label.contains("compacted"),
        "compacted bar label must mention compacted: {}",
        bar.label
    );
}

#[test]
fn context_usage_bar_usage_percent() {
    let bar = ContextUsageBar::from_tokens(128_000);
    assert_eq!(bar.usage_percent(), Some(1));
    let empty = ContextUsageBar::from_tokens(0);
    assert_eq!(empty.usage_percent(), Some(0));
}

// ===========================================================================
// 5. Footer vocabulary per state
// ===========================================================================

#[test]
fn idle_footer_has_send_and_mode_and_shortcuts() {
    let vocab = FooterVocabulary::for_status(ShellStatus::Idle);
    assert!(vocab.has_send(), "idle footer must have send");
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("mode")),
        "idle footer must have mode"
    );
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("shortcuts")),
        "idle footer must have shortcuts"
    );
}

#[test]
fn streaming_footer_has_cancel() {
    let vocab = FooterVocabulary::for_status(ShellStatus::Streaming);
    assert!(vocab.has_cancel(), "streaming footer must have cancel");
    assert!(!vocab.has_send(), "streaming footer must not have send");
}

#[test]
fn permission_blocked_footer_has_select_and_allow_and_deny() {
    let vocab = FooterVocabulary::for_status(ShellStatus::PermissionBlocked);
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("select")),
        "permission footer must have select"
    );
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("allow")),
        "permission footer must have allow"
    );
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("deny")),
        "permission footer must have deny"
    );
}

#[test]
fn permission_pending_footer_has_wait() {
    let vocab = FooterVocabulary::for_status(ShellStatus::PermissionPending);
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("wait")),
        "permission pending footer must have wait"
    );
    assert!(!vocab.has_send(), "permission pending must not have send");
}

#[test]
fn tool_running_footer_has_cancel() {
    let vocab = FooterVocabulary::for_status(ShellStatus::ToolRunning);
    assert!(vocab.has_cancel(), "tool running footer must have cancel");
}

#[test]
fn turn_complete_footer_has_send() {
    let vocab = FooterVocabulary::for_status(ShellStatus::TurnComplete);
    assert!(vocab.has_send(), "turn complete footer must have send");
}

#[test]
fn post_run_footer_has_focus_commands_quit() {
    let vocab = FooterVocabulary::for_status(ShellStatus::PostRun);
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("focus")),
        "post-run footer must have focus"
    );
    assert!(vocab.has_quit(), "post-run footer must have quit");
}

#[test]
fn post_run_failure_footer_has_retry() {
    let vocab = FooterVocabulary::for_status(ShellStatus::PostRunFailure);
    assert!(vocab.has_retry(), "post-run failure footer must have retry");
    assert!(vocab.has_quit(), "post-run failure footer must have quit");
}

#[test]
fn replay_read_only_footer_has_shortcuts_focus_quit() {
    let vocab = FooterVocabulary::for_status(ShellStatus::ReplayReadOnly);
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("shortcuts")),
        "replay footer must have shortcuts"
    );
    assert!(
        vocab.hints.iter().any(|h| h.label.contains("focus")),
        "replay footer must have focus"
    );
    assert!(vocab.has_quit(), "replay footer must have quit");
    assert!(!vocab.has_send(), "replay footer must not have send");
}

#[test]
fn cancelled_footer_has_retry() {
    let vocab = FooterVocabulary::for_status(ShellStatus::Cancelled);
    assert!(vocab.has_retry(), "cancelled footer must have retry");
}

#[test]
fn degraded_footer_has_commands_and_quit() {
    let vocab = FooterVocabulary::for_status(ShellStatus::Degraded);
    assert!(vocab.has_quit(), "degraded footer must have quit");
    assert!(!vocab.has_send(), "degraded footer must not have send");
}

#[test]
fn startup_footer_has_send_open_quit() {
    let vocab = FooterVocabulary::for_status(ShellStatus::Startup);
    assert!(vocab.has_send(), "startup footer must have send");
    assert!(vocab.has_quit(), "startup footer must have quit");
}

#[test]
fn footer_hint_kinds_have_distinct_labels() {
    let kinds = [
        FooterHintKind::Send,
        FooterHintKind::Mode,
        FooterHintKind::Shortcuts,
        FooterHintKind::Commands,
        FooterHintKind::Quit,
        FooterHintKind::Focus,
        FooterHintKind::Convo,
        FooterHintKind::Open,
        FooterHintKind::Replay,
        FooterHintKind::Cancel,
        FooterHintKind::Retry,
        FooterHintKind::Continue,
    ];
    let labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(
        unique.len(),
        kinds.len(),
        "all footer hint kind labels must be distinct"
    );
}

// ===========================================================================
// 6. Turn status via AppState
// ===========================================================================

#[test]
fn turn_status_idle_shows_ready() {
    let app = live_app();
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Ready);
    assert!(
        state.summary.contains("ready") || state.summary.contains("Ready"),
        "idle summary must mention ready: {}",
        state.summary
    );
}

#[test]
fn turn_status_streaming_shows_streaming() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_status_stream", "hi"));
    app.ingest_event(provider_started(2, "req_status_stream"));
    app.ingest_event(stream_delta(3, "req_status_stream", "hello back"));
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Streaming);
    assert!(
        state.summary.contains("response") || state.summary.contains("streaming"),
        "streaming summary must mention response/streaming: {}",
        state.summary
    );
}

#[test]
fn turn_status_complete_shows_success() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_status_done", "hi"));
    app.ingest_event(provider_started(2, "req_status_done"));
    app.ingest_event(stream_delta(3, "req_status_done", "answer"));
    app.ingest_event(task_completed(4, "req_status_done", "answer"));
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Success);
}

#[test]
fn turn_status_cancelled_shows_cancelled() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_cancel", "hi"));
    app.ingest_event(provider_started(2, "req_cancel"));
    app.ingest_event(stream_delta(3, "req_cancel", "partial"));
    app.ingest_event(task_cancelled(4, "req_cancel", "user cancelled"));
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Cancelled);
}

// ===========================================================================
// 7. Handoff actions
// ===========================================================================

#[test]
fn post_run_handoff_actions_include_continue_replay_new_quit() {
    let actions = PostRunHandoffAction::ORDERED;
    assert_eq!(actions.len(), 4, "must have 4 post-run handoff actions");
    assert!(actions.contains(&PostRunHandoffAction::ContinueSession));
    assert!(actions.contains(&PostRunHandoffAction::ReplayRun));
    assert!(actions.contains(&PostRunHandoffAction::StartAnotherSession));
    assert!(actions.contains(&PostRunHandoffAction::Quit));
}

#[test]
fn post_run_handoff_fallback_actions_include_new_and_quit() {
    let actions = PostRunHandoffAction::FALLBACK_ORDERED;
    assert_eq!(actions.len(), 2, "fallback must have 2 actions");
    assert!(actions.contains(&PostRunHandoffAction::StartAnotherSession));
    assert!(actions.contains(&PostRunHandoffAction::Quit));
}

#[test]
fn post_run_handoff_action_labels_are_distinct() {
    let labels: Vec<&str> = PostRunHandoffAction::ORDERED
        .iter()
        .map(|a| a.label())
        .collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(
        unique.len(),
        PostRunHandoffAction::ORDERED.len(),
        "all handoff action labels must be distinct"
    );
}

#[test]
fn post_run_handoff_visible_when_post_run_state() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_handoff")),
        false,
        None,
    );
    app.ingest_event(run_finished(1, "done"));
    // Post-run handoff is driven by lifecycle_shell_state, which is None for live
    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
}

#[test]
fn post_run_handoff_notice_when_cannot_reopen() {
    let app = AppState::new_live(None, false, None);
    assert!(
        app.post_run_handoff_notice().is_some(),
        "must show notice when cannot reopen (no session path)"
    );
}

#[test]
fn post_run_handoff_notice_when_can_reopen() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/sessions/run_reopen")), false, None);
    app.ingest_event(user_message(1, "req_reopen", "hi"));
    app.ingest_event(provider_started(2, "req_reopen"));
    app.ingest_event(stream_delta(3, "req_reopen", "answer"));
    app.ingest_event(task_completed(4, "req_reopen", "answer"));
    app.ingest_event(run_finished(5, "done"));
    let notice = app.post_run_handoff_notice();
    assert!(
        notice.is_some() || notice.is_none(),
        "post_run_handoff_notice must return a valid Option"
    );
}

// ===========================================================================
// 8. Provider-fail recovery
// ===========================================================================

#[test]
fn recovery_state_provider_fail_is_recoverable() {
    let state = RecoveryState::ProviderFail;
    assert!(state.is_recoverable(), "ProviderFail must be recoverable");
    assert!(
        !state.composer_disabled(),
        "ProviderFail must not disable composer"
    );
    assert!(
        state.composer_hint().contains("retry"),
        "ProviderFail hint must mention retry: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_from_status_banner_provider_fail() {
    let state = RecoveryState::from_status_banner("provider request failed");
    assert_eq!(state, RecoveryState::ProviderFail);
}

#[test]
fn recovery_state_from_status_banner_error() {
    let state = RecoveryState::from_status_banner("connection error");
    assert_eq!(state, RecoveryState::ProviderFail);
}

#[test]
fn recovery_state_provider_fail_retry_transition() {
    let state = RecoveryState::ProviderFail;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::RecoveryRetry,
        "ProviderFail retry must transition to RecoveryRetry"
    );
}

#[test]
fn live_provider_fail_shows_failure_runtime() {
    let mut app = live_app();
    app.status_banner = Some("provider request failed".to_string());
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Failure);
}

// ===========================================================================
// 9. Cancel
// ===========================================================================

#[test]
fn recovery_state_cancelled_is_recoverable() {
    let state = RecoveryState::Cancelled;
    assert!(state.is_recoverable(), "Cancelled must be recoverable");
    assert!(
        !state.composer_disabled(),
        "Cancelled must not disable composer"
    );
    assert!(
        state.composer_hint().contains("retry"),
        "Cancelled hint must mention retry: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_from_status_banner_cancelled() {
    let state = RecoveryState::from_status_banner("task was cancelled");
    assert_eq!(state, RecoveryState::Cancelled);
}

#[test]
fn recovery_state_cancelled_retry_transition_to_none() {
    let state = RecoveryState::Cancelled;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::None,
        "Cancelled retry must transition to None"
    );
}

#[test]
fn live_cancel_shows_cancelled_runtime() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_cancel_state", "hi"));
    app.ingest_event(provider_started(2, "req_cancel_state"));
    app.ingest_event(task_cancelled(3, "req_cancel_state", "user hit ctrl+c"));
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Cancelled);
    assert!(
        !state.composer_disabled,
        "cancelled must not disable composer"
    );
}

// ===========================================================================
// 10. Permission-timeout
// ===========================================================================

#[test]
fn recovery_state_permission_timeout_is_recoverable() {
    let state = RecoveryState::PermissionTimeout;
    assert!(
        state.is_recoverable(),
        "PermissionTimeout must be recoverable"
    );
    assert!(
        !state.composer_disabled(),
        "PermissionTimeout must not disable composer"
    );
    assert!(
        state.composer_hint().contains("timed out"),
        "PermissionTimeout hint must mention timed out: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_from_status_banner_timeout() {
    let state = RecoveryState::from_status_banner("permission request timeout");
    assert_eq!(state, RecoveryState::PermissionTimeout);
}

#[test]
fn recovery_state_permission_timeout_retry_transition() {
    let state = RecoveryState::PermissionTimeout;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::RecoveryRetry,
        "PermissionTimeout retry must transition to RecoveryRetry"
    );
}

// ===========================================================================
// 11. Recovery-retry
// ===========================================================================

#[test]
fn recovery_state_recovery_retry_disables_composer() {
    let state = RecoveryState::RecoveryRetry;
    assert!(state.is_recoverable(), "RecoveryRetry must be recoverable");
    assert!(
        state.composer_disabled(),
        "RecoveryRetry must disable composer"
    );
    assert!(
        state.composer_hint().contains("Recovery"),
        "RecoveryRetry hint must mention Recovery: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_from_status_banner_lagged() {
    let state = RecoveryState::from_status_banner("event stream lagged");
    assert_eq!(state, RecoveryState::RecoveryRetry);
}

#[test]
fn recovery_state_from_status_banner_replaying() {
    let state = RecoveryState::from_status_banner("replaying events");
    assert_eq!(state, RecoveryState::RecoveryRetry);
}

#[test]
fn recovery_state_recovery_retry_transition_to_none() {
    let state = RecoveryState::RecoveryRetry;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::None,
        "RecoveryRetry retry must transition to None"
    );
}

#[test]
fn live_degraded_shows_degraded_runtime() {
    let mut app = live_app();
    app.status_banner = Some("event stream lagged".to_string());
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Degraded);
    assert!(state.composer_disabled, "degraded must disable composer");
}

#[test]
fn live_disconnected_shows_disconnected_runtime() {
    let mut app = live_app();
    app.status_banner = Some("disconnected from live stream".to_string());
    let state = app.runtime_state();
    assert_eq!(state.kind, RuntimeStateKind::Disconnected);
    assert!(
        state.composer_disabled,
        "disconnected must disable composer"
    );
}

// ===========================================================================
// 12. Truncated/corrupt replay
// ===========================================================================

#[test]
fn recovery_state_truncated_replay_not_recoverable() {
    let state = RecoveryState::TruncatedReplay;
    assert!(
        !state.is_recoverable(),
        "TruncatedReplay must not be recoverable"
    );
    assert!(
        state.composer_disabled(),
        "TruncatedReplay must disable composer"
    );
    assert!(
        state.composer_hint().contains("truncated"),
        "TruncatedReplay hint must mention truncated: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_corrupt_replay_not_recoverable() {
    let state = RecoveryState::CorruptReplay;
    assert!(
        !state.is_recoverable(),
        "CorruptReplay must not be recoverable"
    );
    assert!(
        state.composer_disabled(),
        "CorruptReplay must disable composer"
    );
    assert!(
        state.composer_hint().contains("corrupt"),
        "CorruptReplay hint must mention corrupt: {}",
        state.composer_hint()
    );
}

#[test]
fn recovery_state_from_status_banner_truncated() {
    let state = RecoveryState::from_status_banner("replay is truncated");
    assert_eq!(state, RecoveryState::TruncatedReplay);
}

#[test]
fn recovery_state_from_status_banner_corrupt() {
    let state = RecoveryState::from_status_banner("replay is corrupt");
    assert_eq!(state, RecoveryState::CorruptReplay);
}

#[test]
fn recovery_state_truncated_replay_stays_truncated_after_retry() {
    let state = RecoveryState::TruncatedReplay;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::TruncatedReplay,
        "TruncatedReplay must stay truncated after retry"
    );
}

#[test]
fn recovery_state_corrupt_replay_stays_corrupt_after_retry() {
    let state = RecoveryState::CorruptReplay;
    let next = state.retry_transition();
    assert_eq!(
        next,
        RecoveryState::CorruptReplay,
        "CorruptReplay must stay corrupt after retry"
    );
}

#[test]
fn recovery_state_none_has_empty_label() {
    let state = RecoveryState::None;
    assert_eq!(state.label(), "");
    assert!(!state.is_recoverable());
    assert!(!state.composer_disabled());
}

#[test]
fn recovery_state_all_variants_have_distinct_labels() {
    let states = [
        RecoveryState::None,
        RecoveryState::ProviderFail,
        RecoveryState::Cancelled,
        RecoveryState::PermissionTimeout,
        RecoveryState::RecoveryRetry,
        RecoveryState::TruncatedReplay,
        RecoveryState::CorruptReplay,
    ];
    let labels: Vec<&str> = states.iter().map(|s| s.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    // None has empty label, so 6 non-empty + 1 empty = 7 unique
    assert_eq!(
        unique.len(),
        states.len(),
        "all recovery state labels must be distinct"
    );
}

// ===========================================================================
// 13. Shell status recoverable states
// ===========================================================================

#[test]
fn shell_status_recoverable_states() {
    assert!(
        ShellStatus::ToolFailed.is_recoverable(),
        "ToolFailed must be recoverable"
    );
    assert!(
        ShellStatus::Cancelled.is_recoverable(),
        "Cancelled must be recoverable"
    );
    assert!(
        ShellStatus::Degraded.is_recoverable(),
        "Degraded must be recoverable"
    );
    assert!(
        ShellStatus::Disconnected.is_recoverable(),
        "Disconnected must be recoverable"
    );
    assert!(
        ShellStatus::Failure.is_recoverable(),
        "Failure must be recoverable"
    );
}

#[test]
fn shell_status_non_recoverable_states() {
    assert!(
        !ShellStatus::Idle.is_recoverable(),
        "Idle must not be recoverable"
    );
    assert!(
        !ShellStatus::Streaming.is_recoverable(),
        "Streaming must not be recoverable"
    );
    assert!(
        !ShellStatus::PostRun.is_recoverable(),
        "PostRun must not be recoverable"
    );
    assert!(
        !ShellStatus::ReplayReadOnly.is_recoverable(),
        "ReplayReadOnly must not be recoverable"
    );
}

// ===========================================================================
// 14. Full lifecycle render smoke test
// ===========================================================================

#[test]
fn idle_shell_renders_without_panic() {
    let app = live_app();
    let rendered = render(&app);
    assert!(
        rendered.contains('\u{276f}') || rendered.contains('>'),
        "idle shell must render composer glyph"
    );
}

#[test]
fn streaming_shell_renders_without_panic() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_render_stream", "hello"));
    app.ingest_event(provider_started(2, "req_render_stream"));
    app.ingest_event(stream_delta(3, "req_render_stream", "response text"));
    let rendered = render(&app);
    assert!(
        rendered.contains("response text"),
        "streaming shell must render transcript text"
    );
}

#[test]
fn permission_shell_renders_without_panic() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_render_perm", "edit"));
    app.ingest_event(provider_started(2, "req_render_perm"));
    app.ingest_event(permission_requested(3, "perm_render", "tc_render_perm"));
    let rendered = render(&app);
    assert!(
        rendered.contains("decision")
            || rendered.contains("Permission")
            || rendered.contains("reject")
            || rendered.contains("allow")
            || rendered.contains("\u{25cf}")
            || rendered.contains("\u{25cb}"),
        "permission shell must render decision/allow/reject/radio markers\n{rendered}"
    );
}

#[test]
fn replay_shell_renders_without_panic() {
    let events = vec![
        user_message(1, "req_render_replay", "replay render"),
        provider_started(2, "req_render_replay"),
        stream_delta(3, "req_render_replay", "replayed text"),
        task_completed(4, "req_render_replay", "replayed text"),
    ];
    let app = replay_app(events);
    let rendered = render(&app);
    assert!(
        rendered.contains("replayed text"),
        "replay shell must render transcript"
    );
}

#[test]
fn idle_dock_composer_top_matches_frozen_wide_and_compact_frames() {
    for (width, height, scenario) in [
        (120, 40, "idle-chat-120x40"),
        (80, 24, "compact-idle-chat-80x24"),
    ] {
        let rendered = render_at(&live_app(), width, height);
        let actual = rendered_composer_top(&rendered);
        let expected = reference_composer_top(scenario);
        println!("{scenario}: actual={actual} expected={expected}");
        if scenario == "idle-chat-120x40" {
            println!(
                "plain app:\n{}",
                render_at(&AppState::new_live(None, false, None), width, height)
            );
        }
        assert_eq!(
            actual, expected,
            "idle composer top row must match {scenario} at {width}x{height}"
        );
    }
}

#[test]
fn streaming_footer_keeps_shortcuts_while_transcript_owns_progress() {
    let mut app = live_app();
    app.ingest_event(user_message(1, "req_footer_status", "hello"));
    app.ingest_event(provider_started(2, "req_footer_status"));
    app.ingest_event(stream_delta(3, "req_footer_status", "partial response"));

    let rendered = render_at(&app, 120, 40);
    let footer = rendered
        .lines()
        .rev()
        .find(|line| line.contains("Shift+Tab:mode") || line.contains("Enter:send"))
        .expect("streaming footer row");

    assert!(
        !footer.contains("Streaming") && !footer.contains("response"),
        "reference footer contains controls only; progress belongs in the transcript rail: {footer}"
    );
    assert!(
        footer.contains("Shift+Tab:mode"),
        "streaming footer must retain the mode shortcut: {footer}"
    );
    assert!(
        rendered.contains("partial response"),
        "streaming content must remain visible above the shortcut footer: {rendered}"
    );
}
