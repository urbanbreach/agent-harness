use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::edit::hashline::HashlineWorkspaceOp;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use crate::code_lsp::{canonical_workspace_root, format_diagnostics, resolve_existing_path};
use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
};
use crate::lsp_support::{execute_lsp_rename, LspPosition, LspRenameRequest, LspRenameResponse};

pub(crate) struct CodeLspRenameExecutor;

impl CodeLspRenameExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn execute(
        &self,
        ctx: &ToolContext,
        request: CodeLspRenameRequest,
    ) -> Result<ToolResult, ToolError> {
        let position = LspPosition::from_one_based(request.line, request.character)?;
        let file_path = resolve_existing_path(ctx, &request.file_path)?;
        let response = tokio::task::spawn_blocking({
            let workspace_root = ctx.workspace_root.clone();
            let file_path = file_path.clone();
            let new_name = request.new_name.clone();
            move || {
                execute_lsp_rename(&LspRenameRequest {
                    file_path: &file_path,
                    position,
                    workspace_root: &workspace_root,
                    new_name: &new_name,
                })
            }
        })
        .await
        .map_err(|err| ToolError::Execution(format!("lsp rename task failed: {err}")))??;

        let plan = build_rename_plan(ctx, &response.workspace_edit)?;
        let symbol_preview = symbol_preview(&file_path, &response.prepare_result)?;
        let applied_results = if request.apply {
            apply_plan(ctx, &plan)?
        } else {
            Vec::new()
        };
        let display_text = render_display_text(
            &request.new_name,
            request.apply,
            symbol_preview.as_deref(),
            &plan.preview,
            &response,
        );
        let artifacts = applied_results
            .iter()
            .flat_map(|result| result.artifacts.clone())
            .collect::<Vec<_>>();
        let applied_edits = applied_results
            .iter()
            .filter_map(|result| result.structured_json.clone())
            .collect::<Vec<_>>();

        Ok(ToolResult {
            display_text,
            structured_json: Some(json!({
                "operation": "renameSymbol",
                "filePath": file_path.display().to_string(),
                "line": request.line,
                "character": request.character,
                "newName": request.new_name,
                "apply": request.apply,
                "applied": request.apply && !plan.operations.is_empty(),
                "server": {
                    "name": response.server.name,
                    "command": response.server.command,
                },
                "prepareRename": response.prepare_result,
                "workspaceEdit": response.workspace_edit,
                "preview": plan.preview,
                "symbol": symbol_preview,
                "diagnostics": response.diagnostics,
                "appliedEdits": applied_edits,
            })),
            artifacts,
        })
    }
}

pub(crate) struct CodeLspRenameTool {
    executor: Arc<CodeLspRenameExecutor>,
}

