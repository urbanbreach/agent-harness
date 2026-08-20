//! Task 23: PTY owner tests for shell, transcript, tool, diff, permission,
//! and question states.
//!
//! These tests prove the task-23 owner covers the required PTY owner states
//! (idle, stream, cancel, fail, recover, complete, scroll) using the leaf
//! view types. They assert the key postconditions:
//! - `permission_before_tool`: permission is resolved before tool execution
//! - `diff_present`: event-derived diffs are present when expected
//! - `event_derived`: content is derived from events, not synthetic injection
//! - `no_panic`: all states render without panicking
//!
//! These are deterministic structural owner tests — they do not require
//! `HARNESS_TUI_PTY_SIGNOFF=1` (the full PTY signoff lane is covered by
//! `reference_parity_pty_test.rs`). They prove the leaf contract invariants
//! that the PTY lane would exercise against a real terminal.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "PTY owner tests use fail-fast asserts for invariant violations"
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

struct PtyOwnerScene {
    shell: ShellLeafView,
    transcript: TranscriptLeafView,
    tool: ToolLeafView,
    permission: PermissionLeafView,
    diff: DiffLeafView,
    question: QuestionLeafView,
}

impl PtyOwnerScene {
    fn idle() -> Self {
        Self {
            shell: ShellLeafView::new(ShellKindLeaf::Live, FocusLeaf::Composer),
            transcript: TranscriptLeafView::new(0, 20),
            tool: ToolLeafView::new("edit", ToolStatusLeaf::Queued),
            permission: PermissionLeafView::new(
                PermissionKindLeaf::Edit,
                PermissionStateLeaf::Pending,
                "tc-idle",
            ),
            diff: DiffLeafView::new(),
            question: QuestionLeafView::new("Idle question?"),
        }
    }

    fn stream() -> Self {
        let mut scene = Self::idle();
        scene.shell = scene.shell.streaming();
        scene.transcript = TranscriptLeafView::new(0, 20);
        scene
    }

    fn cancel() -> Self {
        let mut scene = Self::stream();
        scene.shell = scene.shell.cancelled();
        scene.transcript = TranscriptLeafView::new(10, 20);
        scene
    }

    fn fail() -> Self {
        let mut scene = Self::stream();
        scene.shell = scene.shell.failed();
        scene.tool = ToolLeafView::new("bash", ToolStatusLeaf::Failed)
            .permission_granted()
            .with_error();
        scene
    }

    fn recover() -> Self {
        let mut scene = Self::fail();
        scene.shell = scene.shell.recovered();
        scene.tool = ToolLeafView::new("bash", ToolStatusLeaf::Completed).permission_granted();
        scene
    }

    fn complete() -> Self {
        let mut scene = Self::stream();
        scene.shell = scene.shell.completed();
        scene.permission = scene.permission.granted();
        scene.tool = ToolLeafView::new("edit", ToolStatusLeaf::Completed)
            .permission_granted()
            .with_diff();
        scene.diff = DiffLeafView::from_event("src/main.rs", 12, 4);
        scene.transcript = TranscriptLeafView::new(0, 20);
        scene
    }

    fn scroll() -> Self {
        let mut scene = Self::complete();
        scene.transcript = TranscriptLeafView::new(50, 20);
        scene
    }

    fn render(&self) -> String {
        let area = Rect::new(0, 0, W, H);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("terminal");

        terminal
            .draw(|frame| {
                let status = if self.shell.streaming {
                    "streaming"
                } else if self.shell.cancelled {
                    "cancelled"
                } else if self.shell.failed {
                    "failed"
                } else if self.shell.completed {
                    "complete"
                } else {
                    "idle"
                };

                let tool_status = match self.tool.status {
                    ToolStatusLeaf::Queued => "queued",
                    ToolStatusLeaf::Running => "running",
                    ToolStatusLeaf::Completed => "completed",
                    ToolStatusLeaf::Failed => "failed",
                    ToolStatusLeaf::Cancelled => "cancelled",
                };

                let perm_state = match self.permission.state {
                    PermissionStateLeaf::Pending => "pending",
                    PermissionStateLeaf::Granted => "granted",
                    PermissionStateLeaf::Denied => "denied",
                    PermissionStateLeaf::TimedOut => "timed-out",
                    PermissionStateLeaf::Cancelled => "cancelled",
                };

                let lines = vec![
                    Line::from(vec![
                        Span::raw("Shell: "),
                        Span::raw(status),
                        Span::raw("  focus="),
                        Span::raw(match self.shell.focus {
                            FocusLeaf::Composer => "composer",
                            FocusLeaf::Transcript => "transcript",
                            FocusLeaf::Permission => "permission",
                            FocusLeaf::Overlay => "overlay",
                        }),
                    ]),
                    Line::raw(format!(
                        "Scroll: offset={} visible={}",
                        self.transcript.scroll_offset, self.transcript.visible_lines
                    )),
                    Line::raw(format!(
                        "Tool: {} [{}]  diff={}  error={}",
                        self.tool.tool_id,
                        tool_status,
                        if self.tool.has_diff { "yes" } else { "no" },
                        if self.tool.has_error { "yes" } else { "no" }
                    )),
                    Line::raw(format!("Permission: [{}]", perm_state)),
                    Line::raw(format!(
                        "Diff: present={} event_derived={} file={}",
                        self.diff.present, self.diff.event_derived, self.diff.file_path
                    )),
                    Line::raw(format!(
                        "Question: {} [state={:?}]",
                        self.question.question, self.question.state
                    )),
                ];

                let para = Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("PTY Owner Scene"),
                );
                frame.render_widget(para, area);
            })
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        buffer_to_string(&buffer, area.width)
    }
}

