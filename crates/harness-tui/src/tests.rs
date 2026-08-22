use crate::UnwrapOrAbort;
use std::sync::Mutex;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::app::{ActivityEntry, ActivityStatus, AppState, ToolCallDisplayStatus};
use super::lib_tests::{
    key_with_modifiers, render_live_buffer, render_live_cells, render_live_lines,
    row_text_and_palette, transcript_code_block_app, transcript_diff_block_app,
};
use super::session_events::load_session_events as load_events_from_run_dir;

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, RunFinishedEvent,
    RunStartedEvent, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use ratatui::style::Color;
use ratatui::{backend::TestBackend, Terminal};
use tempfile::TempDir;

#[cfg(test)]
#[path = "tests/snapshot_render_tests.rs"]
mod snapshot_render_tests;

#[test]
pub(super) fn module_replay_mode_snapshot_renders_two_pane_layout() {
    // arrange
    // act
    // assert
    snapshot_render_tests::module_replay_mode_snapshot_renders_two_pane_layout();
}

#[test]
fn replay_mode_r_key_reports_removed_reload() {
    // arrange
    // act
    // assert
    snapshot_render_tests::replay_mode_r_key_reports_removed_reload();
}

#[test]
fn live_mode_snapshot_renders_grouped_streams() {
    // arrange
    // act
    // assert
    snapshot_render_tests::live_mode_snapshot_renders_grouped_streams();
}

#[test]
fn live_mode_renders_activity_and_transcript() {
    // arrange
    // act
    // assert
    snapshot_render_tests::live_mode_renders_activity_and_transcript();
}

#[cfg(test)]
#[path = "tests/permission_modal_tests.rs"]
mod permission_modal_tests;

#[test]
fn permission_modal_snapshot_renders_request() {
    // arrange
    // act
    // assert
    permission_modal_tests::permission_modal_snapshot_renders_request();
}

#[test]
fn permission_dock_packs_measured_content_rows() {
    // arrange
    // act
    // assert
    permission_modal_tests::permission_dock_packs_measured_content_rows();
}

#[test]
fn question_permission_modal_renders_questions_and_answer_input() {
    // arrange
    // act
    // assert
    permission_modal_tests::question_permission_modal_renders_questions_and_answer_input();
}

#[test]
fn question_permission_modal_aligns_option_description_column() {
    // arrange
    // act
    // assert
    permission_modal_tests::question_permission_modal_aligns_option_description_column();
}

#[test]
fn answered_questions_render_in_completed_tool_row() {
    // arrange
    // act
    // assert
    permission_modal_tests::answered_questions_render_in_completed_tool_row();
}

#[test]
fn permission_modal_ctrl_y_emits_resolve_intent_and_closes_on_resolved() {
    // arrange
    // act
    // assert
    permission_modal_tests::permission_modal_ctrl_y_emits_resolve_intent_and_closes_on_resolved();
}

#[cfg(test)]
#[path = "tests/transcript_render_tests.rs"]
mod transcript_render_tests;

#[test]
pub(super) fn module_transcript_edit_snapshot_renders_inline_diff() {
    // arrange
    // act
    // assert
    transcript_render_tests::module_transcript_edit_snapshot_renders_inline_diff();
}

#[test]
pub(super) fn module_inline_diff_does_not_leave_large_gap_before_active_footer() {
    // arrange
    // act
    // assert
    transcript_render_tests::module_inline_diff_does_not_leave_large_gap_before_active_footer();
}

pub(super) fn module_fenced_code_highlighting_uses_syntect_styles_for_known_languages() {
    transcript_render_tests::module_fenced_code_highlighting_uses_syntect_styles_for_known_languages();
}

pub(super) fn module_fenced_code_highlighting_falls_back_to_plain_text_when_unknown() {
    transcript_render_tests::module_fenced_code_highlighting_falls_back_to_plain_text_when_unknown(
    );
}

pub(super) fn module_diff_renderer_uses_stacked_layout_in_narrow_geometries() {
    transcript_render_tests::module_diff_renderer_uses_stacked_layout_in_narrow_geometries();
}

pub(super) fn module_wide_diff_renderer_pairs_before_and_after_columns() {
    transcript_render_tests::module_wide_diff_renderer_pairs_before_and_after_columns();
}

#[test]
pub(super) fn module_diff_renderer_switches_to_side_by_side_at_primary_widths() {
    // arrange
    // act
    // assert
    transcript_render_tests::module_diff_renderer_switches_to_side_by_side_at_primary_widths();
}

pub(super) fn module_transcript_edit_tool_wide_diff_uses_syntax_highlighting_and_split_palettes() {
    transcript_render_tests::module_transcript_edit_tool_wide_diff_uses_syntax_highlighting_and_split_palettes();
}