impl CodeLspRenameTool {
    pub(crate) fn new(executor: Arc<CodeLspRenameExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodeLspRenameRequest {
    pub(crate) file_path: String,
    pub(crate) line: i32,
    pub(crate) character: i32,
    pub(crate) new_name: String,
    pub(crate) apply: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspRenameArgs {
    #[serde(rename = "filePath")]
    file_path: String,
    line: i32,
    character: i32,
    #[serde(rename = "newName")]
    new_name: String,
    apply: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RenamePreview {
    file_count: usize,
    text_edit_count: usize,
    files: Vec<RenameFilePreview>,
    resource_operations: Vec<RenameResourceOperationPreview>,
    annotations: Vec<RenameAnnotationPreview>,
}

#[derive(Debug, Clone, Serialize)]
struct RenameFilePreview {
    path: String,
    edit_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RenameResourceOperationPreview {
    kind: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RenameAnnotationPreview {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    needs_confirmation: bool,
}

struct RenamePlan {
    operations: Vec<HashlineWorkspaceOp>,
    preview: RenamePreview,
}

struct RenameOperationAccumulator<'a> {
    operations: &'a mut Vec<HashlineWorkspaceOp>,
    resource_operations: &'a mut Vec<RenameResourceOperationPreview>,
    next_operation_index: &'a mut usize,
}

struct PreviewFileAccumulator {
    edit_count: usize,
    annotation_ids: BTreeSet<String>,
}

struct ParsedTextEdit {
    start: usize,
    end: usize,
    new_text: String,
    annotation_id: Option<String>,
}

#[async_trait]
impl Tool for CodeLspRenameTool {
    fn id(&self) -> &str {
        "code.lsp.rename"
    }

    fn description(&self) -> &str {
        "Previews or applies a semantic LSP rename through an explicit write-capable flow."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<CodeLspRenameArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: CodeLspRenameArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor
            .execute(
                &ctx,
                CodeLspRenameRequest {
                    file_path: args.file_path,
                    line: args.line,
                    character: args.character,
                    new_name: args.new_name,
                    apply: args.apply,
                },
            )
            .await
    }
}

fn build_rename_plan(ctx: &ToolContext, workspace_edit: &Value) -> Result<RenamePlan, ToolError> {
    if workspace_edit.is_null() {
        return Ok(RenamePlan {
            operations: Vec::new(),
            preview: RenamePreview {
                file_count: 0,
                text_edit_count: 0,
                files: Vec::new(),
                resource_operations: Vec::new(),
                annotations: Vec::new(),
            },
        });
    }

    let workspace_root = canonical_workspace_root(ctx)?;
    let mut virtual_files = BTreeMap::<String, Option<String>>::new();
    let mut operations = Vec::<HashlineWorkspaceOp>::new();
    let mut preview_files = BTreeMap::<String, PreviewFileAccumulator>::new();
    let mut resource_operations = Vec::<RenameResourceOperationPreview>::new();
    let mut next_operation_index = 0usize;

    if let Some(document_changes) = workspace_edit
        .get("documentChanges")
        .and_then(Value::as_array)
    {
        for change in document_changes {
            if change.get("textDocument").is_some() {
                let path = change
                    .get("textDocument")
                    .and_then(|text_document| text_document.get("uri"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned documentChanges without textDocument.uri"
                                .to_string(),
                        )
                    })
                    .and_then(|uri| workspace_relative_path_from_uri(&workspace_root, uri))?;
                let edits = change
                    .get("edits")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned documentChanges without edits".to_string(),
                        )
                    })?;
                rewrite_path_from_text_edits(
                    ctx,
                    &path,
                    edits,
                    &mut virtual_files,
                    &mut preview_files,
                    &mut operations,
                    &mut next_operation_index,
                )?;
                continue;
            }

            let kind = change.get("kind").and_then(Value::as_str).ok_or_else(|| {
                ToolError::Execution(
                    "code.lsp.rename returned documentChanges with an unknown item".to_string(),
                )
            })?;
            match kind {
                "create" => {
                    let path = change
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ToolError::Execution(
                                "code.lsp.rename create operation is missing uri".to_string(),
                            )
                        })
                        .and_then(|uri| workspace_relative_path_from_uri(&workspace_root, uri))?;
                    apply_create_operation(
                        ctx,
                        change,
                        &path,
                        &mut virtual_files,
                        RenameOperationAccumulator {
                            operations: &mut operations,
                            resource_operations: &mut resource_operations,
                            next_operation_index: &mut next_operation_index,
                        },
                    )?;
                }
                "rename" => {
                    let from_path = change
                        .get("oldUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ToolError::Execution(
                                "code.lsp.rename rename operation is missing oldUri".to_string(),
                            )
                        })
                        .and_then(|uri| workspace_relative_path_from_uri(&workspace_root, uri))?;
                    let to_path = change
                        .get("newUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ToolError::Execution(
                                "code.lsp.rename rename operation is missing newUri".to_string(),
                            )
                        })
                        .and_then(|uri| workspace_relative_path_from_uri(&workspace_root, uri))?;
                    apply_rename_operation(
                        ctx,
                        change,
                        &from_path,
                        &to_path,
                        &mut virtual_files,
                        RenameOperationAccumulator {
                            operations: &mut operations,
                            resource_operations: &mut resource_operations,
                            next_operation_index: &mut next_operation_index,
                        },
                    )?;
                }
                "delete" => {
                    let path = change
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ToolError::Execution(
                                "code.lsp.rename delete operation is missing uri".to_string(),
                            )
                        })
                        .and_then(|uri| workspace_relative_path_from_uri(&workspace_root, uri))?;
                    apply_delete_operation(
                        ctx,
                        change,
                        &path,
                        &mut virtual_files,
                        RenameOperationAccumulator {
                            operations: &mut operations,
                            resource_operations: &mut resource_operations,
                            next_operation_index: &mut next_operation_index,
                        },
                    )?;
                }
                other => {
                    return Err(ToolError::Execution(format!(
                        "code.lsp.rename returned unsupported workspace edit operation kind: {other}"
                    )));
                }
            }
        }
    } else if let Some(changes) = workspace_edit.get("changes").and_then(Value::as_object) {
        for (uri, edits_value) in changes {
            let path = workspace_relative_path_from_uri(&workspace_root, uri)?;
            let edits = edits_value.as_array().ok_or_else(|| {
                ToolError::Execution(
                    "code.lsp.rename returned changes with a non-array edit list".to_string(),
                )
            })?;
            rewrite_path_from_text_edits(
                ctx,
                &path,
                edits,
                &mut virtual_files,
                &mut preview_files,
                &mut operations,
                &mut next_operation_index,
            )?;
        }
    } else {
        return Err(ToolError::Execution(
            "code.lsp.rename returned a workspace edit without changes".to_string(),
        ));
    }

    let files = preview_files
        .into_iter()
        .map(|(path, accumulator)| RenameFilePreview {
            path,
            edit_count: accumulator.edit_count,
            annotation_ids: accumulator.annotation_ids.into_iter().collect(),
        })
        .collect::<Vec<_>>();
    let text_edit_count = files.iter().map(|file| file.edit_count).sum();

    Ok(RenamePlan {
        operations,
        preview: RenamePreview {
            file_count: files.len(),
            text_edit_count,
            files,
            resource_operations,
            annotations: parse_change_annotations(workspace_edit),
        },
    })
}

