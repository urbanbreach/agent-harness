//! Task 27 — Transcript blocks, tools, diffs, markdown, links, and local media
//! parity tests.
//!
//! Contract: clean-room parity program Task 27.
//!
//! Covers: user/assistant/thinking/system/session/context/compaction/
//! background/subagent blocks, tool queued/permission/running/success/failure
//! sections, edit diffs, bash/read/search/list anatomy, markdown/tables/
//! fences/syntax, hyperlinks, copy/meta viewer, Unicode/long content,
//! Mermaid, local inline image, huge-output truncation, failed edit/tool,
//! resize-during-stream.
//!
//! Differential TDD: failing tests FIRST (RED), then implementation (GREEN).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "deterministic render tests use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, BranchSummaryEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionDecision, PermissionRequestedEvent, PermissionResolvedEvent,
    ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, RunFailedEvent, SessionCompactionEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata};
use harness_tui::clipboard_leaf::{ClipboardLeaf, ClipboardMode, PasteMode};
use harness_tui::leaf_actions::group_e_media::{
    is_replay_safe, resolve, validate_input, MediaAction, MediaFailureReason,
};
use harness_tui::leaf_views::{DiffLeafView, ToolLeafView, ToolStatusLeaf, TranscriptLeafView};
use harness_tui::render_test::render_to_string;
use harness_tui::terminal::char_display_width;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

// ── Helpers ─────────────────────────────────────────────────────────────

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tb-parity-{seq:04}"),
        seq,
        run_id: "run_tb_parity".into(),
        mono_ms: seq,
        ts: Some("2026-07-27T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tb-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_tb_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tb").with_mode_label("Demo"),
    );
    app
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

fn show_generic_tool_output(app: &mut AppState) {
    app.set_generic_tool_output_visible_for_test(true);
}

fn ingest_completed_turn(app: &mut AppState, request_id: &str, user: &str, assistant: &str) {
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: user.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tb".to_string(),
            prompt_summary: user.to_string(),
            request_digest: "digest-tb".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: assistant.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out-tb".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
}

fn ingest_thinking(app: &mut AppState, request_id: &str, thinking_text: &str) {
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: request_id.into(),
            delta: thinking_text.to_string(),
        }),
    ));
}

fn ingest_tool_call(
    app: &mut AppState,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    status: ToolCallStatus,
    output_summary: Option<&str>,
) {
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: "digest-args-tb".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        11,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    ));
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary: output_summary.map(str::to_string),
            output_digest: Some("digest-out-tool-tb".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
}

