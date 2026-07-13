// allow: SIZE_OK — LSP tool wrapper (diagnostics + symbols + rename)
use crate::UnwrapOrAbort;
use std::path::{Path, PathBuf};

use harness_core::tool::{ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
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
        let (operation, file_path, input, extra_args) = match request {
            CodeLspRequest::Position {
                operation,
                file_path,
                line,
                character,
            } => {
                let position = LspPosition::from_one_based(line, character)?;
                let file_path = resolve_existing_path(ctx, &file_path)?;
                (
                    operation,
                    file_path.clone(),
                    OwnedLspOperationInput::Position {
                        file_path,
                        position,
                    },
                    json!({
                        "line": line,
                        "character": character,
                    }),
                )
            }
            CodeLspRequest::File {
                operation,
                file_path,
            } => {
                let file_path = resolve_existing_path(ctx, &file_path)?;
                (
                    operation,
                    file_path.clone(),
                    OwnedLspOperationInput::File { file_path },
                    json!({}),
                )
            }
            CodeLspRequest::Query {
                operation,
                file_path,
                query,
            } => {
                let file_path = resolve_existing_path(ctx, &file_path)?;
                (
                    operation,
                    file_path.clone(),
                    OwnedLspOperationInput::Query {
                        file_path,
                        query: query.clone(),
                    },
                    json!({
                        "query": query,
                    }),
                )
            }
            CodeLspRequest::InstallDecision { .. } => {
                return Err(ToolError::Execution(
                    "installDecision is handled before LSP dispatch and should not reach execute"
                        .to_string(),
                ));
            }
        };

        let response = run_lsp_operation(ctx.workspace_root.clone(), operation, input).await?;
        build_result(operation, &file_path, extra_args, response)
    }
}

enum OwnedLspOperationInput {
    Position {
        file_path: PathBuf,
        position: LspPosition,
    },
    File {
        file_path: PathBuf,
    },
    Query {
        file_path: PathBuf,
        query: String,
    },
}

