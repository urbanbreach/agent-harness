use crate::text::{
    has_trimmed_content, trimmed_json_nested_string_field, trimmed_json_string_field,
};

use super::{ToolCallDisplayStatus, ToolCallEntry};

pub(super) fn json_string_field(
    output_json: Option<&serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    trimmed_json_string_field(output_json, keys)
}

pub(super) fn task_child_session_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &[
            "child_session_id",
            "session_id",
            "task_id",
            "childSessionId",
            "sessionId",
            "taskId",
        ],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["child_session", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["childSession", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "sessionId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "sessionId"])
    })
}

pub(super) fn task_child_request_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &[
            "child_request_id",
            "request_id",
            "childRequestId",
            "requestId",
        ],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["child_session", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["childSession", "requestId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "requestId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "requestId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "requestId"])
    })
}

pub(super) fn tool_call_has_expandable_output(tool_call: &ToolCallEntry) -> bool {
    if matches!(
        tool_call.effective_tool_id(),
        "fs.read" | "read" | "fs.glob" | "glob" | "fs.grep" | "grep" | "fs.ls" | "list"
    ) {
        return true;
    }

    if matches!(tool_call.effective_tool_id(), "shell.run" | "bash")
        && shell_tool_output_for_expansion(tool_call).is_some_and(has_trimmed_content)
    {
        return true;
    }

    if matches!(tool_call.effective_tool_id(), "edit" | "fs.write")
        && serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_some_and(|object| {
                let path = object
                    .get("filePath")
                    .or_else(|| object.get("path"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(has_trimmed_content);
                let inline_preview = match tool_call.effective_tool_id() {
                    "edit" => {
                        object
                            .get("oldString")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                            || object
                                .get("newString")
                                .and_then(serde_json::Value::as_str)
                                .is_some()
                    }
                    "fs.write" => object
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .is_some(),
                    _ => false,
                };
                path && inline_preview
            })
    {
        return true;
    }

    if tool_call.effective_tool_id() == "apply_patch"
        && tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("files"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|files| !files.is_empty())
    {
        return true;
    }

    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && tool_call.effective_tool_id().starts_with("mcp.")
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(has_trimmed_content)
    {
        return true;
    }

    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let line_count = output.lines().count();
    let has_diff_preview = tool_call
        .edit
        .as_ref()
        .and_then(|edit| edit.diff_rel_path.as_ref())
        .is_some()
        || tool_call
            .artifact_refs
            .iter()
            .any(|artifact| artifact.path.ends_with(".diff"));
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" | "bash" => line_count > 10,
            "edit.hashline_apply" | "fs.write" | "edit" | "apply_patch" => has_diff_preview,
            "agent.spawn" => true,
            _ => has_trimmed_content(output) && line_count > 3,
        }
}

fn shell_tool_output_for_expansion(tool_call: &ToolCallEntry) -> Option<&str> {
    tool_call.output_summary.as_deref().or_else(|| {
        let output_json = tool_call.output_json.as_ref()?;
        output_json
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .filter(|stdout| has_trimmed_content(stdout))
            .or_else(|| {
                output_json
                    .get("stderr")
                    .and_then(serde_json::Value::as_str)
                    .filter(|stderr| has_trimmed_content(stderr))
            })
    })
}
