use std::collections::BTreeSet;

use crate::app::{
    AppState, OrchestrationTaskRow, OrchestrationTaskState, ToolCallDisplayStatus, ToolCallEntry,
};
use crate::text::{collapse_inline_whitespace, non_empty_trimmed};

use super::ui_tool_metadata::{
    task_tool_child_request_id_from_output, task_tool_child_session_id_from_output,
    tool_json_string, tool_json_string_ref, tool_summary_string,
};
use super::ui_tool_titles::format_duration_ms;

pub(super) fn task_tool_child_request_id(tool_call: &ToolCallEntry) -> Option<&str> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_request_id.as_deref())
        .and_then(non_empty_trimmed)
        .or_else(|| task_tool_child_request_id_from_output(tool_call.output_json.as_ref()))
}

pub(super) fn task_tool_child_session_id(tool_call: &ToolCallEntry) -> Option<&str> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_session_id.as_deref())
        .and_then(non_empty_trimmed)
        .or_else(|| task_tool_child_session_id_from_output(tool_call.output_json.as_ref()))
}

pub(super) fn hidden_delegated_child_request_ids(app: &AppState) -> BTreeSet<&str> {
    let current_session_id = app.current_session_id();
    let mut hidden = app.delegated_child_request_ids_for_parent_view(current_session_id);
    hidden.extend(
        app.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| {
                matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
                    || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
            })
            .filter_map(|tool_call| {
                let request_id = task_tool_child_request_id(tool_call)?;
                let child_session_id = task_tool_child_session_id(tool_call);
                child_session_id
                    .is_none_or(|child_session_id| current_session_id != Some(child_session_id))
                    .then_some(request_id)
            }),
    );
    hidden
}

pub(super) fn agent_spawn_title(tool_call: &ToolCallEntry, description: Option<String>) -> String {
    let profile = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("profile"))
        .and_then(serde_json::Value::as_str)
        .map(collapse_inline_whitespace)
        .or_else(|| {
            tool_summary_string(
                &tool_call.args_summary,
                &["profile_name", "profile", "subagent_type"],
            )
        });

    let prefix = format!(
        "{} Task",
        subagent_profile_label(profile.as_deref().unwrap_or("General"))
    );

    match description {
        Some(description) => format!("{prefix} — {description}"),
        None => prefix,
    }
}

pub(super) fn agent_spawn_description(tool_call: &ToolCallEntry) -> Option<String> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("description"))
        .and_then(serde_json::Value::as_str)
        .map(collapse_inline_whitespace)
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["description", "task"]))
}

pub(super) fn subagent_profile_label(profile: &str) -> String {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return "General".to_string();
    }

    let mut label = String::with_capacity(trimmed.len());
    let mut previous_was_word = false;
    for ch in trimmed.chars() {
        let is_word = ch.is_ascii_alphanumeric() || ch == '_';
        if is_word && !previous_was_word {
            label.extend(ch.to_uppercase());
        } else {
            label.push(ch);
        }
        previous_was_word = is_word;
    }
    label
}

pub(super) fn agent_spawn_context_line(
    tool_call: &ToolCallEntry,
    task_row: Option<&OrchestrationTaskRow>,
) -> Option<String> {
    let status = task_row
        .map(|row| orchestration_task_state_label(row.state).to_string())
        .or_else(|| tool_json_string(tool_call.output_json.as_ref(), &["status"]));
    let completed = matches!(tool_call.status, ToolCallDisplayStatus::Succeeded)
        || status.as_deref() == Some("completed");
    let running = matches!(tool_call.status, ToolCallDisplayStatus::Running)
        || status.as_deref() == Some("running");
    let child_tool_call_count = task_row
        .map(|row| row.child_tool_call_count as u64)
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("child_tool_call_count"))
                .and_then(serde_json::Value::as_u64)
        });
    let resumed_existing_session = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("resumed_existing_session"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let duration_ms = task_row
        .and_then(OrchestrationTaskRow::duration_ms)
        .or_else(|| tool_call.duration_ms())
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("duration_ms"))
                .and_then(serde_json::Value::as_u64)
        });

    if completed {
        let mut parts = vec![format!(
            "{} toolcalls",
            child_tool_call_count.unwrap_or_default()
        )];
        parts.push(format_duration_ms(duration_ms.unwrap_or_default()));
        if resumed_existing_session {
            parts.push("resumed session".to_string());
        }
        if let Some(hint) = background_task_next_action_hint(task_row, tool_call) {
            parts.push(format!("details {hint}"));
        }
        return Some(format!("└ {}", parts.join(" · ")));
    }

    if running {
        if let Some(current_tool_title) = task_row
            .and_then(|row| row.current_child_tool_title.as_deref())
            .and_then(collapsed_inline_non_empty)
        {
            return Some(format!("↳ {current_tool_title}"));
        }
        if let Some(count) = child_tool_call_count.filter(|count| *count > 0) {
            return Some(format!("↳ {count} toolcalls"));
        }
        if let Some(hint) = background_task_next_action_hint(task_row, tool_call) {
            return Some(format!("↳ system notifies on completion · status {hint}"));
        }
    }

    let mut parts = Vec::new();
    if resumed_existing_session {
        parts.push("resumed session".to_string());
    }
    if let Some(hint) = background_task_next_action_hint(task_row, tool_call) {
        parts.push(format!("status {hint}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn background_task_next_action_hint(
    task_row: Option<&OrchestrationTaskRow>,
    tool_call: &ToolCallEntry,
) -> Option<String> {
    if !tool_output_indicates_background_task(tool_call.output_json.as_ref()) {
        return None;
    }

    let request_id = task_row
        .and_then(OrchestrationTaskRow::effective_child_request_id)
        .or_else(|| {
            tool_json_string_ref(
                tool_call.output_json.as_ref(),
                &["child_request_id", "request_id"],
            )
        })
        .or_else(|| {
            tool_call
                .lineage
                .as_ref()
                .and_then(|lineage| lineage.child_request_id.as_deref())
        })
        .and_then(collapsed_inline_non_empty)?;
    let mut actions = vec![format!("background_output(request_id=\"{request_id}\")")];
    if let Some(session_id) = task_row
        .and_then(OrchestrationTaskRow::effective_child_session_id)
        .or_else(|| task_tool_child_session_id(tool_call))
        .and_then(collapsed_inline_non_empty)
    {
        actions.push(format!("task(session_id=\"{session_id}\")"));
    }
    Some(actions.join(" · "))
}

fn tool_output_indicates_background_task(output_json: Option<&serde_json::Value>) -> bool {
    output_json
        .and_then(|value| value.get("background"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn orchestration_task_state_label(state: OrchestrationTaskState) -> &'static str {
    match state {
        OrchestrationTaskState::Queued => "queued",
        OrchestrationTaskState::Running => "running",
        OrchestrationTaskState::Stale => "stale",
        OrchestrationTaskState::Completed => "completed",
        OrchestrationTaskState::Cancelled => "cancelled",
        OrchestrationTaskState::Failed => "failed",
        OrchestrationTaskState::TimedOut => "timed out",
        OrchestrationTaskState::LateResult => "late result",
    }
}

fn collapsed_inline_non_empty(text: &str) -> Option<String> {
    let collapsed = collapse_inline_whitespace(text);
    (!collapsed.is_empty()).then_some(collapsed)
}