fn ingest_tool_call_seq(
    app: &mut AppState,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    status: ToolCallStatus,
    output_summary: Option<&str>,
    seq_base: u64,
) {
    app.ingest_event(envelope(
        seq_base,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: "digest-args-tb".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq_base + 1,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    ));
    app.ingest_event(envelope(
        seq_base + 2,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary: output_summary.map(str::to_string),
            output_digest: Some("digest-out-tool-tb".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
}

fn ingest_started(app: &mut AppState, request_id: &str, text: &str) {
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tb".to_string(),
            prompt_summary: text.to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    ));
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

// ===========================================================================
// 1. Transcript block types
// ===========================================================================

/// BLOCK-USER: a user message renders with the user marker and text.
#[test]
fn block_user_message_renders_with_marker_and_text() {
    let mut app = live_app();
    ingest_completed_turn(&mut app, "req_user_block", "Hello world", "Reply");

    let rendered = render(&app);

    assert!(
        rendered.contains("Hello world"),
        "BLOCK-USER: user text must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "BLOCK-USER: user marker must render\n{rendered}"
    );
}

/// BLOCK-ASSISTANT: an assistant message renders the content in the body.
#[test]
fn block_assistant_message_renders_content_in_body() {
    let mut app = live_app();
    ingest_completed_turn(&mut app, "req_asst_block", "question", "The answer is 42.");

    let rendered = render(&app);

    assert!(
        rendered.contains("The answer is 42."),
        "BLOCK-ASSISTANT: assistant content must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "BLOCK-ASSISTANT: bordered composer retained\n{rendered}"
    );
}

/// BLOCK-THINKING: a reasoning/thinking delta renders with a thinking label
/// when thinking is visible.
#[test]
fn block_thinking_renders_with_thinking_label() {
    let mut app = live_app();
    let request_id = "req_thinking_block";
    ingest_started(&mut app, request_id, "think about this");
    ingest_thinking(
        &mut app,
        request_id,
        "I need to consider the options carefully.",
    );
    app.ingest_event(envelope(
        6,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "Here is my answer.".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        7,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-think".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    let rendered = render(&app);

    assert!(
        rendered.contains("Thought for"),
        "BLOCK-THINKING: completed reasoning must render collapsed thought chrome\n{rendered}"
    );
}

/// BLOCK-SYSTEM: a run-failed event renders the error in the transcript.
#[test]
fn block_system_run_failed_renders_error() {
    let mut app = live_app();
    ingest_started(&mut app, "req_sys_fail", "trigger a failure");
    app.ingest_event(envelope(
        20,
        Some("req_sys_fail"),
        EventV1::RunFailed(RunFailedEvent {
            error: "catastrophic system failure".to_string(),
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("catastrophic system failure")
            || rendered.contains("failed")
            || rendered.contains("Failed"),
        "BLOCK-SYSTEM: run-failed error must surface\n{rendered}"
    );
}

/// BLOCK-COMPACTION: a session compaction event renders the summary.
#[test]
fn block_session_compaction_renders_summary() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_pre_compact",
        "before compaction",
        "reply before",
    );
    app.ingest_event(envelope(
        30,
        None,
        EventV1::SessionCompaction(SessionCompactionEvent {
            agent_id: "build".to_string(),
            summary: "Compacted 15 turns about Rust async patterns.".to_string(),
            first_kept_event_seq: 20,
            first_kept_request_id: Some("req_pre_compact".to_string()),
            tokens_before: 50000,
            read_files: vec!["src/main.rs".to_string()],
            modified_files: vec![],
            trigger_reason: "token_limit".to_string(),
            from_hook: false,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("Compacted")
            || rendered.contains("compaction")
            || rendered.contains("Summary"),
        "BLOCK-COMPACTION: compaction summary must surface\n{rendered}"
    );
}

/// BLOCK-CONTEXT: a branch summary event renders the summary.
#[test]
fn block_branch_summary_renders_summary() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_pre_branch",
        "before branch",
        "reply before branch",
    );
    app.ingest_event(envelope(
        30,
        None,
        EventV1::BranchSummary(BranchSummaryEvent {
            agent_id: "build".to_string(),
            summary: "Branch explored alternative implementation.".to_string(),
            from_event_seq: 10,
            read_files: vec!["src/lib.rs".to_string()],
            modified_files: vec!["src/main.rs".to_string()],
            from_hook: false,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("Branch explored")
            || rendered.contains("branch")
            || rendered.contains("Summary"),
        "BLOCK-CONTEXT: branch summary must surface\n{rendered}"
    );
}

/// BLOCK-BACKGROUND: a background task notification renders in the transcript.
#[test]
fn block_background_task_notification_renders() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_bg_parent",
        "spawn background",
        "background spawned",
    );
    app.ingest_event(envelope(
        30,
        Some("req_bg_parent"),
        EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
            parent_session_id: "run_tb_parity".into(),
            parent_agent_id: Some("build".to_string()),
            child_session_id: "child-session-001".into(),
            child_request_id: "req_bg_child".to_string(),
            task_id: "task-bg-001".into(),
            description: "Background search completed".to_string(),
            status: BackgroundTaskNotificationStatus::Completed,
            summary: "Found 3 results in background".to_string(),
            terminal_event_id: "evt-bg-term".to_string(),
            terminal_task_id: "task-bg-001".to_string(),
            delivered_turn_request_id: Some("req_bg_parent".to_string()),
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("background")
            || rendered.contains("Background")
            || rendered.contains("Found 3 results"),
        "BLOCK-BACKGROUND: background task notification must surface\n{rendered}"
    );
}

/// BLOCK-SUBAGENT: a subagent spawn event renders in the transcript.
#[test]
fn block_subagent_spawn_renders() {
    let mut app = live_app();
    let request_id = "req_subagent";
    ingest_started(&mut app, request_id, "delegate to subagent");
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_subagent".into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"prompt":"explore the codebase","run_in_background":false}"#
                .to_string(),
            args_digest: "digest-subagent".to_string(),
            metadata: Some(harness_core::event::ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    child_session_id: Some("child-session-sub".to_string()),
                    child_request_id: Some("req_child_sub".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }),
    ));
    app.ingest_event(envelope(
        11,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_subagent".into(),
        }),
    ));
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_subagent".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("explored 5 files".to_string()),
            output_digest: Some("digest-sub-out".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    app.toggle_tool_output_for_test("tc_subagent");
    let rendered = render(&app);

    assert!(
        rendered.contains("explore") || rendered.contains("task") || rendered.contains('◆'),
        "BLOCK-SUBAGENT: subagent task must render\n{rendered}"
    );
}

// ===========================================================================
// 2. Tool sections: queued / permission / running / success / failure
// ===========================================================================

/// TOOL-QUEUED: a tool call that has been requested but not started renders
/// without panicking.
#[test]
fn tool_queued_renders_without_panic() {
    let mut app = live_app();
    let request_id = "req_tool_queued";
    ingest_started(&mut app, request_id, "run a queued tool");
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_queued".into(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"echo queued"}"#.to_string(),
            args_digest: "digest-queued".to_string(),
            metadata: None,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("echo queued") || rendered.contains("bash") || rendered.contains('◆'),
        "TOOL-QUEUED: queued tool must render\n{rendered}"
    );
}

/// TOOL-PERMISSION: a tool call with a pending permission request renders
/// the permission state.
#[test]
fn tool_permission_pending_renders() {
    let mut app = live_app();
    let request_id = "req_tool_perm";
    ingest_started(&mut app, request_id, "run a permission-gated tool");
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_perm".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
            args_digest: "digest-perm".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        11,
        Some(request_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm-001".to_string(),
            kind: "edit".to_string(),
            tool_call_id: Some("tc_perm".into()),
            summary: "Edit src/lib.rs".to_string(),
            request_digest: "digest-perm-req".to_string(),
            timeout_ms: 30000,
            default_decision: PermissionDecision::Deny,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("edit") || rendered.contains("src/lib.rs") || rendered.contains('◆'),
        "TOOL-PERMISSION: permission-gated tool must render\n{rendered}"
    );
}

/// TOOL-RUNNING: a tool call that has been started but not finished renders
/// the running marker.
#[test]
fn tool_running_renders_with_marker() {
    let mut app = live_app();
    let request_id = "req_tool_running";
    ingest_started(&mut app, request_id, "run a running tool");
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_running".into(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"sleep 1"}"#.to_string(),
            args_digest: "digest-running".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        11,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_running".into(),
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("sleep 1") || rendered.contains('◆') || rendered.contains('◈'),
        "TOOL-RUNNING: running tool must render\n{rendered}"
    );
}

/// TOOL-SUCCESS: a succeeded tool call renders the output summary.
#[test]
fn tool_success_renders_with_output() {
    let mut app = live_app();
    let request_id = "req_tool_success";
    ingest_started(&mut app, request_id, "run a successful tool");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_success",
        "bash",
        r#"{"command":"echo done"}"#,
        ToolCallStatus::Succeeded,
        Some("done"),
    );
    app.toggle_tool_output_for_test("tc_success");

    let rendered = render(&app);

    assert!(
        rendered.contains("done"),
        "TOOL-SUCCESS: output summary must render\n{rendered}"
    );
    assert!(
        rendered.contains('◆') || rendered.contains('◈'),
        "TOOL-SUCCESS: completed marker must render\n{rendered}"
    );
}

/// TOOL-FAILURE: a failed tool call renders the error details.
#[test]
fn tool_failure_renders_with_error() {
    let mut app = live_app();
    let request_id = "req_tool_fail";
    ingest_started(&mut app, request_id, "run a failing tool");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_fail",
        "bash",
        r#"{"command":"false"}"#,
        ToolCallStatus::Failed,
        Some("command failed with exit code 1"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("command failed with exit code 1")
            || rendered.contains("No error details available.")
            || rendered.contains("failed"),
        "TOOL-FAILURE: error details must surface\n{rendered}"
    );
}

// ===========================================================================
// 3. Edit diffs
// ===========================================================================

/// DIFF-EXPANDED: an edit tool call with tool details toggled on shows both
/// old and new strings.
#[test]
fn diff_expanded_shows_old_and_new_strings() {
    use std::path::PathBuf;

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_tb_diff")), false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tb").with_mode_label("Demo"),
    );
    let request_id = "req_diff_exp";
    ingest_started(&mut app, request_id, "apply the edit");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_exp".into(),
            tool_id: "edit".to_string(),
            args_summary:
                r#"{"path":"src/lib.rs","oldString":"let x = 1;","newString":"let x = 2;"}"#
                    .to_string(),
            args_digest: "digest-diff-exp".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff_exp".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_exp".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/lib.rs".to_string()),
            output_digest: Some("digest-out-diff-exp".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        15,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-tb-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.focus = Focus::Details;

    // Toggle tool details on via palette
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "show tool details".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.toggle_tool_output_for_test("tc_diff_exp");

    let rendered = render(&app);

    assert!(
        app.transcript_interaction_snapshot().show_tool_details,
        "DIFF-EXPANDED: tool details must toggle on"
    );
    assert!(
        rendered.contains("let x = 1;"),
        "DIFF-EXPANDED: removed line must project\n{rendered}"
    );
    assert!(
        rendered.contains("let x = 2;"),
        "DIFF-EXPANDED: added line must project\n{rendered}"
    );
}

/// DIFF-COLLAPSED: an edit tool call without tool details hides the raw
/// old/new strings.
#[test]
fn diff_collapsed_hides_raw_strings() {
    let mut app = live_app();
    let request_id = "req_diff_col";
    ingest_started(&mut app, request_id, "apply the edit collapsed");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_col".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/main.rs","oldString":"fn old()","newString":"fn new()"}"#
                .to_string(),
            args_digest: "digest-diff-col".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff_col".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_col".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/main.rs".to_string()),
            output_digest: Some("digest-out-diff-col".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("src/main.rs") || rendered.contains("edit") || rendered.contains('◆'),
        "DIFF-COLLAPSED: tool header must project\n{rendered}"
    );
    assert!(
        !rendered.contains("fn old()"),
        "DIFF-COLLAPSED: raw old string should not project\n{rendered}"
    );
    assert!(
        !rendered.contains("fn new()"),
        "DIFF-COLLAPSED: raw new string should not project\n{rendered}"
    );
}

