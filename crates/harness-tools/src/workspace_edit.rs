use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::edit::hashline::{
    apply_hashline_patch, compute_line_hash, HashlineOp, HashlinePatch, HashlineWorkspaceOp,
    LineAnchor,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
    write_file_via_hashline_engine,
};

pub(crate) struct WorkspaceEditExecutor;

impl WorkspaceEditExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn write_file(
        &self,
        ctx: &ToolContext,
        file_path: &str,
        content: &str,
    ) -> Result<ToolResult, ToolError> {
        let path = Path::new(file_path);
        if path.is_absolute() {
            return Err(ToolError::InvalidArguments(
                "path must be relative to workspace root".to_string(),
            ));
        }

        let mut result = write_file_via_hashline_engine(
            ctx,
            file_path,
            content,
            &format!("fs-write-{}", ctx.tool_call_id),
        )?;

        let resolved_path = result
            .structured_json
            .as_ref()
            .and_then(|json| json.get("resolved_path"))
            .and_then(Value::as_str)
            .unwrap_or(file_path)
            .to_string();

        result.display_text = format!("Wrote file successfully: {resolved_path}");
        Ok(result)
    }

    pub(crate) fn apply_patch(
        &self,
        ctx: &ToolContext,
        patch_text: &str,
    ) -> Result<ToolResult, ToolError> {
        let sections = parse_apply_patch_sections(patch_text)?;
        if sections.is_empty() {
            return Err(ToolError::InvalidArguments(
                "patch rejected: empty patch".to_string(),
            ));
        }

        let translated = translate_apply_patch_sections(ctx, &sections)?;
        let mut artifacts = Vec::new();
        for op in translated.ops {
            let result = apply_hashline_workspace_op_to_workspace(ctx, op)?;
            artifacts.extend(result.artifacts);
        }

        Ok(ToolResult {
            display_text: format!(
                "Success. Updated the following files:\n{}",
                translated.files.join("\n")
            ),
            structured_json: Some(json!({
                "files": translated.files,
            })),
            artifacts,
        })
    }
}

pub(crate) struct FsWriteTool {
    executor: Arc<WorkspaceEditExecutor>,
}

impl FsWriteTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FsWriteArgs {
    path: String,
    content: String,
}

#[derive(Debug)]
enum ApplyPatchSection {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<Vec<String>>,
    },
    Delete {
        path: String,
    },
}

struct TranslatedPatchPlan {
    ops: Vec<HashlineWorkspaceOp>,
    files: Vec<String>,
}

#[async_trait]
impl Tool for FsWriteTool {
    fn id(&self) -> &str {
        "fs.write"
    }

    fn description(&self) -> &str {
        "Writes file contents to a workspace-relative path via the hashline edit engine."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<FsWriteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: FsWriteArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.write_file(&ctx, &args.path, &args.content)
    }
}

