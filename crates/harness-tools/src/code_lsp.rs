use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lsp_support::{
    execute_lsp_operation, LspDiagnosticReport, LspOperation, LspOperationRequest,
    LspOperationResponse, LspPosition,
};

pub(crate) struct CodeLspExecutor;

impl CodeLspExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn execute(
        &self,
        ctx: &ToolContext,
        request: CodeLspRequest,
    ) -> Result<ToolResult, ToolError> {
        let operation = LspOperation::parse(&request.operation)?;
        let position = LspPosition::from_one_based(request.line, request.character)?;
        let file_path = resolve_existing_path(ctx, &request.file_path)?;
        let response = tokio::task::spawn_blocking({
            let workspace_root = ctx.workspace_root.clone();
            let file_path = file_path.clone();
            move || {
                execute_lsp_operation(&LspOperationRequest {
                    operation,
                    file_path: &file_path,
                    position,
                    workspace_root: &workspace_root,
                })
            }
        })
        .await
        .map_err(|err| ToolError::Execution(format!("lsp task failed: {err}")))??;
        let operation_name = operation.as_str();
        let display_text = render_display_text(operation_name, &response)?;
        Ok(ToolResult {
            display_text,
            structured_json: Some(json!({
                "operation": operation_name,
                "filePath": file_path.display().to_string(),
                "line": request.line,
                "character": request.character,
                "server": {
                    "name": response.server.name,
                    "command": response.server.command,
                },
                "result": response.result,
                "diagnostics": response.diagnostics,
            })),
            artifacts: Vec::new(),
        })
    }
}

pub(crate) struct CodeLspTool {
    executor: Arc<CodeLspExecutor>,
}

impl CodeLspTool {
    pub(crate) fn new(executor: Arc<CodeLspExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodeLspRequest {
    pub(crate) operation: String,
    pub(crate) file_path: String,
    pub(crate) line: i32,
    pub(crate) character: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspArgs {
    operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
    line: i32,
    character: i32,
}

#[async_trait]
impl Tool for CodeLspTool {
    fn id(&self) -> &str {
        "code.lsp"
    }

    fn description(&self) -> &str {
        "Performs language-server operations through local LSP servers."
    }

    fn parameters_json_schema(&self) -> Value {
        let mut schema = super::json_schema_for::<CodeLspArgs>();
        if let Some(operation_schema) = schema
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut("operation"))
            .and_then(Value::as_object_mut)
        {
            operation_schema.insert("enum".to_string(), json!(LspOperation::supported_names()));
        }
        schema
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: CodeLspArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor
            .execute(
                &ctx,
                CodeLspRequest {
                    operation: args.operation,
                    file_path: args.file_path,
                    line: args.line,
                    character: args.character,
                },
            )
            .await
    }
}

fn lsp_result_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn render_display_text(
    operation_name: &str,
    response: &LspOperationResponse,
) -> Result<String, ToolError> {
    let mut text = if lsp_result_is_empty(&response.result) {
        format!("No results found for {operation_name}")
    } else {
        serde_json::to_string_pretty(&response.result)
            .map_err(|err| ToolError::Execution(format!("failed to render lsp result: {err}")))?
    };

    let diagnostics = format_diagnostics(&response.diagnostics);
    if !diagnostics.is_empty() {
        text.push_str("\n\nDiagnostics:\n");
        text.push_str(&diagnostics);
    }

    Ok(text)
}

fn format_diagnostics(reports: &[LspDiagnosticReport]) -> String {
    reports
        .iter()
        .flat_map(|report| {
            report.diagnostics.iter().map(|diagnostic| {
                let line = diagnostic
                    .get("range")
                    .and_then(|range| range.get("start"))
                    .and_then(|start| start.get("line"))
                    .and_then(Value::as_u64)
                    .map(|line| line + 1)
                    .unwrap_or(1);
                let character = diagnostic
                    .get("range")
                    .and_then(|range| range.get("start"))
                    .and_then(|start| start.get("character"))
                    .and_then(Value::as_u64)
                    .map(|character| character + 1)
                    .unwrap_or(1);
                let severity = match diagnostic.get("severity").and_then(Value::as_u64) {
                    Some(1) => "Error",
                    Some(2) => "Warning",
                    Some(3) => "Information",
                    Some(4) => "Hint",
                    _ => "Diagnostic",
                };
                let message = diagnostic
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing message>");
                format!(
                    "{}:{}:{} {} {}",
                    report.file_path, line, character, severity, message
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_existing_path(ctx: &ToolContext, input: &str) -> Result<PathBuf, ToolError> {
    let workspace = canonical_workspace_root(ctx)?;
    let candidate = normalize_workspace_target_path(&workspace, Path::new(input))?;
    let canonical = candidate
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve path: {err}")))?;
    ensure_within_workspace_path(&workspace, &canonical)?;
    Ok(canonical)
}

fn canonical_workspace_root(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    ctx.workspace_root
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve workspace root: {err}")))
}

fn ensure_within_workspace_path(workspace: &Path, candidate: &Path) -> Result<(), ToolError> {
    if candidate.starts_with(workspace) {
        Ok(())
    } else {
        Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: candidate.display().to_string(),
        })
    }
}

fn normalize_workspace_target_path(workspace: &Path, input: &Path) -> Result<PathBuf, ToolError> {
    let relative = if input.is_absolute() {
        input
            .strip_prefix(workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: input.display().to_string(),
            })?
    } else {
        input
    };

    let mut normalized = workspace.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::ParentDir => {
                if normalized == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: input.display().to_string(),
                    });
                }
                normalized.pop();
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(ToolError::InvalidArguments(
                    "path must be workspace-relative or inside the workspace".to_string(),
                ));
            }
        }
    }
    Ok(normalized)
}