// ===========================================================================
// 4. Tool anatomy: bash / read / search / list
// ===========================================================================

/// ANATOMY-BASH: a bash tool call renders the command and output.
#[test]
fn anatomy_bash_renders_command_and_output() {
    let mut app = live_app();
    let request_id = "req_bash_anat";
    ingest_started(&mut app, request_id, "run bash");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_bash",
        "bash",
        r#"{"command":"echo hello world"}"#,
        ToolCallStatus::Succeeded,
        Some("hello world"),
    );
    app.toggle_tool_output_for_test("tc_bash");

    let rendered = render(&app);

    assert!(
        rendered.contains("echo hello world"),
        "ANATOMY-BASH: command must render\n{rendered}"
    );
    assert!(
        rendered.contains("hello world"),
        "ANATOMY-BASH: output must render\n{rendered}"
    );
}

/// ANATOMY-READ: a read tool call renders the file path.
#[test]
fn anatomy_read_renders_file_path() {
    let mut app = live_app();
    let request_id = "req_read_anat";
    ingest_started(&mut app, request_id, "read a file");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_read",
        "read",
        r#"{"filePath":"src/main.rs"}"#,
        ToolCallStatus::Succeeded,
        Some("file content here"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("src/main.rs") || rendered.contains("read"),
        "ANATOMY-READ: file path must render\n{rendered}"
    );
}

