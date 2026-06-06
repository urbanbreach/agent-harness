use std::path::Path;

use serde_json::{json, Value};

use crate::config::registered_mcp_server_first_class_tool_id;
use crate::digest::digest12_json;
use crate::edit::hashline::HashlinePatch;
use crate::event::{
    EventArtifactRef, ExecutionTimingMetadata, HookExecutionMetadata, HookExecutionStatus,
    TaskLineageMetadata, ToolCallMetadata, ToolIdentityMetadata,
};
use crate::text::non_empty_trimmed;
use crate::tool::{canonical_tool_id_for, sanitize_mcp_tool_segment, ToolResult};

use super::HASHLINE_APPLY_TOOL_ID;

#[derive(Debug, Clone)]
pub(super) struct HashlineEditMetadata {
    pub(super) edit_id: String,
    pub(super) path: String,
    pub(super) summary: String,
    pub(super) patch_digest: String,
}

#[derive(Debug, Clone)]
pub(super) struct AppliedToolEditMetadata {
    pub(super) metadata: HashlineEditMetadata,
    pub(super) diff_rel_path: Option<String>,
    pub(super) diff_digest: Option<String>,
    pub(super) deleted: bool,
}

pub(super) fn hashline_edit_metadata(
    tool_id: &str,
    args_json: &Value,
    tool_call_id: &str,
) -> Option<HashlineEditMetadata> {
    if tool_id != HASHLINE_APPLY_TOOL_ID {
        let canonical_tool_id = canonical_tool_id_for(tool_id)?;
        if canonical_tool_id != "edit" {
            return None;
        }

        let path = args_json
            .get("path")
            .or_else(|| args_json.get("filePath"))
            .and_then(Value::as_str)?;
        let (edit_id, summary) = (
            edit_id_from_native_edit_args(args_json, tool_call_id),
            "rewrite file through native edit tool".to_string(),
        );

        return Some(HashlineEditMetadata {
            edit_id,
            path: path.to_string(),
            summary,
            patch_digest: digest12_json(args_json),
        });
    }

    let patch: HashlinePatch = serde_json::from_value(args_json.clone()).ok()?;
    let patch_digest = digest12_json(&patch);

    Some(HashlineEditMetadata {
        edit_id: patch.edit_id,
        path: patch.path,
        summary: format!("apply hashline patch with {} op(s)", patch.ops.len()),
        patch_digest,
    })
}

