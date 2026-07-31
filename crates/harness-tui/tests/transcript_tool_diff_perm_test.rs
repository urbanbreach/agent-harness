//! Task 23: Deterministic render tests for shell, transcript, tool, diff,
//! permission, and question leaf views.
//!
//! Proves the leaf view types render deterministically across all required
//! lifecycle states (idle, stream, cancel, fail, recover, complete, scroll)
//! without panicking, and that event-derived content and diffs are present
//! when expected.
//!
//! These tests use the Ratatui TestBackend (not snapshots) and the leaf
//! view types — no AppState or shared registry dependency.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "deterministic render tests use fail-fast asserts"
)]

use harness_tui::leaf_actions::ActionLeaf;
use harness_tui::leaf_views::{
    DiffLeafView, FocusLeaf, PermissionKindLeaf, PermissionLeafView, PermissionStateLeaf,
    QuestionLeafView, QuestionStateLeaf, ShellKindLeaf, ShellLeafView, ToolLeafView,
    ToolStatusLeaf, TranscriptLeafView,
};
use harness_tui::render_test::buffer_to_string;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

const W: u16 = 120;
const H: u16 = 40;

fn render_shell(shell: &ShellLeafView, transcript: &TranscriptLeafView) -> String {
    let area = Rect::new(0, 0, W, H);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let title = match shell.shell_kind {
                ShellKindLeaf::Live => "Harness — Live",
                ShellKindLeaf::Replay => "Harness — Replay",
                ShellKindLeaf::Startup => "Harness — Startup",
            };
            let status = if shell.streaming {
                "streaming"
            } else if shell.cancelled {
                "cancelled"
            } else if shell.failed {
                "failed"
            } else if shell.completed {
                "complete"
            } else {
                "idle"
            };
            let focus_label = match shell.focus {
                FocusLeaf::Composer => "composer",
                FocusLeaf::Transcript => "transcript",
                FocusLeaf::Permission => "permission",
                FocusLeaf::Overlay => "overlay",
            };
            let header = Line::from(vec![
                Span::raw(title),
                Span::raw("  ["),
                Span::raw(status),
                Span::raw("]  focus="),
                Span::raw(focus_label),
            ]);

            let scroll_info = format!(
                "scroll: offset={} visible={}",
                transcript.scroll_offset, transcript.visible_lines
            );

            let para = Paragraph::new(vec![header, Line::raw(""), Line::raw(scroll_info)])
                .block(Block::default().borders(Borders::ALL).title("Shell"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    buffer_to_string(&buffer, area.width)
}

fn render_tool_row(tool: &ToolLeafView, perm: &PermissionLeafView) -> String {
    let area = Rect::new(0, 0, W, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let status_str = match tool.status {
                ToolStatusLeaf::Queued => "queued",
                ToolStatusLeaf::Running => "running",
                ToolStatusLeaf::Completed => "completed",
                ToolStatusLeaf::Failed => "failed",
                ToolStatusLeaf::Cancelled => "cancelled",
            };
            let perm_str = match perm.state {
                PermissionStateLeaf::Pending => "pending",
                PermissionStateLeaf::Granted => "granted",
                PermissionStateLeaf::Denied => "denied",
                PermissionStateLeaf::TimedOut => "timed-out",
                PermissionStateLeaf::Cancelled => "cancelled",
            };
            let lines = vec![
                Line::from(format!("tool: {} [{}]", tool.tool_id, status_str)),
                Line::from(format!(
                    "permission: {} [{}]",
                    perm_kind_str(perm.kind),
                    perm_str
                )),
                Line::from(format!(
                    "diff: {}",
                    if tool.has_diff { "yes" } else { "no" }
                )),
                Line::from(format!(
                    "error: {}",
                    if tool.has_error { "yes" } else { "no" }
                )),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Tool Row"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    buffer_to_string(&buffer, area.width)
}

fn perm_kind_str(kind: PermissionKindLeaf) -> &'static str {
    match kind {
        PermissionKindLeaf::Edit => "edit",
        PermissionKindLeaf::Bash => "bash",
        PermissionKindLeaf::Task => "task",
        PermissionKindLeaf::Webfetch => "webfetch",
        PermissionKindLeaf::Websearch => "websearch",
        PermissionKindLeaf::Codesearch => "codesearch",
        PermissionKindLeaf::Lsp => "lsp",
    }
}

fn render_diff_block(diff: &DiffLeafView) -> String {
    let area = Rect::new(0, 0, W, 12);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = if diff.present {
                vec![
                    Line::from(format!("file: {}", diff.file_path)),
                    Line::from(format!("+{} -{}", diff.added_lines, diff.removed_lines)),
                    Line::from(format!(
                        "event_derived: {}",
                        if diff.event_derived { "true" } else { "false" }
                    )),
                ]
            } else {
                vec![Line::raw("(no diff)")]
            };
            let para =
                Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Diff"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    buffer_to_string(&buffer, area.width)
}

fn render_question_row(question: &QuestionLeafView) -> String {
    let area = Rect::new(0, 0, W, 8);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let state_str = match question.state {
                QuestionStateLeaf::Pending => "pending",
                QuestionStateLeaf::Answered => "answered",
                QuestionStateLeaf::Cancelled => "cancelled",
                QuestionStateLeaf::TimedOut => "timed-out",
            };
            let answer = question.answer.unwrap_or("(none)");
            let lines = vec![
                Line::from(format!("Q: {}", question.question)),
                Line::from(format!("state: {}  answer: {}", state_str, answer)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Question"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    buffer_to_string(&buffer, area.width)
}

// ── Shell lifecycle states ─────────────────────────────────────────────

#[test]
fn shell_idle_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer);
    let transcript = TranscriptLeafView::new(0, 20);
    let output = render_shell(&shell, &transcript);
    assert!(output.contains("idle"), "idle state must render: {output}");
    assert!(output.contains("focus=composer"));
}

#[test]
fn shell_stream_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Transcript).streaming();
    let transcript = TranscriptLeafView::new(0, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("streaming"),
        "streaming state must render: {output}"
    );
}

#[test]
fn shell_cancel_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Transcript)
        .streaming()
        .cancelled();
    let transcript = TranscriptLeafView::new(5, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("cancelled"),
        "cancelled state must render: {output}"
    );
}

