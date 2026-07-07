use super::*;
use crate::layout::FrameLayoutPlan;
use crate::overlay::OverlayKind;
use crate::theme::Theme;
use crate::ui::{
    render_app, reset_transcript_selection_cache_metrics_for_test, subagent_footer_target_at,
    transcript_mouse_target, transcript_selection_cache_build_count_for_test,
    transcript_selection_cell, transcript_selection_debug_snapshot, SubagentFooterTarget,
    TranscriptMouseTarget, TranscriptScrollbarHit, WheelTarget,
};
use crate::UnwrapOrAbort;
use crossterm::event::{MouseButton, MouseEvent};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, CompactionWrittenEvent, EditAppliedEvent, EventActor,
    EventEnvelopeV1, EventV1, ExecutionTimingMetadata, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent,
    ProviderRequestFinishedMetadata, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent,
    TaskTerminalScope, ToolCallFinishedEvent, ToolCallLifecycleState, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::proj::inspect_resume_plan;
use harness_providers::ProviderErrorCategory;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::{backend::TestBackend, Terminal};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const TEST_FRAME_AREA: Rect = Rect::new(0, 0, 140, 40);

struct ClipboardModeGuard;

impl ClipboardModeGuard {
    fn disabled_copy_on_select() -> Self {
        crate::clipboard::set_copy_on_select_disabled_override(Some(true));
        Self
    }
}

impl Drop for ClipboardModeGuard {
    fn drop(&mut self) {
        crate::clipboard::set_copy_override(None);
        crate::clipboard::set_copy_on_select_disabled_override(None);
    }
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_app_{seq:04}"),
        seq,
        run_id: "run_app_tests".to_string(),
        mono_ms: seq,
        ts: Some("2026-02-03T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("app-tests".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    request_id: &str,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    let mut event = envelope(seq, request_id, payload);
    event.actor = actor;
    event
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn render_debug(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    format!("{:?}", terminal.backend().buffer())
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! delegate_test {
    ($name:ident => $target:path) => {
        #[test]
        fn $name() {
            $target();
        }
    };
}

#[cfg(test)]
#[path = "tests/toggles_menu_tests.rs"]
mod toggles_menu_tests;

delegate_test!(toggles_slash_command_opens_command_styled_menu => toggles_menu_tests::toggles_slash_command_opens_command_styled_menu);
delegate_test!(yolo_toggle_requires_confirmation_and_enables_entries => toggles_menu_tests::yolo_toggle_requires_confirmation_and_enables_entries);
delegate_test!(toggles_config_preserves_launch_metadata_entries => toggles_menu_tests::toggles_config_preserves_launch_metadata_entries);
delegate_test!(toggles_menu_sanitizes_config_derived_text => toggles_menu_tests::toggles_menu_sanitizes_config_derived_text);

#[test]
fn compaction_written_status_surfaces_deterministic_fallback() {
    // arrange
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "compaction:agent_000001",
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: "checkpoint_000001".to_string(),
            agent_id: "agent_000001".to_string(),
            artifact_path: "artifacts/compactions/agent_000001/checkpoint_000001.json".to_string(),
            artifact_digest: Some("digest-checkpoint".to_string()),
            artifact_bytes: 123,
            trigger_reason: "manual".to_string(),
            through_seq: 10,
            through_request_id: Some("req_000001".to_string()),
            provider_id: Some("mock".to_string()),
            model_id: Some("model-1".to_string()),
            tokens_before: Some(1000),
            tokens_before_estimate: Some(980),
            tokens_after_estimate: Some(400),
            summary_tokens_estimate: Some(80),
            compacted_turns: Some(3),
            reduction_tokens_estimate: Some(580),
            reduction_percent_estimate: Some(59),
            estimate_source: Some("provider_usage".to_string()),
            summary_source: Some(harness_core::agent::ProviderCompactionSummarySource {
                strategy: "model_backed_deterministic_fallback".to_string(),
                model_ref: "mock:model-1".to_string(),
                provider_id: Some("mock".to_string()),
                model_id: Some("model-1".to_string()),
                reasoning_effort: None,
                text_verbosity: None,
                previous_summary_used: false,
                model_backed: true,
                deterministic_fallback: true,
                summary_contract_version: Some(1),
                summary_contract_enforced: Some(true),
            }),
            preserved_turns: 1,
        }),
    ));

    // act
    let status = app.compaction_status().unwrap_or_abort();
    // assert
    assert_eq!(status.state, CompactionState::Written);
    assert!(status.message.contains("deterministic fallback"));
}

fn transcript_click_position(app: &AppState, needle: &str) -> (u16, u16) {
    transcript_click_position_in_area(app, TEST_FRAME_AREA, needle)
}

fn transcript_click_position_in_area(app: &AppState, area: Rect, needle: &str) -> (u16, u16) {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();

    for y in 0..area.height {
        let row = (0..area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (u16::try_from(column + 1).unwrap_or_abort(), y);
        }
    }

    panic!("expected row containing {needle:?}");
}

fn rendered_cell_bg(app: &AppState, column: u16, row: u16) -> Color {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer()[(column, row)].bg
}

fn default_navigation_keybindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "session_child_first".to_string(),
            "<leader>down".to_string(),
        ),
        ("session_child_cycle".to_string(), "right".to_string()),
        (
            "session_child_cycle_reverse".to_string(),
            "left".to_string(),
        ),
        ("session_parent".to_string(), "up".to_string()),
        ("session_background".to_string(), "ctrl+b".to_string()),
        ("variant_cycle".to_string(), "tab".to_string()),
    ])
}

#[test]
fn session_background_emits_intent_from_default_prompt_focus() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::new()));
    let captured_intents = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            captured_intents.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.apply_keybindings(default_navigation_keybindings());
    assert_eq!(app.focus, Focus::Prompt);

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert_eq!(
        app.status_banner.as_deref(),
        Some("foreground subagent backgrounding requested")
    );
    assert!(matches!(
        intents.lock().unwrap_or_abort().as_slice(),
        [UiIntent::BackgroundForegroundSubagents]
    ));
}

#[cfg(test)]
#[path = "tests/tool_disclosure_tests.rs"]
mod tool_disclosure_tests;

delegate_test!(mouse_click_toggles_transcript_tool_disclosure => tool_disclosure_tests::mouse_click_toggles_transcript_tool_disclosure);
delegate_test!(mouse_click_toggles_apply_patch_file_disclosure => tool_disclosure_tests::mouse_click_toggles_apply_patch_file_disclosure);
delegate_test!(apply_patch_default_expansion_skips_deleted_files => tool_disclosure_tests::apply_patch_default_expansion_skips_deleted_files);

#[cfg(test)]
#[path = "tests/subagent_navigation_tests.rs"]
mod subagent_navigation_tests;