fn edit_id_from_native_edit_args(args_json: &Value, tool_call_id: &str) -> String {
    args_json
        .get("editId")
        .or_else(|| args_json.get("edit_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("edit-{tool_call_id}"))
}

fn hashline_diff_refs(result: &ToolResult) -> (Option<String>, Option<String>) {
    let structured = result.structured_json.as_ref().and_then(Value::as_object);
    let structured_path = structured
        .and_then(|value| value.get("diff_rel_path"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let structured_digest = structured
        .and_then(|value| value.get("diff_digest"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    if structured_path.is_some() && structured_digest.is_some() {
        return (structured_path, structured_digest);
    }

    let artifact = result
        .artifacts
        .iter()
        .find(|artifact| artifact.path.ends_with(".diff"));
    let artifact_path = artifact.map(|artifact| artifact.path.clone());
    let artifact_digest = artifact.and_then(|artifact| artifact.digest.clone());

    (
        structured_path.or(artifact_path),
        structured_digest.or(artifact_digest),
    )
}

pub(super) fn applied_tool_edit_metadata(
    _tool_id: &str,
    result: &ToolResult,
    fallback: Option<&HashlineEditMetadata>,
) -> Vec<AppliedToolEditMetadata> {
    let Some(metadata) = fallback else {
        return Vec::new();
    };
    let structured = result.structured_json.as_ref().and_then(Value::as_object);
    let mut metadata = metadata.clone();
    if let Some(edit_id) = structured
        .and_then(|value| value.get("edit_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
    {
        metadata.edit_id = edit_id.to_string();
    }
    if let Some(path) = structured
        .and_then(|value| value.get("path"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
    {
        metadata.path = path.to_string();
    }
    let deleted = structured
        .and_then(|value| value.get("resolved_to_path"))
        .is_none()
        && structured
            .and_then(|value| value.get("resolved_path"))
            .and_then(Value::as_str)
            .is_some_and(|path| !Path::new(path).exists());
    let (diff_rel_path, diff_digest) = hashline_diff_refs(result);
    vec![AppliedToolEditMetadata {
        metadata,
        diff_rel_path,
        diff_digest,
        deleted,
    }]
}

pub(super) fn requested_tool_call_metadata(
    tool_id: &str,
    args_json: &Value,
) -> Option<ToolCallMetadata> {
    let tool_identity = tool_identity_metadata(tool_id, args_json);
    tool_call_metadata(tool_identity.as_ref(), None, Vec::new(), None, Vec::new())
}

pub(super) fn tool_identity_metadata(
    tool_id: &str,
    args_json: &Value,
) -> Option<ToolIdentityMetadata> {
    if let Some(canonical_tool_id) = effective_mcp_tool_id(tool_id, args_json) {
        return Some(ToolIdentityMetadata {
            canonical_tool_id: Some(canonical_tool_id),
            alias_source_tool_id: None,
        });
    }

    Some(ToolIdentityMetadata {
        canonical_tool_id: Some(tool_id.to_string()),
        alias_source_tool_id: None,
    })
}

pub(super) fn effective_mcp_tool_id(tool_id: &str, args_json: &Value) -> Option<String> {
    let mut segments = tool_id.split('.');
    let Some("mcp") = segments.next() else {
        return None;
    };
    let server_id = segments.next()?.trim();
    if server_id.is_empty() {
        return None;
    }

    let suffix = segments.collect::<Vec<_>>().join(".");
    if suffix == "tool.call" {
        let remote_tool_name = args_json
            .get("tool")
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)?;
        if let Some(tool_id) =
            registered_mcp_server_first_class_tool_id(server_id, remote_tool_name)
        {
            return Some(tool_id);
        }
        return Some(format!(
            "mcp.{server_id}.{}",
            sanitize_mcp_tool_segment(remote_tool_name)
        ));
    }

    Some(tool_id.to_string())
}

pub(super) fn tool_call_metadata(
    tool_identity: Option<&ToolIdentityMetadata>,
    lineage: Option<TaskLineageMetadata>,
    artifact_refs: Vec<EventArtifactRef>,
    timing: Option<ExecutionTimingMetadata>,
    hook_executions: Vec<HookExecutionMetadata>,
) -> Option<ToolCallMetadata> {
    let canonical_tool_id = tool_identity.and_then(|value| value.canonical_tool_id.clone());
    let alias_source_tool_id = tool_identity.and_then(|value| value.alias_source_tool_id.clone());

    if canonical_tool_id.is_none()
        && alias_source_tool_id.is_none()
        && lineage.is_none()
        && artifact_refs.is_empty()
        && timing.is_none()
        && hook_executions.is_empty()
    {
        return None;
    }

    Some(ToolCallMetadata {
        canonical_tool_id,
        alias_source_tool_id,
        lineage,
        artifact_refs,
        timing,
        hook_executions,
    })
}

pub(super) fn event_artifact_refs(artifacts: &[crate::tool::ArtifactRef]) -> Vec<EventArtifactRef> {
    let mut refs = artifacts
        .iter()
        .map(|artifact| EventArtifactRef {
            path: artifact.path.clone(),
            digest: artifact.digest.clone(),
        })
        .collect::<Vec<_>>();
    refs.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    refs
}

pub(super) fn execution_timing_metadata(
    started_mono_ms: u64,
    finished_mono_ms: u64,
) -> ExecutionTimingMetadata {
    ExecutionTimingMetadata {
        started_mono_ms: Some(started_mono_ms),
        finished_mono_ms: Some(finished_mono_ms),
        elapsed_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
    }
}

pub(super) fn tool_task_lineage_metadata(
    parent_tool_call_id: &str,
    parent_request_id: Option<&str>,
    output_json: Option<&Value>,
) -> TaskLineageMetadata {
    TaskLineageMetadata {
        parent_tool_call_id: Some(parent_tool_call_id.to_string()),
        parent_task_id: None,
        parent_request_id: parent_request_id.map(ToOwned::to_owned),
        parent_session_id: extract_lineage_value(output_json, &["parent_session_id"]),
        child_session_id: extract_lineage_value(
            output_json,
            &["child_session_id", "session_id", "task_id"],
        ),
        child_request_id: extract_lineage_value(output_json, &["child_request_id", "request_id"]),
        child_provider_id: extract_lineage_value(
            output_json,
            &["child_provider_id", "provider_id", "provider"],
        ),
        child_model_id: extract_lineage_value(
            output_json,
            &["child_model_id", "model_id", "model"],
        ),
    }
}

fn extract_lineage_value(output_json: Option<&Value>, candidate_keys: &[&str]) -> Option<String> {
    let root = output_json?.as_object()?;
    for key in candidate_keys {
        if let Some(value) = root
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    let nested = root.get("lineage").and_then(Value::as_object)?;
    for key in candidate_keys {
        if let Some(value) = nested
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    None
}

pub(super) fn stable_tool_output_json(
    structured_output: Option<Value>,
    output_summary: &str,
    artifact_refs: &[EventArtifactRef],
    lineage: &TaskLineageMetadata,
    timing: &ExecutionTimingMetadata,
    hook_executions: &[HookExecutionMetadata],
) -> Value {
    let harness_metadata = json!({
        "output_summary": output_summary,
        "artifact_refs": artifact_refs,
        "lineage": lineage,
        "timing": timing,
        "hook_executions": hook_executions,
    });

    match structured_output {
        Some(Value::Object(mut value)) => {
            value.insert("_harness".to_string(), harness_metadata);
            Value::Object(value)
        }
        Some(value) => json!({
            "_harness": harness_metadata,
            "structured_output": value,
        }),
        None => json!({
            "_harness": harness_metadata,
        }),
    }
}

pub(super) fn extract_hook_execution_metadata(
    output_json: Option<&Value>,
) -> Vec<HookExecutionMetadata> {
    let Some(output_json) = output_json else {
        return Vec::new();
    };

    let mut hook_executions = Vec::new();
    for source in [
        output_json.get("hook_executions"),
        output_json.get("hooks"),
        output_json
            .get("_harness")
            .and_then(|harness| harness.get("hook_executions")),
    ] {
        let Some(items) = source.and_then(Value::as_array) else {
            continue;
        };

        for item in items {
            let Some(parsed) = parse_hook_execution_metadata(item) else {
                continue;
            };
            if hook_executions.iter().any(|existing| existing == &parsed) {
                continue;
            }
            hook_executions.push(parsed);
        }
    }

    hook_executions
}

fn parse_hook_execution_metadata(value: &Value) -> Option<HookExecutionMetadata> {
    let object = value.as_object()?;
    let hook_name = extract_object_string(object, &["hook_name", "name", "hook", "id", "hook_id"])
        .or_else(|| {
            object
                .get("hook")
                .and_then(Value::as_object)
                .and_then(|hook| extract_object_string(hook, &["name", "id"]))
        })
        .unwrap_or_else(|| "unknown_hook".to_string());

    let status = extract_object_string(object, &["status", "result", "outcome"])
        .map(|status| parse_hook_execution_status(&status))
        .unwrap_or_default();

    Some(HookExecutionMetadata {
        hook_name,
        status,
        hook_event: extract_object_string(object, &["hook_event", "event", "phase", "trigger"]),
        command_digest: extract_object_string(
            object,
            &["command_digest", "command_hash", "command_blake3"],
        ),
        output_digest: extract_object_string(object, &["output_digest", "result_digest", "digest"]),
        output_summary: extract_object_string(
            object,
            &["output_summary", "summary", "message", "output_message"],
        ),
        duration_ms: extract_object_u64(object, &["duration_ms", "elapsed_ms"]),
    })
}

fn extract_object_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object
            .get(*key)
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)
            .map(ToOwned::to_owned)
        {
            return Some(value);
        }
    }

    None
}

fn extract_object_u64(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(value) = object.get(*key).and_then(Value::as_u64) {
            return Some(value);
        }
    }

    None
}

fn parse_hook_execution_status(status: &str) -> HookExecutionStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "succeeded" | "success" | "ok" | "passed" => HookExecutionStatus::Succeeded,
        "failed" | "error" => HookExecutionStatus::Failed,
        "skipped" | "ignored" => HookExecutionStatus::Skipped,
        _ => HookExecutionStatus::Unknown,
    }
}

pub(super) fn failed_tool_output_json(
    reason: &str,
    hook_executions: &[HookExecutionMetadata],
) -> Value {
    json!({
        "_harness": {
            "status": "failed",
            "error": reason,
            "hook_executions": hook_executions,
        }
    })
}