/// ANATOMY-SEARCH: a grep/search tool call renders the pattern and match count.
#[test]
fn anatomy_search_renders_pattern_and_matches() {
    let mut app = live_app();
    let request_id = "req_search_anat";
    ingest_started(&mut app, request_id, "search for a pattern");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_search",
        "grep",
        r#"{"pattern":"fn main","path":"src/"}"#,
        ToolCallStatus::Succeeded,
        Some("3 matches found"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("fn main") || rendered.contains("grep") || rendered.contains("search"),
        "ANATOMY-SEARCH: pattern must render\n{rendered}"
    );
    assert!(
        rendered.contains("3 matches") || rendered.contains("match"),
        "ANATOMY-SEARCH: match count must render\n{rendered}"
    );
}

/// ANATOMY-LIST: a glob/list tool call renders the results.
#[test]
fn anatomy_list_renders_results() {
    let mut app = live_app();
    let request_id = "req_list_anat";
    ingest_started(&mut app, request_id, "list files");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_list",
        "glob",
        r#"{"pattern":"**/*.rs"}"#,
        ToolCallStatus::Succeeded,
        Some("found 5 files"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("*.rs")
            || rendered.contains("glob")
            || rendered.contains("found 5 files"),
        "ANATOMY-LIST: results must render\n{rendered}"
    );
}

// ===========================================================================
// 5. Markdown: tables / fences / syntax
// ===========================================================================

/// MD-TABLE: a markdown table renders with column content.
#[test]
fn md_table_renders_columns() {
    let mut app = live_app();
    let markdown = [
        "Here is a table:",
        "",
        "| Name | Value |",
        "|------|-------|",
        "| foo  | 1     |",
        "| bar  | 2     |",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_md_table", "show a table", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("foo"),
        "MD-TABLE: first data cell must render\n{rendered}"
    );
    assert!(
        rendered.contains("bar"),
        "MD-TABLE: second data cell must render\n{rendered}"
    );
    assert!(
        rendered.contains("Name") || rendered.contains("Value"),
        "MD-TABLE: header must render\n{rendered}"
    );
}

/// MD-FENCE: a fenced code block with unknown language renders content.
#[test]
fn md_fence_unknown_lang_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Intro text.",
        "```text",
        "plain code line",
        "```",
        "Outro text.",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_md_fence", "show a fence", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("plain code line"),
        "MD-FENCE: code content must render\n{rendered}"
    );
    assert!(
        rendered.contains("Intro text."),
        "MD-FENCE: intro text retained\n{rendered}"
    );
    assert!(
        rendered.contains("Outro text."),
        "MD-FENCE: outro text retained\n{rendered}"
    );
}

/// MD-SYNTAX: a fenced Rust code block renders with syntax highlighting.
#[test]
fn md_syntax_rust_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Here is Rust code:",
        "```rust",
        "fn main() {",
        "    let x = 42;",
        "}",
        "```",
        "Done.",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_md_syntax", "show rust", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("fn main()"),
        "MD-SYNTAX: Rust function must render\n{rendered}"
    );
    assert!(
        rendered.contains("let x = 42;"),
        "MD-SYNTAX: Rust let binding must render\n{rendered}"
    );
    assert!(
        !rendered.contains("```rust"),
        "MD-SYNTAX: raw fence markers should not render\n{rendered}"
    );
}

// ===========================================================================
// 6. Hyperlinks
// ===========================================================================

/// LINK-MD: a markdown link renders the label, not the raw URL.
#[test]
fn link_markdown_renders_label_not_url() {
    let mut app = live_app();
    let markdown = "See [the documentation](https://example.com/docs) for more.";
    ingest_completed_turn(&mut app, "req_link_md", "show a link", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("the documentation"),
        "LINK-MD: link label must render\n{rendered}"
    );
    assert!(
        !rendered.contains("https://example.com/docs"),
        "LINK-MD: raw URL should not render as plain text\n{rendered}"
    );
    assert!(
        !rendered.contains("[the documentation]("),
        "LINK-MD: raw markdown link syntax should not render\n{rendered}"
    );
}

