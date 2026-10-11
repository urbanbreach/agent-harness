use super::*;
#[cfg(unix)]
mod plan;
#[cfg(unix)]
mod text;

pub(super) struct RenameTool(pub Arc<LspTool>);
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenameArgs {
    #[serde(rename = "filePath", alias = "file_path")]
    file: PathBuf,
    line: u32,
    character: u32,
    #[serde(rename = "newName")]
    name: String,
    #[serde(default)]
    apply: bool,
    #[serde(rename = "serverId")]
    server: Option<String>,
}
fn parse(value: Value) -> Result<RenameArgs, ToolError> {
    let args: RenameArgs =
        serde_json::from_value(value).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
    if args.file.as_os_str().is_empty()
        || args.line == 0
        || args.character == 0
        || args.name.trim().is_empty()
        || args.name.len() > 1024
        || args.name.chars().any(char::is_control)
    {
        return Err(ToolError::InvalidArguments(
            "rename requires a file, 1-based position and nonempty newName of at most 1024 bytes"
                .into(),
        ));
    }
    Ok(args)
}
#[async_trait::async_trait]
impl Tool for RenameTool {
    fn id(&self) -> &str {
        "lsp.rename"
    }
    fn description(&self) -> &str {
        "Rename a symbol across files using its language server. Positions use 1-based UTF-16 columns. Preview by default; apply:true checks every affected path before writing."
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(RenameArgs).to_value()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(vec![parse(args.clone())?.file])
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let path = args
            .get("filePath")
            .or_else(|| args.get("file_path"))
            .and_then(Value::as_str)
            .unwrap_or("*");
        let mut permissions = vec![("read".into(), path.into()), ("lsp".into(), path.into())];
        if args["apply"] == true {
            permissions.push(("edit".into(), path.into()));
        }
        permissions
    }
    fn secret_values(&self) -> Vec<String> {
        self.0.secret_values()
    }
    async fn call(&self, context: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let args = parse(value)?;
        if self.0.config.disabled {
            return Err(failure("LSP is disabled"));
        }
        #[cfg(not(unix))]
        {
            let _ = (context, args);
            Err(failure(
                "LSP process control is unavailable on this platform",
            ))
        }
        #[cfg(unix)]
        {
            let primary = context.resolve_workspace_path(&args.file)?;
            let query_args = || Args {
                operation: Operation::Rename(args.name.clone()),
                file: args.file.clone(),
                line: Some(args.line),
                character: Some(args.character),
                query: None,
                server: args.server.clone(),
                decision: None,
            };
            let response = self.0.query(&context, query_args()).await?;
            let scan_context = context.clone();
            let scan_primary = primary.clone();
            let (mut response, sources) = tokio::task::spawn_blocking(move || {
                let sources = plan::snapshot(&scan_context, &scan_primary, &response)?;
                Ok::<_, ToolError>((response, sources))
            })
            .await
            .map_err(|_| failure("rename source reader stopped"))??;
            if sources.keys().any(|path| path != &primary) {
                response = self
                    .0
                    .query_until(
                        &context,
                        query_args(),
                        tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                        Some(&sources),
                    )
                    .await?;
            }
            // Join filesystem work even when cancelled; no edits may outlive their tool call.
            tokio::task::spawn_blocking(move || {
                let mut result =
                    plan::execute(&context, &primary, &response, &sources, args.apply)?;
                if let Some(metadata) = result.structured_json.as_mut() {
                    metadata["newName"] = args.name.into();
                    metadata["server"] = response.data["server"].clone();
                    metadata["prepareRename"] = response.data["result"]["prepareRename"].clone();
                }
                Ok(result)
            })
            .await
            .map_err(|_| failure("rename worker stopped"))?
        }
    }
}
