use std::path::{Component, Path};

use harness_core::event::{EventEnvelopeV1, EventV1};
use serde_json::Value;

use super::{json_string_field, AppState};
use crate::text::non_empty_trimmed;

#[derive(Debug, Clone)]
pub(super) struct ChildTaskInfo {
    pub(super) label: Option<String>,
    pub(super) description: Option<String>,
    pub(super) request_id: Option<String>,
}

pub(super) fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

pub(super) fn safe_session_id_path_component(session_id: &str) -> Option<&str> {
    let session_id = session_id.trim();
    if session_id.is_empty()
        || session_id.contains(['/', '\\'])
        || session_id.chars().any(char::is_control)
    {
        return None;
    }

    let mut components = Path::new(session_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component.to_str() == Some(session_id) => {
            Some(session_id)
        }
        _ => None,
    }
}

pub(super) fn child_task_info_from_events(
    events: &[EventEnvelopeV1],
    current_session_id: &str,
) -> Option<ChildTaskInfo> {
    events.iter().rev().find_map(|event| {
        let EventV1::ToolCallRequested(tool_call) = &event.payload else {
            return None;
        };
        let lineage_session = tool_call
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref())
            .and_then(non_empty_trimmed);
        let args = serde_json::from_str::<Value>(&tool_call.args_summary).ok();
        let output_session = args.as_ref().and_then(|value| {
            json_string_field(Some(value), &["child_session_id", "session_id", "task_id"])
        });
        if lineage_session != Some(current_session_id)
            && output_session.as_deref() != Some(current_session_id)
        {
            return None;
        }

        let request_id = tool_call
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref())
            .and_then(non_empty_trimmed)
            .map(str::to_string)
            .or_else(|| {
                args.as_ref().and_then(|value| {
                    json_string_field(Some(value), &["child_request_id", "request_id"])
                })
            });

        Some(ChildTaskInfo {
            label: args.as_ref().and_then(|value| {
                json_string_field(Some(value), &["subagent_type", "profile", "profile_name"])
            }),
            description: args
                .as_ref()
                .and_then(|value| json_string_field(Some(value), &["description", "task"])),
            request_id,
        })
    })
}

pub(super) fn child_agent_info_from_events(
    events: &[EventEnvelopeV1],
    current_session_id: &str,
) -> Option<ChildTaskInfo> {
    events.iter().find_map(|event| {
        let EventV1::AgentSpawned(agent) = &event.payload else {
            return None;
        };
        (agent.agent_id == current_session_id).then(|| ChildTaskInfo {
            label: Some(agent.profile.clone()),
            description: None,
            request_id: None,
        })
    })
}

pub(super) fn subagent_usage_label(app: &AppState) -> Option<String> {
    let total_tokens = app
        .activities
        .iter()
        .filter_map(|activity| activity.usage)
        .map(|usage| u64::from(usage.total_tokens))
        .sum::<u64>();
    if total_tokens == 0 {
        return None;
    }
    Some(compact_usage_count(total_tokens))
}

fn compact_usage_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!(
            "{:.1}M",
            f64::from(u32::try_from(value).unwrap_or(u32::MAX)) / 1_000_000.0
        )
    } else if value >= 1_000 {
        format!(
            "{:.1}K",
            f64::from(u32::try_from(value).unwrap_or(u32::MAX)) / 1_000.0
        )
    } else {
        value.to_string()
    }
}

pub(super) fn sibling_session_id(
    session_ids: &[String],
    current_session_id: &str,
    reverse: bool,
) -> Option<String> {
    if session_ids.is_empty() {
        return None;
    }

    let current_index = session_ids
        .iter()
        .position(|session_id| session_id == current_session_id)?;
    let next_index = if reverse {
        current_index
            .checked_sub(1)
            .unwrap_or(session_ids.len().saturating_sub(1))
    } else {
        (current_index + 1) % session_ids.len()
    };
    session_ids.get(next_index).cloned()
}