fn parse_apply_patch_sections(patch_text: &str) -> Result<Vec<ApplyPatchSection>, ToolError> {
    let normalized = patch_text.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch")
        || lines.last().copied() != Some("*** End Patch")
    {
        return Err(ToolError::InvalidArguments(
            "patch must start with '*** Begin Patch' and end with '*** End Patch'".to_string(),
        ));
    }

    let mut sections = Vec::new();
    let mut idx = 1usize;
    while idx + 1 < lines.len() {
        let line = lines[idx];

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            idx += 1;
            let mut file_lines = Vec::new();
            while idx < lines.len() && !lines[idx].starts_with("*** ") {
                let add_line = lines[idx];
                if !add_line.starts_with('+') {
                    return Err(ToolError::Execution(
                        "apply_patch add-file lines must start with '+'".to_string(),
                    ));
                }
                file_lines.push(add_line[1..].to_string());
                idx += 1;
            }
            sections.push(ApplyPatchSection::Add {
                path: path.to_string(),
                lines: file_lines,
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            idx += 1;
            if idx < lines.len() && !lines[idx].starts_with("*** ") {
                return Err(ToolError::Execution(
                    "apply_patch delete-file sections cannot contain hunks".to_string(),
                ));
            }
            sections.push(ApplyPatchSection::Delete {
                path: path.to_string(),
            });
            continue;
        }

        let Some(path) = line.strip_prefix("*** Update File: ") else {
            return Err(ToolError::Execution(
                "apply_patch translation currently supports only '*** Add File', '*** Update File', and '*** Delete File' sections"
                    .to_string(),
            ));
        };
        idx += 1;

        let mut move_to = None;
        if idx < lines.len() {
            if let Some(destination) = lines[idx].strip_prefix("*** Move to: ") {
                move_to = Some(destination.to_string());
                idx += 1;
            }
        }

        let mut hunks = Vec::new();
        let mut current_hunk = Vec::new();
        while idx < lines.len() && !lines[idx].starts_with("*** ") {
            let hunk_line = lines[idx];
            if hunk_line.starts_with("@@") {
                if !current_hunk.is_empty() {
                    hunks.push(current_hunk);
                    current_hunk = Vec::new();
                }
                idx += 1;
                continue;
            }
            if !(hunk_line.starts_with('+')
                || hunk_line.starts_with('-')
                || hunk_line.starts_with(' '))
            {
                return Err(ToolError::Execution(
                    "apply_patch hunk lines must start with '+', '-', or space".to_string(),
                ));
            }
            current_hunk.push(hunk_line.to_string());
            idx += 1;
        }
        if !current_hunk.is_empty() {
            hunks.push(current_hunk);
        }

        sections.push(ApplyPatchSection::Update {
            path: path.to_string(),
            move_to,
            hunks,
        });
    }

    Ok(sections)
}

fn translate_apply_patch_sections(
    ctx: &ToolContext,
    sections: &[ApplyPatchSection],
) -> Result<TranslatedPatchPlan, ToolError> {
    let mut virtual_files = BTreeMap::<String, Option<String>>::new();
    let mut ops = Vec::new();
    let mut files = BTreeSet::new();

    for (section_index, section) in sections.iter().enumerate() {
        match section {
            ApplyPatchSection::Add { path, lines } => {
                let existing = read_virtual_file_state(ctx, &mut virtual_files, path)?;
                if existing.is_some() {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: add file target already exists: {path}"
                    )));
                }

                let content = lines.join("\n");
                virtual_files.insert(path.clone(), Some(content.clone()));
                ops.push(HashlineWorkspaceOp::RewriteFile {
                    edit_id: format!("apply-patch-{}-{section_index}", ctx.tool_call_id),
                    path: path.clone(),
                    content,
                });
                files.insert(format!("A {path}"));
            }
            ApplyPatchSection::Delete { path } => {
                let existing = read_virtual_file_state(ctx, &mut virtual_files, path)?;
                if existing.is_none() {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: delete file target not found: {path}"
                    )));
                }

                virtual_files.insert(path.clone(), None);
                ops.push(HashlineWorkspaceOp::DeleteFile {
                    edit_id: format!("apply-patch-{}-{section_index}", ctx.tool_call_id),
                    path: path.clone(),
                });
                files.insert(format!("D {path}"));
            }
            ApplyPatchSection::Update {
                path,
                move_to,
                hunks,
            } => {
                let Some(mut source) = read_virtual_file_state(ctx, &mut virtual_files, path)?
                else {
                    return Err(ToolError::Execution(format!(
                        "apply_patch verification failed: update file target not found: {path}"
                    )));
                };

                if hunks.is_empty() && move_to.is_none() {
                    return Err(ToolError::Execution(
                        "apply_patch verification failed: update section has no hunks".to_string(),
                    ));
                }

                for (hunk_index, hunk) in hunks.iter().enumerate() {
                    let patch =
                        translate_update_hunk(ctx, section_index, hunk_index, path, &source, hunk)?;
                    let applied = apply_hashline_patch(&source, &patch).map_err(|err| {
                        ToolError::Execution(format!(
                            "hashline apply rejected [{}]: {err}",
                            err.code()
                        ))
                    })?;
                    source = applied.content;
                    ops.push(HashlineWorkspaceOp::Patch { patch });
                }
                virtual_files.insert(path.clone(), Some(source.clone()));

                if let Some(destination) = move_to {
                    if destination != path {
                        let existing_destination =
                            read_virtual_file_state(ctx, &mut virtual_files, destination)?;
                        if existing_destination.is_some() {
                            return Err(ToolError::Execution(format!(
                                "apply_patch verification failed: move destination already exists: {destination}"
                            )));
                        }

                        virtual_files.insert(path.clone(), None);
                        virtual_files.insert(destination.clone(), Some(source));
                        ops.push(HashlineWorkspaceOp::MoveFile {
                            edit_id: format!(
                                "apply-patch-{}-{section_index}-move",
                                ctx.tool_call_id
                            ),
                            from_path: path.clone(),
                            to_path: destination.clone(),
                        });
                        files.insert(format!("M {path} -> {destination}"));
                    } else {
                        files.insert(format!("M {path}"));
                    }
                } else {
                    files.insert(format!("M {path}"));
                }
            }
        }
    }

    Ok(TranslatedPatchPlan {
        ops,
        files: files.into_iter().collect(),
    })
}