#[test]
fn transcript_edit_snapshot_handles_missing_artifact() {
    // arrange
    // act
    // assert
    transcript_render_tests::transcript_edit_snapshot_handles_missing_artifact();
}

#[cfg(test)]
#[path = "tests/activity_projection_tests.rs"]
mod activity_projection_tests;

#[test]
fn prompt_focus_enter_emits_submit_intent() {
    // arrange
    // act
    // assert
    activity_projection_tests::prompt_focus_enter_emits_submit_intent();
}

#[test]
fn activity_groups_by_request_id() {
    // arrange
    // act
    // assert
    activity_projection_tests::activity_groups_by_request_id();
}

#[test]
fn transcript_accumulates_stream_deltas() {
    // arrange
    // act
    // assert
    activity_projection_tests::transcript_accumulates_stream_deltas();
}

#[test]
fn activity_status_done_on_request_finished() {
    // arrange
    // act
    // assert
    activity_projection_tests::activity_status_done_on_request_finished();
}

#[test]
fn activity_status_error_on_run_failed() {
    // arrange
    // act
    // assert
    activity_projection_tests::activity_status_error_on_run_failed();
}

#[test]
fn memory_cap_enforces_max_events() {
    // arrange
    // act
    // assert
    activity_projection_tests::memory_cap_enforces_max_events();
}

#[test]
fn memory_cap_enforces_max_transcript_chars() {
    // arrange
    // act
    // assert
    activity_projection_tests::memory_cap_enforces_max_transcript_chars();
}

#[test]
fn run_workspace_renders_activity_with_compact_format() {
    // arrange
    // act
    // assert
    activity_projection_tests::run_workspace_renders_activity_with_compact_format();
}

#[test]
fn tool_call_requested_renders_pending_status() {
    // arrange
    // act
    // assert
    activity_projection_tests::tool_call_requested_renders_pending_status();
}

#[test]
fn tool_call_started_renders_running_status() {
    // arrange
    // act
    // assert
    activity_projection_tests::tool_call_started_renders_running_status();
}

#[test]
fn tool_call_finished_renders_truncated_output() {
    // arrange
    // act
    // assert
    activity_projection_tests::tool_call_finished_renders_truncated_output();
}

#[test]
fn tool_call_failed_renders_error() {
    // arrange
    // act
    // assert
    activity_projection_tests::tool_call_failed_renders_error();
}

#[test]
fn assistant_markdown_renders_headings_lists_and_quotes() {
    // arrange
    // act
    // assert
    transcript_render_tests::assistant_markdown_renders_headings_lists_and_quotes();
}

#[test]
fn block_style_tool_rows_render_titles_and_argument_blocks() {
    // arrange
    // act
    // assert
    transcript_render_tests::block_style_tool_rows_render_titles_and_argument_blocks();
}

#[test]
fn generic_tool_output_toggle_reveals_block_payload() {
    // arrange
    // act
    // assert
    transcript_render_tests::generic_tool_output_toggle_reveals_block_payload();
}

