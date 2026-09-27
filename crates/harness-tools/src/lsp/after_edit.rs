use super::*;
use std::{collections::BTreeSet, time::Duration};
use tokio::time::Instant;

struct CheckedEdit {
    tool: Arc<dyn Tool>,
    lsp: Arc<LspTool>,
}
pub(super) fn register(registry: &mut ToolRegistry, lsp: &Arc<LspTool>) {
    // Replacing a configuration replaces these wrappers, rather than nesting the old ones.
    let edits: [Arc<dyn Tool>; 5] = [
        Arc::new(crate::files::FileTool::Write),
        Arc::new(crate::files::FileTool::Edit),
        Arc::new(crate::patch::PatchTool),
        Arc::new(crate::ast_grep::AstTool::Replace),
        Arc::new(rename::RenameTool(Arc::clone(lsp))),
    ];
    for tool in edits {
        registry.register(Arc::new(CheckedEdit {
            tool,
            lsp: Arc::clone(lsp),
        }));
    }
}
#[async_trait::async_trait]
impl Tool for CheckedEdit {
    fn id(&self) -> &str {
        self.tool.id()
    }
    fn description(&self) -> &str {
        self.tool.description()
    }
    fn parameters_json_schema(&self) -> Value {
        self.tool.parameters_json_schema()
    }
    fn capability(&self) -> ToolCapability {
        self.tool.capability()
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        self.tool.filesystem_paths(args)
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        self.tool.permission_requests(args)
    }
    fn secret_values(&self) -> Vec<String> {
        self.tool.secret_values()
    }
    async fn call(&self, context: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mut result = self.tool.call(context.clone(), args).await?;
        if self.lsp.config.disabled {
            return Ok(result);
        }
        let Some(metadata) = &mut result.structured_json else {
            return Ok(result);
        };
        if metadata["applied"] == false {
            return Ok(result);
        }
        let mut paths = BTreeSet::new();
        for value in
            std::iter::once(&*metadata).chain(metadata["files"].as_array().into_iter().flatten())
        {
            if value["operation"] == "delete" || value["result"]["operation"] == "delete" {
                continue;
            }
            if let Some(path) = value["path"].as_str() {
                paths.insert(PathBuf::from(path));
            }
        }
        for path in metadata["completed_files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            paths.insert(PathBuf::from(path));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut reports = serde_json::Map::new();
        let mut bytes = 0;
        for (index, path) in paths.into_iter().enumerate() {
            let report = if index >= 16 {
                Err(failure("post-edit diagnostics are limited to 16 files"))
            } else {
                self.diagnose(&context, path.clone(), deadline).await
            };
            let report = match report {
                Ok(value) if bytes + value.to_string().len() <= 1024 * 1024 => value,
                Ok(_) => json!({"unavailable":true,"reason":"combined diagnostics exceed 1 MiB"}),
                Err(error) => json!({"unavailable":true,"reason":error.to_string()}),
            };
            let serialized = report.to_string();
            bytes += serialized.len();
            let text = context.redactor.redact_text(&serialized);
            let end = text.floor_char_boundary(text.len().min(4096));
            result.display_text.push_str(&format!(
                "\nLSP diagnostics for {}: {}{}",
                path.display(),
                &text[..end],
                if end < text.len() { "…" } else { "" }
            ));
            reports.insert(path.to_string_lossy().into_owned(), report);
        }
        if !reports.is_empty() {
            metadata["diagnostics"] = Value::Object(reports);
        }
        Ok(result)
    }
}
impl CheckedEdit {
    async fn diagnose(
        &self,
        ctx: &ToolContext,
        path: PathBuf,
        deadline: Instant,
    ) -> Result<Value, ToolError> {
        if self.lsp.config.disabled {
            return Err(failure("LSP is disabled"));
        }
        let allowed = ctx
            .coordinator
            .allowed_paths(
                ctx.tool_call_id.to_string(),
                vec![path.clone()],
                &["read", "lsp"],
            )
            .await
            .map_err(|e| failure(&e.to_string()))?;
        if allowed.is_empty() {
            return Err(failure(
                "read or LSP policy requires separate authorization",
            ));
        }
        let response = self
            .lsp
            .query_until(
                ctx,
                Args {
                    operation: Operation::FileDiagnostics,
                    file: path,
                    line: None,
                    character: None,
                    query: None,
                    server: None,
                    decision: None,
                },
                deadline,
            )
            .await?;
        Ok(response.data["result"].clone())
    }
}
