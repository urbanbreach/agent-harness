use ratatui::text::Line;

use crate::app::{ActivityEntry, ActivityStatus, ToolCallDisplayStatus, ToolCallEntry};

pub(crate) fn transcript_section_model_test_activity(
    request_id: &str,
    status: ActivityStatus,
    transcript_text: &str,
) -> ActivityEntry {
    ActivityEntry {
        request_id: request_id.to_string(),
        revision: 1,
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: None,
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
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
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