#[test]
fn task_scheduled_queued_does_not_reuse_tool_call_id_as_task_id() {
    // arrange
    // act
    // assert
    activity_projection_tests::task_scheduled_queued_does_not_reuse_tool_call_id_as_task_id();
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sample_replay_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "replay-run".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn sample_live_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "live-run".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_1"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_1".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "summarized prompt".to_string(),
                request_digest: "digest-req-1".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some("req_1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_1".into(),
                delta: "hello ".to_string(),
            }),
        ),
        envelope(
            4,
            Some("req_1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_1".into(),
                delta: "world".to_string(),
            }),
        ),
        envelope(
            5,
            Some("req_1"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        permission_requested_event(6, "perm_1", "tool_call_1"),
        permission_resolved_event(7, "perm_1", PermissionDecision::Allow),
        envelope(
            8,
            Some("tool_call_1"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_1".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest-new-file".to_string(),
                diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
                diff_digest: Some("diff-digest".to_string()),
            }),
        ),
        envelope(
            9,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn sample_tool_spacing_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "tool-spacing".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_spacing"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_spacing".into(),
                text: "Match harness tool spacing".to_string(),
            }),
        ),
        envelope(
            3,
            Some("req_spacing"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_spacing".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Match harness tool spacing".to_string(),
                request_digest: "digest-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read_spacing".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui_transcript.rs"}"#.to_string(),
                args_digest: "digest-read-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read_spacing".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read".to_string()),
                output_digest: Some("digest-read-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            6,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_glob_spacing".into(),
                tool_id: "fs.glob".to_string(),
                args_summary: r#"{"pattern":"src/**/*.rs","path":"."}"#.to_string(),
                args_digest: "digest-glob-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            7,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_glob_spacing".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("4 files".to_string()),
                output_digest: Some("digest-glob-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            8,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_grep_spacing".into(),
                tool_id: "fs.grep".to_string(),
                args_summary: r#"{"pattern":"tool spacing","include":"*.rs","path":"src"}"#
                    .to_string(),
                args_digest: "digest-grep-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            9,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_grep_spacing".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("3 matches".to_string()),
                output_digest: Some("digest-grep-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            10,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_list_spacing".into(),
                tool_id: "fs.ls".to_string(),
                args_summary: r#"{"path":"src"}"#.to_string(),
                args_digest: "digest-list-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            11,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_list_spacing".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ui_transcript.rs\nlayout.rs".to_string()),
                output_digest: Some("digest-list-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            12,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell_spacing".into(),
                tool_id: "shell.run".to_string(),
                args_summary:
                    r#"{"cmd":"cargo test -p harness-tui ui::ui_transcript::","cwd":"/tmp/demo"}"#
                        .to_string(),
                args_digest: "digest-shell-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            13,
            Some("req_spacing"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_spacing".into(),
            }),
        ),
        envelope(
            14,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell_spacing".into(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: snapshot mismatch".to_string()),
                output_digest: Some("digest-shell-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            15,
            Some("req_spacing"),
            EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_spacing".into(),
                delta:
                    "The grouped context rows need to stay compact before the visible shell block."
                        .to_string(),
            }),
        ),
        envelope(
            16,
            Some("req_spacing"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_spacing".into(),
                delta: "I matched the body-to-tool and tool-to-body spacing to the harness shell."
                    .to_string(),
            }),
        ),
        envelope(
            17,
            Some("req_spacing"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_spacing".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-spacing-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            18,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
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
            request_digest: "digest-perm".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: PermissionDecision,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some("tool_call_1"),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: permission_id.to_string(),
            decision: match decision {
                PermissionDecision::Allow => harness_core::event::PermissionDecision::Allow,
                PermissionDecision::Deny => harness_core::event::PermissionDecision::Deny,
            },
            reason: Some("resolved in test".to_string()),
        }),
    )
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        correlation_id,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        payload,
    )
}

fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

fn write_replay_fixture(events: Vec<EventEnvelopeV1>) -> TempDir {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    write_events_jsonl(run_dir.path(), &events);
    run_dir
}

fn write_diff_fixture(with_diff_file: bool) -> TempDir {
    let run_dir = tempfile::tempdir().unwrap_or_abort();

    if with_diff_file {
        let artifacts_dir = run_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
        fs::write(
            artifacts_dir.join("edit-edit-golden-path.diff"),
            "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n",
        )
        .unwrap_or_abort();
    }

    let events = vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "diff-fixture".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_diff_1"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_diff_1".into(),
                text: "Show me the changes inline".to_string(),
            }),
        ),
        envelope(
            3,
            Some("req_diff_1"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_diff_1".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Show me the changes inline".to_string(),
                request_digest: "digest-inline-diff-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some("req_diff_1"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_call_1".into(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"demo.txt"}"#.to_string(),
                args_digest: "digest-inline-diff-tool".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            Some("tool_call_1"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_1".into(),
            }),
        ),
        envelope(
            6,
            Some("tool_call_1"),
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit-golden-path".to_string(),
                path: "demo.txt".to_string(),
                summary: "Replace beta with BETA".to_string(),
                patch_digest: "digest-inline-diff-patch".to_string(),
            }),
        ),
        envelope(
            7,
            Some("tool_call_1"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit-golden-path".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest".to_string(),
                diff_rel_path: Some("artifacts/edit-edit-golden-path.diff".to_string()),
                diff_digest: Some("digest-diff".to_string()),
            }),
        ),
        envelope(
            8,
            Some("tool_call_1"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_call_1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Edit applied".to_string()),
                output_digest: Some("digest-inline-diff-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            Some("req_diff_1"),
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    write_events_jsonl(run_dir.path(), &events);
    run_dir
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn assert_buffer_snapshot(name: &str, buffer: &ratatui::buffer::Buffer) {
    let normalized = normalize_temp_paths(&format!("{buffer:#?}"));
    insta::assert_snapshot!(name, normalized);
}

fn normalize_temp_paths(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"/tmp/.tmp") {
            output.push_str("/tmp/TMPDIR");
            index += b"/tmp/.tmp".len();
            while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
                index += 1;
            }
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }

    output
}
