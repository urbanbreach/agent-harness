use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::edit::hashline::HashlineWorkspaceOp;
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
};
use crate::lsp_support::{
    execute_lsp_rename, format_diagnostics, LspPosition, LspRenameRequest, LspRenameResponse,
};
use crate::parse_tool_args;
use crate::workspace_paths::{
    canonical_workspace_root, resolve_existing_path, workspace_relative_path_from_file_uri,
};

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

        let plan = RenamePlan::from_workspace_edit(ctx, &response.workspace_edit)?;
        let symbol_preview = RenamePreparePreview::from_prepare_result(&response.prepare_result)
            .symbol_preview(&file_path)?;
        let applied_results = if request.apply {
            plan.apply(ctx)?
        } else {
            AppliedRenameResults::default()
        };
        let display_text = plan.preview.display_text(
            &request.new_name,
            request.apply,
            symbol_preview.as_deref(),
            &response,
        );

        Ok(crate::text_json_artifacts_tool_result(
            display_text,
            json!({
                "operation": "renameSymbol",
                "filePath": file_path.display().to_string(),
                "line": request.line,
                "character": request.character,
                "newName": request.new_name,
                "apply": request.apply,
                "applied": request.apply && plan.has_operations(),
                "server": {
                    "name": response.server.name,
                    "command": response.server.command,
                },
                "prepareRename": response.prepare_result,
                "workspaceEdit": response.workspace_edit,
                "preview": plan.preview,
                "symbol": symbol_preview,
                "diagnostics": response.diagnostics,
                "appliedEdits": applied_results.structured_json,
            }),
            applied_results.artifacts,
        ))
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

#[derive(Debug, Clone, Default, Serialize)]
struct RenamePreview {
    file_count: usize,
    text_edit_count: usize,
    files: Vec<RenameFilePreview>,
    resource_operations: Vec<RenameResourceOperationPreview>,
    annotations: Vec<RenameAnnotationPreview>,
}

impl RenamePreview {
    fn from_accumulators(
        preview_files: BTreeMap<String, PreviewFileAccumulator>,
        resource_operations: Vec<RenameResourceOperationPreview>,
        workspace_edit: &Value,
    ) -> Self {
        let files = preview_files
            .into_iter()
            .map(|(path, accumulator)| RenameFilePreview {
                path,
                edit_count: accumulator.edit_count,
                annotation_ids: accumulator.annotation_ids.into_iter().collect(),
            })
            .collect::<Vec<_>>();
        let text_edit_count = files.iter().map(|file| file.edit_count).sum();

        Self {
            file_count: files.len(),
            text_edit_count,
            files,
            resource_operations,
            annotations: RenameAnnotationPreview::from_workspace_edit(workspace_edit),
        }
    }

    fn append_summary_lines(&self, lines: &mut Vec<String>, apply: bool) {
        if self.file_count == 0 && self.resource_operations.is_empty() {
            lines.push("No workspace changes were returned.".to_string());
            return;
        }

        lines.push(format!(
            "{} text edit{} across {} file{}",
            self.text_edit_count,
            plural(self.text_edit_count),
            self.file_count,
            plural(self.file_count),
        ));
        for file in &self.files {
            lines.push(file.display_line());
        }
        for operation in &self.resource_operations {
            lines.push(operation.display_line());
        }
        if !apply {
            lines.push("Re-run with `apply: true` to execute these edits.".to_string());
        }
    }

