use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use harness_core::edit::hashline::{
    apply_hashline_patch, ChangedLineRange, HashlinePatch, HashlineWorkspaceOp,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde_json::json;
use similar::TextDiff;

pub struct HashlineApplyTool;

#[async_trait]
impl Tool for HashlineApplyTool {
    fn id(&self) -> &str {
        "edit.hashline_apply"
    }

    fn description(&self) -> &str {
        "Applies a hashline patch to a workspace file using LINE#HASH anchors and writes an artifact diff. Re-read anchors first if the file may have changed."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["edit_id", "path", "ops"],
            "properties": {
                "edit_id": { "type": "string" },
                "path": { "type": "string" },
                "ops": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Rewrite"],
                                "properties": {
                                    "Rewrite": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["lines"],
                                        "properties": {
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["InsertBefore"],
                                "properties": {
                                    "InsertBefore": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["anchor", "lines"],
                                        "properties": {
                                            "anchor": { "$ref": "#/definitions/LineAnchor" },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["InsertAfter"],
                                "properties": {
                                    "InsertAfter": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["anchor", "lines"],
                                        "properties": {
                                            "anchor": { "$ref": "#/definitions/LineAnchor" },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Replace"],
                                "properties": {
                                    "Replace": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["expected", "lines"],
                                        "properties": {
                                            "expected": {
                                                "type": "array",
                                                "items": { "$ref": "#/definitions/LineAnchor" }
                                            },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Delete"],
                                "properties": {
                                    "Delete": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["expected"],
                                        "properties": {
                                            "expected": {
                                                "type": "array",
                                                "items": { "$ref": "#/definitions/LineAnchor" }
                                            }
                                        }
                                    }
                                }
                            }
                        ]
                    }
                }
            },
            "definitions": {
                "LineAnchor": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["line", "hash"],
                    "properties": {
                        "line": { "type": "integer", "minimum": 0 },
                        "hash": { "type": "string" }
                    }
                }
            }
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let patch: HashlinePatch = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        apply_hashline_patch_to_workspace(&ctx, patch)
    }
}

pub(crate) fn apply_hashline_patch_to_workspace(
    ctx: &ToolContext,
    patch: HashlinePatch,
) -> Result<ToolResult, ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, &patch.path)?;
    let source = std::fs::read_to_string(&resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to read target file: {err}")))?;

    let applied = apply_hashline_patch(&source, &patch)
        .map_err(|err| ToolError::Execution(format_hashline_apply_rejection(&err)))?;

    write_atomic(&resolved_path, &applied.content)?;
    let diff = TextDiff::from_lines(&source, &applied.content)
        .unified_diff()
        .to_string();

    build_hashline_tool_result(
        ctx,
        &patch.edit_id,
        &resolved_path,
        applied.changed_ranges,
        &diff,
    )
}

fn format_hashline_apply_rejection(err: &harness_core::edit::hashline::HashlineError) -> String {
    let mut message = format!("hashline apply rejected [{}]: {err}", err.code());
    if err.code() == "OVERLAP" {
        message.push_str(". Recovery: all operations in one patch target the original file snapshot; merge touching changes into one replace, move inserts outside replaced ranges, avoid two inserts at the same anchor, or re-read and apply conflicting changes in a second patch.");
    }
    message
}

pub(crate) fn apply_hashline_workspace_op_to_workspace(
    ctx: &ToolContext,
    op: HashlineWorkspaceOp,
) -> Result<ToolResult, ToolError> {
    match op {
        HashlineWorkspaceOp::Patch { patch } => apply_hashline_patch_to_workspace(ctx, patch),
        HashlineWorkspaceOp::RewriteFile {
            edit_id,
            path,
            content,
        } => rewrite_workspace_file(ctx, &path, &content, &edit_id),
        HashlineWorkspaceOp::DeleteFile { edit_id, path } => {
            delete_workspace_file(ctx, &path, &edit_id)
        }
        HashlineWorkspaceOp::MoveFile {
            edit_id,
            from_path,
            to_path,
        } => move_workspace_file(ctx, &from_path, &to_path, &edit_id),
    }
}

pub(crate) fn resolve_workspace_target_path(
    ctx: &ToolContext,
    file_path: &str,
) -> Result<PathBuf, ToolError> {
    let workspace = ctx
        .workspace_root
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve workspace root: {err}")))?;
    let input = Path::new(file_path);
    let relative = if input.is_absolute() {
        input
            .strip_prefix(&workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: input.display().to_string(),
            })?
    } else {
        input
    };

    let mut resolved = workspace.clone();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => resolved.push(segment),
            std::path::Component::ParentDir => {
                if resolved == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: input.display().to_string(),
                    });
                }
                resolved.pop();
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(ToolError::InvalidArguments(
                    "path must be relative to workspace root".to_string(),
                ));
            }
        }
    }

    ensure_target_stays_within_workspace(&workspace, &resolved, input)?;

    Ok(resolved)
}