delegate_test!(mouse_click_on_task_inline_row_opens_subagent_session => subagent_navigation_tests::keyboard_mouse_click_on_task_inline_row_opens_subagent_session);
delegate_test!(keyboard_sidebar_subagent_selection_opens_child_session => subagent_navigation_tests::keyboard_keyboard_sidebar_subagent_selection_opens_child_session);
delegate_test!(live_subagent_hitbox_uses_rendered_transcript_area => subagent_navigation_tests::keyboard_live_subagent_hitbox_uses_rendered_transcript_area);
delegate_test!(disk_backed_child_navigation_stays_in_live_tui_stack => subagent_navigation_tests::keyboard_disk_backed_child_navigation_stays_in_live_tui_stack);
delegate_test!(mouse_click_on_task_inline_row_uses_task_row_child_session => subagent_navigation_tests::mouse_click_on_task_inline_row_uses_task_row_child_session);
delegate_test!(mouse_up_on_completed_general_task_row_opens_child_session => subagent_navigation_tests::mouse_up_on_completed_general_task_row_opens_child_session);
delegate_test!(mouse_click_on_task_row_uses_harness_session_metadata => subagent_navigation_tests::mouse_click_on_task_row_uses_harness_session_metadata);
delegate_test!(slash_exit_from_inline_subagent_restores_parent_before_quit => subagent_navigation_tests::slash_exit_from_inline_subagent_restores_parent_before_quit);

#[cfg(test)]
#[path = "tests/subagent_footer_navigation_tests.rs"]
mod subagent_footer_navigation_tests;

delegate_test!(subagent_footer_hover_elevates_parent_target => subagent_footer_navigation_tests::subagent_footer_hover_elevates_parent_target);
delegate_test!(subagent_footer_parent_click_restores_parent_session => subagent_footer_navigation_tests::subagent_footer_parent_click_restores_parent_session);
delegate_test!(subagent_footer_sibling_clicks_switch_between_children => subagent_footer_navigation_tests::subagent_footer_sibling_clicks_switch_between_children);
delegate_test!(subagent_footer_scrollbar_drag_release_does_not_navigate => subagent_footer_navigation_tests::subagent_footer_scrollbar_drag_release_does_not_navigate);
delegate_test!(subagent_footer_up_only_release_does_not_activate => subagent_footer_navigation_tests::subagent_footer_up_only_release_does_not_activate);

#[cfg(test)]
#[path = "tests/opencode_subagent_parity_apps.rs"]
mod opencode_subagent_parity_apps;

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).unwrap_or_abort();
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn transcript_selection_test_app_with_text(transcript_text: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "req_copy_select".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(UserMessageSubmittedEvent {
            request_id: "req_copy_select".to_string(),
            text: "Select this".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: transcript_text.to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;
    app
}

fn transcript_selection_test_app_with_reasoning(
    thinking_text: &str,
    transcript_text: &str,
) -> AppState {
    let mut app = transcript_selection_test_app_with_text(transcript_text);
    app.activities[0].thinking_text = thinking_text.to_string();
    app
}

fn transcript_selection_test_app() -> AppState {
    transcript_selection_test_app_with_text("Copy this exact reply")
}

fn shell_card_selection_test_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_shell_card_copy",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_shell_card_copy".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-shell".to_string(),
            prompt_summary: "shell card copy".to_string(),
            request_digest: "digest-shell-card-copy".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_shell_card_copy",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_shell_card_copy".to_string(),
            tool_id: "bash".to_string(),
            args_summary:
                r#"{"command":"run-copy-command","description":"Run copy-safe shell card"}"#
                    .to_string(),
            args_digest: "digest-shell-card-copy-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_shell_card_copy",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_shell_card_copy".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_shell_card_copy",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell_card_copy".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("copy target output".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "command": "run-copy-command",
                "stdout": "copy target output\n",
                "stderr": "",
                "status": 0,
                "success": true,
            })),
            metadata: None,
        }),
    ));
    app.transcript_view.selected_activity_index = 0;
    app
}

fn operator_sidebar_selection_test_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_sidebar_copy",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_sidebar_copy".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-sidebar".to_string(),
            prompt_summary: "sidebar copy".to_string(),
            request_digest: "digest-sidebar-copy".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_sidebar_copy",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_sidebar_todo".to_string(),
            tool_id: "todo.write".to_string(),
            args_summary: "update todo list".to_string(),
            args_digest: "digest-sidebar-todo-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_sidebar_copy",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_sidebar_todo".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("todo list updated".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "todos": [
                    {"content": "Copy sidebar task", "status": "in_progress", "priority": "high"},
                    {"content": "Keep existing sidebar clicks", "status": "pending", "priority": "medium"}
                ]
            })),
            metadata: None,
        }),
    ));
    app
}

fn transcript_selection_text_position(app: &AppState, needle: &str) -> (u16, u16) {
    let snapshot = transcript_selection_debug_snapshot(app, TEST_FRAME_AREA).unwrap_or_abort();
    for (row_idx, row) in snapshot.rows.iter().enumerate() {
        if let Some(column_idx) = row.find(needle) {
            return (
                snapshot.viewport.x + u16::try_from(column_idx).unwrap_or_abort(),
                snapshot.viewport.y + u16::try_from(row_idx).unwrap_or_abort(),
            );
        }
    }

    panic!("missing transcript text: {needle}");
}

fn transcript_selection_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row) = transcript_selection_text_position(app, needle);
    (
        column,
        row,
        u16::try_from(needle.chars().count()).unwrap_or_abort(),
    )
}

fn operator_sidebar_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let sidebar = FrameLayoutPlan::for_app(app, TEST_FRAME_AREA)
        .operator_sidebar
        .unwrap_or_abort();

    for y in sidebar.y..sidebar.bottom() {
        let row = (sidebar.x..sidebar.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (
                sidebar
                    .x
                    .saturating_add(u16::try_from(row[..column].chars().count()).unwrap_or_abort()),
                y,
                u16::try_from(needle.chars().count()).unwrap_or_abort(),
            );
        }
    }

    panic!("missing rendered text: {needle}");
}

fn drag_transcript_selection_range(app: &mut AppState, start: (u16, u16), end: (u16, u16)) {
    let (start_column, start_row) = start;
    let (end_column, end_row) = end;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: start_column,
            row: start_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
}

fn drag_transcript_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = transcript_selection_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn drag_operator_sidebar_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = operator_sidebar_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn run_started(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_run_started",
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
        }),
    )
}

fn agent_spawned(seq: u64, agent_id: &str, profile: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )
}

fn child_agent_spawned(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: Some(parent_agent_id.to_string()),
        }),
    )
}