    fn display_text(
        &self,
        new_name: &str,
        apply: bool,
        symbol_preview: Option<&str>,
        response: &LspRenameResponse,
    ) -> String {
        let current = symbol_preview.unwrap_or("<symbol>");
        let mut lines = vec![if apply {
            format!("Applied LSP rename `{current}` → `{new_name}`")
        } else {
            format!("Prepared LSP rename preview `{current}` → `{new_name}`")
        }];
        lines.push(format!("Server: {}", response.server.name));

        self.append_summary_lines(&mut lines, apply);

        let diagnostics = format_diagnostics(&response.diagnostics);
        if !diagnostics.is_empty() {
            lines.push(String::new());
            lines.push("Diagnostics:".to_string());
            lines.push(diagnostics);
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize)]
struct RenameFilePreview {
    path: String,
    edit_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotation_ids: Vec<String>,
}

impl RenameFilePreview {
    fn display_line(&self) -> String {
        let annotation_suffix = if self.annotation_ids.is_empty() {
            String::new()
        } else {
            format!(" · annotations: {}", self.annotation_ids.join(", "))
        };
        format!(
            "- {} ({} edit{}){}",
            self.path,
            self.edit_count,
            plural(self.edit_count),
            annotation_suffix
        )
    }
}

#[derive(Debug, Clone, Serialize)]
struct RenameResourceOperationPreview {
    kind: RenameResourceOperationKind,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation_id: Option<String>,
}

impl RenameResourceOperationPreview {
    fn from_change(
        kind: RenameResourceOperationKind,
        path: &str,
        to_path: Option<&str>,
        change: &Value,
    ) -> Self {
        Self {
            kind,
            path: path.to_string(),
            to_path: to_path.map(str::to_string),
            annotation_id: change
                .get("annotationId")
                .and_then(Value::as_str)
                .map(str::to_string),
        }
    }

    fn display_line(&self) -> String {
        let detail = match &self.to_path {
            Some(to_path) => format!("{} → {}", self.path, to_path),
            None => self.path.clone(),
        };
        format!("- {} {}", self.kind.as_str(), detail)
    }
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

impl RenameAnnotationPreview {
    fn from_workspace_edit(workspace_edit: &Value) -> Vec<Self> {
        let Some(annotations) = workspace_edit
            .get("changeAnnotations")
            .and_then(Value::as_object)
        else {
            return Vec::new();
        };

        annotations
            .iter()
            .map(|(id, annotation)| Self::from_change_annotation(id, annotation))
            .collect()
    }

    fn from_change_annotation(id: &str, annotation: &Value) -> Self {
        Self {
            id: id.to_string(),
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
        }
    }
}

#[derive(Default)]
struct RenamePlan {
    operations: Vec<HashlineWorkspaceOp>,
    preview: RenamePreview,
}

impl RenamePlan {
    fn from_workspace_edit(ctx: &ToolContext, workspace_edit: &Value) -> Result<Self, ToolError> {
        if workspace_edit.is_null() {
            return Ok(Self::default());
        }

        let workspace_root = canonical_workspace_root(ctx)?;
        let mut state = RenamePlanningState::default();

        RenameWorkspaceEdit::from_workspace_edit(workspace_edit)?.plan(
            ctx,
            &workspace_root,
            &mut state,
        )?;

        Ok(state.into_plan(workspace_edit))
    }

    fn has_operations(&self) -> bool {
        !self.operations.is_empty()
    }

    fn apply(&self, ctx: &ToolContext) -> Result<AppliedRenameResults, ToolError> {
        let mut results = AppliedRenameResults::default();
        for operation in self.operations.iter().cloned() {
            results.push(apply_hashline_workspace_op_to_workspace(ctx, operation)?);
        }
        Ok(results)
    }
}

enum RenameWorkspaceEdit<'a> {
    DocumentChanges(&'a [Value]),
    LegacyChanges(&'a Map<String, Value>),
}

impl RenameWorkspaceEdit<'_> {
    fn from_workspace_edit(workspace_edit: &Value) -> Result<RenameWorkspaceEdit<'_>, ToolError> {
        if let Some(document_changes) = workspace_edit
            .get("documentChanges")
            .and_then(Value::as_array)
        {
            return Ok(RenameWorkspaceEdit::DocumentChanges(document_changes));
        }

        if let Some(changes) = workspace_edit.get("changes").and_then(Value::as_object) {
            return Ok(RenameWorkspaceEdit::LegacyChanges(changes));
        }

        Err(ToolError::Execution(
            "lsp.rename returned a workspace edit without changes".to_string(),
        ))
    }