async fn run_lsp_operation(
    workspace_root: PathBuf,
    operation: LspOperation,
    input: OwnedLspOperationInput,
) -> Result<LspOperationResponse, ToolError> {
    tokio::task::spawn_blocking(move || match input {
        OwnedLspOperationInput::Position {
            file_path,
            position,
        } => execute_lsp_operation(&LspOperationRequest {
            operation,
            input: LspOperationInput::Position {
                file_path: &file_path,
                position,
            },
            workspace_root: &workspace_root,
        }),
        OwnedLspOperationInput::File { file_path } => execute_lsp_operation(&LspOperationRequest {
            operation,
            input: LspOperationInput::File {
                file_path: &file_path,
            },
            workspace_root: &workspace_root,
        }),
        OwnedLspOperationInput::Query { file_path, query } => {
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
    .tool_err("lsp task failed")?
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
    InstallDecision {
        server_id: String,
        decision: String,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeLspArgs {
    operation: String,
    #[serde(default, rename = "filePath")]
    file_path: Option<String>,
    #[serde(default)]
    line: Option<i32>,
    #[serde(default)]
    character: Option<i32>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default, rename = "serverId")]
    server_id: Option<String>,
    #[serde(default)]
    decision: Option<String>,
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
            },
            "serverId": {
                "type": "string"
            },
            "decision": {
                "type": "string",
                "enum": ["allowed", "declined"]
            }
        },
        "required": ["operation"],
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
    names.extend_from_slice(LspOperation::supported_names_for(
        LspOperationInputKind::None,
    ));
    names.push("installDecision");
    names
}

pub(crate) fn parse_code_lsp_request(args_json: Value) -> Result<CodeLspRequest, ToolError> {
    let args: CodeLspArgs = crate::parse_tool_args(args_json)?;
    let operation = LspOperation::parse(&args.operation)?;

    match operation.input_kind() {
        LspOperationInputKind::Position => {
            let file_path = required_lsp_field(args.file_path, "filePath")?;
            let line = required_lsp_field(args.line, "line")?;
            let character = required_lsp_field(args.character, "character")?;
            Ok(CodeLspRequest::Position {
                operation,
                file_path,
                line,
                character,
            })
        }
        LspOperationInputKind::File => {
            let file_path = required_lsp_field(args.file_path, "filePath")?;
            Ok(CodeLspRequest::File {
                operation,
                file_path,
            })
        }
        LspOperationInputKind::Query => {
            let file_path = required_lsp_field(args.file_path, "filePath")?;
            let query = required_lsp_field(args.query, "query")?;
            Ok(CodeLspRequest::Query {
                operation,
                file_path,
                query,
            })
        }
        LspOperationInputKind::None => {
            let server_id = required_lsp_field(args.server_id, "serverId")?;
            let decision = required_lsp_field(args.decision, "decision")?;
            if decision != "allowed" && decision != "declined" {
                return Err(ToolError::InvalidArguments(format!(
                    "decision must be 'allowed' or 'declined', got '{decision}'"
                )));
            }
            Ok(CodeLspRequest::InstallDecision {
                server_id,
                decision,
            })
        }
    }
}

fn required_lsp_field<T>(value: Option<T>, field: &str) -> Result<T, ToolError> {
    value.ok_or_else(|| ToolError::InvalidArguments(format!("missing field `{field}`")))
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
        serde_json::to_string_pretty(&response.result).tool_err("failed to render lsp result")?
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

    Ok(crate::text_json_tool_result(display_text, structured_json))
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
    use crate::UnwrapOrAbort;
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
        .unwrap_or_abort();
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
        .unwrap_or_abort();
        assert!(matches!(
            file_only,
            CodeLspRequest::File { file_path, .. } if file_path == "src/lib.rs"
        ));

        let query = parse_code_lsp_request(json!({
            "operation": "workspaceSymbol",
            "filePath": "src/lib.rs",
            "query": "helper",
        }))
        .unwrap_or_abort();
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
        .unwrap_or_abort();
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
    fn parse_code_lsp_request_accepts_install_decision_without_file_path() {
        let request = parse_code_lsp_request(json!({
            "operation": "installDecision",
            "serverId": "rust",
            "decision": "allowed",
        }))
        .unwrap_or_abort();
        assert!(matches!(
            request,
            CodeLspRequest::InstallDecision {
                server_id,
                decision,
            } if server_id == "rust" && decision == "allowed"
        ));
    }

    #[test]
    fn parse_code_lsp_request_rejects_install_decision_missing_fields() {
        let missing_server_id = parse_code_lsp_request(json!({
            "operation": "installDecision",
            "decision": "allowed",
        }))
        .expect_err("installDecision should require serverId");
        assert!(matches!(
            missing_server_id,
            ToolError::InvalidArguments(message) if message.contains("missing field `serverId`")
        ));

        let missing_decision = parse_code_lsp_request(json!({
            "operation": "installDecision",
            "serverId": "rust",
        }))
        .expect_err("installDecision should require decision");
        assert!(matches!(
            missing_decision,
            ToolError::InvalidArguments(message) if message.contains("missing field `decision`")
        ));
    }

    #[test]
    fn parse_code_lsp_request_rejects_install_decision_invalid_decision_value() {
        let invalid = parse_code_lsp_request(json!({
            "operation": "installDecision",
            "serverId": "rust",
            "decision": "maybe",
        }))
        .expect_err("installDecision should reject invalid decision value");
        assert!(matches!(
            invalid,
            ToolError::InvalidArguments(message) if message.contains("decision must be 'allowed' or 'declined'")
        ));
    }

    #[test]
    fn code_lsp_schema_is_cliproxy_compatible() {
        let schema = code_lsp_parameters_json_schema();
        assert_eq!(schema["type"], json!("object"));
        assert!(schema["properties"].is_object());
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["operation"]));
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
                "installDecision",
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
