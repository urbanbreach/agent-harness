use std::collections::BTreeSet;

use crate::app::{AppState, ToolCallEntry};
use crate::text::{collapse_inline_whitespace, non_empty_trimmed};

use super::ui_tool_metadata::{
    task_tool_child_request_id_from_output, task_tool_child_session_id_from_output,
    tool_summary_string,
};
use super::ui_tool_paths::tool_id_matches;

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
            .filter(|tool_call| tool_id_matches(tool_call, &["agent.spawn", "task"]))
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

pub(super) fn agent_spawn_title(
    description: Option<String>,
    verb: &str,
    activity: Option<&str>,
) -> String {
    let description = description.unwrap_or_default();
    let activity = activity
        .filter(|activity| !activity.is_empty())
        .map(|activity| format!(" · {}", collapse_inline_whitespace(activity)))
        .unwrap_or_default();
    format!("Subagent {verb}: “{description}”{activity}")
}

pub(super) fn agent_spawn_subtitle(tool: &ToolCallEntry, app: &AppState) -> String {
    use super::ui_tool_metadata::tool_json_string;
    use harness_core::event::EventV1;

    let projection = app.subagent_request_projection(tool);
    let task = app.transcript_task_row_for_tool_call(tool);
    let child_session =
        task_tool_child_session_id(tool).or_else(|| task.as_ref()?.effective_child_session_id());
    let child_request = task_tool_child_request_id(tool)
        .or_else(|| projection.as_ref().map(|row| row.request_id.as_str()));
    let output = tool.output_json.as_ref();
    let route = output.and_then(|value| value.get("route"));
    let agent = app
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(data) if child_session == Some(data.agent_id.as_str()) => {
                Some(data.profile.clone())
            }
            _ => None,
        })
        .or_else(|| tool_json_string(route, &["resolved_profile", "profile_id"]))
        .or_else(|| tool_json_string(output, &["profile", "subagent_type"]))
        .or_else(|| {
            tool_summary_string(
                &tool.args_summary,
                &["persona", "subagent_type", "profile", "role", "category"],
            )
        })
        .unwrap_or_else(|| "Subagent".into());
    let model = app
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if child_request == Some(data.request_id.as_str())
                    || child_request
                        .is_some_and(|id| event.correlation_id.as_deref() == Some(id))
                    || (child_request.is_none()
                        && task_tool_child_session_id(tool).is_some_and(|child| {
                            event.actor.agent_id.as_deref() == Some(child)
                        })) =>
            {
                Some(data.model_id.clone())
            }
            EventV1::TaskScheduled(data) => {
                let lineage = data.metadata.as_ref()?.lineage.as_ref()?;
                (lineage.parent_tool_call_id.as_deref() == Some(tool.tool_call_id.as_str()))
                    .then(|| lineage.child_model_id.clone())
                    .flatten()
            }
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool.tool_call_id => {
                data.metadata
                    .as_ref()?
                    .lineage
                    .as_ref()?
                    .child_model_id
                    .clone()
            }
            _ => None,
        })
        .or_else(|| {
            tool_json_string(
                route.and_then(|route| route.get("model")),
                &["model", "model_ref"],
            )
        })
        .or_else(|| tool_json_string(output, &["model", "model_id"]))
        .or_else(|| tool_summary_string(&tool.args_summary, &["model"]))
        .unwrap_or_else(|| "model unknown".into());
    format!("{} · {model}", subagent_profile_label(&agent))
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

pub(super) fn agent_spawn_is_background(tool_call: &ToolCallEntry) -> bool {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("background"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
            .ok()
            .and_then(|value| {
                value
                    .get("background")
                    .or_else(|| value.get("run_in_background"))
                    .cloned()
            })
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}