fn provider_started(seq: u64, request_id: &str, provider: &str, model: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.to_string(),
            provider_id: provider.to_string(),
            model_id: model.to_string(),
            prompt_summary: "prompt summary".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn shell_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    args_summary: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "bash".to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_finished(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.to_string(),
            status,
            output_summary: Some("shell output summary".to_string()),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json: Some(output_json),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                timing: Some(ExecutionTimingMetadata {
                    elapsed_ms: Some(250),
                    ..ExecutionTimingMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_panel".to_string(),
                text: "Run a shell command".to_string(),
            }),
        ),
        provider_started(2, "req_shell_panel", "default", "model-1"),
        shell_requested(
            3,
            "req_shell_panel",
            "tc_shell_panel",
            r#"{"command":"cargo test -p harness-tui","description":"run TUI tests"}"#,
        ),
        envelope(
            4,
            "req_shell_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_panel".to_string(),
            }),
        ),
        shell_finished(5, "req_shell_panel", "tc_shell_panel", status, output_json),
    ]
}

fn shell_run_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_run_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_run_panel".to_string(),
                text: "Run shell.run".to_string(),
            }),
        ),
        provider_started(2, "req_shell_run_panel", "default", "model-1"),
        envelope(
            3,
            "req_shell_run_panel",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"bash","args":["-lc","printf shell-run"],"cwd":"."}"#
                    .to_string(),
                args_digest: "digest-tc-shell-run-args".to_string(),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            4,
            "req_shell_run_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
            }),
        ),
        envelope(
            5,
            "req_shell_run_panel",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
                status,
                output_summary: Some("shell-run".to_string()),
                output_digest: Some("digest-tc-shell-run-output".to_string()),
                output_json: Some(output_json),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    timing: Some(ExecutionTimingMetadata {
                        elapsed_ms: Some(42),
                        ..ExecutionTimingMetadata::default()
                    }),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
    ]
}

fn child_link_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: Option<&str>,
    parent_session_id: Option<&str>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "agent.spawn".to_string(),
            args_summary: "{}".to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_session_id: parent_session_id.map(str::to_string),
                    child_session_id: child_session_id.map(str::to_string),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn child_task_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: &str,
    child_request_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(tool_call_id.to_string()),
                    parent_request_id: Some(request_id.to_string()),
                    child_session_id: Some(child_session_id.to_string()),
                    child_request_id: Some(child_request_id.to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

#[cfg(test)]
#[path = "tests/permission_projection_tests.rs"]
mod permission_projection_tests;
#[cfg(test)]
#[path = "tests/terminal_panel_tests.rs"]
mod terminal_panel_tests;

delegate_test!(terminal_panel_is_hidden_by_default_and_toggles_from_keybinding => terminal_panel_tests::terminal_panel_is_hidden_by_default_and_toggles_from_keybinding);
delegate_test!(terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle => terminal_panel_tests::terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle);
delegate_test!(terminal_panel_extracts_successful_bash_command_output => terminal_panel_tests::terminal_panel_extracts_successful_bash_command_output);
delegate_test!(terminal_panel_renders_failed_command_stderr_and_exit_status => terminal_panel_tests::terminal_panel_renders_failed_command_stderr_and_exit_status);
delegate_test!(terminal_panel_extracts_shell_run_direct_command_schema => terminal_panel_tests::terminal_panel_extracts_shell_run_direct_command_schema);
delegate_test!(terminal_panel_replay_reconstructs_from_events_without_execution => terminal_panel_tests::terminal_panel_replay_reconstructs_from_events_without_execution);
delegate_test!(terminal_panel_focus_scrolls_independently_from_transcript => terminal_panel_tests::terminal_panel_focus_scrolls_independently_from_transcript);

#[cfg(test)]
#[path = "tests/permission_modal_tests.rs"]
mod permission_modal_tests;

delegate_test!(overlay_stack_orders_details_palette_permission => permission_modal_tests::overlay_stack_orders_details_palette_permission);
delegate_test!(overlay_stack_orders_permission_above_commands_and_slash => permission_modal_tests::overlay_stack_orders_permission_above_commands_and_slash);
delegate_test!(permission_modal_preempts_palette => permission_modal_tests::permission_modal_preempts_palette);
delegate_test!(permission_modal_ignores_unmapped_chars_without_buffering => permission_modal_tests::permission_modal_ignores_unmapped_chars_without_buffering);
delegate_test!(permission_modal_escape_rejects_without_hiding_pending_permission => permission_modal_tests::permission_modal_escape_rejects_without_hiding_pending_permission);
delegate_test!(permission_modal_ctrl_n_emits_deny_intent_without_hiding_pending_permission => permission_modal_tests::permission_modal_ctrl_n_emits_deny_intent_without_hiding_pending_permission);
delegate_test!(question_permission_modal_collects_answers_and_emits_reason_payload => permission_modal_tests::question_permission_modal_collects_answers_and_emits_reason_payload);
delegate_test!(question_permission_modal_multi_question_uses_tabs_before_submit => permission_modal_tests::question_permission_modal_multi_question_uses_tabs_before_submit);
delegate_test!(question_modal_ignores_digits_past_visible_choices => permission_modal_tests::question_modal_ignores_digits_past_visible_choices);
delegate_test!(question_modal_multi_custom_selection_toggles_saved_custom_answer => permission_modal_tests::question_modal_multi_custom_selection_toggles_saved_custom_answer);
delegate_test!(question_modal_submit_allows_unanswered_questions_on_confirm => permission_modal_tests::question_modal_submit_allows_unanswered_questions_on_confirm);
delegate_test!(permission_modal_allow_always_requests_durable_run_grant => permission_modal_tests::permission_modal_allow_always_requests_durable_run_grant);

#[cfg(test)]
#[path = "tests/model_context_tests.rs"]
mod model_context_tests;

delegate_test!(runtime_context_labels_distinguish_live_continue_and_replay => model_context_tests::runtime_context_labels_distinguish_live_continue_and_replay);
delegate_test!(composer_metadata_prefers_short_agent_name_and_configured_source_label => model_context_tests::composer_metadata_prefers_short_agent_name_and_configured_source_label);
delegate_test!(composer_metadata_source_label_uses_provider_display_label_only => model_context_tests::composer_metadata_source_label_uses_provider_display_label_only);
delegate_test!(live_switch_model_labels_next_turn_only => model_context_tests::live_switch_model_labels_next_turn_only);
delegate_test!(tab_cycles_build_and_plan_primary_agents => model_context_tests::tab_cycles_build_and_plan_primary_agents);
delegate_test!(agent_cycle_preserves_user_selected_provider_model_across_profiles => model_context_tests::agent_cycle_preserves_user_selected_provider_model_across_profiles);
delegate_test!(switching_agent_after_submit_keeps_existing_turn_footer_agent => model_context_tests::switching_agent_after_submit_keeps_existing_turn_footer_agent);

#[cfg(test)]
#[path = "tests/interaction_tests.rs"]
mod interaction_tests;

delegate_test!(focus_returns_after_palette_close => interaction_tests::focus_returns_after_palette_close);

delegate_test!(details_drawer_toggles_without_stealing_transcript_state => interaction_tests::details_drawer_toggles_without_stealing_transcript_state);

#[cfg(test)]
#[path = "tests/lifecycle_shell_tests.rs"]
mod lifecycle_shell_tests;

delegate_test!(config_backed_live_launch_starts_in_session_shell_without_details_drawer => lifecycle_shell_tests::config_backed_live_launch_starts_in_session_shell_without_details_drawer);

delegate_test!(mouse_wheel_scrolls_transcript_without_stealing_focus => interaction_tests::mouse_wheel_scrolls_transcript_without_stealing_focus);

delegate_test!(transcript_navigation_keys_match_scroll_expectations => interaction_tests::transcript_navigation_keys_match_scroll_expectations);

delegate_test!(mouse_wheel_scrolls_inspector_when_hovered => interaction_tests::mouse_wheel_scrolls_inspector_when_hovered);

delegate_test!(mouse_wheel_ignores_non_scrollable_areas => interaction_tests::mouse_wheel_ignores_non_scrollable_areas);

delegate_test!(mouse_click_toggles_operator_sidebar_section_without_stealing_focus => interaction_tests::mouse_click_toggles_operator_sidebar_section_without_stealing_focus);

delegate_test!(edit_applied_auto_opens_modified_files_section => interaction_tests::edit_applied_auto_opens_modified_files_section);

delegate_test!(diff_hunk_navigation_advances_and_retreats_between_hunks => interaction_tests::diff_hunk_navigation_advances_and_retreats_between_hunks);

delegate_test!(dragging_transcript_scrollbar_updates_scroll_position => interaction_tests::dragging_transcript_scrollbar_updates_scroll_position);

delegate_test!(clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag => interaction_tests::clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag);

#[cfg(test)]
#[path = "tests/transcript_selection_tests.rs"]
mod transcript_selection_tests;

#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection => transcript_selection_tests::mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_shell_card_text => transcript_selection_tests::mouse_drag_copy_on_select_copies_shell_card_text);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_operator_sidebar_text => transcript_selection_tests::mouse_drag_copy_on_select_copies_operator_sidebar_text);
delegate_test!(disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy => transcript_selection_tests::disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy);
delegate_test!(mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails => transcript_selection_tests::mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails);
delegate_test!(mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding => transcript_selection_tests::mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding);
delegate_test!(disabled_copy_on_select_keeps_selection_until_right_click_copy => transcript_selection_tests::disabled_copy_on_select_keeps_selection_until_right_click_copy);
delegate_test!(disabled_copy_on_select_supports_ctrl_c_and_escape => transcript_selection_tests::disabled_copy_on_select_supports_ctrl_c_and_escape);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap => transcript_selection_tests::mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap);
delegate_test!(transcript_selection_hit_testing_reuses_cached_snapshot_during_drag => transcript_selection_tests::transcript_selection_hit_testing_reuses_cached_snapshot_during_drag);
delegate_test!(transcript_selection_snapshot_uses_transcript_rail_for_user_rows => transcript_selection_tests::transcript_selection_snapshot_uses_transcript_rail_for_user_rows);
delegate_test!(mouse_wheel_does_not_build_transcript_selection_snapshot => transcript_selection_tests::mouse_wheel_does_not_build_transcript_selection_snapshot);
delegate_test!(transcript_selection_render_reuses_cached_snapshot => transcript_selection_tests::transcript_selection_render_reuses_cached_snapshot);
delegate_test!(transcript_selection_render_stays_aligned_after_large_reasoning_block => transcript_selection_tests::transcript_selection_render_stays_aligned_after_large_reasoning_block);
delegate_test!(transcript_render_key_is_cached_across_selection_drag_path => transcript_selection_tests::transcript_render_key_is_cached_across_selection_drag_path);
delegate_test!(transcript_render_key_reuses_cache_until_marked_dirty => transcript_selection_tests::transcript_render_key_reuses_cache_until_marked_dirty);