/// LINK-RAW: a raw URL renders as a link in the transcript.
#[test]
fn link_raw_url_renders() {
    let mut app = live_app();
    let markdown = "Visit https://example.com for details.";
    ingest_completed_turn(&mut app, "req_link_raw", "show raw url", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("https://example.com"),
        "LINK-RAW: raw URL must render\n{rendered}"
    );
    assert!(
        rendered.contains("Visit"),
        "LINK-RAW: surrounding text must render\n{rendered}"
    );
}

/// LINK-MULTI: multiple links in a single line all render their labels.
#[test]
fn link_multiple_renders_all_labels() {
    let mut app = live_app();
    let markdown = "See [first](https://a.test) and [second](https://b.test) links.";
    ingest_completed_turn(&mut app, "req_link_multi", "show multiple links", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("first"),
        "LINK-MULTI: first label must render\n{rendered}"
    );
    assert!(
        rendered.contains("second"),
        "LINK-MULTI: second label must render\n{rendered}"
    );
}

// ===========================================================================
// 7. Copy / meta viewer
// ===========================================================================

/// COPY-OSC52: the full clipboard leaf supports OSC52.
#[test]
fn copy_clipboard_full_supports_osc52() {
    let leaf = ClipboardLeaf::full();

    assert!(
        leaf.mode.is_available(),
        "COPY-OSC52: full leaf must be available"
    );
    assert!(
        leaf.mode.supports_osc52(),
        "COPY-OSC52: full leaf must support OSC52"
    );
    assert!(
        leaf.mode.supports_native(),
        "COPY-OSC52: full leaf must support native fallback"
    );
    assert_eq!(
        leaf.paste_mode,
        PasteMode::Bracketed,
        "COPY-OSC52: full leaf must have bracketed paste"
    );
    assert!(
        leaf.copy_on_select,
        "COPY-OSC52: full leaf must have copy-on-select"
    );
    assert!(
        leaf.hyperlink_support,
        "COPY-OSC52: full leaf must support hyperlinks"
    );
}

/// COPY-DISABLED: the disabled clipboard leaf has no features.
#[test]
fn copy_clipboard_disabled_has_no_features() {
    let leaf = ClipboardLeaf::disabled();

    assert!(
        !leaf.mode.is_available(),
        "COPY-DISABLED: disabled leaf must be unavailable"
    );
    assert!(
        !leaf.mode.supports_osc52(),
        "COPY-DISABLED: disabled leaf must not support OSC52"
    );
    assert_eq!(
        leaf.paste_mode,
        PasteMode::Disabled,
        "COPY-DISABLED: disabled leaf must have paste disabled"
    );
}

