use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use harness_core::edit::hashline::{
    apply_hashline_patch, ChangedLineRange, HashlinePatch, HashlineWorkspaceOp,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde_json::json;
use similar::TextDiff;

use crate::fs_walk::workspace_relative_display;
use crate::workspace_paths::canonical_workspace_root;

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
        let patch: HashlinePatch = crate::parse_tool_args(args_json)?;

        apply_hashline_patch_to_workspace(&ctx, patch)
    }
}

pub(crate) fn apply_hashline_patch_to_workspace(
    ctx: &ToolContext,
    patch: HashlinePatch,
) -> Result<ToolResult, ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, &patch.path)?;
    let source = read_existing_file(&resolved_path, "failed to read target file")?;

    let applied = apply_hashline_patch(&source, &patch)
        .map_err(|err| ToolError::Execution(format_hashline_apply_rejection(&err)))?;

    write_atomic(&resolved_path, &applied.content)?;
    let diff = unified_diff(&source, &applied.content);

    build_hashline_tool_result(
        ctx,
        &patch.edit_id,
        &resolved_path,
        applied.changed_ranges,
        &diff,
        Some(&source),
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
    let workspace = canonical_workspace_root(ctx)?;
    let input = Path::new(file_path);
    let relative = workspace_relative_input(&workspace, input)?;
    let resolved = resolve_relative_workspace_path(&workspace, relative, input)?;

    ensure_target_stays_within_workspace(&workspace, &resolved, input)?;

    Ok(resolved)
}

fn workspace_relative_input<'a>(workspace: &Path, input: &'a Path) -> Result<&'a Path, ToolError> {
    if input.is_absolute() {
        input
            .strip_prefix(workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: input.display().to_string(),
            })
    } else {
        Ok(input)
    }
}