    fn plan(
        self,
        ctx: &ToolContext,
        workspace_root: &Path,
        state: &mut RenamePlanningState,
    ) -> Result<(), ToolError> {
        match self {
            Self::DocumentChanges(document_changes) => {
                for change in document_changes {
                    RenameDocumentChange::from_change(change)?.plan(ctx, workspace_root, state)?;
                }
            }
            Self::LegacyChanges(changes) => {
                for (uri, edits_value) in changes {
                    let path = workspace_relative_path_from_file_uri(workspace_root, uri)?;
                    let edits = required_array(
                        Some(edits_value),
                        "lsp.rename returned changes with a non-array edit list",
                    )?;
                    state.plan_text_edits_for_path(ctx, &path, edits)?;
                }
            }
        }
        Ok(())
    }
}

enum RenamePreparePreview<'a> {
    Placeholder(&'a str),
    Range(&'a Value),
    DefaultBehavior,
    None,
}

impl RenamePreparePreview<'_> {
    fn from_prepare_result(prepare_result: &Value) -> RenamePreparePreview<'_> {
        if let Some(placeholder) = prepare_result.get("placeholder").and_then(Value::as_str) {
            return RenamePreparePreview::Placeholder(placeholder);
        }
        if prepare_result
            .get("defaultBehavior")
            .and_then(Value::as_bool)
            .is_some()
        {
            return RenamePreparePreview::DefaultBehavior;
        }
        if prepare_result.get("start").is_some() && prepare_result.get("end").is_some() {
            return RenamePreparePreview::Range(prepare_result);
        }
        prepare_result
            .get("range")
            .map(RenamePreparePreview::Range)
            .unwrap_or(RenamePreparePreview::None)
    }

    fn symbol_preview(self, file_path: &Path) -> Result<Option<String>, ToolError> {
        match self {
            Self::Placeholder(placeholder) => Ok(Some(placeholder.to_string())),
            Self::Range(range) => {
                let source = std::fs::read_to_string(file_path).map_err(|err| {
                    ToolError::Execution(format!(
                        "failed to read rename source file for preview: {err}"
                    ))
                })?;
                let source = SourceText::new(&source);
                source
                    .text_for_lsp_range(range, RangeErrorMessages::prepare_result())
                    .map(str::to_string)
                    .map(Some)
            }
            Self::DefaultBehavior | Self::None => Ok(None),
        }
    }
}

enum RenameDocumentChange<'a> {
    TextDocument {
        text_document: &'a Value,
        change: &'a Value,
    },
    Resource {
        kind: RenameResourceOperationKind,
        change: &'a Value,
    },
}

