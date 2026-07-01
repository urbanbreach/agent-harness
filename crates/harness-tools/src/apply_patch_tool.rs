use async_trait::async_trait;
use harness_core::edit::hashline::HashlineWorkspaceOp;
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
};

mod matching;
mod parser;
mod plan;

use parser::{add_file_content, parse_patch};
use plan::{prepare_hunks, PreparedHunk};

pub(crate) struct ApplyPatchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyPatchArgs {
    #[serde(rename = "patchText", alias = "patch_text")]
    patch_text: String,
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn id(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply one patch containing add, update, and delete file operations. Operations apply sequentially; earlier operations remain applied if a later operation fails."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["patchText"],
            "properties": {
                "patchText": {
                    "type": "string",
                    "description": "The full patch text describing add, update, and delete operations"
                }
            }
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ApplyPatchArgs = crate::parse_tool_args(args_json)?;
        apply_patch(&ctx, &args.patch_text)
    }
}

struct AppliedPatch {
    kind: &'static str,
    resource: String,
    target: String,
}

fn apply_patch(ctx: &ToolContext, patch_text: &str) -> Result<ToolResult, ToolError> {
    if patch_text.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "patchText is required".to_string(),
        ));
    }
    let hunks = parse_patch(patch_text)?;
    if hunks.is_empty() {
        return Err(ToolError::Execution(
            "patch rejected: empty patch".to_string(),
        ));
    }

    let prepared = prepare_hunks(ctx, &hunks)?;

    let mut applied = Vec::new();
    let mut artifacts = Vec::new();
    for (index, hunk) in prepared.iter().enumerate() {
        apply_hunk(ctx, hunk, index, &mut applied, &mut artifacts)?;
    }

    Ok(crate::text_json_artifacts_tool_result(
        format_model_output(&applied),
        json!({
            "applied": applied.iter().map(applied_json).collect::<Vec<_>>()
        }),
        artifacts,
    ))
}

fn apply_hunk(
    ctx: &ToolContext,
    hunk: &PreparedHunk,
    index: usize,
    applied: &mut Vec<AppliedPatch>,
    artifacts: &mut Vec<ArtifactRef>,
) -> Result<(), ToolError> {
    let path = hunk.path();
    let mut result = match hunk {
        PreparedHunk::Add { path, contents } => {
            let target = resolve_workspace_target_path(ctx, path)?;
            if target.exists() {
                return Err(patch_failure(path, applied));
            }
            apply_hashline_workspace_op_to_workspace(
                ctx,
                HashlineWorkspaceOp::RewriteFile {
                    edit_id: format!("apply-patch-{}-{}", ctx.tool_call_id, index + 1),
                    path: path.clone(),
                    content: add_file_content(contents),
                },
            )?
        }
        PreparedHunk::Delete { path } => apply_hashline_workspace_op_to_workspace(
            ctx,
            HashlineWorkspaceOp::DeleteFile {
                edit_id: format!("apply-patch-{}-{}", ctx.tool_call_id, index + 1),
                path: path.clone(),
            },
        )?,
        PreparedHunk::Update {
            path,
            source,
            content,
        } => {
            let target = resolve_workspace_target_path(ctx, path)?;
            let current =
                std::fs::read_to_string(&target).map_err(|_| patch_failure(path, applied))?;
            if current != *source {
                return Err(patch_failure(path, applied));
            }
            apply_hashline_workspace_op_to_workspace(
                ctx,
                HashlineWorkspaceOp::RewriteFile {
                    edit_id: format!("apply-patch-{}-{}", ctx.tool_call_id, index + 1),
                    path: path.clone(),
                    content: content.clone(),
                },
            )?
        }
    };
    let applied_entry = applied_from_result(hunk, &result, path);
    artifacts.append(&mut result.artifacts);
    applied.push(applied_entry);
    Ok(())
}

fn applied_from_result(hunk: &PreparedHunk, result: &ToolResult, fallback: &str) -> AppliedPatch {
    let structured = result.structured_json.as_ref().unwrap_or(&Value::Null);
    AppliedPatch {
        kind: hunk.kind(),
        resource: structured["path"].as_str().unwrap_or(fallback).to_string(),
        target: structured["resolved_path"]
            .as_str()
            .unwrap_or(fallback)
            .to_string(),
    }
}

fn applied_json(applied: &AppliedPatch) -> Value {
    json!({ "type": applied.kind, "resource": applied.resource, "target": applied.target })
}

fn format_model_output(applied: &[AppliedPatch]) -> String {
    let mut lines = vec!["Applied patch sequentially:".to_string()];
    lines.extend(applied.iter().map(|item| {
        let prefix = match item.kind {
            "add" => "A",
            "delete" => "D",
            _ => "M",
        };
        format!("{prefix} {}", item.resource)
    }));
    lines.join("\n")
}

fn patch_failure(path: &str, applied: &[AppliedPatch]) -> ToolError {
    let message = if applied.is_empty() {
        format!("Unable to apply patch at {path}")
    } else {
        format!(
            "Patch partially applied before failing at {path}. Applied: {}",
            applied
                .iter()
                .map(|item| item.resource.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    ToolError::Execution(message)
}