fn translate_update_hunk(
    ctx: &ToolContext,
    section_index: usize,
    hunk_index: usize,
    path: &str,
    source: &str,
    hunk: &[String],
) -> Result<HashlinePatch, ToolError> {
    let source_lines = source.lines().collect::<Vec<_>>();
    let old_lines = hunk
        .iter()
        .filter_map(|line| {
            if line.starts_with('-') || line.starts_with(' ') {
                Some(line[1..].to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let new_lines = hunk
        .iter()
        .filter_map(|line| {
            if line.starts_with('+') || line.starts_with(' ') {
                Some(line[1..].to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if old_lines.is_empty() {
        return Err(ToolError::Execution(
            "apply_patch translation currently requires at least one context or removed line"
                .to_string(),
        ));
    }

    let start_idx = find_subsequence(&source_lines, &old_lines).ok_or_else(|| {
        ToolError::Execution("apply_patch verification failed: hunk context not found".to_string())
    })?;

    let expected = source_lines[start_idx..start_idx + old_lines.len()]
        .iter()
        .enumerate()
        .map(|(offset, line)| LineAnchor {
            line: (start_idx + offset + 1) as u32,
            hash: compute_line_hash(line),
        })
        .collect::<Vec<_>>();

    Ok(HashlinePatch {
        edit_id: format!(
            "apply-patch-{}-{section_index}-{hunk_index}",
            ctx.tool_call_id
        ),
        path: path.to_string(),
        ops: vec![HashlineOp::Replace {
            expected,
            lines: new_lines,
        }],
    })
}

fn read_virtual_file_state(
    ctx: &ToolContext,
    virtual_files: &mut BTreeMap<String, Option<String>>,
    path: &str,
) -> Result<Option<String>, ToolError> {
    if let Some(existing) = virtual_files.get(path) {
        return Ok(existing.clone());
    }

    let resolved = resolve_workspace_target_path(ctx, path)?;
    let content = match std::fs::read_to_string(&resolved) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(ToolError::Execution(format!(
                "failed to read file for patching: {err}"
            )));
        }
    };
    virtual_files.insert(path.to_string(), content.clone());
    Ok(content)
}

fn find_subsequence(haystack: &[&str], needle: &[String]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let needle_ref = needle.iter().map(String::as_str).collect::<Vec<_>>();
    haystack
        .windows(needle_ref.len())
        .position(|window| window == needle_ref.as_slice())
}
