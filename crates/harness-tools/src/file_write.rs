use async_trait::async_trait;
use harness_core::config::registered_formatter_config;
use harness_core::coord::run_formatter_for_path;
use harness_core::edit::hashline::HashlineWorkspaceOp;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fs_walk::workspace_relative_display;
use crate::hashline_apply::{
    apply_hashline_workspace_op_to_workspace, resolve_workspace_target_path,
};
use crate::lsp_support::{
    execute_lsp_operation, format_diagnostics, LspDiagnosticReport, LspOperation,
    LspOperationInput, LspOperationRequest, LspOperationResponse,
};
use crate::workspace_paths::canonical_workspace_root;

const MAX_PROJECT_DIAGNOSTICS_FILES: usize = 5;

pub(crate) struct WriteTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    #[serde(rename = "path", alias = "filePath")]
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteTool {
    fn id(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to one file. Relative paths resolve within the workspace; absolute paths must stay inside the workspace."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to write. Relative paths resolve within the workspace; absolute paths must stay inside the workspace."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            }
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WriteArgs = crate::parse_tool_args(args_json)?;
        let resolved_path = resolve_workspace_target_path(&ctx, &args.path)?;
        let existed = resolved_path.exists();
        let mut result = apply_hashline_workspace_op_to_workspace(
            &ctx,
            HashlineWorkspaceOp::RewriteFile {
                edit_id: format!("write-{}", ctx.tool_call_id),
                path: args.path,
                content: args.content,
            },
        )?;
        let workspace = canonical_workspace_root(&ctx)?;
        let resource = workspace_relative_display(&workspace, &resolved_path)?;
        let format_warning = run_file_formatter(&workspace, &resolved_path).await;
        let diagnostics = run_lsp_diagnostics(workspace.clone(), resolved_path.clone()).await;
        result.display_text = format!(
            "{} file successfully: {resource}",
            if existed { "Wrote" } else { "Created" }
        );
        if let Some(warning) = &format_warning {
            result.display_text.push_str("\n\nFormatter warning:\n");
            result.display_text.push_str(warning);
        }
        append_lsp_diagnostics(&mut result.display_text, &diagnostics, &resolved_path);
        if let Some(structured) = result.structured_json.as_mut() {
            structured["operation"] = json!("write");
            structured["target"] = json!(resolved_path.display().to_string());
            structured["resource"] = json!(resource);
            structured["existed"] = json!(existed);
            structured["format_warning"] = match format_warning {
                Some(warning) => json!(warning),
                None => Value::Null,
            };
            structured["diagnostics"] = diagnostics_json(&diagnostics);
        }
        Ok(result)
    }
}

pub(crate) async fn run_file_formatter(
    workspace: &std::path::Path,
    path: &std::path::Path,
) -> Option<String> {
    let config = registered_formatter_config().unwrap_or_default();
    match run_formatter_for_path(&config, workspace, &path.display().to_string()).await {
        Ok(()) => None,
        Err(warning) => Some(warning),
    }
}

pub(crate) async fn run_lsp_diagnostics(
    workspace_root: std::path::PathBuf,
    file_path: std::path::PathBuf,
) -> Result<LspOperationResponse, String> {
    tokio::task::spawn_blocking(move || {
        execute_lsp_operation(&LspOperationRequest {
            operation: LspOperation::WorkspaceDiagnostics,
            input: LspOperationInput::File {
                file_path: &file_path,
            },
            workspace_root: &workspace_root,
        })
    })
    .await
    .map_err(|err| format!("lsp task failed: {err}"))?
    .map_err(|err| err.to_string())
}

pub(crate) fn append_lsp_diagnostics(
    display_text: &mut String,
    diagnostics: &Result<LspOperationResponse, String>,
    file_path: &std::path::Path,
) {
    let Ok(response) = diagnostics else {
        return;
    };

    let mut project_reports = 0usize;
    for report in &response.diagnostics {
        let block = format_diagnostics(std::slice::from_ref(report));
        if block.is_empty() {
            continue;
        }
        if is_current_file_report(report, file_path) {
            display_text.push_str("\n\nLSP errors detected in this file, please fix:\n");
            display_text.push_str(&block);
            continue;
        }
        if project_reports >= MAX_PROJECT_DIAGNOSTICS_FILES {
            continue;
        }
        project_reports += 1;
        display_text.push_str("\n\nLSP errors detected in other files:\n");
        display_text.push_str(&block);
    }
}

fn is_current_file_report(report: &LspDiagnosticReport, file_path: &std::path::Path) -> bool {
    std::path::Path::new(&report.file_path) == file_path
}

pub(crate) fn diagnostics_json(diagnostics: &Result<LspOperationResponse, String>) -> Value {
    match diagnostics {
        Ok(response) => json!(response.diagnostics),
        Err(error) => json!({
            "unavailable": true,
            "error": error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{append_lsp_diagnostics, LspDiagnosticReport, LspOperationResponse};
    use crate::lsp_support::LspServerMetadata;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn append_lsp_diagnostics_reports_current_and_limited_project_files() {
        // arrange
        let current = "/workspace/src/main.rs";
        let mut display = "Wrote file successfully: src/main.rs".to_string();
        let diagnostics = Ok(LspOperationResponse {
            server: LspServerMetadata {
                name: "rust".to_string(),
                command: vec!["rust-analyzer".to_string()],
            },
            result: json!({}),
            diagnostics: diagnostic_reports(current),
        });

        // act
        append_lsp_diagnostics(&mut display, &diagnostics, Path::new(current));

        // assert
        assert!(display.contains("LSP errors detected in this file, please fix:"));
        assert_eq!(
            display
                .matches("LSP errors detected in other files:")
                .count(),
            5
        );
        assert!(!display.contains("src/other5.rs:1:1"));
    }

    fn diagnostic_reports(current: &str) -> Vec<LspDiagnosticReport> {
        let mut reports = vec![report(current, "current")];
        reports.extend(
            (0..6).map(|index| report(&format!("/workspace/src/other{index}.rs"), "other")),
        );
        reports
    }

    fn report(file_path: &str, message: &str) -> LspDiagnosticReport {
        LspDiagnosticReport {
            file_path: file_path.to_string(),
            diagnostics: vec![json!({
                "range": { "start": { "line": 0, "character": 0 } },
                "severity": 1,
                "message": message,
            })],
        }
    }
}