delegate_test!(historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit => lifecycle_shell_tests::historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit);

delegate_test!(historical_terminal_events_stay_in_session_shell_after_live_finish => lifecycle_shell_tests::historical_terminal_events_stay_in_session_shell_after_live_finish);

delegate_test!(continued_quiescent_bootstrap_stays_in_session_shell_without_handoff => lifecycle_shell_tests::continued_quiescent_bootstrap_stays_in_session_shell_without_handoff);

delegate_test!(startup_prompt_enter_echoes_prompt_and_selects_new_session => lifecycle_shell_tests::startup_prompt_enter_echoes_prompt_and_selects_new_session);

delegate_test!(slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit => lifecycle_shell_tests::slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit);

#[cfg(test)]
#[path = "tests/activity_lifecycle_tests.rs"]
mod activity_lifecycle_tests;

delegate_test!(provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text => activity_lifecycle_tests::provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text);

delegate_test!(provider_request_finished_keeps_activity_streaming_until_turn_task_completes => activity_lifecycle_tests::provider_request_finished_keeps_activity_streaming_until_turn_task_completes);

delegate_test!(cache_read_write_tokens_render_as_separate_status_labels => activity_lifecycle_tests::cache_read_write_tokens_render_as_separate_status_labels);

delegate_test!(task_cancelled_marks_matching_activity_as_error => activity_lifecycle_tests::task_cancelled_marks_matching_activity_as_error);

delegate_test!(provider_error_categories_surface_in_tui_activity_and_runtime_state => activity_lifecycle_tests::provider_error_categories_surface_in_tui_activity_and_runtime_state);

delegate_test!(child_tool_task_completed_does_not_finish_parent_turn_activity => activity_lifecycle_tests::child_tool_task_completed_does_not_finish_parent_turn_activity);

delegate_test!(child_tool_task_cancelled_does_not_mark_parent_turn_activity_error => activity_lifecycle_tests::child_tool_task_cancelled_does_not_mark_parent_turn_activity_error);

delegate_test!(terminal_only_turn_completion_scope_marks_activity_done_without_task_row => activity_lifecycle_tests::terminal_only_turn_completion_scope_marks_activity_done_without_task_row);

delegate_test!(terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row => activity_lifecycle_tests::terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row);

delegate_test!(terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state => activity_lifecycle_tests::terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state);

delegate_test!(replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row => activity_lifecycle_tests::terminal_replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row);

delegate_test!(replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row => activity_lifecycle_tests::terminal_replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row);

delegate_test!(replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state => activity_lifecycle_tests::terminal_replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state);

#[cfg(test)]
#[path = "tests/prompt_input_tests.rs"]
mod prompt_input_tests;

#[cfg(test)]
#[path = "tests/prompt_stash_tests.rs"]
mod prompt_stash_tests;