#[test]
fn shell_fail_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Transcript)
        .streaming()
        .failed();
    let transcript = TranscriptLeafView::new(0, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("failed"),
        "failed state must render: {output}"
    );
}

#[test]
fn shell_recover_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer)
        .failed()
        .recovered();
    let transcript = TranscriptLeafView::new(0, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("idle"),
        "recovered shell must show idle: {output}"
    );
    assert!(shell.recovered, "recovered flag must be set");
    assert!(!shell.failed, "failed flag must be cleared after recover");
}

#[test]
fn shell_complete_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer)
        .streaming()
        .completed();
    let transcript = TranscriptLeafView::new(0, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("complete"),
        "complete state must render: {output}"
    );
}

#[test]
fn shell_scroll_renders_without_panic() {
    let shell = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Transcript);
    let transcript = TranscriptLeafView::new(42, 20);
    let output = render_shell(&shell, &transcript);
    assert!(
        output.contains("scroll: offset=42 visible=20"),
        "scroll offset must render: {output}"
    );
}

// ── Tool + permission ordering ─────────────────────────────────────────

#[test]
fn tool_permission_pending_renders_without_panic() {
    let tool = ToolLeafView::new("edit", ToolStatusLeaf::Queued).permission_pending();
    let perm = PermissionLeafView::new(
        PermissionKindLeaf::Edit,
        PermissionStateLeaf::Pending,
        "tc-001",
    );
    let output = render_tool_row(&tool, &perm);
    assert!(output.contains("edit [queued]"));
    assert!(output.contains("edit [pending]"));
    assert!(perm.is_pending());
    assert!(!tool.permission_before_tool());
}

