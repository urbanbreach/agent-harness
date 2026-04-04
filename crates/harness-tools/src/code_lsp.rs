use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lsp_support::{
    execute_lsp_operation, LspDiagnosticReport, LspOperation, LspOperationInput,
    LspOperationInputKind, LspOperationRequest, LspOperationResponse, LspPosition,
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
        match request {
            CodeLspRequest::Position {
                operation,
                file_path,
                line,
                character,
            } => {
                let position = LspPosition::from_one_based(line, character)?;
                let file_path = resolve_existing_path(ctx, &file_path)?;
                let response = tokio::task::spawn_blocking({
                    let workspace_root = ctx.workspace_root.clone();
                    let file_path = file_path.clone();
                    move || {
                        execute_lsp_operation(&LspOperationRequest {
                            operation,
                            input: LspOperationInput::Position {
                                file_path: &file_path,
                                position,
                            },
                            workspace_root: &workspace_root,
                        })
                    }
                })
                .await
                .map_err(|err| ToolError::Execution(format!("lsp task failed: {err}")))??;
                build_result(
                    operation.as_str(),
                    &file_path,
                    json!({
                        "line": line,
                        "character": character,
                    }),
                    response,
                )
            }
            CodeLspRequest::File {
                operation,
                file_path,
            } => {
                let file_path = resolve_existing_path(ctx, &file_path)?;
                let response = tokio::task::spawn_blocking({
                    let workspace_root = ctx.workspace_root.clone();
                    let file_path = file_path.clone();
                    move || {
                        execute_lsp_operation(&LspOperationRequest {
                            operation,
                            input: LspOperationInput::File {
                                file_path: &file_path,
                            },
                            workspace_root: &workspace_root,
                        })
                    }
                })
                .await
                .map_err(|err| ToolError::Execution(format!("lsp task failed: {err}")))??;
                build_result(operation.as_str(), &file_path, json!({}), response)
            }
            CodeLspRequest::Query {
                operation,
                file_path,
                query,
            } => {
                let file_path = resolve_existing_path(ctx, &file_path)?;
                let response = tokio::task::spawn_blocking({
                    let workspace_root = ctx.workspace_root.clone();
                    let file_path = file_path.clone();
                    let query = query.clone();
                    move || {
                        execute_lsp_operation(&LspOperationRequest {
                            operation,
                            input: LspOperationInput::Query {
                                file_path: &file_path,
                                query: &query,
                            },
                            workspace_root: &workspace_root,
                        })
                    }
                })
                .await
                .map_err(|err| ToolError::Execution(format!("lsp task failed: {err}")))??;
                build_result(
                    operation.as_str(),
                    &file_path,
                    json!({
                        "query": query,
                    }),
                    response,
                )
            }
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodeLspRequest {
    Position {
        operation: LspOperation,
        file_path: String,
        line: i32,
        character: i32,
    },
    File {
        operation: LspOperation,
        file_path: String,
    },
    Query {
        operation: LspOperation,
        file_path: String,
        query: String,
    },
}

#[derive(Debug, Deserialize)]
struct CodeLspOperationProbe {
    operation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspPositionArgs {
    #[serde(rename = "operation")]
    _operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
    line: i32,
    character: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspFileArgs {
    #[serde(rename = "operation")]
    _operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspQueryArgs {
    #[serde(rename = "operation")]
    _operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
    query: String,
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
        code_lsp_parameters_json_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor
            .execute(&ctx, parse_code_lsp_request(args_json)?)
            .await
    }
}

pub(crate) fn code_lsp_parameters_json_schema() -> Value {
    json!({
        "oneOf": [
            schema_for_operation_shape::<CodeLspPositionArgs>(
                LspOperation::supported_names_for(LspOperationInputKind::Position),
            ),
            schema_for_operation_shape::<CodeLspFileArgs>(
                LspOperation::supported_names_for(LspOperationInputKind::File),
            ),
            schema_for_operation_shape::<CodeLspQueryArgs>(
                LspOperation::supported_names_for(LspOperationInputKind::Query),
            ),
        ]
    })
}

pub(crate) fn parse_code_lsp_request(args_json: Value) -> Result<CodeLspRequest, ToolError> {
    let operation_name = serde_json::from_value::<CodeLspOperationProbe>(args_json.clone())
        .map_err(|err| ToolError::InvalidArguments(err.to_string()))?
        .operation;
    let operation = LspOperation::parse(&operation_name)?;

    match operation.input_kind() {
        LspOperationInputKind::Position => {
            let args: CodeLspPositionArgs = serde_json::from_value(args_json)
                .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
            Ok(CodeLspRequest::Position {
                operation,
                file_path: args.file_path,
                line: args.line,
                character: args.character,
            })
        }
        LspOperationInputKind::File => {
            let args: CodeLspFileArgs = serde_json::from_value(args_json)
                .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
            Ok(CodeLspRequest::File {
                operation,
                file_path: args.file_path,
            })
        }
        LspOperationInputKind::Query => {
            let args: CodeLspQueryArgs = serde_json::from_value(args_json)
                .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
            Ok(CodeLspRequest::Query {
                operation,
                file_path: args.file_path,
                query: args.query,
            })
        }
    }
}

fn schema_for_operation_shape<T: JsonSchema>(operations: &[&str]) -> Value {
    let mut schema = super::json_schema_for::<T>();
    if let Some(operation_schema) = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut("operation"))
        .and_then(Value::as_object_mut)
    {
        operation_schema.insert("enum".to_string(), json!(operations));
    }
    schema
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

fn build_result(
    operation_name: &str,
    file_path: &Path,
    extra_args: Value,
    response: LspOperationResponse,
) -> Result<ToolResult, ToolError> {
    let display_text = render_display_text(operation_name, &response)?;
    let mut structured_json = json!({
        "operation": operation_name,
        "filePath": file_path.display().to_string(),
        "server": {
            "name": response.server.name,
            "command": response.server.command,
        },
        "result": response.result,
        "diagnostics": response.diagnostics,
    });
    if let (Some(extra), Some(base)) = (extra_args.as_object(), structured_json.as_object_mut()) {
        base.extend(extra.clone());
    }

    Ok(ToolResult {
        display_text,
        structured_json: Some(structured_json),
        artifacts: Vec::new(),
    })
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

#[cfg(test)]
mod tests {
    use super::{code_lsp_parameters_json_schema, parse_code_lsp_request, CodeLspRequest};
    use harness_core::tool::ToolError;
    use serde_json::json;

    #[test]
    fn parse_code_lsp_request_accepts_operation_specific_shapes() {
        let position = parse_code_lsp_request(json!({
            "operation": "hover",
            "filePath": "src/lib.rs",
            "line": 2,
            "character": 4,
        }))
        .expect("position-based request should parse");
        assert!(matches!(
            position,
            CodeLspRequest::Position {
                file_path,
                line: 2,
                character: 4,
                ..
            } if file_path == "src/lib.rs"
        ));

        let file_only = parse_code_lsp_request(json!({
            "operation": "documentSymbol",
            "filePath": "src/lib.rs",
        }))
        .expect("file-only request should parse");
        assert!(matches!(
            file_only,
            CodeLspRequest::File { file_path, .. } if file_path == "src/lib.rs"
        ));

        let query = parse_code_lsp_request(json!({
            "operation": "workspaceSymbol",
            "filePath": "src/lib.rs",
            "query": "helper",
        }))
        .expect("query request should parse");
        assert!(matches!(
            query,
            CodeLspRequest::Query {
                file_path,
                query,
                ..
            } if file_path == "src/lib.rs" && query == "helper"
        ));
    }

    #[test]
    fn parse_code_lsp_request_rejects_mismatched_shapes() {
        let missing_position = parse_code_lsp_request(json!({
            "operation": "hover",
            "filePath": "src/lib.rs",
        }))
        .expect_err("position-based request should require coordinates");
        assert!(matches!(
            missing_position,
            ToolError::InvalidArguments(message) if message.contains("missing field `line`")
        ));

        let extra_position = parse_code_lsp_request(json!({
            "operation": "documentSymbol",
            "filePath": "src/lib.rs",
            "line": 1,
            "character": 1,
        }))
        .expect_err("file-only request should reject cursor coordinates");
        assert!(matches!(
            extra_position,
            ToolError::InvalidArguments(message) if message.contains("unknown field")
        ));

        let missing_query = parse_code_lsp_request(json!({
            "operation": "workspaceSymbol",
            "filePath": "src/lib.rs",
        }))
        .expect_err("query request should require a query");
        assert!(matches!(
            missing_query,
            ToolError::InvalidArguments(message) if message.contains("missing field `query`")
        ));
    }

    #[test]
    fn code_lsp_schema_exposes_per_operation_variants() {
        let schema = code_lsp_parameters_json_schema();
        let variants = schema["oneOf"].as_array().expect("oneOf variants");
        assert_eq!(variants.len(), 3);
        assert_eq!(
            variants[0]["properties"]["operation"]["enum"],
            json!([
                "goToDefinition",
                "findReferences",
                "hover",
                "goToImplementation",
                "prepareCallHierarchy",
                "incomingCalls",
                "outgoingCalls",
            ])
        );
        assert_eq!(variants[1]["required"], json!(["filePath", "operation"]));
        assert_eq!(
            variants[2]["required"],
            json!(["filePath", "operation", "query"])
        );
    }
}
