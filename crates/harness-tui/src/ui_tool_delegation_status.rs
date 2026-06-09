use crate::app::{
    OrchestrationTaskRow, OrchestrationTaskState, ToolCallDisplayStatus, ToolCallEntry,
};

use super::super::ui_tool_metadata::tool_json_string_ref;

pub(in crate::ui) fn agent_spawn_display_status(
    tool_call: &ToolCallEntry,
    task_row: Option<&OrchestrationTaskRow>,
) -> ToolCallDisplayStatus {
    let output_status = agent_spawn_output_display_status(tool_call.output_json.as_ref());
    if let Some(status) = output_status.filter(|status| tool_display_status_is_terminal(*status)) {
        return status;
    }
    if let Some(status) = task_row
        .map(|row| orchestration_task_display_status(row.state))
        .filter(|status| tool_display_status_is_terminal(*status))
    {
        return status;
    }
    if tool_display_status_is_terminal(tool_call.status)
        && !output_status.is_some_and(tool_display_status_is_active)
    {
        return tool_call.status;
    }
    output_status
        .or_else(|| task_row.map(|row| orchestration_task_display_status(row.state)))
        .unwrap_or(tool_call.status)
}

fn agent_spawn_output_display_status(
    output_json: Option<&serde_json::Value>,
) -> Option<ToolCallDisplayStatus> {
    let status = tool_json_string_ref(output_json, &["status", "final_status"])?;
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" => Some(ToolCallDisplayStatus::Queued),
        "scheduled" | "running" | "in_progress" => Some(ToolCallDisplayStatus::Running),
        "completed" | "succeeded" | "success" => Some(ToolCallDisplayStatus::Succeeded),
        "cancelled" | "failed" | "timed_out" | "error" => Some(ToolCallDisplayStatus::Failed),
        _ => None,
    }
}

fn orchestration_task_display_status(state: OrchestrationTaskState) -> ToolCallDisplayStatus {
    match state {
        OrchestrationTaskState::Queued => ToolCallDisplayStatus::Queued,
        OrchestrationTaskState::Running | OrchestrationTaskState::Stale => {
            ToolCallDisplayStatus::Running
        }
        OrchestrationTaskState::Completed | OrchestrationTaskState::LateResult => {
            ToolCallDisplayStatus::Succeeded
        }
        OrchestrationTaskState::Cancelled
        | OrchestrationTaskState::Failed
        | OrchestrationTaskState::TimedOut => ToolCallDisplayStatus::Failed,
    }
}

fn tool_display_status_is_terminal(status: ToolCallDisplayStatus) -> bool {
    matches!(
        status,
        ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed
    )
}

fn tool_display_status_is_active(status: ToolCallDisplayStatus) -> bool {
    matches!(
        status,
        ToolCallDisplayStatus::PendingPermission
            | ToolCallDisplayStatus::Queued
            | ToolCallDisplayStatus::Running
    )
}
