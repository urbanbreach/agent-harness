use ratatui::text::Line;

use crate::app::{ActivityEntry, ActivityStatus, AppState, ToolCallDisplayStatus, ToolCallEntry};
use harness_core::event::UserMessageSubmittedEvent;

pub(crate) fn transcript_section_model_test_activity(
    request_id: &str,
    status: ActivityStatus,
    transcript_text: &str,
) -> ActivityEntry {
    ActivityEntry {
        request_id: request_id.to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: transcript_text.to_string(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }
}

pub(crate) fn transcript_section_model_test_tool_call(
    tool_call_id: &str,
    tool_id: &str,
) -> ToolCallEntry {
    ToolCallEntry {
        tool_call_id: tool_call_id.to_string(),
        tool_id: tool_id.to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: "{}".to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Queued,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 0,
        last_mono_ms: 0,
        first_timestamp: None,
        last_timestamp: None,
    }
}

pub(crate) fn transcript_test_line_texts(lines: Vec<Line<'static>>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect()
}

pub(crate) fn transcript_grammar_test_app() -> AppState {
    let mut activity = transcript_section_model_test_activity(
        "grammar-turn",
        ActivityStatus::Done,
        "settled body こんにちは 👩🏽‍💻",
    );
    activity.user_message = Some(UserMessageSubmittedEvent {
        request_id: "grammar-turn".into(),
        text: "prompt こんにちは 👩🏽‍💻".to_string(),
    });
    activity.user_timestamp = Some("2026-08-13T09:41:00Z".to_string());
    activity.thinking_text = "considering options".to_string();
    activity.error_message = Some("recoverable error".to_string());
    activity.tool_calls = [
        ("generic", "fs.read", ToolCallDisplayStatus::Succeeded),
        ("shell", "bash", ToolCallDisplayStatus::Running),
        ("diff", "apply_patch", ToolCallDisplayStatus::Succeeded),
        ("subagent", "task", ToolCallDisplayStatus::Succeeded),
        (
            "permission",
            "write",
            ToolCallDisplayStatus::PendingPermission,
        ),
        (
            "question",
            "question",
            ToolCallDisplayStatus::PendingPermission,
        ),
    ]
    .into_iter()
    .map(|(id, tool_id, status)| {
        let mut tool = transcript_section_model_test_tool_call(id, tool_id);
        tool.status = status;
        tool.output_summary = Some(format!("{id} output"));
        tool.timing_elapsed_ms = Some(10);
        tool
    })
    .collect();

    let mut app = AppState::default();
    app.activities.push_back(activity);
    app.transcript_view.selected_activity_index = 0;
    app
}