fn rewrite_path_from_text_edits(
    ctx: &ToolContext,
    path: &str,
    edits: &[Value],
    virtual_files: &mut BTreeMap<String, Option<String>>,
    preview_files: &mut BTreeMap<String, PreviewFileAccumulator>,
    operations: &mut Vec<HashlineWorkspaceOp>,
    next_operation_index: &mut usize,
) -> Result<(), ToolError> {
    let source = load_virtual_file(ctx, virtual_files, path)?.ok_or_else(|| {
        ToolError::Execution(format!(
            "code.lsp.rename returned edits for a missing workspace path: {path}"
        ))
    })?;
    let parsed_edits = parse_text_edits(&source, edits)?;
    let updated = apply_text_edits(&source, &parsed_edits)?;
    virtual_files.insert(path.to_string(), Some(updated.clone()));
    operations.push(HashlineWorkspaceOp::RewriteFile {
        edit_id: next_rename_edit_id(ctx, *next_operation_index),
        path: path.to_string(),
        content: updated,
    });
    *next_operation_index += 1;

    let preview_entry =
        preview_files
            .entry(path.to_string())
            .or_insert_with(|| PreviewFileAccumulator {
                edit_count: 0,
                annotation_ids: BTreeSet::new(),
            });
    preview_entry.edit_count += parsed_edits.len();
    preview_entry.annotation_ids.extend(
        parsed_edits
            .into_iter()
            .filter_map(|edit| edit.annotation_id),
    );
    Ok(())
}

