use std::path::Path;

use harness_core::tool::{ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lsp_support::{
    execute_lsp_operation, format_diagnostics, LspOperation, LspOperationInput,
    LspOperationInputKind, LspOperationRequest, LspOperationResponse, LspPosition,
};
use crate::workspace_paths::resolve_existing_path;

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
                    operation,
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
                build_result(operation, &file_path, json!({}), response)
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
                    operation,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspArgs {
    operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(default)]
    line: Option<i32>,
    #[serde(default)]
    character: Option<i32>,
    #[serde(default)]
    query: Option<String>,
}

pub(crate) fn code_lsp_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "operation": {
                "type": "string",
                "enum": supported_operation_names(),
            },
            "filePath": {
                "type": "string"
            },
            "line": {
                "type": "integer",
                "minimum": 1
            },
            "character": {
                "type": "integer",
                "minimum": 0
            },
            "query": {
                "type": "string"
            }
        },
        "required": ["operation", "filePath"],
        "additionalProperties": false
    })
}

fn supported_operation_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    names.extend_from_slice(LspOperation::supported_names_for(
        LspOperationInputKind::Position,
    ));
    names.extend_from_slice(LspOperation::supported_names_for(
        LspOperationInputKind::File,
    ));
    names.extend_from_slice(LspOperation::supported_names_for(
        LspOperationInputKind::Query,
    ));
    names
}

pub(crate) fn parse_code_lsp_request(args_json: Value) -> Result<CodeLspRequest, ToolError> {
    let args: CodeLspArgs = serde_json::from_value(args_json)
        .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
    let operation = LspOperation::parse(&args.operation)?;

    match operation.input_kind() {
        LspOperationInputKind::Position => {
            let line = args
                .line
                .ok_or_else(|| ToolError::InvalidArguments("missing field `line`".to_string()))?;
            let character = args.character.ok_or_else(|| {
                ToolError::InvalidArguments("missing field `character`".to_string())
            })?;
            Ok(CodeLspRequest::Position {
                operation,
                file_path: args.file_path,
                line,
                character,
            })
        }
        LspOperationInputKind::File => Ok(CodeLspRequest::File {
            operation,
            file_path: args.file_path,
        }),
        LspOperationInputKind::Query => {
            let query = args
                .query
                .ok_or_else(|| ToolError::InvalidArguments("missing field `query`".to_string()))?;
            Ok(CodeLspRequest::Query {
                operation,
                file_path: args.file_path,
                query,
            })
        }
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
    operation: LspOperation,
    response: &LspOperationResponse,
) -> Result<String, ToolError> {
    if matches!(
        operation,
        LspOperation::FileDiagnostics | LspOperation::WorkspaceDiagnostics
    ) {
        return render_diagnostics_only_display_text(response);
    }

    let operation_name = operation.as_str();
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
    operation: LspOperation,
    file_path: &Path,
    extra_args: Value,
    response: LspOperationResponse,
) -> Result<ToolResult, ToolError> {
    let operation_name = operation.as_str();
    let display_text = render_display_text(operation, &response)?;
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

fn render_diagnostics_only_display_text(
    response: &LspOperationResponse,
) -> Result<String, ToolError> {
    let target = response
        .result
        .get("filePath")
        .or_else(|| response.result.get("workspaceRoot"))
        .and_then(Value::as_str)
        .unwrap_or("requested target");
    let diagnostics = format_diagnostics(&response.diagnostics);
    if diagnostics.is_empty() {
        Ok(format!("No diagnostics found for {target}"))
    } else {
        Ok(format!("Diagnostics for {target}:\n{diagnostics}"))
    }
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

        let file_with_cursor_metadata = parse_code_lsp_request(json!({
            "operation": "fileDiagnostics",
            "filePath": "src/lib.rs",
            "line": 1,
            "character": 1,
        }))
        .expect("file diagnostics should ignore cursor metadata that the public schema allows");
        assert!(matches!(
            file_with_cursor_metadata,
            CodeLspRequest::File { file_path, .. } if file_path == "src/lib.rs"
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
    fn code_lsp_schema_is_cliproxy_compatible() {
        let schema = code_lsp_parameters_json_schema();
        assert_eq!(schema["type"], json!("object"));
        assert!(schema["properties"].is_object());
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["operation", "filePath"]));
        assert_eq!(
            schema["properties"]["operation"]["enum"],
            json!([
                "goToDefinition",
                "findReferences",
                "hover",
                "goToImplementation",
                "prepareCallHierarchy",
                "incomingCalls",
                "outgoingCalls",
                "documentSymbol",
                "fileDiagnostics",
                "workspaceDiagnostics",
                "workspaceSymbol",
            ])
        );
        for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
            assert!(
                schema.get(forbidden).is_none(),
                "unexpected top-level {forbidden}"
            );
        }
    }
}