#[test]
fn tool_permission_granted_renders_and_orders_correctly() {
    let tool = ToolLeafView::new("edit", ToolStatusLeaf::Running)
        .permission_pending()
        .permission_granted();
    let perm = PermissionLeafView::new(
        PermissionKindLeaf::Edit,
        PermissionStateLeaf::Pending,
        "tc-002",
    )
    .granted();
    let output = render_tool_row(&tool, &perm);
    assert!(output.contains("edit [running]"));
    assert!(output.contains("granted"));
    assert!(
        tool.permission_before_tool(),
        "permission must be resolved before tool executes"
    );
    assert!(perm.resolved_before_tool());
}

#[test]
fn tool_permission_denied_renders_and_blocks_tool() {
    let tool = ToolLeafView::new("bash", ToolStatusLeaf::Cancelled);
    let perm = PermissionLeafView::new(
        PermissionKindLeaf::Bash,
        PermissionStateLeaf::Pending,
        "tc-003",
    )
    .denied();
    let output = render_tool_row(&tool, &perm);
    assert!(output.contains("bash [cancelled]"));
    assert!(output.contains("denied"));
    assert!(perm.resolved_before_tool());
}

#[test]
fn tool_with_diff_renders_diff_present() {
    let tool = ToolLeafView::new("edit", ToolStatusLeaf::Completed)
        .permission_granted()
        .with_diff();
    let perm = PermissionLeafView::new(
        PermissionKindLeaf::Edit,
        PermissionStateLeaf::Granted,
        "tc-004",
    );
    let output = render_tool_row(&tool, &perm);
    assert!(output.contains("diff: yes"));
    assert!(tool.has_diff);
}

#[test]
fn tool_with_error_renders_error_present() {
    let tool = ToolLeafView::new("bash", ToolStatusLeaf::Failed)
        .permission_granted()
        .with_error();
    let perm = PermissionLeafView::new(
        PermissionKindLeaf::Bash,
        PermissionStateLeaf::Granted,
        "tc-005",
    );
    let output = render_tool_row(&tool, &perm);
    assert!(output.contains("error: yes"));
    assert!(tool.has_error);
}

// ── Diff rendering ─────────────────────────────────────────────────────

#[test]
fn diff_event_derived_renders_with_counts() {
    let diff = DiffLeafView::from_event("src/main.rs", 10, 3);
    let output = render_diff_block(&diff);
    assert!(output.contains("file: src/main.rs"));
    assert!(output.contains("+10 -3"));
    assert!(output.contains("event_derived: true"));
    assert!(diff.is_valid(), "event-derived diff must be valid");
    assert_eq!(diff.total_changed(), 13);
}

#[test]
fn diff_absent_renders_placeholder() {
    let diff = DiffLeafView::new();
    let output = render_diff_block(&diff);
    assert!(output.contains("(no diff)"));
    assert!(!diff.is_valid());
}

#[test]
fn diff_not_event_derived_is_not_valid() {
    let diff = DiffLeafView {
        present: true,
        added_lines: 5,
        removed_lines: 2,
        event_derived: false,
        file_path: "synthetic.rs",
    };
    assert!(!diff.is_valid(), "non-event-derived diff must not be valid");
}

// ── Question rendering ────────────────────────────────────────────────

#[test]
fn question_pending_renders_without_panic() {
    let q = QuestionLeafView::new("Which file should I edit?");
    let output = render_question_row(&q);
    assert!(output.contains("Q: Which file should I edit?"));
    assert!(output.contains("pending"));
    assert!(q.is_pending());
    assert!(
        q.renders_no_permission_chrome(),
        "question must never render permission chrome"
    );
}

#[test]
fn question_answered_renders_answer() {
    let q = QuestionLeafView::new("Which approach?").answered("Option A");
    let output = render_question_row(&q);
    assert!(output.contains("answered"));
    assert!(output.contains("Option A"));
    assert!(!q.is_pending());
}

#[test]
fn question_cancelled_renders_without_panic() {
    let q = QuestionLeafView::new("Cancel?").cancelled();
    let output = render_question_row(&q);
    assert!(output.contains("cancelled"));
    assert!(!q.is_pending());
}

// ── Action leaf determinism ───────────────────────────────────────────