pub(crate) fn validate_workspace_move_target(
    ctx: &ToolContext,
    from_path: &str,
    to_path: &str,
) -> Result<(), ToolError> {
    let from_resolved_path = resolve_workspace_target_path(ctx, from_path)?;
    let to_resolved_path = resolve_workspace_target_path(ctx, to_path)?;
    if from_resolved_path == to_resolved_path {
        return Err(ToolError::InvalidArguments(
            "move source and destination must differ".to_string(),
        ));
    }
    if !from_resolved_path.exists() {
        return Err(ToolError::Execution(format!(
            "failed to move file: source is missing: {}",
            from_resolved_path.display()
        )));
    }
    if to_resolved_path.exists() {
        return Err(ToolError::Execution(format!(
            "failed to move file: destination already exists: {}",
            to_resolved_path.display()
        )));
    }
    Ok(())
}

fn ensure_target_stays_within_workspace(
    workspace: &Path,
    resolved: &Path,
    input: &Path,
) -> Result<(), ToolError> {
    let Some(existing_ancestor) = nearest_existing_ancestor(resolved) else {
        return Err(ToolError::Execution(format!(
            "failed to resolve an existing parent for {}",
            input.display()
        )));
    };

    let canonical_ancestor = existing_ancestor.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to canonicalize resolved path ancestor {}: {err}",
            existing_ancestor.display()
        ))
    })?;

    if !canonical_ancestor.starts_with(workspace) {
        return Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: input.display().to_string(),
        });
    }

    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return Some(current);
        }
        candidate = current.parent();
    }
    None
}

fn rewrite_workspace_file(
    ctx: &ToolContext,
    file_path: &str,
    content: &str,
    edit_id: &str,
) -> Result<ToolResult, ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, file_path)?;
    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(source) => source,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(ToolError::Execution(format!(
                "failed to read target file for rewrite: {err}"
            )));
        }
    };

    if let Some(parent) = resolved_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create parent directory: {err}"))
        })?;
    }
    write_atomic(&resolved_path, content)?;

    let diff = TextDiff::from_lines(source.as_str(), content)
        .unified_diff()
        .to_string();
    let changed_ranges = vec![ChangedLineRange {
        start_line: 1,
        removed_lines: line_count(&source),
        added_lines: line_count(content),
    }];

    build_hashline_tool_result(ctx, edit_id, &resolved_path, changed_ranges, &diff)
}