impl RenameDocumentChange<'_> {
    fn from_change(change: &Value) -> Result<RenameDocumentChange<'_>, ToolError> {
        if let Some(text_document) = change.get("textDocument") {
            return Ok(RenameDocumentChange::TextDocument {
                text_document,
                change,
            });
        }

        let kind = RenameResourceOperationKind::from_change(change)?;
        Ok(RenameDocumentChange::Resource { kind, change })
    }

    fn plan(
        self,
        ctx: &ToolContext,
        workspace_root: &Path,
        state: &mut RenamePlanningState,
    ) -> Result<(), ToolError> {
        match self {
            Self::TextDocument {
                text_document,
                change,
            } => {
                let path = required_workspace_uri(
                    workspace_root,
                    text_document,
                    "uri",
                    "lsp.rename returned documentChanges without textDocument.uri",
                )?;
                let edits = required_array(
                    change.get("edits"),
                    "lsp.rename returned documentChanges without edits",
                )?;
                state.plan_text_edits_for_path(ctx, &path, edits)
            }
            Self::Resource { kind, change } => kind.plan(ctx, workspace_root, change, state),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum RenameResourceOperationKind {
    Create,
    Rename,
    Delete,
}

impl RenameResourceOperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Rename => "rename",
            Self::Delete => "delete",
        }
    }

    fn from_change(change: &Value) -> Result<Self, ToolError> {
        match change.get("kind").and_then(Value::as_str).ok_or_else(|| {
            ToolError::Execution(
                "lsp.rename returned documentChanges with an unknown item".to_string(),
            )
        })? {
            "create" => Ok(Self::Create),
            "rename" => Ok(Self::Rename),
            "delete" => Ok(Self::Delete),
            other => Err(ToolError::Execution(format!(
                "lsp.rename returned unsupported workspace edit operation kind: {other}"
            ))),
        }
    }

    fn plan(
        self,
        ctx: &ToolContext,
        workspace_root: &Path,
        change: &Value,
        state: &mut RenamePlanningState,
    ) -> Result<(), ToolError> {
        match self {
            Self::Create => {
                let path = required_workspace_uri(
                    workspace_root,
                    change,
                    "uri",
                    "lsp.rename create operation is missing uri",
                )?;
                state.plan_create_operation(ctx, change, &path)
            }
            Self::Rename => {
                let from_path = required_workspace_uri(
                    workspace_root,
                    change,
                    "oldUri",
                    "lsp.rename rename operation is missing oldUri",
                )?;
                let to_path = required_workspace_uri(
                    workspace_root,
                    change,
                    "newUri",
                    "lsp.rename rename operation is missing newUri",
                )?;
                state.plan_rename_operation(ctx, change, &from_path, &to_path)
            }
            Self::Delete => {
                let path = required_workspace_uri(
                    workspace_root,
                    change,
                    "uri",
                    "lsp.rename delete operation is missing uri",
                )?;
                state.plan_delete_operation(ctx, change, &path)
            }
        }
    }
}

#[derive(Default)]
struct AppliedRenameResults {
    artifacts: Vec<ArtifactRef>,
    structured_json: Vec<Value>,
}

impl AppliedRenameResults {
    fn push(&mut self, result: ToolResult) {
        self.artifacts.extend(result.artifacts);
        if let Some(structured_json) = result.structured_json {
            self.structured_json.push(structured_json);
        }
    }
}

#[derive(Default)]
struct RenamePlanningState {
    virtual_files: BTreeMap<String, Option<String>>,
    operations: Vec<HashlineWorkspaceOp>,
    preview_files: BTreeMap<String, PreviewFileAccumulator>,
    resource_operations: Vec<RenameResourceOperationPreview>,
    next_operation_index: usize,
}

impl RenamePlanningState {
    fn plan_file_rewrite(&mut self, ctx: &ToolContext, path: &str, content: String) {
        self.set_virtual_file_content(path, content.clone());
        self.push_operation(ctx, |edit_id| HashlineWorkspaceOp::RewriteFile {
            edit_id,
            path: path.to_string(),
            content,
        });
    }

    fn plan_file_delete(&mut self, ctx: &ToolContext, path: &str) {
        self.set_virtual_file_deleted(path);
        self.push_operation(ctx, |edit_id| HashlineWorkspaceOp::DeleteFile {
            edit_id,
            path: path.to_string(),
        });
    }

    fn plan_file_move(
        &mut self,
        ctx: &ToolContext,
        from_path: &str,
        to_path: &str,
        content: String,
    ) {
        self.set_virtual_file_deleted(from_path);
        self.set_virtual_file_content(to_path, content);
        self.push_operation(ctx, |edit_id| HashlineWorkspaceOp::MoveFile {
            edit_id,
            from_path: from_path.to_string(),
            to_path: to_path.to_string(),
        });
    }

    fn plan_resource_create(&mut self, ctx: &ToolContext, change: &Value, path: &str) {
        self.plan_file_rewrite(ctx, path, String::new());
        self.push_resource_preview(RenameResourceOperationKind::Create, path, None, change);
    }

    fn plan_resource_rename(
        &mut self,
        ctx: &ToolContext,
        change: &Value,
        from_path: &str,
        to_path: &str,
        content: String,
    ) {
        self.plan_file_move(ctx, from_path, to_path, content);
        self.push_resource_preview(
            RenameResourceOperationKind::Rename,
            from_path,
            Some(to_path),
            change,
        );
    }

