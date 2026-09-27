#[cfg(unix)]
mod after_edit;
#[cfg(unix)]
mod diagnostics;
#[cfg(unix)]
mod query;
mod rename;
#[cfg(unix)]
mod session;
mod wire;
use harness_core::{
    config::{LspConfig, LspServerConfig},
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use wire::failure;

pub fn register_lsp_tools(registry: &mut ToolRegistry, config: LspConfig) {
    let tool = Arc::new(LspTool {
        config,
        #[cfg(unix)]
        sessions: Mutex::new(BTreeMap::new()),
    });
    #[cfg(unix)]
    after_edit::register(registry, &tool);
    #[cfg(not(unix))]
    registry.register(Arc::new(rename::RenameTool(Arc::clone(&tool))));
    registry.register(tool);
}
struct LspTool {
    config: LspConfig,
    // ponytail: serialize LSP calls within a registry; split locks per server if measured contention warrants it.
    #[cfg(unix)]
    sessions: Mutex<BTreeMap<(String, String), session::Session>>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum Operation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    FileDiagnostics,
    WorkspaceDiagnostics,
    InstallDecision,
    #[serde(skip)]
    Rename(String),
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Allowed,
    Declined,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    operation: Operation,
    #[serde(default, rename = "filePath", alias = "file_path")]
    file: PathBuf,
    line: Option<u32>,
    character: Option<u32>,
    query: Option<String>,
    #[serde(rename = "serverId", alias = "server_id")]
    server: Option<String>,
    decision: Option<Decision>,
}
fn parse(args: Value) -> Result<Args, ToolError> {
    let args: Args =
        serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
    let valid = if matches!(args.operation, Operation::InstallDecision) {
        args.server.as_ref().is_some_and(|name| {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        }) && args.decision.is_some()
    } else {
        !args.file.as_os_str().is_empty()
    };
    if !valid {
        return Err(ToolError::InvalidArguments(
            "provide filePath for queries, or serverId and decision for installDecision".into(),
        ));
    }
    Ok(args)
}
#[async_trait::async_trait]
impl Tool for LspTool {
    fn id(&self) -> &str {
        "lsp"
    }
    fn description(&self) -> &str {
        "Query a language server using filePath for definitions, references, hover, symbols, call hierarchy or diagnostics. Positions are 1-based UTF-16 columns. installDecision records serverId and decision (allowed/declined); it does not install software."
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        let args = parse(args.clone())?;
        Ok(if matches!(args.operation, Operation::InstallDecision) {
            Vec::new()
        } else {
            vec![args.file]
        })
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        if args["operation"] == "installDecision" {
            return vec![(
                "lsp".into(),
                args["serverId"]
                    .as_str()
                    .or_else(|| args["server_id"].as_str())
                    .unwrap_or("*")
                    .into(),
            )];
        }
        let path = args
            .get("filePath")
            .or_else(|| args.get("file_path"))
            .and_then(Value::as_str)
            .unwrap_or("*");
        vec![("lsp".into(), path.into()), ("read".into(), path.into())]
    }
    fn secret_values(&self) -> Vec<String> {
        self.config
            .servers
            .values()
            .flat_map(|s| s.env.values().cloned())
            .collect()
    }
    async fn call(&self, context: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let args = parse(args)?;
        if matches!(args.operation, Operation::InstallDecision) {
            let server = args
                .server
                .ok_or_else(|| ToolError::InvalidArguments("serverId is required".into()))?;
            let allowed = matches!(args.decision, Some(Decision::Allowed));
            let receipt = context
                .coordinator
                .record_lsp_install_decision(
                    context.tool_call_id.to_string(),
                    server.clone(),
                    allowed,
                )
                .await
                .map_err(|e| failure(&e.to_string()))?;
            return Ok(ToolResult::structured_with_artifacts(
                format!(
                    "Recorded {} installation choice for {server}; no software was installed.",
                    if allowed { "allowed" } else { "declined" }
                ),
                json!({"operation":"installDecision","serverId":server,"decision":if allowed {"allowed"} else {"declined"},"recorded_only":true,"artifactPath":receipt.path}),
                vec![receipt],
            ));
        }
        if self.config.disabled {
            return Err(failure("LSP is disabled"));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        #[cfg(not(unix))]
        {
            let _ = args;
            Err(failure(
                "LSP process control is unavailable on this platform",
            ))
        }
        #[cfg(unix)]
        {
            let response = self.query(&context, args).await?;
            Ok(ToolResult::structured(
                serde_json::to_string_pretty(&response.data["result"])
                    .map_err(|_| failure("invalid LSP result"))?,
                response.data,
            ))
        }
    }
    async fn close_run(&self, run_id: &str) -> Result<(), ToolError> {
        #[cfg(unix)]
        {
            let mut sessions = self.sessions.lock().await;
            let keys: Vec<_> = sessions
                .keys()
                .filter(|(run, _)| run == run_id)
                .cloned()
                .collect();
            let mut result = Ok(());
            for key in keys {
                if let Some(mut session) = sessions.remove(&key) {
                    if let Err(error) = session.close().await {
                        result = Err(error);
                    }
                }
            }
            return result;
        }
        #[cfg(not(unix))]
        {
            let _ = run_id;
            Ok(())
        }
    }
}
fn supports(capabilities: &Value, name: &str) -> bool {
    capabilities
        .get(name)
        .is_some_and(|v| v == true || v.is_object())
}
fn request(
    args: &Args,
    uri: &str,
    text: &str,
) -> Result<(&'static str, &'static str, Value), ToolError> {
    use Operation::{
        DocumentSymbol, FileDiagnostics, FindReferences, GoToDefinition, GoToImplementation, Hover,
        IncomingCalls, OutgoingCalls, PrepareCallHierarchy, WorkspaceDiagnostics, WorkspaceSymbol,
    };
    let (method, capability) = match args.operation {
        GoToDefinition => ("textDocument/definition", "definitionProvider"),
        FindReferences => ("textDocument/references", "referencesProvider"),
        Hover => ("textDocument/hover", "hoverProvider"),
        GoToImplementation => ("textDocument/implementation", "implementationProvider"),
        PrepareCallHierarchy | IncomingCalls | OutgoingCalls => {
            ("textDocument/prepareCallHierarchy", "callHierarchyProvider")
        }
        DocumentSymbol => ("textDocument/documentSymbol", "documentSymbolProvider"),
        WorkspaceSymbol => ("workspace/symbol", "workspaceSymbolProvider"),
        FileDiagnostics => ("textDocument/diagnostic", "diagnosticProvider"),
        WorkspaceDiagnostics => ("workspace/diagnostic", "diagnosticProvider"),
        Operation::Rename(_) => ("textDocument/rename", "renameProvider"),
        Operation::InstallDecision => {
            return Err(ToolError::InvalidArguments(
                "installation choices do not issue LSP requests".into(),
            ))
        }
    };
    let mut params = json!({"textDocument":{"uri":uri}});
    match args.operation {
        WorkspaceSymbol => {
            let query = args
                .query
                .as_ref()
                .filter(|q| !q.trim().is_empty() && q.len() <= 4096)
                .ok_or_else(|| {
                    ToolError::InvalidArguments("query must contain 1–4096 bytes".into())
                })?;
            params = json!({"query":query});
        }
        WorkspaceDiagnostics => params = json!({"previousResultIds":[]}),
        DocumentSymbol | FileDiagnostics => {}
        _ => {
            let line = args
                .line
                .and_then(|n| n.checked_sub(1))
                .ok_or_else(|| ToolError::InvalidArguments("line must be at least 1".into()))?;
            let character = args
                .character
                .and_then(|n| n.checked_sub(1))
                .ok_or_else(|| {
                    ToolError::InvalidArguments("character must be at least 1".into())
                })?;
            let content = text
                .split('\n')
                .nth(line as usize)
                .ok_or_else(|| ToolError::InvalidArguments("line is outside the file".into()))?
                .trim_end_matches('\r');
            let mut units = 0;
            for ch in content.chars() {
                if units >= character as usize {
                    break;
                }
                units += ch.len_utf16();
            }
            if units != character as usize {
                return Err(ToolError::InvalidArguments(
                    "character is outside the line or splits a UTF-16 surrogate pair".into(),
                ));
            }
            params["position"] = json!({"line":line,"character":character});
            if matches!(args.operation, FindReferences) {
                params["context"] = json!({"includeDeclaration":true});
            }
        }
    }
    if let Operation::Rename(name) = &args.operation {
        params["newName"] = name.clone().into();
    }
    Ok((method, capability, params))
}
fn server(config: &LspConfig, args: &Args) -> Result<(String, LspServerConfig, String), ToolError> {
    let extension = args.file.extension().and_then(|s| s.to_str()).unwrap_or("");
    let defaults = [
        ("rust", vec!["rust-analyzer"], vec!["rs"]),
        (
            "typescript",
            vec!["typescript-language-server", "--stdio"],
            vec!["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        ),
        (
            "python",
            vec!["pyright-langserver", "--stdio"],
            vec!["py", "pyi"],
        ),
        ("go", vec!["gopls"], vec!["go"]),
    ];
    let mut servers = config.servers.clone();
    for (name, command, extensions) in defaults {
        let server = servers.entry(name.into()).or_default();
        server
            .command
            .get_or_insert_with(|| command.into_iter().map(str::to_owned).collect());
        server
            .extensions
            .get_or_insert_with(|| extensions.into_iter().map(str::to_owned).collect());
    }
    let mut servers: Vec<_> = servers.into_iter().collect();
    servers.sort_by_key(|(name, _)| !config.servers.contains_key(name));
    servers
        .into_iter()
        .find(|(name, server)| {
            args.server
                .as_ref()
                .is_none_or(|requested| requested == name)
                && !server.disabled
                && server.extensions.as_ref().is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.trim_start_matches('.') == extension)
                })
        })
        .map(|(name, server)| (name, server, language(&args.file)))
        .ok_or_else(|| failure("no enabled LSP server matches this file"))
}
fn language(path: &std::path::Path) -> String {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "py" | "pyi" => "python",
        "go" => "go",
        other => other,
    }
    .into()
}