delegate_test!(ctrl_j_inserts_newline_without_submitting => prompt_input_tests::ctrl_j_inserts_newline_without_submitting);
delegate_test!(paste_multiline_text_inserts_newlines_without_submitting => prompt_input_tests::paste_multiline_text_inserts_newlines_without_submitting);
delegate_test!(multiline_history_keys_move_cursor_before_recalling_history => prompt_input_tests::multiline_history_keys_move_cursor_before_recalling_history);
delegate_test!(prompt_history_persists_and_restores_draft_after_recall => prompt_input_tests::prompt_history_persists_and_restores_draft_after_recall);
delegate_test!(startup_auto_submit_persists_prompt_history_once => prompt_input_tests::startup_auto_submit_persists_prompt_history_once);
delegate_test!(live_bootstrap_auto_submit_echoes_and_emits_first_prompt => prompt_input_tests::live_bootstrap_auto_submit_echoes_and_emits_first_prompt);
delegate_test!(submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent => prompt_input_tests::submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent);

delegate_test!(prompt_stash_push_clears_composer_and_persists_entry => prompt_stash_tests::prompt_stash_push_clears_composer_and_persists_entry);
delegate_test!(prompt_stash_pop_restores_text_cursor_and_selection => prompt_stash_tests::prompt_stash_pop_restores_text_cursor_and_selection);
delegate_test!(prompt_stash_pop_with_empty_stash_is_noop => prompt_stash_tests::prompt_stash_pop_with_empty_stash_is_noop);
delegate_test!(prompt_stash_push_with_empty_composer_is_noop => prompt_stash_tests::prompt_stash_push_with_empty_composer_is_noop);
delegate_test!(prompt_stash_list_dialog_opens_and_closes => prompt_stash_tests::prompt_stash_list_dialog_opens_and_closes);
delegate_test!(prompt_stash_list_dialog_renders_entries => prompt_stash_tests::prompt_stash_list_dialog_renders_entries);
delegate_test!(prompt_stash_list_delete_removes_selected_entry => prompt_stash_tests::prompt_stash_list_delete_removes_selected_entry);
delegate_test!(prompt_stash_list_restore_loads_selected_entry_to_composer => prompt_stash_tests::prompt_stash_list_restore_loads_selected_entry_to_composer);
delegate_test!(prompt_stash_persists_across_session_restart => prompt_stash_tests::prompt_stash_persists_across_session_restart);
delegate_test!(queued_prompt_count_tracks_queued_activities => prompt_stash_tests::queued_prompt_count_tracks_queued_activities);
delegate_test!(queued_prompt_indicator_renders_when_count_positive => prompt_stash_tests::queued_prompt_indicator_renders_when_count_positive);

#[cfg(test)]
#[path = "tests/composer_editing_tests.rs"]
mod composer_editing_tests;

delegate_test!(move_word_left_skips_separators_then_word => composer_editing_tests::move_word_left_skips_separators_then_word);
delegate_test!(move_word_right_skips_word_then_separators => composer_editing_tests::move_word_right_skips_word_then_separators);
delegate_test!(move_word_left_at_start_stays_at_zero => composer_editing_tests::move_word_left_at_start_stays_at_zero);
delegate_test!(move_word_right_at_end_stays_at_end => composer_editing_tests::move_word_right_at_end_stays_at_end);
delegate_test!(move_word_left_handles_leading_separators => composer_editing_tests::move_word_left_handles_leading_separators);
delegate_test!(delete_word_backward_removes_word_and_pushes_undo => composer_editing_tests::delete_word_backward_removes_word_and_pushes_undo);
delegate_test!(delete_word_forward_removes_word_and_pushes_undo => composer_editing_tests::delete_word_forward_removes_word_and_pushes_undo);
delegate_test!(redo_re_applies_after_undo => composer_editing_tests::redo_re_applies_after_undo);
delegate_test!(undo_restores_selection_anchor => composer_editing_tests::undo_restores_selection_anchor);
delegate_test!(select_char_left_extends_selection => composer_editing_tests::select_char_left_extends_selection);
delegate_test!(select_word_right_extends_selection => composer_editing_tests::select_word_right_extends_selection);
delegate_test!(select_all_selects_entire_buffer => composer_editing_tests::select_all_selects_entire_buffer);
delegate_test!(select_line_selects_current_line => composer_editing_tests::select_line_selects_current_line);
delegate_test!(move_line_start_clears_selection => composer_editing_tests::move_line_start_clears_selection);
delegate_test!(move_line_end_clears_selection => composer_editing_tests::move_line_end_clears_selection);
delegate_test!(move_buffer_start_clears_selection => composer_editing_tests::move_buffer_start_clears_selection);
delegate_test!(move_buffer_end_clears_selection => composer_editing_tests::move_buffer_end_clears_selection);
delegate_test!(delete_line_removes_entire_line_including_newline => composer_editing_tests::delete_line_removes_entire_line_including_newline);
delegate_test!(kill_to_line_start_deletes_from_cursor_to_line_start => composer_editing_tests::kill_to_line_start_deletes_from_cursor_to_line_start);
delegate_test!(kill_to_line_end_deletes_from_cursor_to_line_end => composer_editing_tests::kill_to_line_end_deletes_from_cursor_to_line_end);
delegate_test!(typing_after_select_replaces_selection => composer_editing_tests::typing_after_select_replaces_selection);
delegate_test!(backspace_with_selection_deletes_selection => composer_editing_tests::backspace_with_selection_deletes_selection);
delegate_test!(undo_stack_caps_at_max_entries => composer_editing_tests::undo_stack_caps_at_max_entries);
delegate_test!(history_navigation_preserves_draft_via_undo => composer_editing_tests::history_navigation_preserves_draft_via_undo);
delegate_test!(cursor_left_clears_selection => composer_editing_tests::cursor_left_clears_selection);
delegate_test!(word_boundary_detects_punctuation_as_separator => composer_editing_tests::word_boundary_detects_punctuation_as_separator);

#[cfg(test)]
#[path = "tests/file_mention_tests.rs"]
mod file_mention_tests;

delegate_test!(typing_at_opens_file_mention_menu_with_directories => file_mention_tests::typing_at_opens_file_mention_menu_with_directories);
delegate_test!(file_mention_tab_expands_directory_without_closing_menu => file_mention_tests::file_mention_tab_expands_directory_without_closing_menu);
delegate_test!(file_mention_enter_inserts_selected_file_with_space => file_mention_tests::file_mention_enter_inserts_selected_file_with_space);
delegate_test!(file_mentions_use_injected_scanner_workspace_and_clock => file_mention_tests::file_mentions_use_injected_scanner_workspace_and_clock);
delegate_test!(submitting_selected_file_mention_emits_structured_file_part => file_mention_tests::submitting_selected_file_mention_emits_structured_file_part);
delegate_test!(file_mention_picker_selects_agent_parts_from_launch_metadata => file_mention_tests::file_mention_picker_selects_agent_parts_from_launch_metadata);
delegate_test!(file_mention_picker_selects_mcp_resource_parts_from_launch_metadata => file_mention_tests::file_mention_picker_selects_mcp_resource_parts_from_launch_metadata);
delegate_test!(file_mention_tag_is_removed_when_user_edits_inside_it => file_mention_tests::file_mention_tag_is_removed_when_user_edits_inside_it);