    fn plan_resource_delete(&mut self, ctx: &ToolContext, change: &Value, path: &str) {
        self.plan_file_delete(ctx, path);
        self.push_resource_preview(RenameResourceOperationKind::Delete, path, None, change);
    }

    fn push_operation(
        &mut self,
        ctx: &ToolContext,
        build: impl FnOnce(String) -> HashlineWorkspaceOp,
    ) {
        let edit_id = self.next_edit_id(ctx);
        self.operations.push(build(edit_id));
    }

    fn push_resource_preview(
        &mut self,
        kind: RenameResourceOperationKind,
        path: &str,
        to_path: Option<&str>,
        change: &Value,
    ) {
        self.resource_operations
            .push(RenameResourceOperationPreview::from_change(
                kind, path, to_path, change,
            ));
    }

    fn load_virtual_file(
        &mut self,
        ctx: &ToolContext,
        path: &str,
    ) -> Result<Option<String>, ToolError> {
        if let Some(existing) = self.virtual_files.get(path) {
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
        self.virtual_files.insert(path.to_string(), content.clone());
        Ok(content)
    }

    fn virtual_path_exists(&mut self, ctx: &ToolContext, path: &str) -> Result<bool, ToolError> {
        Ok(self.load_virtual_file(ctx, path)?.is_some())
    }

    fn require_virtual_file(
        &mut self,
        ctx: &ToolContext,
        path: &str,
        missing_message: String,
    ) -> Result<String, ToolError> {
        self.load_virtual_file(ctx, path)?
            .ok_or(ToolError::Execution(missing_message))
    }

    fn set_virtual_file_content(&mut self, path: &str, content: String) {
        self.virtual_files.insert(path.to_string(), Some(content));
    }

    fn set_virtual_file_deleted(&mut self, path: &str) {
        self.virtual_files.insert(path.to_string(), None);
    }

    fn next_edit_id(&mut self, ctx: &ToolContext) -> String {
        let edit_id = format!(
            "lsp-rename-{}-{}",
            ctx.tool_call_id, self.next_operation_index
        );
        self.next_operation_index += 1;
        edit_id
    }

    fn record_text_edits(&mut self, path: &str, parsed_edits: &[ParsedTextEdit]) {
        self.preview_files
            .entry(path.to_string())
            .or_default()
            .record_text_edits(parsed_edits);
    }

    fn plan_text_edits_for_path(
        &mut self,
        ctx: &ToolContext,
        path: &str,
        edits: &[Value],
    ) -> Result<(), ToolError> {
        let source = self.require_virtual_file(
            ctx,
            path,
            format!("lsp.rename returned edits for a missing workspace path: {path}"),
        )?;
        let source = SourceText::new(&source);
        let parsed_edits = source.parse_lsp_text_edits(edits)?;
        let updated = source.apply_text_edits(&parsed_edits)?;
        self.plan_file_rewrite(ctx, path, updated);
        self.record_text_edits(path, &parsed_edits);
        Ok(())
    }

    fn plan_create_operation(
        &mut self,
        ctx: &ToolContext,
        change: &Value,
        path: &str,
    ) -> Result<(), ToolError> {
        let options = RenameResourceOperationOptions::from_change(change);
        let target_exists = self.virtual_path_exists(ctx, path)?;
        if target_exists {
            match options.existing_target_policy() {
                ExistingTargetPolicy::Ignore => return Ok(()),
                ExistingTargetPolicy::Reject => {
                    return Err(ToolError::Execution(format!(
                        "lsp.rename create operation would overwrite existing path: {path}"
                    )));
                }
                ExistingTargetPolicy::Overwrite => {}
            }
        }

        self.plan_resource_create(ctx, change, path);
        Ok(())
    }

    fn plan_rename_operation(
        &mut self,
        ctx: &ToolContext,
        change: &Value,
        from_path: &str,
        to_path: &str,
    ) -> Result<(), ToolError> {
        let options = RenameResourceOperationOptions::from_change(change);
        let source = self.require_virtual_file(
            ctx,
            from_path,
            format!("lsp.rename rename operation source is missing: {from_path}"),
        )?;
        if self.virtual_path_exists(ctx, to_path)? {
            match options.existing_target_policy() {
                ExistingTargetPolicy::Ignore => return Ok(()),
                ExistingTargetPolicy::Reject => {
                    return Err(ToolError::Execution(format!(
                        "lsp.rename rename operation destination already exists: {to_path}"
                    )));
                }
                ExistingTargetPolicy::Overwrite => self.plan_file_delete(ctx, to_path),
            }
        }

        self.plan_resource_rename(ctx, change, from_path, to_path, source);
        Ok(())
    }

    fn plan_delete_operation(
        &mut self,
        ctx: &ToolContext,
        change: &Value,
        path: &str,
    ) -> Result<(), ToolError> {
        let options = RenameResourceOperationOptions::from_change(change);
        let target_missing = !self.virtual_path_exists(ctx, path)?;
        if target_missing {
            match options.missing_target_policy() {
                MissingTargetPolicy::Ignore => return Ok(()),
                MissingTargetPolicy::Reject => {
                    return Err(ToolError::Execution(format!(
                        "lsp.rename delete operation target is missing: {path}"
                    )));
                }
            }
        }

        self.plan_resource_delete(ctx, change, path);
        Ok(())
    }

    fn into_plan(self, workspace_edit: &Value) -> RenamePlan {
        RenamePlan {
            operations: self.operations,
            preview: RenamePreview::from_accumulators(
                self.preview_files,
                self.resource_operations,
                workspace_edit,
            ),
        }
    }
}

struct RenameResourceOperationOptions {
    existing_target_policy: ExistingTargetPolicy,
    missing_target_policy: MissingTargetPolicy,
}

#[derive(Clone, Copy)]
enum ExistingTargetPolicy {
    Ignore,
    Reject,
    Overwrite,
}

#[derive(Clone, Copy)]
enum MissingTargetPolicy {
    Ignore,
    Reject,
}

impl RenameResourceOperationOptions {
    fn from_change(change: &Value) -> Self {
        let overwrite_existing_target = Self::bool_option(change, "overwrite");
        let ignore_existing_target = Self::bool_option(change, "ignoreIfExists");
        let ignore_missing_target = Self::bool_option(change, "ignoreIfNotExists");

        Self {
            existing_target_policy: ExistingTargetPolicy::from_options(
                overwrite_existing_target,
                ignore_existing_target,
            ),
            missing_target_policy: MissingTargetPolicy::from_options(ignore_missing_target),
        }
    }

