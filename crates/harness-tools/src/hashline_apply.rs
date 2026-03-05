use std::io::Write;
use std::path::Path;

use async_trait::async_trait;
use harness_core::edit::hashline::{apply_hashline_patch, HashlinePatch};
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
        "Applies a hashline patch to a workspace file and writes an artifact diff."
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

        let patch_path = Path::new(&patch.path);
        if patch_path.is_absolute() {
            return Err(ToolError::InvalidArguments(
                "path must be relative to workspace root".to_string(),
            ));
        }

        let resolved_path = ctx.resolve_workspace_path(patch_path)?;
        let source = std::fs::read_to_string(&resolved_path)
            .map_err(|err| ToolError::Execution(format!("failed to read target file: {err}")))?;

        let applied = apply_hashline_patch(&source, &patch).map_err(|err| {
            ToolError::Execution(format!("hashline apply rejected [{}]: {err}", err.code()))
        })?;

        write_atomic(&resolved_path, &applied.content)?;
        let diff = TextDiff::from_lines(&source, &applied.content)
            .unified_diff()
            .to_string();
        let artifact = ctx
            .artifact_store()
            .map_err(|e| ToolError::Execution(format!("failed to access artifact store: {e}")))?
            .write_text(&format!("edit-{}.diff", patch.edit_id), &diff)
            .map_err(|e| ToolError::Execution(format!("failed to write diff artifact: {e}")))?;

        Ok(ToolResult {
            display_text: format!(
                "applied hashline edit {} to {}",
                patch.edit_id,
                resolved_path.display()
            ),
            structured_json: Some(json!({
                "edit_id": patch.edit_id,
                "diff_rel_path": artifact.path,
                "diff_digest": artifact.digest,
                "path": patch.path,
                "resolved_path": resolved_path.display().to_string(),
                "changed_ranges": applied.changed_ranges,
            })),
            artifacts: vec![artifact],
        })
    }
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
