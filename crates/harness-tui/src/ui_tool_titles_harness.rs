use crate::app::{ToolCallDisplayStatus, ToolCallEntry};

use super::ui_tool_metadata::{tool_json_string, tool_summary_string};

pub(super) fn background_cancel_tool_title(tool_call: &ToolCallEntry) -> String {
    let request_id = tool_json_string(tool_call.output_json.as_ref(), &["request_id"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["request_id"]))
        .or_else(|| tool_json_string(tool_call.output_json.as_ref(), &["task_id", "session_id"]))
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["task_id", "session_id"]));

    let label = match tool_call.status {
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
            "Cancel background task"
        }
        ToolCallDisplayStatus::Running => "Cancelling background task...",
        ToolCallDisplayStatus::Succeeded => "Cancelled background task",
        ToolCallDisplayStatus::Failed => "Background cancel failed",
    };
    request_id
        .map(|id| format!("{label} · {id}"))
        .unwrap_or_else(|| label.to_string())
}

pub(super) fn plan_enter_tool_title(tool_call: &ToolCallEntry) -> String {
    let goal = tool_summary_string(&tool_call.args_summary, &["goal", "reason"]);
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
            "Enter plan mode".to_string()
        }
        ToolCallDisplayStatus::Running => "Entering plan mode...".to_string(),
        ToolCallDisplayStatus::Succeeded => goal
            .as_deref()
            .map(|g| format!("Plan mode · {g}"))
            .unwrap_or_else(|| "Entered plan mode".to_string()),
        ToolCallDisplayStatus::Failed => "Plan mode entry failed".to_string(),
    }
}

pub(super) fn plan_exit_tool_title(tool_call: &ToolCallEntry) -> String {
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
            "Exit plan mode".to_string()
        }
        ToolCallDisplayStatus::Running => "Exiting plan mode...".to_string(),
        ToolCallDisplayStatus::Succeeded => "Exited plan mode".to_string(),
        ToolCallDisplayStatus::Failed => "Plan mode exit failed".to_string(),
    }
}

pub(super) fn invalid_tool_title(tool_call: &ToolCallEntry) -> String {
    let original = tool_json_string(tool_call.output_json.as_ref(), &["tool", "tool_id"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["tool", "tool_id"]));
    match tool_call.status {
        ToolCallDisplayStatus::Failed => original
            .as_deref()
            .map(|t| format!("Invalid tool · {t}"))
            .unwrap_or_else(|| "Invalid tool call".to_string()),
        _ => "Invalid tool call".to_string(),
    }
}

pub(super) fn session_tool_title(tool_call: &ToolCallEntry, verb: &str) -> String {
    let query = tool_summary_string(&tool_call.args_summary, &["query", "session_id", "run_id"]);
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
            format!("{verb} session")
        }
        ToolCallDisplayStatus::Running => format!("{verb} session..."),
        ToolCallDisplayStatus::Succeeded => query
            .as_deref()
            .map(|q| format!("{verb} session · {q}"))
            .unwrap_or_else(|| format!("{verb} session")),
        ToolCallDisplayStatus::Failed => format!("{verb} session failed"),
    }
}

pub(super) fn ast_grep_tool_title(tool_call: &ToolCallEntry, verb: &str) -> String {
    let pattern = tool_summary_string(&tool_call.args_summary, &["pattern"]);
    let suffix = pattern
        .as_deref()
        .map(|p| format!(" \"{p}\""))
        .unwrap_or_default();
    format!("{verb}{suffix}")
}

pub(super) fn lsp_tool_title(tool_call: &ToolCallEntry) -> String {
    let operation = tool_summary_string(&tool_call.args_summary, &["operation", "command"])
        .unwrap_or_else(|| "lsp".to_string());
    let path = tool_summary_string(&tool_call.args_summary, &["path"]);
    let suffix = path
        .as_deref()
        .map(|p| format!(" · {p}"))
        .unwrap_or_default();
    format!("LSP {operation}{suffix}")
}

pub(super) fn skill_tool_title(tool_call: &ToolCallEntry) -> String {
    let name = tool_summary_string(&tool_call.args_summary, &["name"])
        .unwrap_or_else(|| "skill".to_string());
    format!("Load skill {name}")
}