    fn bool_option(change: &Value, name: &str) -> bool {
        change
            .get("options")
            .and_then(|options| options.get(name))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn existing_target_policy(&self) -> ExistingTargetPolicy {
        self.existing_target_policy
    }

    fn missing_target_policy(&self) -> MissingTargetPolicy {
        self.missing_target_policy
    }
}

impl ExistingTargetPolicy {
    fn from_options(overwrite_existing_target: bool, ignore_existing_target: bool) -> Self {
        match (overwrite_existing_target, ignore_existing_target) {
            (true, _) => ExistingTargetPolicy::Overwrite,
            (false, true) => ExistingTargetPolicy::Ignore,
            (false, false) => ExistingTargetPolicy::Reject,
        }
    }
}

impl MissingTargetPolicy {
    fn from_options(ignore_missing_target: bool) -> Self {
        if ignore_missing_target {
            MissingTargetPolicy::Ignore
        } else {
            MissingTargetPolicy::Reject
        }
    }
}

#[derive(Default)]
struct PreviewFileAccumulator {
    edit_count: usize,
    annotation_ids: BTreeSet<String>,
}

impl PreviewFileAccumulator {
    fn record_text_edits(&mut self, parsed_edits: &[ParsedTextEdit]) {
        self.edit_count += parsed_edits.len();
        self.annotation_ids.extend(
            parsed_edits
                .iter()
                .filter_map(|edit| edit.annotation_id.as_deref())
                .map(str::to_string),
        );
    }
}

struct ParsedTextEdit {
    range: TextByteRange,
    new_text: String,
    annotation_id: Option<String>,
}

impl ParsedTextEdit {
    fn from_lsp_edit(source: &SourceText<'_>, edit: &Value) -> Result<Self, ToolError> {
        let range = edit.get("range").ok_or_else(|| {
            ToolError::Execution("lsp.rename returned a text edit without a range".to_string())
        })?;
        let range = TextByteRange::from_lsp_range(source, range, RangeErrorMessages::text_edit())?;
        let new_text = edit
            .get("newText")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::Execution("lsp.rename returned a text edit without newText".to_string())
            })?
            .to_string();
        let annotation_id = edit
            .get("annotationId")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Self {
            range,
            new_text,
            annotation_id,
        })
    }
}