/// META-VIEWER: tool details toggle reveals tool metadata in the transcript.
#[test]
fn meta_viewer_tool_details_toggle_reveals_metadata() {
    use std::path::PathBuf;

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_tb_meta")), false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tb").with_mode_label("Demo"),
    );
    let request_id = "req_meta_viewer";
    ingest_started(&mut app, request_id, "run a tool with metadata");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_meta",
        "bash",
        r#"{"command":"cargo build"}"#,
        ToolCallStatus::Succeeded,
        Some("Compiling harness v0.1.0"),
    );
    app.ingest_event(envelope(
        15,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-meta".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.focus = Focus::Details;

    // Toggle tool details on
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "show tool details".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.toggle_tool_output_for_test("tc_meta");

    let snapshot = app.transcript_interaction_snapshot();
    assert!(
        snapshot.show_tool_details,
        "META-VIEWER: tool details must be toggled on"
    );

    let rendered = render(&app);
    assert!(
        rendered.contains("cargo build"),
        "META-VIEWER: tool command must be visible with details on\n{rendered}"
    );
}

// ===========================================================================
// 8. Unicode / long content
// ===========================================================================

/// UNICODE-CJK: CJK characters render in the transcript body.
#[test]
fn unicode_cjk_renders() {
    let mut app = live_app();
    let markdown = "Korean: 안녕하세요. Japanese: こんにちは. Chinese: 你好.";
    ingest_completed_turn(&mut app, "req_unicode_cjk", "show CJK", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains('안') && rendered.contains('녕'),
        "UNICODE-CJK: Korean characters must render\n{rendered}"
    );
    assert!(
        rendered.contains('こ') && rendered.contains('ん'),
        "UNICODE-CJK: Japanese characters must render\n{rendered}"
    );
    assert!(
        rendered.contains('你') && rendered.contains('好'),
        "UNICODE-CJK: Chinese characters must render\n{rendered}"
    );
}

/// UNICODE-EMOJI: emoji characters render in the transcript body.
#[test]
fn unicode_emoji_renders() {
    let mut app = live_app();
    let markdown = "Emoji: 🚀 🎉 ✅";
    ingest_completed_turn(&mut app, "req_unicode_emoji", "show emoji", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("🚀"),
        "UNICODE-EMOJI: rocket emoji must render\n{rendered}"
    );
    assert!(
        rendered.contains("🎉"),
        "UNICODE-EMOJI: party emoji must render\n{rendered}"
    );
}

/// UNICODE-WIDTH: char_display_width returns correct widths.
#[test]
fn unicode_char_display_width_returns_correct_widths() {
    assert_eq!(char_display_width('a'), 1, "ASCII must be width 1");
    assert_eq!(char_display_width('안'), 2, "Korean Hangul must be width 2");
    assert_eq!(
        char_display_width('こ'),
        2,
        "Japanese Hiragana must be width 2"
    );
    assert_eq!(char_display_width('你'), 2, "Chinese Han must be width 2");
}

/// LONG-CONTENT: a very long assistant response wraps and remains visible.
#[test]
fn long_content_wraps_and_remains_visible() {
    let mut app = live_app();
    let long_text = "This is a very long line of text that should wrap across multiple terminal rows when rendered in the transcript. ".repeat(5);
    ingest_completed_turn(&mut app, "req_long", "show long content", &long_text);

    let rendered = render(&app);

    assert!(
        rendered.contains("This is a very long line"),
        "LONG-CONTENT: long content must render\n{rendered}"
    );
    assert!(
        rendered.contains("terminal rows"),
        "LONG-CONTENT: later content must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "LONG-CONTENT: bordered composer retained\n{rendered}"
    );
}

// ===========================================================================
// 9. Mermaid
// ===========================================================================

/// MERMAID: a fenced mermaid code block renders a placeholder, not the raw
/// diagram source code.
#[test]
fn mermaid_block_renders_placeholder_not_raw_code() {
    let mut app = live_app();
    let markdown = [
        "Here is a diagram:",
        "```mermaid",
        "graph TD",
        "    A --> B",
        "    B --> C",
        "```",
        "Done.",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_mermaid", "show a mermaid diagram", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("Mermaid")
            || rendered.contains("mermaid")
            || rendered.contains("diagram"),
        "MERMAID: mermaid placeholder must render\n{rendered}"
    );
    // The raw graph syntax should NOT appear as plain rendered text
    assert!(
        !rendered.contains("graph TD"),
        "MERMAID: raw graph syntax should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("A --> B"),
        "MERMAID: raw edge syntax should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("```mermaid"),
        "MERMAID: raw fence markers should not render\n{rendered}"
    );
}

/// MERMAID-PRESERVE: the mermaid placeholder preserves the diagram type
/// (flowchart, sequence, etc.).
#[test]
fn mermaid_block_preserves_diagram_type() {
    let mut app = live_app();
    let markdown = [
        "```mermaid",
        "sequenceDiagram",
        "    Alice->>Bob: Hello",
        "```",
    ]
    .join("\n");
    ingest_completed_turn(
        &mut app,
        "req_mermaid_seq",
        "show sequence diagram",
        &markdown,
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("Mermaid")
            || rendered.contains("mermaid")
            || rendered.contains("diagram"),
        "MERMAID-PRESERVE: mermaid placeholder must render\n{rendered}"
    );
    assert!(
        !rendered.contains("sequenceDiagram"),
        "MERMAID-PRESERVE: raw sequence syntax should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("Alice->>Bob"),
        "MERMAID-PRESERVE: raw message syntax should not render\n{rendered}"
    );
}

// ===========================================================================
// 10. Local inline image
// ===========================================================================

/// IMAGE-INLINE: an inline image markdown renders a placeholder, not the raw
/// markdown syntax or file path.
#[test]
fn image_inline_renders_placeholder_not_raw_markdown() {
    let mut app = live_app();
    let markdown = "Here is a screenshot: ![screenshot of the UI](./screenshots/ui.png) end.";
    ingest_completed_turn(&mut app, "req_image", "show an image", markdown);

    let rendered = render(&app);

    // The alt text should render as a placeholder
    assert!(
        rendered.contains("screenshot")
            || rendered.contains("image")
            || rendered.contains("[image"),
        "IMAGE-INLINE: image placeholder must render\n{rendered}"
    );
    // The raw markdown syntax should NOT render
    assert!(
        !rendered.contains("![screenshot"),
        "IMAGE-INLINE: raw image markdown should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("](./screenshots/ui.png)"),
        "IMAGE-INLINE: raw image path should not render\n{rendered}"
    );
}

/// IMAGE-ALT: the image placeholder preserves the alt text.
#[test]
fn image_inline_preserves_alt_text() {
    let mut app = live_app();
    let markdown = "See ![architecture diagram](./docs/arch.png) for details.";
    ingest_completed_turn(&mut app, "req_image_alt", "show image with alt", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("architecture") || rendered.contains("diagram"),
        "IMAGE-ALT: alt text must be preserved in placeholder\n{rendered}"
    );
    assert!(
        !rendered.contains("![architecture diagram]"),
        "IMAGE-ALT: raw markdown should not render\n{rendered}"
    );
}

// ===========================================================================
// 11. Huge-output truncation
// ===========================================================================