delegate_test!(queued_turn_schedule_keeps_activity_queued_until_provider_starts => activity_lifecycle_tests::terminal_queued_turn_schedule_keeps_activity_queued_until_provider_starts);

#[cfg(test)]
#[path = "tests/session_navigation_tests.rs"]
mod session_navigation_tests;

delegate_test!(parent_transcript_hides_child_prompt_before_task_tool_finishes => session_navigation_tests::parent_child_parent_transcript_hides_child_prompt_before_task_tool_finishes);

delegate_test!(replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits => session_navigation_tests::replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits);

delegate_test!(child_session_navigation_keybinds_follow_default_contract => session_navigation_tests::child_session_navigation_keybinds_follow_default_contract);

delegate_test!(replay_child_navigation_does_not_emit_live_intents => session_navigation_tests::replay_child_navigation_does_not_emit_live_intents);

delegate_test!(replay_handoff_parent_navigation_continues_resumable_parent_session => session_navigation_tests::replay_handoff_parent_navigation_continues_resumable_parent_session);

delegate_test!(task_child_navigation_opens_inline_subagent_view_without_child_run_dir => session_navigation_tests::task_child_navigation_opens_inline_subagent_view_without_child_run_dir);

delegate_test!(parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript => session_navigation_tests::parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript);

delegate_test!(live_inline_child_navigation_restores_live_parent_mode => session_navigation_tests::live_inline_child_navigation_restores_live_parent_mode);

delegate_test!(live_parent_events_update_parent_snapshot_while_inline_child_is_selected => session_navigation_tests::live_parent_events_update_parent_snapshot_while_inline_child_is_selected);

#[cfg(test)]
#[path = "tests/slash_menu_tests.rs"]
mod slash_menu_tests;

delegate_test!(slash_menu_closes_after_whitespace => slash_menu_tests::slash_menu_closes_after_whitespace);
delegate_test!(slash_menu_resets_selection_when_filter_changes => slash_menu_tests::slash_menu_resets_selection_when_filter_changes);
delegate_test!(slash_menu_matches_descriptions_and_boosts_prefixes => slash_menu_tests::slash_menu_matches_descriptions_and_boosts_prefixes);
delegate_test!(slash_alias_executes_matching_command_without_menu => slash_menu_tests::slash_alias_executes_matching_command_without_menu);
delegate_test!(slash_help_opens_help_surface_and_preserves_draft => slash_menu_tests::slash_help_opens_help_surface_and_preserves_draft);
delegate_test!(slash_escape_clears_token_or_restores_prior_draft => slash_menu_tests::slash_escape_clears_token_or_restores_prior_draft);
delegate_test!(slash_exit_matches_quit_requested_behavior => slash_menu_tests::slash_exit_matches_quit_requested_behavior);
delegate_test!(resume_history_surface_uses_meaningful_session_title => slash_menu_tests::resume_history_surface_uses_meaningful_session_title);
delegate_test!(slash_menu_supports_mouse_selection => slash_menu_tests::slash_menu_supports_mouse_selection);
delegate_test!(slash_menu_exposes_model_switcher_when_models_are_configured => slash_menu_tests::slash_menu_exposes_model_switcher_when_models_are_configured);
delegate_test!(rename_slash_command_availability_matches_mode => slash_menu_tests::rename_slash_command_availability_matches_mode);
delegate_test!(rename_slash_command_emits_update_session_title_intent => slash_menu_tests::rename_slash_command_emits_update_session_title_intent);
delegate_test!(rename_slash_empty_title_emits_error_toast => slash_menu_tests::rename_slash_empty_title_emits_error_toast);

delegate_test!(startup_mode_uses_pending_launch_metadata => lifecycle_shell_tests::startup_mode_uses_pending_launch_metadata);

delegate_test!(lifecycle_shell_state_transitions => lifecycle_shell_tests::lifecycle_shell_state_transitions);

delegate_test!(default_shell_registry_exposes_home_and_session_shell_only => lifecycle_shell_tests::default_shell_registry_exposes_home_and_session_shell_only);

delegate_test!(post_run_handoff_ignores_completed_turns_without_terminal_event => lifecycle_shell_tests::post_run_handoff_ignores_completed_turns_without_terminal_event);

delegate_test!(tool_task_completion_does_not_copy_tool_output_into_activity_transcript => activity_lifecycle_tests::terminal_tool_task_completion_does_not_copy_tool_output_into_activity_transcript);

delegate_test!(replay_mode_never_reports_lifecycle_shell_actions => lifecycle_shell_tests::replay_mode_never_reports_lifecycle_shell_actions);

#[cfg(test)]
#[path = "tests/palette_parity_tests.rs"]
mod palette_parity_tests;

delegate_test!(palette_opens_with_ctrl_p => palette_parity_tests::palette_opens_with_ctrl_p);
delegate_test!(palette_closes_with_escape => palette_parity_tests::palette_closes_with_escape);
delegate_test!(palette_closes_with_ctrl_c => palette_parity_tests::palette_closes_with_ctrl_c);
delegate_test!(palette_empty_filter_has_suggested_duplicates => palette_parity_tests::palette_empty_filter_has_suggested_duplicates);
delegate_test!(palette_non_empty_filter_has_no_suggested_duplicates => palette_parity_tests::palette_non_empty_filter_has_no_suggested_duplicates);
delegate_test!(palette_filter_matches_title_not_id => palette_parity_tests::palette_filter_matches_title_not_id);
delegate_test!(palette_filter_does_not_match_command_id => palette_parity_tests::palette_filter_does_not_match_command_id);
delegate_test!(palette_no_results_shows_empty_message => palette_parity_tests::palette_no_results_shows_empty_message);
delegate_test!(palette_navigation_wraps_around => palette_parity_tests::palette_navigation_wraps_around);
delegate_test!(palette_home_end_navigation => palette_parity_tests::palette_home_end_navigation);
delegate_test!(palette_page_navigation => palette_parity_tests::palette_page_navigation);
delegate_test!(palette_excludes_excluded_commands_in_live_session => palette_parity_tests::palette_excludes_excluded_commands_in_live_session);
delegate_test!(palette_excludes_hidden_non_targets_in_live_session => palette_parity_tests::palette_excludes_hidden_non_targets_in_live_session);
delegate_test!(palette_excludes_excluded_commands_in_startup_shell => palette_parity_tests::palette_excludes_excluded_commands_in_startup_shell);
delegate_test!(palette_excludes_hidden_non_targets_in_startup_shell => palette_parity_tests::palette_excludes_hidden_non_targets_in_startup_shell);
delegate_test!(palette_includes_model_list_in_live_session => palette_parity_tests::palette_includes_model_list_in_live_session);
delegate_test!(palette_harness_only_commands_are_prefixed => palette_parity_tests::palette_harness_only_commands_are_prefixed);
delegate_test!(palette_toggle_commands_use_dynamic_titles => palette_parity_tests::palette_toggle_commands_use_dynamic_titles);
delegate_test!(palette_no_split_show_hide_entries => palette_parity_tests::palette_no_split_show_hide_entries);
delegate_test!(palette_suggested_duplicate_dispatches_same_command => palette_parity_tests::palette_suggested_duplicate_dispatches_same_command);
delegate_test!(palette_replay_mode_restricts_commands => palette_parity_tests::palette_replay_mode_restricts_commands);
delegate_test!(palette_startup_shell_restricts_commands => palette_parity_tests::palette_startup_shell_restricts_commands);
delegate_test!(palette_all_filtered_results_are_valid_commands => palette_parity_tests::palette_all_filtered_results_are_valid_commands);
delegate_test!(palette_all_parity_included_ids_have_entries => palette_parity_tests::palette_all_parity_included_ids_have_entries);
delegate_test!(palette_no_parity_excluded_ids_in_entries => palette_parity_tests::palette_no_parity_excluded_ids_in_entries);
delegate_test!(palette_no_hidden_non_target_ids_in_entries => palette_parity_tests::palette_no_hidden_non_target_ids_in_entries);