struct TextPosition {
    line: u64,
    character: u64,
}

impl TextPosition {
    fn from_range_endpoint(
        range: &Value,
        endpoint: &str,
        error_messages: RangePositionErrorMessages,
    ) -> Result<Self, ToolError> {
        let position = range.get(endpoint);
        let line = position
            .and_then(|position| position.get("line"))
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::Execution(error_messages.missing_line.to_string()))?;
        let character = position
            .and_then(|position| position.get("character"))
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::Execution(error_messages.missing_character.to_string()))?;
        Ok(Self { line, character })
    }

    fn line_index(&self) -> Result<usize, ToolError> {
        usize::try_from(self.line)
            .map_err(|_| ToolError::Execution("line index overflow in workspace edit".to_string()))
    }

    fn character_offset(&self) -> Result<usize, ToolError> {
        usize::try_from(self.character).map_err(|_| {
            ToolError::Execution("character index overflow in workspace edit".to_string())
        })
    }

    fn to_byte_offset(&self, source: &SourceText<'_>) -> Result<usize, ToolError> {
        let line = self.line_index()?;
        let character = self.character_offset()?;
        source.line(line)?.utf16_position_to_byte_offset(character)
    }
}

struct TextByteRange {
    start: usize,
    end: usize,
}

impl TextByteRange {
    fn from_lsp_range(
        source: &SourceText<'_>,
        range: &Value,
        error_messages: RangeErrorMessages,
    ) -> Result<Self, ToolError> {
        let start = TextPosition::from_range_endpoint(range, "start", error_messages.start)?;
        let end = TextPosition::from_range_endpoint(range, "end", error_messages.end)?;
        let start = start.to_byte_offset(source)?;
        let end = end.to_byte_offset(source)?;
        Ok(Self { start, end })
    }

    fn validate_after(&self, previous_end: usize) -> Result<(), ToolError> {
        if self.start > self.end {
            return Err(ToolError::Execution(
                "lsp.rename returned a text edit with an inverted range".to_string(),
            ));
        }
        if self.start < previous_end {
            return Err(ToolError::Execution(
                "lsp.rename returned overlapping text edits".to_string(),
            ));
        }
        Ok(())
    }

    fn replace_in(&self, text: &mut String, replacement: &str) {
        text.replace_range(self.start..self.end, replacement);
    }

    fn slice_from<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }
}