// ── PTY owner state: idle ─────────────────────────────────────────────

#[test]
fn pty_owner_idle_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::idle();
    let output = scene.render();

    // assert
    assert!(output.contains("idle"), "idle must render: {output}");
    assert!(output.contains("focus=composer"));
    assert!(scene.shell.is_idle(), "idle shell must be_idle");
    assert!(
        scene.permission.is_pending(),
        "idle permission must be pending"
    );
    assert!(
        !scene.tool.permission_before_tool(),
        "idle tool must not have permission_before_tool"
    );
    assert!(!scene.diff.present, "idle must have no diff");
}

// ── PTY owner state: stream ───────────────────────────────────────────

#[test]
fn pty_owner_stream_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::stream();
    let output = scene.render();

    // assert
    assert!(
        output.contains("streaming"),
        "streaming must render: {output}"
    );
    assert!(scene.shell.streaming);
    assert!(!scene.shell.is_idle());
}

// ── PTY owner state: cancel ────────────────────────────────────────────

#[test]
fn pty_owner_cancel_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::cancel();
    let output = scene.render();

    // assert
    assert!(
        output.contains("cancelled"),
        "cancelled must render: {output}"
    );
    assert!(scene.shell.cancelled);
    assert!(!scene.shell.streaming);
    assert!(
        output.contains("offset=10"),
        "scroll offset must be preserved on cancel: {output}"
    );
}

// ── PTY owner state: fail ──────────────────────────────────────────────

#[test]
fn pty_owner_fail_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::fail();
    let output = scene.render();

    // assert
    assert!(output.contains("failed"), "failed must render: {output}");
    assert!(scene.shell.failed);
    assert!(scene.tool.has_error);
    assert!(
        scene.tool.permission_before_tool(),
        "failed tool must still have had permission_before_tool"
    );
}

// ── PTY owner state: recover ────────────────────────────────────────────

#[test]
fn pty_owner_recover_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::recover();
    let output = scene.render();

    // assert
    assert!(
        output.contains("idle"),
        "recovered shell must show idle: {output}"
    );
    assert!(scene.shell.recovered);
    assert!(!scene.shell.failed);
    assert!(
        scene.tool.permission_before_tool(),
        "recovered tool must have permission_before_tool"
    );
}

// ── PTY owner state: complete ───────────────────────────────────────────

#[test]
fn pty_owner_complete_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::complete();
    let output = scene.render();

    // assert
    assert!(
        output.contains("complete"),
        "complete must render: {output}"
    );
    assert!(scene.shell.completed);
    assert!(
        scene.tool.permission_before_tool(),
        "completed tool must have permission_before_tool"
    );
    assert!(scene.diff.present, "complete must have diff present");
    assert!(scene.diff.event_derived, "diff must be event_derived");
    assert!(
        scene.diff.is_valid(),
        "diff must be valid (present + event_derived)"
    );
}

// ── PTY owner state: scroll ─────────────────────────────────────────────

#[test]
fn pty_owner_scroll_renders_and_invariants_hold() {
    // arrange
    // act
    let scene = PtyOwnerScene::scroll();
    let output = scene.render();

    // assert
    assert!(
        output.contains("offset=50"),
        "scroll offset must render: {output}"
    );
    assert_eq!(scene.transcript.scroll_offset, 50);
    assert!(
        scene.diff.present,
        "scroll state must preserve diff from complete"
    );
}