delegate_test!(palette_dispatch_quit_exits_app => palette_parity_tests::palette_dispatch_quit_exits_app);
delegate_test!(palette_dispatch_toggle_thinking_flips_state => palette_parity_tests::palette_dispatch_toggle_thinking_flips_state);
delegate_test!(palette_dispatch_toggle_timestamps_flips_state => palette_parity_tests::palette_dispatch_toggle_timestamps_flips_state);
delegate_test!(palette_dispatch_toggle_tool_details_flips_state => palette_parity_tests::palette_dispatch_toggle_tool_details_flips_state);
delegate_test!(palette_dispatch_placeholder_shows_status_banner => palette_parity_tests::palette_dispatch_placeholder_shows_status_banner);
delegate_test!(palette_dispatch_new_session_clears_events => palette_parity_tests::palette_dispatch_new_session_clears_events);
delegate_test!(palette_dynamic_title_thinking_changes_with_state => palette_parity_tests::palette_dynamic_title_thinking_changes_with_state);
delegate_test!(palette_dynamic_title_timestamps_changes_with_state => palette_parity_tests::palette_dynamic_title_timestamps_changes_with_state);
delegate_test!(palette_title_weighting_prefers_title_match => palette_parity_tests::palette_title_weighting_prefers_title_match);
delegate_test!(palette_state_home_no_sessions => palette_parity_tests::palette_state_home_no_sessions);
delegate_test!(palette_state_live_session_idle => palette_parity_tests::palette_state_live_session_idle);
delegate_test!(palette_state_replay_mode => palette_parity_tests::palette_state_replay_mode);
delegate_test!(palette_state_review_surface_open => palette_parity_tests::palette_state_review_surface_open);
delegate_test!(palette_state_provider_disconnected => palette_parity_tests::palette_state_provider_disconnected);
delegate_test!(palette_state_provider_connected => palette_parity_tests::palette_state_provider_connected);
delegate_test!(palette_state_prompt_with_input => palette_parity_tests::palette_state_prompt_with_input);
delegate_test!(palette_state_prompt_empty => palette_parity_tests::palette_state_prompt_empty);
delegate_test!(palette_state_workspace_commands_unavailable => palette_parity_tests::palette_state_workspace_commands_unavailable);
delegate_test!(palette_state_unavailable_commands_not_dispatchable => palette_parity_tests::palette_state_unavailable_commands_not_dispatchable);
delegate_test!(palette_state_harness_only_commands_present => palette_parity_tests::palette_state_harness_only_commands_present);
delegate_test!(palette_filtered_results_preserve_category_grouping => palette_parity_tests::palette_filtered_results_preserve_category_grouping);
delegate_test!(palette_state_session_unshare_unavailable => palette_parity_tests::palette_state_session_unshare_unavailable);
delegate_test!(palette_state_session_redo_unavailable => palette_parity_tests::palette_state_session_redo_unavailable);
delegate_test!(palette_state_live_shared_session => palette_parity_tests::palette_state_live_shared_session);
delegate_test!(palette_state_live_session_with_revert => palette_parity_tests::palette_state_live_session_with_revert);
delegate_test!(palette_state_variants_absent => palette_parity_tests::palette_state_variants_absent);
delegate_test!(palette_dispatch_toggle_generic_output_flips_state => palette_parity_tests::palette_dispatch_toggle_generic_output_flips_state);
delegate_test!(palette_footer_derived_from_keymap => palette_parity_tests::palette_footer_derived_from_keymap);
delegate_test!(palette_dynamic_title_sidebar_reflects_state => palette_parity_tests::palette_dynamic_title_sidebar_reflects_state);
delegate_test!(palette_dynamic_title_all_toggles_reflect_state => palette_parity_tests::palette_dynamic_title_all_toggles_reflect_state);
delegate_test!(palette_suggested_row_dispatches_via_enter => palette_parity_tests::palette_suggested_row_dispatches_via_enter);
delegate_test!(palette_matrix_and_registry_dispatch_consistent => palette_parity_tests::palette_matrix_and_registry_dispatch_consistent);
delegate_test!(palette_exact_dispatch_targets => palette_parity_tests::palette_exact_dispatch_targets);
delegate_test!(palette_state_matrix_full_inventories => palette_parity_tests::palette_state_matrix_full_inventories);
delegate_test!(palette_dispatch_lineage_browser_stays_open => palette_parity_tests::palette_dispatch_lineage_browser_stays_open);
delegate_test!(palette_dispatch_open_event_log_from_prompt_focus => palette_parity_tests::palette_dispatch_open_event_log_from_prompt_focus);
delegate_test!(palette_dispatch_provider_connect_opens_connect_dialog => palette_parity_tests::palette_dispatch_provider_connect_opens_connect_dialog);
delegate_test!(palette_golden_ranking_prefix_match_ranks_first => palette_parity_tests::palette_golden_ranking_prefix_match_ranks_first);
delegate_test!(palette_golden_ranking_title_match_preferred_over_category => palette_parity_tests::palette_golden_ranking_title_match_preferred_over_category);
delegate_test!(palette_golden_ranking_consecutive_match_bonus => palette_parity_tests::palette_golden_ranking_consecutive_match_bonus);
delegate_test!(palette_golden_ranking_no_results_for_gibberish => palette_parity_tests::palette_golden_ranking_no_results_for_gibberish);
delegate_test!(palette_golden_ranking_category_match_works => palette_parity_tests::palette_golden_ranking_category_match_works);
delegate_test!(palette_golden_ranking_typo_tolerance => palette_parity_tests::palette_golden_ranking_typo_tolerance);
delegate_test!(palette_golden_ranking_mixed_title_category_match => palette_parity_tests::palette_golden_ranking_mixed_title_category_match);
delegate_test!(palette_inventory_all_commands_have_status => palette_parity_tests::palette_inventory_all_commands_have_status);
delegate_test!(palette_inventory_excluded_commands_absent_from_palette => palette_parity_tests::palette_inventory_excluded_commands_absent_from_palette);
delegate_test!(palette_inventory_zero_placeholders_for_included => palette_parity_tests::palette_inventory_zero_placeholders_for_included);
delegate_test!(palette_inventory_excluded_commands_have_rationale => palette_parity_tests::palette_inventory_excluded_commands_have_rationale);
delegate_test!(palette_inventory_harness_only_absent_from_palette => palette_parity_tests::palette_inventory_harness_only_absent_from_palette);
delegate_test!(palette_slash_alias_inventory_complete => palette_parity_tests::palette_slash_alias_inventory_complete);
delegate_test!(palette_footer_no_category_fallback => palette_parity_tests::palette_footer_no_category_fallback);
delegate_test!(palette_log_success_dispatch => palette_parity_tests::palette_log_success_dispatch);
delegate_test!(palette_log_rejection_for_unavailable => palette_parity_tests::palette_log_rejection_for_unavailable);
delegate_test!(palette_log_contains_command_id_and_target => palette_parity_tests::palette_log_contains_command_id_and_target);
delegate_test!(palette_log_contains_filter_length => palette_parity_tests::palette_log_contains_filter_length);
delegate_test!(palette_golden_ranking_consecutive_beats_scattered => palette_parity_tests::palette_golden_ranking_consecutive_beats_scattered);
delegate_test!(palette_golden_ranking_prefix_beats_non_prefix => palette_parity_tests::palette_golden_ranking_prefix_beats_non_prefix);
delegate_test!(palette_golden_ranking_title_weighted_double_category => palette_parity_tests::palette_golden_ranking_title_weighted_double_category);
delegate_test!(palette_golden_ranking_no_match_returns_none => palette_parity_tests::palette_golden_ranking_no_match_returns_none);
delegate_test!(palette_behavior_session_rename_opens_dialog => palette_parity_tests::palette_behavior_session_rename_opens_dialog);
delegate_test!(palette_behavior_session_fork_opens_selector => palette_parity_tests::palette_behavior_session_fork_opens_selector);
delegate_test!(palette_behavior_session_copy_copies_transcript => palette_parity_tests::palette_behavior_session_copy_copies_transcript);
delegate_test!(palette_behavior_dialog_clears_before_dispatch => palette_parity_tests::palette_behavior_dialog_clears_before_dispatch);
delegate_test!(palette_behavior_page_down_wraps => palette_parity_tests::palette_behavior_page_down_wraps);
delegate_test!(palette_behavior_page_up_wraps => palette_parity_tests::palette_behavior_page_up_wraps);
delegate_test!(palette_behavior_tab_is_noop_in_command_palette => palette_parity_tests::palette_behavior_tab_is_noop_in_command_palette);
delegate_test!(palette_behavior_no_current_dot_in_palette => palette_parity_tests::palette_behavior_no_current_dot_in_palette);
delegate_test!(palette_harness_only_filtering_is_deliberate => palette_parity_tests::palette_harness_only_filtering_is_deliberate);
delegate_test!(palette_mouse_semantics_platform_limitation => palette_parity_tests::palette_mouse_semantics_platform_limitation);
delegate_test!(palette_behavior_model_list_opens_switcher => palette_parity_tests::palette_behavior_model_list_opens_switcher);
delegate_test!(palette_behavior_provider_connect_opens_dialog => palette_parity_tests::palette_behavior_provider_connect_opens_dialog);
delegate_test!(palette_behavior_session_list_opens_history => palette_parity_tests::palette_behavior_session_list_opens_history);
delegate_test!(palette_behavior_mcp_list_opens_toggles => palette_parity_tests::palette_behavior_mcp_list_opens_toggles);
delegate_test!(palette_golden_ranking_exact_page_down_by_10 => palette_parity_tests::palette_golden_ranking_exact_page_down_by_10);
delegate_test!(palette_golden_ranking_exact_page_up_by_10 => palette_parity_tests::palette_golden_ranking_exact_page_up_by_10);
delegate_test!(palette_golden_ranking_page_down_wraps_from_end => palette_parity_tests::palette_golden_ranking_page_down_wraps_from_end);
delegate_test!(palette_golden_ranking_page_up_wraps_from_start => palette_parity_tests::palette_golden_ranking_page_up_wraps_from_start);
delegate_test!(palette_log_failure_has_error_kind => palette_parity_tests::palette_log_failure_has_error_kind);
delegate_test!(palette_log_has_redacted_ids => palette_parity_tests::palette_log_has_redacted_ids);
delegate_test!(palette_tab_shift_tab_explicit_noop => palette_parity_tests::palette_tab_shift_tab_explicit_noop);
delegate_test!(palette_golden_ranking_stash_ordering => palette_parity_tests::palette_golden_ranking_stash_ordering);
delegate_test!(palette_slash_alias_global_inventory => palette_parity_tests::palette_slash_alias_global_inventory);
delegate_test!(palette_log_open_close_lifecycle => palette_parity_tests::palette_log_open_close_lifecycle);
delegate_test!(palette_log_filtered_lifecycle => palette_parity_tests::palette_log_filtered_lifecycle);
delegate_test!(palette_log_selected_lifecycle => palette_parity_tests::palette_log_selected_lifecycle);
delegate_test!(palette_log_dispatch_started_then_succeeded_in_order => palette_parity_tests::palette_log_dispatch_started_then_succeeded_in_order);
delegate_test!(palette_log_dispatch_failed_for_placeholder => palette_parity_tests::palette_log_dispatch_failed_for_placeholder);
delegate_test!(palette_golden_ranking_title_vs_category_weighting => palette_parity_tests::palette_golden_ranking_title_vs_category_weighting);
delegate_test!(palette_golden_ranking_stable_tie_order => palette_parity_tests::palette_golden_ranking_stable_tie_order);
delegate_test!(palette_inventory_comprehensive_fields => palette_parity_tests::palette_inventory_comprehensive_fields);

#[cfg(test)]
#[path = "tests/opencode_subagent_parity_evidence.rs"]
mod opencode_subagent_parity_evidence;

#[test]
#[ignore = "manual evidence export; set HARNESS_TUI_OPENCODE_SUBAGENT_EVIDENCE_DIR"]
fn opencode_subagent_parity_evidence_export() {
    opencode_subagent_parity_evidence::opencode_subagent_parity_evidence_export();
}