fn apply_create_operation(
    ctx: &ToolContext,
    change: &Value,
    path: &str,
    virtual_files: &mut BTreeMap<String, Option<String>>,
    operation_accumulator: RenameOperationAccumulator<'_>,
) -> Result<(), ToolError> {
    let RenameOperationAccumulator {
        operations,
        resource_operations,
        next_operation_index,
    } = operation_accumulator;
    let overwrite = change
        .get("options")
        .and_then(|options| options.get("overwrite"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ignore_if_exists = change
        .get("options")
        .and_then(|options| options.get("ignoreIfExists"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let existing = load_virtual_file(ctx, virtual_files, path)?;
    if existing.is_some() && !overwrite {
        if ignore_if_exists {
            return Ok(());
        }
        return Err(ToolError::Execution(format!(
            "code.lsp.rename create operation would overwrite existing path: {path}"
        )));
    }

    virtual_files.insert(path.to_string(), Some(String::new()));
    operations.push(HashlineWorkspaceOp::RewriteFile {
        edit_id: next_rename_edit_id(ctx, *next_operation_index),
        path: path.to_string(),
        content: String::new(),
    });
    *next_operation_index += 1;
    resource_operations.push(RenameResourceOperationPreview {
        kind: "create".to_string(),
        path: path.to_string(),
        to_path: None,
        annotation_id: change
            .get("annotationId")
            .and_then(Value::as_str)
            .map(str::to_string),
    });
    Ok(())
}

fn apply_rename_operation(
    ctx: &ToolContext,
    change: &Value,
    from_path: &str,
    to_path: &str,
    virtual_files: &mut BTreeMap<String, Option<String>>,
    operation_accumulator: RenameOperationAccumulator<'_>,
) -> Result<(), ToolError> {
    let RenameOperationAccumulator {
        operations,
        resource_operations,
        next_operation_index,
    } = operation_accumulator;
    let overwrite = change
        .get("options")
        .and_then(|options| options.get("overwrite"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ignore_if_exists = change
        .get("options")
        .and_then(|options| options.get("ignoreIfExists"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source = load_virtual_file(ctx, virtual_files, from_path)?.ok_or_else(|| {
        ToolError::Execution(format!(
            "code.lsp.rename rename operation source is missing: {from_path}"
        ))
    })?;
    let destination = load_virtual_file(ctx, virtual_files, to_path)?;
    if destination.is_some() {
        if ignore_if_exists && !overwrite {
            return Ok(());
        }
        if overwrite {
            virtual_files.insert(to_path.to_string(), None);
            operations.push(HashlineWorkspaceOp::DeleteFile {
                edit_id: next_rename_edit_id(ctx, *next_operation_index),
                path: to_path.to_string(),
            });
            *next_operation_index += 1;
        } else {
            return Err(ToolError::Execution(format!(
                "code.lsp.rename rename operation destination already exists: {to_path}"
            )));
        }
    }

    virtual_files.insert(from_path.to_string(), None);
    virtual_files.insert(to_path.to_string(), Some(source));
    operations.push(HashlineWorkspaceOp::MoveFile {
        edit_id: next_rename_edit_id(ctx, *next_operation_index),
        from_path: from_path.to_string(),
        to_path: to_path.to_string(),
    });
    *next_operation_index += 1;
    resource_operations.push(RenameResourceOperationPreview {
        kind: "rename".to_string(),
        path: from_path.to_string(),
        to_path: Some(to_path.to_string()),
        annotation_id: change
            .get("annotationId")
            .and_then(Value::as_str)
            .map(str::to_string),
    });
    Ok(())
}

fn apply_delete_operation(
    ctx: &ToolContext,
    change: &Value,
    path: &str,
    virtual_files: &mut BTreeMap<String, Option<String>>,
    operation_accumulator: RenameOperationAccumulator<'_>,
) -> Result<(), ToolError> {
    let RenameOperationAccumulator {
        operations,
        resource_operations,
        next_operation_index,
    } = operation_accumulator;
    let ignore_if_not_exists = change
        .get("options")
        .and_then(|options| options.get("ignoreIfNotExists"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let existing = load_virtual_file(ctx, virtual_files, path)?;
    if existing.is_none() {
        if ignore_if_not_exists {
            return Ok(());
        }
        return Err(ToolError::Execution(format!(
            "code.lsp.rename delete operation target is missing: {path}"
        )));
    }

    virtual_files.insert(path.to_string(), None);
    operations.push(HashlineWorkspaceOp::DeleteFile {
        edit_id: next_rename_edit_id(ctx, *next_operation_index),
        path: path.to_string(),
    });
    *next_operation_index += 1;
    resource_operations.push(RenameResourceOperationPreview {
        kind: "delete".to_string(),
        path: path.to_string(),
        to_path: None,
        annotation_id: change
            .get("annotationId")
            .and_then(Value::as_str)
            .map(str::to_string),
    });
    Ok(())
}

fn parse_change_annotations(workspace_edit: &Value) -> Vec<RenameAnnotationPreview> {
    let Some(annotations) = workspace_edit
        .get("changeAnnotations")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    annotations
        .iter()
        .map(|(id, annotation)| RenameAnnotationPreview {
            id: id.clone(),
            label: annotation
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: annotation
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            needs_confirmation: annotation
                .get("needsConfirmation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

fn load_virtual_file(
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
                "failed to read workspace path {path}: {err}"
            )));
        }
    };
    virtual_files.insert(path.to_string(), content.clone());
    Ok(content)
}

fn parse_text_edits(source: &str, edits: &[Value]) -> Result<Vec<ParsedTextEdit>, ToolError> {
    let line_starts = build_line_starts(source);
    edits
        .iter()
        .map(|edit| {
            let range = edit.get("range").ok_or_else(|| {
                ToolError::Execution(
                    "code.lsp.rename returned a text edit without a range".to_string(),
                )
            })?;
            let start = position_to_byte_offset(
                source,
                &line_starts,
                range
                    .get("start")
                    .and_then(|start| start.get("line"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned a text edit with an invalid start line"
                                .to_string(),
                        )
                    })?,
                range
                    .get("start")
                    .and_then(|start| start.get("character"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned a text edit with an invalid start character"
                                .to_string(),
                        )
                    })?,
            )?;
            let end = position_to_byte_offset(
                source,
                &line_starts,
                range
                    .get("end")
                    .and_then(|end| end.get("line"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned a text edit with an invalid end line"
                                .to_string(),
                        )
                    })?,
                range
                    .get("end")
                    .and_then(|end| end.get("character"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned a text edit with an invalid end character"
                                .to_string(),
                        )
                    })?,
            )?;
            Ok(ParsedTextEdit {
                start,
                end,
                new_text: edit
                    .get("newText")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "code.lsp.rename returned a text edit without newText".to_string(),
                        )
                    })?
                    .to_string(),
                annotation_id: edit
                    .get("annotationId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn apply_text_edits(source: &str, edits: &[ParsedTextEdit]) -> Result<String, ToolError> {
    let mut ordered = edits
        .iter()
        .map(|edit| (edit.start, edit.end, edit.new_text.clone()))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(start, end, _)| (*start, *end));
    let mut previous_end = 0usize;
    for (start, end, _) in &ordered {
        if start > end {
            return Err(ToolError::Execution(
                "code.lsp.rename returned a text edit with an inverted range".to_string(),
            ));
        }
        if *start < previous_end {
            return Err(ToolError::Execution(
                "code.lsp.rename returned overlapping text edits".to_string(),
            ));
        }
        previous_end = *end;
    }

    let mut updated = source.to_string();
    for (start, end, new_text) in ordered.into_iter().rev() {
        updated.replace_range(start..end, &new_text);
    }
    Ok(updated)
}

fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn position_to_byte_offset(
    source: &str,
    line_starts: &[usize],
    line: u64,
    character: u64,
) -> Result<usize, ToolError> {
    let line = usize::try_from(line)
        .map_err(|_| ToolError::Execution("line index overflow in workspace edit".to_string()))?;
    let character = usize::try_from(character).map_err(|_| {
        ToolError::Execution("character index overflow in workspace edit".to_string())
    })?;
    let Some(&line_start) = line_starts.get(line) else {
        return Err(ToolError::Execution(format!(
            "workspace edit referenced missing line index {line}"
        )));
    };
    let raw_line_end = line_starts.get(line + 1).copied().unwrap_or(source.len());
    let mut line_end = raw_line_end;
    if line_end > line_start && source.as_bytes()[line_end - 1] == b'\n' {
        line_end -= 1;
        if line_end > line_start && source.as_bytes()[line_end - 1] == b'\r' {
            line_end -= 1;
        }
    }
    let line_text = &source[line_start..line_end];
    if character == 0 {
        return Ok(line_start);
    }

    let mut utf16_offset = 0usize;
    for (byte_offset, ch) in line_text.char_indices() {
        if utf16_offset == character {
            return Ok(line_start + byte_offset);
        }
        utf16_offset += ch.len_utf16();
        if utf16_offset == character {
            return Ok(line_start + byte_offset + ch.len_utf8());
        }
        if utf16_offset > character {
            return Err(ToolError::Execution(
                "workspace edit referenced a non-boundary UTF-16 character offset".to_string(),
            ));
        }
    }
    Ok(line_end)
}

fn workspace_relative_path_from_uri(
    workspace_root: &PathBuf,
    uri: &str,
) -> Result<String, ToolError> {
    let url = reqwest::Url::parse(uri)
        .map_err(|err| ToolError::Execution(format!("invalid workspace edit uri: {err}")))?;
    let path = url.to_file_path().map_err(|_| {
        ToolError::Execution(format!(
            "workspace edit uri does not map to a local filesystem path: {uri}"
        ))
    })?;
    let normalized = crate::code_lsp::normalize_workspace_target_path(workspace_root, &path)?;
    normalized
        .strip_prefix(workspace_root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|_| ToolError::PathEscapesWorkspace {
            workspace_root: workspace_root.display().to_string(),
            path: normalized.display().to_string(),
        })
}

fn next_rename_edit_id(ctx: &ToolContext, index: usize) -> String {
    format!("lsp-rename-{}-{index}", ctx.tool_call_id)
}

fn apply_plan(ctx: &ToolContext, plan: &RenamePlan) -> Result<Vec<ToolResult>, ToolError> {
    let mut results = Vec::with_capacity(plan.operations.len());
    for operation in plan.operations.iter().cloned() {
        results.push(apply_hashline_workspace_op_to_workspace(ctx, operation)?);
    }
    Ok(results)
}

fn symbol_preview(
    file_path: &PathBuf,
    prepare_result: &Value,
) -> Result<Option<String>, ToolError> {
    if let Some(placeholder) = prepare_result.get("placeholder").and_then(Value::as_str) {
        return Ok(Some(placeholder.to_string()));
    }
    if prepare_result
        .get("defaultBehavior")
        .and_then(Value::as_bool)
        .is_some()
    {
        return Ok(None);
    }
    if prepare_result.get("start").is_some() && prepare_result.get("end").is_some() {
        let source = std::fs::read_to_string(file_path).map_err(|err| {
            ToolError::Execution(format!(
                "failed to read rename source file for preview: {err}"
            ))
        })?;
        return extract_text_for_range(&source, prepare_result).map(Some);
    }
    if let Some(range) = prepare_result.get("range") {
        let source = std::fs::read_to_string(file_path).map_err(|err| {
            ToolError::Execution(format!(
                "failed to read rename source file for preview: {err}"
            ))
        })?;
        return extract_text_for_range(&source, range).map(Some);
    }
    Ok(None)
}

fn extract_text_for_range(source: &str, range: &Value) -> Result<String, ToolError> {
    let line_starts = build_line_starts(source);
    let start = position_to_byte_offset(
        source,
        &line_starts,
        range
            .get("start")
            .and_then(|start| start.get("line"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::Execution("rename prepare result is missing start.line".to_string())
            })?,
        range
            .get("start")
            .and_then(|start| start.get("character"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::Execution("rename prepare result is missing start.character".to_string())
            })?,
    )?;
    let end = position_to_byte_offset(
        source,
        &line_starts,
        range
            .get("end")
            .and_then(|end| end.get("line"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::Execution("rename prepare result is missing end.line".to_string())
            })?,
        range
            .get("end")
            .and_then(|end| end.get("character"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::Execution("rename prepare result is missing end.character".to_string())
            })?,
    )?;
    Ok(source[start..end].to_string())
}

fn render_display_text(
    new_name: &str,
    apply: bool,
    symbol_preview: Option<&str>,
    preview: &RenamePreview,
    response: &LspRenameResponse,
) -> String {
    let current = symbol_preview.unwrap_or("<symbol>");
    let mut lines = vec![if apply {
        format!("Applied LSP rename `{current}` → `{new_name}`")
    } else {
        format!("Prepared LSP rename preview `{current}` → `{new_name}`")
    }];
    lines.push(format!("Server: {}", response.server.name));

    if preview.file_count == 0 && preview.resource_operations.is_empty() {
        lines.push("No workspace changes were returned.".to_string());
    } else {
        lines.push(format!(
            "{} text edit{} across {} file{}",
            preview.text_edit_count,
            plural(preview.text_edit_count),
            preview.file_count,
            plural(preview.file_count),
        ));
        for file in &preview.files {
            let annotation_suffix = if file.annotation_ids.is_empty() {
                String::new()
            } else {
                format!(" · annotations: {}", file.annotation_ids.join(", "))
            };
            lines.push(format!(
                "- {} ({} edit{}){}",
                file.path,
                file.edit_count,
                plural(file.edit_count),
                annotation_suffix
            ));
        }
        for operation in &preview.resource_operations {
            let detail = match &operation.to_path {
                Some(to_path) => format!("{} → {}", operation.path, to_path),
                None => operation.path.clone(),
            };
            lines.push(format!("- {} {}", operation.kind, detail));
        }
        if !apply {
            lines.push("Re-run with `apply: true` to execute these edits.".to_string());
        }
    }

    let diagnostics = format_diagnostics(&response.diagnostics);
    if !diagnostics.is_empty() {
        lines.push(String::new());
        lines.push("Diagnostics:".to_string());
        lines.push(diagnostics);
    }

    lines.join("\n")
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