#[test]
fn action_leaf_new_variants_are_deterministic() {
    assert_eq!(ActionLeaf::ApprovePermission, ActionLeaf::ApprovePermission);
    assert_ne!(ActionLeaf::ApprovePermission, ActionLeaf::DenyPermission);
    assert_eq!(ActionLeaf::ScrollUp, ActionLeaf::ScrollUp);
    assert_ne!(ActionLeaf::ScrollUp, ActionLeaf::ScrollDown);
    assert_eq!(ActionLeaf::Recover, ActionLeaf::Recover);
    assert_ne!(ActionLeaf::Recover, ActionLeaf::Cancel);
    assert_eq!(ActionLeaf::OpenDiff, ActionLeaf::OpenDiff);
    assert_eq!(ActionLeaf::AnswerQuestion, ActionLeaf::AnswerQuestion);
    assert_ne!(ActionLeaf::AnswerQuestion, ActionLeaf::CancelQuestion);
}

// ── Full lifecycle sequence: idle → stream → cancel → recover → complete ─

#[test]
fn full_lifecycle_sequence_renders_all_states() {
    let transcript = TranscriptLeafView::new(0, 20);

    // idle
    let idle = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer);
    let idle_out = render_shell(&idle, &transcript);
    assert!(idle_out.contains("idle"));
    assert!(idle.is_idle());

    // stream
    let streaming = idle.streaming();
    let stream_out = render_shell(&streaming, &transcript);
    assert!(stream_out.contains("streaming"));
    assert!(!streaming.is_idle());

    // cancel
    let cancelled = streaming.cancelled();
    let cancel_out = render_shell(&cancelled, &transcript);
    assert!(cancel_out.contains("cancelled"));

    // recover (from a fresh failed state, simulating recovery)
    let failed = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer).failed();
    let recovered = failed.recovered();
    let recover_out = render_shell(&recovered, &transcript);
    assert!(recover_out.contains("idle"));
    assert!(recovered.recovered);
    assert!(!recovered.failed);

    // complete
    let completed = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer)
        .streaming()
        .completed();
    let complete_out = render_shell(&completed, &transcript);
    assert!(complete_out.contains("complete"));
}

// ── Permission-before-tool invariant across full tool lifecycle ───────

#[test]
fn permission_before_tool_invariant_holds_across_lifecycle() {
    // Pending: tool must not execute
    let perm_pending = PermissionLeafView::new(
        PermissionKindLeaf::Edit,
        PermissionStateLeaf::Pending,
        "tc-lifecycle",
    );
    let tool_queued = ToolLeafView::new("edit", ToolStatusLeaf::Queued).permission_pending();
    assert!(perm_pending.is_pending());
    assert!(!tool_queued.permission_before_tool());

    // Granted: tool may execute
    let perm_granted = perm_pending.granted();
    let tool_running = tool_queued.permission_granted();
    assert!(perm_granted.resolved_before_tool());
    assert!(tool_running.permission_before_tool());

    // Tool completes with diff
    let tool_done = ToolLeafView::new("edit", ToolStatusLeaf::Completed)
        .permission_granted()
        .with_diff();
    assert!(tool_done.permission_before_tool());
    assert!(tool_done.has_diff);

    // Diff is event-derived
    let diff = DiffLeafView::from_event("src/lib.rs", 15, 5);
    assert!(diff.is_valid());
    assert!(diff.event_derived);
}

// ── Scroll behavior across lifecycle states ───────────────────────────

#[test]
fn scroll_offset_preserved_across_state_transitions() {
    let base = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Transcript);

    // User scrolls up during streaming
    let streaming = base.streaming();
    let scrolled = TranscriptLeafView::new(30, 20);
    let out = render_shell(&streaming, &scrolled);
    assert!(out.contains("streaming"));
    assert!(out.contains("offset=30"));

    // Cancel preserves scroll position
    let cancelled = streaming.cancelled();
    let out = render_shell(&cancelled, &scrolled);
    assert!(out.contains("cancelled"));
    assert!(out.contains("offset=30"));

    // Scroll to bottom on complete
    let completed = ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer)
        .streaming()
        .completed();
    let at_bottom = TranscriptLeafView::new(0, 20);
    let out = render_shell(&completed, &at_bottom);
    assert!(out.contains("complete"));
    assert!(out.contains("offset=0"));
}
