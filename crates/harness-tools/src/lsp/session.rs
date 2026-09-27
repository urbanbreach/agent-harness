use super::wire::{failure, Wire, LIMIT};
use harness_core::{
    config::LspServerConfig,
    process::{spawn_group, Group},
    tool::ToolError,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct Document {
    hash: blake3::Hash,
    version: i32,
    end: Value,
}
pub(super) struct Session {
    wire: Wire<ChildStdout, ChildStdin>,
    child: Child,
    group: Group,
    sequence: u64,
    pub capabilities: Value,
    documents: BTreeMap<String, Document>,
    pub diagnostics: BTreeMap<String, Value>,
    root: String,
    settings: Value,
    ready: Option<bool>,
    pub status: Option<Value>,
}
impl Session {
    pub fn start(config: &LspServerConfig, root: &Path) -> Result<Self, ToolError> {
        let command = config
            .command
            .as_ref()
            .filter(|c| !c.is_empty() && !c[0].is_empty())
            .ok_or_else(|| failure("LSP server has no command"))?;
        let mut command_line = Command::new(&command[0]);
        command_line
            .args(&command[1..])
            .envs(&config.env)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, group) = spawn_group(command_line)?;
        let read = child
            .stdout
            .take()
            .ok_or_else(|| failure("LSP stdout unavailable"))?;
        let write = child
            .stdin
            .take()
            .ok_or_else(|| failure("LSP stdin unavailable"))?;
        Ok(Self {
            wire: Wire::new(read, write),
            child,
            group,
            sequence: 0,
            capabilities: Value::Null,
            documents: BTreeMap::new(),
            diagnostics: BTreeMap::new(),
            root: uri(root)?,
            settings: config.initialization.clone().unwrap_or(Value::Null),
            ready: None,
            status: None,
        })
    }
    pub async fn initialize(&mut self) -> Result<(), ToolError> {
        if !self.capabilities.is_null() {
            return Ok(());
        }
        let result = self.request("initialize", json!({
            "processId":std::process::id(), "clientInfo":{"name":"harness"}, "rootUri":self.root,
            "workspaceFolders":[{"uri":self.root,"name":"workspace"}], "initializationOptions":self.settings,
            "capabilities":{"general":{"positionEncodings":["utf-16"]},
                "experimental":{"serverStatusNotification":true},
                "workspace":{"configuration":true,"workspaceFolders":true,"applyEdit":false,
                    "workspaceEdit":{"documentChanges":true,"resourceOperations":["create","rename","delete"],"failureHandling":"abort"}},
                "textDocument":{"synchronization":{"dynamicRegistration":false},
                    "diagnostic":{"dynamicRegistration":false,"relatedDocumentSupport":false},
                    "rename":{"prepareSupport":true,"honorsChangeAnnotations":false},
                    "hover":{"contentFormat":["plaintext","markdown"]}}}
        })).await?;
        let capabilities = result
            .get("capabilities")
            .filter(|v| v.is_object())
            .ok_or_else(|| failure("LSP server returned no capabilities"))?;
        if capabilities
            .get("positionEncoding")
            .and_then(Value::as_str)
            .is_some_and(|value| value != "utf-16")
        {
            return Err(failure("LSP server did not negotiate UTF-16 positions"));
        }
        self.capabilities = capabilities.clone();
        if result["serverInfo"]["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("rust-analyzer"))
        {
            self.ready = Some(false);
        }
        self.notify("initialized", json!({})).await?;
        while self.ready == Some(false) {
            let message = self.wire.read().await?;
            self.handle(message).await?;
        }
        Ok(())
    }
    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        self.wire
            .send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        self.sequence += 1;
        let id = self.sequence;
        self.wire
            .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        for _ in 0..4096 {
            let message = self.wire.read().await?;
            if message.get("method").is_none() && message.get("id") == Some(&json!(id)) {
                if let Some(error) = message.get("error") {
                    return Err(failure(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("LSP request failed"),
                    ));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| failure("LSP response has no result"));
            }
            self.handle(message).await?;
        }
        Err(failure("too many LSP messages without a response"))
    }
    async fn handle(&mut self, message: Value) -> Result<(), ToolError> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if let Some(id) = message.get("id") {
            let result = match method {
                "workspace/applyEdit" => json!({"applied":false,"failureReason":"use an approved harness editing tool"}),
                "workspace/workspaceFolders" => json!([{"uri":self.root,"name":"workspace"}]),
                "workspace/configuration" => {
                    let items = message["params"]["items"].as_array().filter(|items| items.len() <= 128).ok_or_else(|| failure("invalid LSP configuration request"))?;
                    Value::Array(items.iter().map(|item| setting(&self.settings, item)).collect())
                },
                "window/workDoneProgress/create" | "window/showMessageRequest" | "workspace/diagnostic/refresh" => Value::Null,
                _ => return self.wire.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"client method is unavailable"}})).await,
            };
            self.wire
                .send(json!({"jsonrpc":"2.0","id":id,"result":result}))
                .await?;
        } else if method == "experimental/serverStatus" {
            self.ready = message["params"]["quiescent"].as_bool();
            self.status = Some(message["params"].clone());
            if message["params"]["health"] == "error" {
                return Err(failure("language server failed to load the workspace"));
            }
        } else if method == "textDocument/publishDiagnostics" {
            let params = &message["params"];
            if let Some(uri) = params["uri"]
                .as_str()
                .filter(|uri| self.documents.contains_key(*uri))
            {
                if params
                    .get("version")
                    .and_then(Value::as_i64)
                    .is_some_and(|version| {
                        self.documents
                            .get(uri)
                            .is_some_and(|d| i64::from(d.version) != version)
                    })
                {
                    return Ok(());
                }
                let size = self
                    .diagnostics
                    .iter()
                    .filter(|(key, _)| *key != uri)
                    .map(|(_, v)| v.to_string().len())
                    .sum::<usize>()
                    + params["diagnostics"].to_string().len();
                if size > LIMIT {
                    return Err(failure("LSP diagnostics exceed 4 MiB"));
                }
                if !params["diagnostics"].is_array() {
                    return Err(failure("invalid LSP diagnostics"));
                }
                self.diagnostics
                    .insert(uri.into(), params["diagnostics"].clone());
            }
        }
        Ok(())
    }
    pub async fn document(
        &mut self,
        uri: &str,
        language: &str,
        text: &str,
    ) -> Result<(), ToolError> {
        let hash = blake3::hash(text.as_bytes());
        let previous = self.documents.get(uri);
        if previous.is_some_and(|d| d.hash == hash) {
            return Ok(());
        }
        let version = previous
            .map_or(Some(1), |d| d.version.checked_add(1))
            .ok_or_else(|| failure("LSP document version exhausted"))?;
        let kind = self.capabilities["textDocumentSync"]
            .as_u64()
            .or_else(|| self.capabilities["textDocumentSync"]["change"].as_u64())
            .unwrap_or(0);
        if let Some(previous) = previous {
            if kind == 0 {
                return Err(failure("LSP server cannot synchronize changed documents"));
            }
            let mut change = json!({"text":text});
            if kind == 2 {
                change["range"] = json!({"start":{"line":0,"character":0},"end":previous.end});
            }
            self.notify(
                "textDocument/didChange",
                json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[change]}),
            )
            .await?;
        } else {
            if self.documents.len() >= 64 {
                if let Some((uri, _)) = self.documents.pop_first() {
                    self.diagnostics.remove(&uri);
                    self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
                        .await?;
                }
            }
            self.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":language,"version":version,"text":text}})).await?;
        }
        self.diagnostics.remove(uri);
        let (mut line, mut character) = (0, 0);
        for ch in text.chars() {
            if ch == '\n' {
                line += 1;
                character = 0;
            } else {
                character += ch.len_utf16();
            }
        }
        self.documents.insert(
            uri.into(),
            Document {
                hash,
                version,
                end: json!({"line":line,"character":character}),
            },
        );
        Ok(())
    }
    pub async fn published_diagnostics(&mut self, uri: &str) -> Result<Value, ToolError> {
        for _ in 0..4096 {
            if let Some(diagnostics) = self.diagnostics.get(uri) {
                return Ok(diagnostics.clone());
            }
            let message = self.wire.read().await?;
            self.handle(message).await?;
        }
        Err(failure("LSP server did not publish diagnostics"))
    }
    pub fn versions(&self) -> BTreeMap<String, i32> {
        self.documents
            .iter()
            .map(|(uri, doc)| (uri.clone(), doc.version))
            .collect()
    }
    pub async fn close(&mut self) -> Result<(), ToolError> {
        let _ = tokio::time::timeout(Duration::from_millis(200), async {
            self.request("shutdown", Value::Null).await?;
            self.notify("exit", Value::Null).await
        })
        .await;
        self.group.terminate(&mut self.child).await?;
        Ok(())
    }
}
pub(super) fn uri(path: &Path) -> Result<String, ToolError> {
    reqwest::Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| failure("path cannot be represented as a file URI"))
}
fn setting(settings: &Value, item: &Value) -> Value {
    match item.get("section").and_then(Value::as_str) {
        Some(section) => section
            .split('.')
            .try_fold(settings, |v, key| v.get(key))
            .cloned()
            .unwrap_or(Value::Null),
        None => settings.clone(),
    }
}