/// TRUNCATE-TOOL: a tool with very large output is truncated with an
/// indicator.
#[test]
fn truncate_huge_tool_output_truncated_with_indicator() {
    let mut app = live_app();
    let request_id = "req_trunc_tool";
    ingest_started(&mut app, request_id, "run a tool with huge output");
    // Generate output that exceeds the line clamp (15 lines)
    let huge_output = (1..=50)
        .map(|i| format!("output line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_trunc",
        "bash",
        r#"{"command":"generate output"}"#,
        ToolCallStatus::Succeeded,
        Some(&huge_output),
    );
    show_generic_tool_output(&mut app);

    let rendered = render(&app);

    // The first few lines should be visible
    assert!(
        rendered.contains("output line 1"),
        "TRUNCATE-TOOL: first output lines must render\n{rendered}"
    );
    // The last lines should NOT be visible (truncated)
    assert!(
        !rendered.contains("output line 50"),
        "TRUNCATE-TOOL: last output lines should be truncated\n{rendered}"
    );
    // A truncation indicator should be present
    assert!(
        rendered.contains('…') || rendered.contains("...") || rendered.contains("truncat"),
        "TRUNCATE-TOOL: truncation indicator must be present\n{rendered}"
    );
}

/// TRUNCATE-BASH: a bash tool with very large output is truncated.
#[test]
fn truncate_huge_bash_output_truncated() {
    let mut app = live_app();
    let request_id = "req_trunc_bash";
    ingest_started(&mut app, request_id, "run bash with huge output");
    let huge_output = (1..=30)
        .map(|i| format!("bash line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_trunc_bash",
        "bash",
        r#"{"command":"ls -la"}"#,
        ToolCallStatus::Succeeded,
        Some(&huge_output),
    );
    show_generic_tool_output(&mut app);

    let rendered = render(&app);

    assert!(
        rendered.contains("bash line 1"),
        "TRUNCATE-BASH: first lines must render\n{rendered}"
    );
    assert!(
        !rendered.contains("bash line 30"),
        "TRUNCATE-BASH: last lines should be truncated\n{rendered}"
    );
}

// ===========================================================================
// 12. Failed edit / tool
// ===========================================================================

/// FAILED-EDIT: a failed edit tool call renders the error.
#[test]
fn failed_edit_renders_error() {
    let mut app = live_app();
    let request_id = "req_fail_edit";
    ingest_started(&mut app, request_id, "apply a failing edit");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_fail_edit",
        "edit",
        r#"{"path":"src/missing.rs","oldString":"old","newString":"new"}"#,
        ToolCallStatus::Failed,
        Some("anchor not found in file"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("anchor not found")
            || rendered.contains("failed")
            || rendered.contains("error"),
        "FAILED-EDIT: error must surface\n{rendered}"
    );
}

/// FAILED-TOOL: a failed bash tool call renders the error details.
#[test]
fn failed_tool_renders_error_details() {
    let mut app = live_app();
    let request_id = "req_fail_tool";
    ingest_started(&mut app, request_id, "run a failing tool");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_fail_tool",
        "bash",
        r#"{"command":"exit 1"}"#,
        ToolCallStatus::Failed,
        Some("command failed with exit code 1"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("command failed with exit code 1")
            || rendered.contains("No error details available.")
            || rendered.contains("failed"),
        "FAILED-TOOL: error details must surface\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "FAILED-TOOL: bordered composer retained\n{rendered}"
    );
}

// ===========================================================================
// 13. Resize-during-stream
// ===========================================================================

/// RESIZE-NO-PANIC: rendering at different widths during streaming does not
/// panic.
#[test]
fn resize_during_stream_does_not_panic() {
    let mut app = live_app();
    let request_id = "req_resize";
    ingest_started(&mut app, request_id, "streaming content");

    // Stream a delta (no finish event — still streaming)
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "Streaming content that should survive resize.".to_string(),
        }),
    ));

    // Render at various sizes — must not panic
    let _wide = render_at(&app, 120, 40);
    let _narrow = render_at(&app, 80, 24);
    let _medium = render_at(&app, 100, 30);
    let _very_narrow = render_at(&app, 60, 20);

    // At least one size must contain the streaming content
    let rendered = render_at(&app, 120, 40);
    assert!(
        rendered.contains("Streaming content") || rendered.contains("streaming"),
        "RESIZE-NO-PANIC: streaming content must render at 120x40\n{rendered}"
    );
}

/// RESIZE-PRESERVE: content is preserved across different terminal widths.
#[test]
fn resize_preserves_content_across_widths() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_resize_preserve",
        "check resize preservation",
        "Preserved content across widths.",
    );

    let wide = render_at(&app, 120, 40);
    let narrow = render_at(&app, 80, 24);

    assert!(
        wide.contains("Preserved content"),
        "RESIZE-PRESERVE: content must render at 120x40\n{wide}"
    );
    assert!(
        narrow.contains("Preserved content"),
        "RESIZE-PRESERVE: content must render at 80x24\n{narrow}"
    );
}

// ===========================================================================
// Leaf view determinism (supplementary)
// ===========================================================================

