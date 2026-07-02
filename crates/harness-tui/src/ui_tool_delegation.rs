use std::collections::BTreeSet;

use crate::app::{AppState, ToolCallEntry};
use crate::text::{collapse_inline_whitespace, non_empty_trimmed};

use super::ui_tool_metadata::{
    task_tool_child_request_id_from_output, task_tool_child_session_id_from_output,
    tool_summary_string,
};

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

    match description {
        Some(description) => description,
        None => format!(
            "{} Task",
            subagent_profile_label(profile.as_deref().unwrap_or("General"))
        ),
    }
}

pub(super) fn agent_spawn_subtitle(tool_call: &ToolCallEntry) -> Option<String> {
    let _ = agent_spawn_description(tool_call)?;
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
    Some(format!(
        "{} Agent",
        subagent_profile_label(profile.as_deref().unwrap_or("General"))
    ))
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