fn delete_workspace_file(
    ctx: &ToolContext,
    file_path: &str,
    edit_id: &str,
) -> Result<ToolResult, ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, file_path)?;
    let source = std::fs::read_to_string(&resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to read file for delete: {err}")))?;

    std::fs::remove_file(&resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to delete file: {err}")))?;

    let diff = TextDiff::from_lines(source.as_str(), "")
        .unified_diff()
        .to_string();
    let changed_ranges = vec![ChangedLineRange {
        start_line: 1,
        removed_lines: line_count(&source),
        added_lines: 0,
    }];

    build_hashline_tool_result(ctx, edit_id, &resolved_path, changed_ranges, &diff)
}

fn move_workspace_file(
    ctx: &ToolContext,
    from_path: &str,
    to_path: &str,
    edit_id: &str,
) -> Result<ToolResult, ToolError> {
    let from_resolved_path = resolve_workspace_target_path(ctx, from_path)?;
    let to_resolved_path = resolve_workspace_target_path(ctx, to_path)?;
    validate_workspace_move_target(ctx, from_path, to_path)?;

    let source = std::fs::read_to_string(&from_resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to read file for move: {err}")))?;
    if let Some(parent) = to_resolved_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create parent directory: {err}"))
        })?;
    }
    std::fs::rename(&from_resolved_path, &to_resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to move file: {err}")))?;

    let mut diff = format!("--- {from_path}\n+++ {to_path}\n");
    if !source.is_empty() {
        diff.push_str(
            &TextDiff::from_lines(&source, &source)
                .unified_diff()
                .to_string(),
        );
    }

    let artifact = write_diff_artifact(ctx, edit_id, &diff)?;
    let from_display_path = workspace_relative_display(ctx, &from_resolved_path)?;
    let to_display_path = workspace_relative_display(ctx, &to_resolved_path)?;
    Ok(ToolResult {
        display_text: format!(
            "applied hashline edit {} to {}",
            edit_id,
            to_resolved_path.display()
        ),
        structured_json: Some(json!({
            "edit_id": edit_id,
            "diff_rel_path": artifact.path,
            "diff_digest": artifact.digest,
            "path": from_display_path,
            "resolved_path": from_resolved_path.display().to_string(),
            "to_path": to_display_path,
            "resolved_to_path": to_resolved_path.display().to_string(),
            "changed_ranges": [],
        })),
        artifacts: vec![artifact],
    })
}

fn build_hashline_tool_result(
    ctx: &ToolContext,
    edit_id: &str,
    resolved_path: &Path,
    changed_ranges: Vec<ChangedLineRange>,
    diff: &str,
) -> Result<ToolResult, ToolError> {
    let artifact = write_diff_artifact(ctx, edit_id, diff)?;
    let display_path = workspace_relative_display(ctx, resolved_path)?;
    Ok(ToolResult {
        display_text: format!(
            "applied hashline edit {} to {}",
            edit_id,
            resolved_path.display()
        ),
        structured_json: Some(json!({
            "edit_id": edit_id,
            "diff_rel_path": artifact.path,
            "diff_digest": artifact.digest,
            "path": display_path,
            "resolved_path": resolved_path.display().to_string(),
            "changed_ranges": changed_ranges,
        })),
        artifacts: vec![artifact],
    })
}

fn write_diff_artifact(
    ctx: &ToolContext,
    edit_id: &str,
    diff: &str,
) -> Result<harness_core::tool::ArtifactRef, ToolError> {
    ctx.artifact_store()
        .map_err(|e| ToolError::Execution(format!("failed to access artifact store: {e}")))?
        .write_text(&format!("edit-{edit_id}.diff"), diff)
        .map_err(|e| ToolError::Execution(format!("failed to write diff artifact: {e}")))
}

fn line_count(content: &str) -> u32 {
    content.lines().count() as u32
}

fn workspace_relative_display(
    ctx: &ToolContext,
    resolved_path: &Path,
) -> Result<String, ToolError> {
    let workspace = ctx
        .workspace_root
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve workspace root: {err}")))?;
    let relative =
        resolved_path
            .strip_prefix(&workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: resolved_path.display().to_string(),
            })?;

    if relative.as_os_str().is_empty() {
        return Ok(".".to_string());
    }

    Ok(relative
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/"))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), ToolError> {
    let parent = path.parent().ok_or_else(|| {
        ToolError::Execution(format!(
            "failed to resolve parent directory for {}",
            path.display()
        ))
    })?;

    let mut temp = tempfile::Builder::new()
        .prefix(".hashline-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|err| ToolError::Execution(format!("failed to create temp file: {err}")))?;

    temp.write_all(content.as_bytes())
        .map_err(|err| ToolError::Execution(format!("failed to write temp file: {err}")))?;
    temp.flush()
        .map_err(|err| ToolError::Execution(format!("failed to flush temp file: {err}")))?;
    temp.as_file()
        .sync_data()
        .map_err(|err| ToolError::Execution(format!("failed to sync temp file: {err}")))?;

    temp.persist(path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            err.error
        ))
    })?;

    Ok(())
}