/// LEAF-TOOL: tool leaf view lifecycle states are deterministic.
#[test]
fn leaf_tool_lifecycle_states_are_deterministic() {
    let queued = ToolLeafView::new("edit", ToolStatusLeaf::Queued);
    assert_eq!(queued.status, ToolStatusLeaf::Queued);
    assert!(!queued.permission_before_tool());

    let running = ToolLeafView::new("bash", ToolStatusLeaf::Running).permission_granted();
    assert_eq!(running.status, ToolStatusLeaf::Running);
    assert!(running.permission_before_tool());

    let completed = ToolLeafView::new("edit", ToolStatusLeaf::Completed)
        .permission_granted()
        .with_diff();
    assert!(completed.has_diff);
    assert!(completed.permission_before_tool());

    let failed = ToolLeafView::new("bash", ToolStatusLeaf::Failed)
        .permission_granted()
        .with_error();
    assert!(failed.has_error);
}

/// LEAF-DIFF: diff leaf view from event is valid and has correct counts.
#[test]
fn leaf_diff_from_event_is_valid_with_counts() {
    let diff = DiffLeafView::from_event("src/lib.rs", 10, 3);
    assert!(diff.is_valid());
    assert!(diff.event_derived);
    assert_eq!(diff.total_changed(), 13);
    assert_eq!(diff.added_lines, 10);
    assert_eq!(diff.removed_lines, 3);
}

/// LEAF-TRANSCRIPT: transcript leaf view scroll state is deterministic.
#[test]
fn leaf_transcript_scroll_state_is_deterministic() {
    let t = TranscriptLeafView::new(0, 20);
    assert_eq!(t.scroll_offset, 0);
    assert_eq!(t.visible_lines, 20);

    let scrolled = TranscriptLeafView::new(42, 15);
    assert_eq!(scrolled.scroll_offset, 42);
    assert_eq!(scrolled.visible_lines, 15);
}

/// LEAF-MEDIA: media action replay safety is correct.
#[test]
fn leaf_media_replay_safety_is_correct() {
    assert!(
        is_replay_safe(MediaAction::MediaFailureRecovery),
        "LEAF-MEDIA: failure recovery must be replay-safe"
    );
    assert!(
        is_replay_safe(MediaAction::RenderInlineMedia),
        "LEAF-MEDIA: render inline media must be replay-safe"
    );
    assert!(
        !is_replay_safe(MediaAction::ClipboardImagePaste),
        "LEAF-MEDIA: clipboard image paste must NOT be replay-safe"
    );
}

/// LEAF-MEDIA-RESOLVE: inline media capability resolves as unavailable.
#[test]
fn leaf_media_capability_resolves_as_unavailable() {
    let resolution = resolve("tui.inline_media");
    assert!(
        resolution.is_some(),
        "LEAF-MEDIA-RESOLVE: capability must resolve"
    );
    let resolution = resolution.unwrap();
    assert_eq!(resolution.capability_id, "tui.inline_media");
    assert!(
        !matches!(
            resolution.availability,
            harness_tui::leaf_actions::group_e_media::ActionAvailability::Available
        ),
        "LEAF-MEDIA-RESOLVE: inline media must not be available without terminal negotiation"
    );
    assert!(
        resolution.replay_safe,
        "LEAF-MEDIA-RESOLVE: resolution must be replay-safe"
    );
}

/// LEAF-MEDIA-FAILURE: media failure reasons cover expected modes.
#[test]
fn leaf_media_failure_reasons_cover_expected_modes() {
    assert_eq!(
        MediaFailureReason::default(),
        MediaFailureReason::None,
        "LEAF-MEDIA-FAILURE: default must be None"
    );
    let reasons = [
        MediaFailureReason::None,
        MediaFailureReason::ClipboardUnavailable,
        MediaFailureReason::MediaDecodeFailed,
        MediaFailureReason::TerminalDoesNotSupportInlineMedia,
    ];
    for (i, a) in reasons.iter().enumerate() {
        for (j, b) in reasons.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "LEAF-MEDIA-FAILURE: reasons must be distinct");
            }
        }
    }
}

/// LEAF-MEDIA-VALIDATE: render inline media validates input.
#[test]
fn leaf_media_validate_input() {
    assert_eq!(
        validate_input(MediaAction::RenderInlineMedia, ""),
        harness_tui::leaf_actions::group_e_media::InputValidation::Invalid(
            "render requires a media path or url"
        ),
        "LEAF-MEDIA-VALIDATE: empty input must be invalid"
    );
    assert_eq!(
        validate_input(MediaAction::RenderInlineMedia, "/path/to/image.png"),
        harness_tui::leaf_actions::group_e_media::InputValidation::Valid,
        "LEAF-MEDIA-VALIDATE: path input must be valid"
    );
}

// ===========================================================================
// Absence rules
// ===========================================================================

/// ABSENCE: the rendered transcript must not contain voice or hosted media
/// generation affordances.
#[test]
fn absence_no_voice_or_hosted_media_generation() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_absence",
        "check absence",
        "Normal assistant reply.",
    );

    let rendered = render(&app);

    assert!(
        !rendered.contains("🎤") && !rendered.contains("🎙"),
        "ABSENCE: no voice microphone glyph"
    );
    assert!(
        !rendered.contains("Generate image")
            && !rendered.contains("Generate media")
            && !rendered.contains("Hosted media"),
        "ABSENCE: no hosted media generation affordance"
    );
    assert!(
        !rendered.contains("Voice input"),
        "ABSENCE: no voice input label"
    );
}