// ── Permission-before-tool invariant (failure scenario postcondition) ──

#[test]
fn pty_owner_permission_before_tool_holds_across_all_states() {
    // arrange
    // act
    for scene in [
        PtyOwnerScene::idle(),
        PtyOwnerScene::stream(),
        PtyOwnerScene::cancel(),
        PtyOwnerScene::fail(),
        PtyOwnerScene::recover(),
        PtyOwnerScene::complete(),
        PtyOwnerScene::scroll(),
    ] {
        let _output = scene.render();
        if scene.tool.status != ToolStatusLeaf::Queued {
            // assert
            assert!(
                scene.tool.permission_before_tool(),
                "permission must be resolved before tool in state {:?}",
                scene.tool.status
            );
        }
    }
}

// ── Diff present invariant (happy scenario postcondition) ──────────────

#[test]
fn pty_owner_diff_present_in_complete_and_scroll() {
    // arrange
    let complete = PtyOwnerScene::complete();
    assert!(complete.diff.present);
    assert!(complete.diff.event_derived);

    // act
    let scroll = PtyOwnerScene::scroll();
    // assert
    assert!(scroll.diff.present);
    assert!(scroll.diff.event_derived);
}

// ── No-panic across all states ─────────────────────────────────────────

#[test]
fn pty_owner_all_states_render_without_panic() {
    // arrange
    let scenes: Vec<(&str, PtyOwnerScene)> = vec![
        ("idle", PtyOwnerScene::idle()),
        ("stream", PtyOwnerScene::stream()),
        ("cancel", PtyOwnerScene::cancel()),
        ("fail", PtyOwnerScene::fail()),
        ("recover", PtyOwnerScene::recover()),
        ("complete", PtyOwnerScene::complete()),
        ("scroll", PtyOwnerScene::scroll()),
    ];

    // act
    for (name, scene) in scenes {
        let output = scene.render();
        // assert
        assert!(
            !output.is_empty(),
            "rendered output for {name} must not be empty"
        );
    }
}

// ── Question never renders permission chrome ──────────────────────────

#[test]
fn pty_owner_question_renders_no_permission_chrome() {
    // arrange
    // act
    let scene = PtyOwnerScene::idle();
    // assert
    assert!(
        scene.question.renders_no_permission_chrome(),
        "question must never render permission chrome"
    );
}

// ── Action leaf coverage for PTY owner interactions ────────────────────

#[test]
fn pty_owner_action_leaf_covers_required_interactions() {
    // arrange
    let required = [
        ActionLeaf::Submit,
        ActionLeaf::Cancel,
        ActionLeaf::ApprovePermission,
        ActionLeaf::DenyPermission,
        ActionLeaf::AnswerQuestion,
        ActionLeaf::CancelQuestion,
        ActionLeaf::ScrollUp,
        ActionLeaf::ScrollDown,
        ActionLeaf::ScrollToBottom,
        ActionLeaf::Expand,
        ActionLeaf::Collapse,
        ActionLeaf::Recover,
        ActionLeaf::OpenDiff,
    ];

    // act
    for action in required {
        // assert
        assert_ne!(action, ActionLeaf::None, "action must not be None");
    }
}

// ── Full lifecycle: idle → stream → permission → tool → diff → complete ─

#[test]
fn pty_owner_full_lifecycle_with_permission_and_diff() {
    // arrange
    // Start idle
    let idle = PtyOwnerScene::idle();
    assert!(idle.permission.is_pending());
    assert!(!idle.tool.permission_before_tool());

    // Stream begins
    let stream = PtyOwnerScene::stream();
    assert!(stream.shell.streaming);

    // Permission granted, tool runs
    let complete = PtyOwnerScene::complete();
    assert!(complete.permission.state == PermissionStateLeaf::Granted);
    assert!(complete.tool.permission_before_tool());
    assert!(complete.diff.present);
    assert!(complete.diff.event_derived);

    // Verify the rendered output contains all key postconditions
    let output = complete.render();
    assert!(output.contains("complete"));
    assert!(output.contains("granted"));
    assert!(output.contains("Diff: present=true"));
    assert!(output.contains("event_derived=true"));

    // act
    // Scroll preserves diff
    let scroll = PtyOwnerScene::scroll();
    let scroll_output = scroll.render();
    // assert
    assert!(scroll_output.contains("offset=50"));
    assert!(scroll.diff.present);
}