fn resolve_relative_workspace_path(
    workspace: &Path,
    relative: &Path,
    original_input: &Path,
) -> Result<PathBuf, ToolError> {
    let mut resolved = workspace.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => resolved.push(segment),
            std::path::Component::ParentDir => {
                if resolved == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: original_input.display().to_string(),
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

    Ok(resolved)
}

pub(crate) fn validate_workspace_move_target(
    ctx: &ToolContext,
    from_path: &str,
    to_path: &str,
) -> Result<(), ToolError> {
    let (from_resolved_path, to_resolved_path) =
        resolve_workspace_move_paths(ctx, from_path, to_path)?;
    validate_resolved_workspace_move_target(&from_resolved_path, &to_resolved_path)
}

fn resolve_workspace_move_paths(
    ctx: &ToolContext,
    from_path: &str,
    to_path: &str,
) -> Result<(PathBuf, PathBuf), ToolError> {
    Ok((
        resolve_workspace_target_path(ctx, from_path)?,
        resolve_workspace_target_path(ctx, to_path)?,
    ))
}

fn validate_resolved_workspace_move_target(
    from_resolved_path: &Path,
    to_resolved_path: &Path,
) -> Result<(), ToolError> {
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
    let source =
        read_optional_existing_file(&resolved_path, "failed to read target file for rewrite")?;

    create_parent_dir(&resolved_path)?;
    write_atomic(&resolved_path, content)?;
    build_full_file_change_result(ctx, edit_id, &resolved_path, &source, content)
}

fn delete_workspace_file(
    ctx: &ToolContext,
    file_path: &str,
    edit_id: &str,
) -> Result<ToolResult, ToolError> {
    let resolved_path = resolve_workspace_target_path(ctx, file_path)?;
    let source = read_existing_file(&resolved_path, "failed to read file for delete")?;

    std::fs::remove_file(&resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to delete file: {err}")))?;

    build_full_file_change_result(ctx, edit_id, &resolved_path, &source, "")
}

fn move_workspace_file(
    ctx: &ToolContext,
    from_path: &str,
    to_path: &str,
    edit_id: &str,
) -> Result<ToolResult, ToolError> {
    let (from_resolved_path, to_resolved_path) =
        resolve_workspace_move_paths(ctx, from_path, to_path)?;
    validate_resolved_workspace_move_target(&from_resolved_path, &to_resolved_path)?;

    let source = read_existing_file(&from_resolved_path, "failed to read file for move")?;
    create_parent_dir(&to_resolved_path)?;
    std::fs::rename(&from_resolved_path, &to_resolved_path)
        .map_err(|err| ToolError::Execution(format!("failed to move file: {err}")))?;

    build_move_file_result(
        ctx,
        edit_id,
        &from_resolved_path,
        &to_resolved_path,
        from_path,
        to_path,
        &source,
    )
}

fn build_hashline_tool_result(
    ctx: &ToolContext,
    edit_id: &str,
    resolved_path: &Path,
    changed_ranges: Vec<ChangedLineRange>,
    diff: &str,
    before_content: Option<&str>,
) -> Result<ToolResult, ToolError> {
    let artifact = write_diff_artifact(ctx, edit_id, diff)?;
    let before_artifact = before_content
        .filter(|content| !content.is_empty())
        .and_then(|content| write_before_artifact(ctx, edit_id, content).ok());
    let workspace = canonical_workspace_root(ctx)?;
    let display_path = workspace_relative_display(&workspace, resolved_path)?;
    let mut json = json!({
        "edit_id": edit_id,
        "diff_rel_path": artifact.path,
        "diff_digest": artifact.digest,
        "path": display_path,
        "resolved_path": resolved_path.display().to_string(),
        "changed_ranges": changed_ranges,
    });
    if let Some(ref before) = before_artifact {
        json["before_rel_path"] = json!(before.path);
    }
    let mut artifacts = vec![artifact];
    artifacts.extend(before_artifact);
    Ok(crate::text_json_artifacts_tool_result(
        format!(
            "applied hashline edit {} to {}",
            edit_id,
            resolved_path.display()
        ),
        json,
        artifacts,
    ))
}

fn build_full_file_change_result(
    ctx: &ToolContext,
    edit_id: &str,
    resolved_path: &Path,
    before: &str,
    after: &str,
) -> Result<ToolResult, ToolError> {
    let diff = unified_diff(before, after);
    let changed_ranges = full_file_changed_ranges(before, after);

    build_hashline_tool_result(
        ctx,
        edit_id,
        resolved_path,
        changed_ranges,
        &diff,
        Some(before),
    )
}

fn build_move_file_result(
    ctx: &ToolContext,
    edit_id: &str,
    from_resolved_path: &Path,
    to_resolved_path: &Path,
    from_path: &str,
    to_path: &str,
    source: &str,
) -> Result<ToolResult, ToolError> {
    let diff = move_file_diff(from_path, to_path, source);
    let artifact = write_diff_artifact(ctx, edit_id, &diff)?;
    let before_artifact = if source.is_empty() {
        None
    } else {
        Some(write_before_artifact(ctx, edit_id, source)?)
    };
    let workspace = canonical_workspace_root(ctx)?;
    let from_display_path = workspace_relative_display(&workspace, from_resolved_path)?;
    let to_display_path = workspace_relative_display(&workspace, to_resolved_path)?;
    let mut json = json!({
        "edit_id": edit_id,
        "diff_rel_path": artifact.path,
        "diff_digest": artifact.digest,
        "path": from_display_path,
        "resolved_path": from_resolved_path.display().to_string(),
        "to_path": to_display_path,
        "resolved_to_path": to_resolved_path.display().to_string(),
        "changed_ranges": [],
    });
    if let Some(ref before) = before_artifact {
        json["before_rel_path"] = json!(before.path);
    }
    let mut artifacts = vec![artifact];
    artifacts.extend(before_artifact);
    Ok(crate::text_json_artifacts_tool_result(
        format!(
            "applied hashline edit {} to {}",
            edit_id,
            to_resolved_path.display()
        ),
        json,
        artifacts,
    ))
}

fn move_file_diff(from_path: &str, to_path: &str, source: &str) -> String {
    let mut diff = format!("--- {from_path}\n+++ {to_path}\n");
    if !source.is_empty() {
        diff.push_str(&unified_diff(source, source));
    }
    diff
}

fn unified_diff(before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .to_string()
}

fn full_file_changed_ranges(before: &str, after: &str) -> Vec<ChangedLineRange> {
    vec![ChangedLineRange {
        start_line: 1,
        removed_lines: line_count(before),
        added_lines: line_count(after),
    }]
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

fn write_before_artifact(
    ctx: &ToolContext,
    edit_id: &str,
    before: &str,
) -> Result<harness_core::tool::ArtifactRef, ToolError> {
    ctx.artifact_store()
        .map_err(|e| ToolError::Execution(format!("failed to access artifact store: {e}")))?
        .write_text(&format!("edit-{edit_id}.before"), before)
        .map_err(|e| ToolError::Execution(format!("failed to write before artifact: {e}")))
}

fn line_count(content: &str) -> u32 {
    u32::try_from(content.lines().count()).unwrap_or(u32::MAX)
}

fn read_existing_file(path: &Path, failure_context: &str) -> Result<String, ToolError> {
    std::fs::read_to_string(path)
        .map_err(|err| ToolError::Execution(format!("{failure_context}: {err}")))
}

fn read_optional_existing_file(path: &Path, failure_context: &str) -> Result<String, ToolError> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(ToolError::Execution(format!("{failure_context}: {err}"))),
    }
}

fn create_parent_dir(path: &Path) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create parent directory: {err}"))
        })?;
    }
    Ok(())
}

pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<(), ToolError> {
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
