use crate::app::ToolCallEntry;
use crate::text::collapse_inline_whitespace;

use super::ui_tool_input::{compact_tool_trigger_subtitle, tool_input_args, tool_input_label};
use super::ui_tool_metadata::{tool_json_nested_string, tool_json_string, tool_summary_string};
use super::ui_tool_style::status_label;

pub(super) fn generic_tool_title(tool_call: &ToolCallEntry, tool_id: &str) -> String {
    let suffix = compact_tool_trigger_subtitle(
        tool_input_label(&tool_call.args_summary, false),
        tool_input_args(&tool_call.args_summary, false, &[]),
    );
    match suffix {
        Some(suffix) => format!("{} {}", generic_tool_name(tool_id), suffix),
        None => generic_tool_name(tool_id),
    }
}

fn generic_tool_name(tool_id: &str) -> String {
    tool_id.trim().to_string()
}

pub(super) fn background_output_tool_title(tool_call: &ToolCallEntry) -> String {
    status_label(
        tool_call.status,
        "Check background output",
        "Checking background output...",
        "Checked background output",
        "Background output check failed",
    )
    .to_string()
}

pub(super) fn background_output_tool_subtitle(tool_call: &ToolCallEntry) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(request_id) = tool_json_string(tool_call.output_json.as_ref(), &["request_id"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["request_id"]))
    {
        parts.push(request_id);
    } else if let Some(task_id) = tool_json_string(tool_call.output_json.as_ref(), &["task_id"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["task_id", "session_id"]))
    {
        parts.push(task_id);
    }

    if let Some(status) = tool_json_string(tool_call.output_json.as_ref(), &["status"]) {
        parts.push(status);
    }

    if let Some(count) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("child_tool_call_count"))
        .and_then(serde_json::Value::as_u64)
    {
        parts.push(format!(
            "{count} child tool call{}",
            if count == 1 { "" } else { "s" }
        ));
    }

    if let Some(duration_ms) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("duration_ms"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_call.duration_ms())
    {
        parts.push(format_duration_ms(duration_ms));
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(super) fn batch_tool_title(tool_call: &ToolCallEntry) -> String {
    let count = tool_call
        .args_summary
        .parse::<serde_json::Value>()
        .ok()
        .and_then(|value| {
            value
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
        })
        .map(|count| u64::try_from(count).unwrap_or(0))
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("requested_call_count"))
                .and_then(serde_json::Value::as_u64)
        })
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("processed_call_count"))
                .and_then(serde_json::Value::as_u64)
        });

    count
        .map(|count| format!("Batch {count} tool{}", if count == 1 { "" } else { "s" }))
        .unwrap_or_else(|| "Batch".to_string())
}

pub(super) fn write_tool_title(tool_call: &ToolCallEntry) -> String {
    let path = tool_summary_string(&tool_call.args_summary, &["filePath", "path"]);
    match tool_call.status {
        crate::app::ToolCallDisplayStatus::Succeeded => path
            .map(|path| format!("Created {path}"))
            .unwrap_or_else(|| "Created".to_string()),
        crate::app::ToolCallDisplayStatus::Failed => path
            .map(|path| format!("Write {path}"))
            .unwrap_or_else(|| "Write".to_string()),
        crate::app::ToolCallDisplayStatus::PendingPermission
        | crate::app::ToolCallDisplayStatus::Queued
        | crate::app::ToolCallDisplayStatus::Running => path
            .map(|path| format!("Creating {path}"))
            .unwrap_or_else(|| "Creating file...".to_string()),
    }
}

pub(super) fn edit_tool_title(tool_call: &ToolCallEntry) -> String {
    tool_summary_string(&tool_call.args_summary, &["filePath", "path"])
        .map(|path| format!("Edit {path}"))
        .unwrap_or_else(|| "Edit".to_string())
}

pub(super) fn mcp_tool_title(tool_call: &ToolCallEntry, display_tool_id: &str) -> String {
    let title = mcp_display_name(tool_call, display_tool_id);
    let suffix = compact_tool_trigger_subtitle(
        tool_input_label(&tool_call.args_summary, true),
        tool_input_args(&tool_call.args_summary, true, &["tool"]),
    );
    match suffix {
        Some(suffix) => format!("{title} {suffix}"),
        None => title,
    }
}

fn mcp_display_name(tool_call: &ToolCallEntry, display_tool_id: &str) -> String {
    let server = mcp_server_name(tool_call, display_tool_id);
    if let Some(tool) = mcp_remote_tool_name(tool_call, display_tool_id) {
        return server
            .map(|server| format!("{server}_{tool}"))
            .unwrap_or(tool);
    }

    let fallback = mcp_tool_suffix(display_tool_id)
        .map(|suffix| suffix.replace('.', "_"))
        .or_else(|| mcp_tool_id_body(display_tool_id).map(str::to_string))
        .unwrap_or_else(|| display_tool_id.to_string());
    server
        .map(|server| format!("{server}_{fallback}"))
        .unwrap_or(fallback)
}

fn mcp_server_name(tool_call: &ToolCallEntry, display_tool_id: &str) -> Option<String> {
    tool_json_nested_string(tool_call.output_json.as_ref(), &["server", "id"]).or_else(|| {
        mcp_tool_id_parts(display_tool_id)
            .map(|(server, _)| collapse_inline_whitespace(server))
            .filter(|server| !server.is_empty())
    })
}

fn mcp_tool_suffix(display_tool_id: &str) -> Option<&str> {
    mcp_tool_id_parts(display_tool_id).map(|(_, suffix)| suffix)
}

pub(super) fn is_mcp_tool_id(tool_id: &str) -> bool {
    mcp_tool_id_body(tool_id).is_some()
}

fn mcp_tool_id_body(tool_id: &str) -> Option<&str> {
    tool_id.strip_prefix("mcp.")
}

fn mcp_tool_id_parts(tool_id: &str) -> Option<(&str, &str)> {
    mcp_tool_id_body(tool_id)?.split_once('.')
}

fn mcp_remote_tool_name(tool_call: &ToolCallEntry, display_tool_id: &str) -> Option<String> {
    tool_json_nested_string(tool_call.output_json.as_ref(), &["payload", "tool"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["tool"]))
        .or_else(|| {
            let suffix = mcp_tool_suffix(display_tool_id)?;
            (!matches!(
                suffix,
                "tools.list"
                    | "tool.call"
                    | "resources.list"
                    | "resource.read"
                    | "prompts.list"
                    | "prompt.get"
            ))
            .then(|| suffix.rsplit('.').next().unwrap_or(suffix).to_string())
        })
        .map(|value| collapse_inline_whitespace(&value))
        .filter(|value| !value.is_empty())
}

pub(super) fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 86_400_000 {
        format!(
            "{}d {}h",
            duration_ms / 86_400_000,
            (duration_ms % 86_400_000) / 3_600_000
        )
    } else if duration_ms >= 3_600_000 {
        format!(
            "{}h {}m",
            duration_ms / 3_600_000,
            (duration_ms % 3_600_000) / 60_000
        )
    } else if duration_ms >= 60_000 {
        format!(
            "{}m {}s",
            duration_ms / 60_000,
            (duration_ms % 60_000) / 1_000
        )
    } else if duration_ms >= 1_000 {
        format!(
            "{:.1}s",
            f64::from(u32::try_from(duration_ms).unwrap_or(u32::MAX)) / 1_000.0
        )
    } else {
        format!("{duration_ms}ms")
    }
}