struct SourceText<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceText<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            line_starts: Self::line_starts(text),
        }
    }

    fn line_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        starts
    }

    fn line(&self, line: usize) -> Result<SourceLine<'a>, ToolError> {
        let Some(&start) = self.line_starts.get(line) else {
            return Err(ToolError::Execution(format!(
                "workspace edit referenced missing line index {line}"
            )));
        };
        Ok(SourceLine::from_bounds(
            self.text,
            start,
            self.line_end(line),
        ))
    }

    fn line_end(&self, line: usize) -> usize {
        self.line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len())
    }

    fn apply_text_edits(&self, edits: &[ParsedTextEdit]) -> Result<String, ToolError> {
        let ordered = Self::ordered_non_overlapping_text_edits(edits)?;

        let mut updated = self.text.to_string();
        for edit in ordered.into_iter().rev() {
            edit.range.replace_in(&mut updated, &edit.new_text);
        }
        Ok(updated)
    }

    fn ordered_non_overlapping_text_edits(
        edits: &[ParsedTextEdit],
    ) -> Result<Vec<&ParsedTextEdit>, ToolError> {
        let mut ordered = edits.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|edit| (edit.range.start, edit.range.end));

        let mut previous_end = 0usize;
        for edit in &ordered {
            edit.range.validate_after(previous_end)?;
            previous_end = edit.range.end;
        }

        Ok(ordered)
    }

    fn parse_lsp_text_edits(&self, edits: &[Value]) -> Result<Vec<ParsedTextEdit>, ToolError> {
        edits
            .iter()
            .map(|edit| ParsedTextEdit::from_lsp_edit(self, edit))
            .collect()
    }

    fn text_for_lsp_range(
        &self,
        range: &Value,
        errors: RangeErrorMessages,
    ) -> Result<&'a str, ToolError> {
        let range = TextByteRange::from_lsp_range(self, range, errors)?;
        Ok(range.slice_from(self.text))
    }
}

struct SourceLine<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl<'a> SourceLine<'a> {
    fn from_bounds(source: &'a str, start: usize, mut end: usize) -> Self {
        if end > start && source.as_bytes()[end - 1] == b'\n' {
            end -= 1;
            if end > start && source.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
        }
        SourceLine {
            text: &source[start..end],
            start,
            end,
        }
    }

    fn utf16_position_to_byte_offset(&self, character: usize) -> Result<usize, ToolError> {
        if character == 0 {
            return Ok(self.start);
        }

        let mut utf16_offset = 0usize;
        for (byte_offset, ch) in self.text.char_indices() {
            if utf16_offset == character {
                return Ok(self.start + byte_offset);
            }
            utf16_offset += ch.len_utf16();
            if utf16_offset == character {
                return Ok(self.start + byte_offset + ch.len_utf8());
            }
            if utf16_offset > character {
                return Err(ToolError::Execution(
                    "workspace edit referenced a non-boundary UTF-16 character offset".to_string(),
                ));
            }
        }
        Ok(self.end)
    }
}

struct RangePositionErrorMessages {
    missing_line: &'static str,
    missing_character: &'static str,
}

struct RangeErrorMessages {
    start: RangePositionErrorMessages,
    end: RangePositionErrorMessages,
}

impl RangeErrorMessages {
    fn text_edit() -> Self {
        Self {
            start: RangePositionErrorMessages {
                missing_line: "lsp.rename returned a text edit with an invalid start line",
                missing_character:
                    "lsp.rename returned a text edit with an invalid start character",
            },
            end: RangePositionErrorMessages {
                missing_line: "lsp.rename returned a text edit with an invalid end line",
                missing_character: "lsp.rename returned a text edit with an invalid end character",
            },
        }
    }

    fn prepare_result() -> Self {
        Self {
            start: RangePositionErrorMessages {
                missing_line: "rename prepare result is missing start.line",
                missing_character: "rename prepare result is missing start.character",
            },
            end: RangePositionErrorMessages {
                missing_line: "rename prepare result is missing end.line",
                missing_character: "rename prepare result is missing end.character",
            },
        }
    }
}

#[async_trait]
impl Tool for CodeLspRenameTool {
    fn id(&self) -> &str {
        "lsp.rename"
    }

    fn description(&self) -> &str {
        "Previews or applies a semantic LSP rename through an explicit workspace-editing flow."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<CodeLspRenameArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: CodeLspRenameArgs = parse_tool_args(args_json)?;
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

fn required_workspace_uri(
    workspace_root: &Path,
    value: &Value,
    key: &str,
    missing_message: &str,
) -> Result<String, ToolError> {
    let uri = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution(missing_message.to_string()))?;
    workspace_relative_path_from_file_uri(workspace_root, uri)
}

fn required_array<'a>(
    value: Option<&'a Value>,
    missing_or_invalid_message: &str,
) -> Result<&'a [Value], ToolError> {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| ToolError::Execution(missing_or_invalid_message.to_string()))
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
