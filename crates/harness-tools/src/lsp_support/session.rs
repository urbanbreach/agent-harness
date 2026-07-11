// allow: SIZE_OK — LSP support (client connection + message handling)
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use harness_core::tool::ToolError;
use harness_core::ToolResultExt;
use serde_json::{json, Value};

use crate::workspace_paths::{file_path_from_uri, file_uri_from_path};

use super::{LspDiagnosticReport, LspServerSpec};

const DEFAULT_LSP_BOOT_DELAY_MS: u64 = 150;
const DEFAULT_LSP_RETRY_ATTEMPTS: usize = 8;

pub(super) struct LspSession {
    child: Box<dyn LspChild>,
    stdin: Box<dyn Write + Send>,
    stdout: BufReader<Box<dyn Read + Send>>,
    pub(super) next_id: u64,
    root: PathBuf,
    diagnostics: BTreeMap<String, Vec<Value>>,
}

pub(super) struct LspProcess {
    pub(super) child: Box<dyn LspChild>,
    pub(super) stdin: Box<dyn Write + Send>,
    pub(super) stdout: Box<dyn Read + Send>,
}

pub(super) trait LspChild: Send {
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<()>;
}

impl LspChild for Child {
    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }

    fn wait(&mut self) -> io::Result<()> {
        Child::wait(self).map(|_| ())
    }
}

pub(super) trait LspProcessStarter {
    fn start(&self, spec: &LspServerSpec, root: &Path) -> Result<LspProcess, ToolError>;
}

#[derive(Debug, Default)]
struct RealLspProcessStarter;

impl LspProcessStarter for RealLspProcessStarter {
    fn start(&self, spec: &LspServerSpec, root: &Path) -> Result<LspProcess, ToolError> {
        let mut command = Command::new(&spec.command[0]);
        command
            .args(spec.command.iter().skip(1))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if !spec.env.is_empty() {
            command.envs(spec.env.iter());
        }

        let mut child = command.spawn().map_err(|err| {
            ToolError::Execution(format!("failed to start language server: {err}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ToolError::Execution("language server stdin unavailable".to_string()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolError::Execution("language server stdout unavailable".to_string())
        })?;

        Ok(LspProcess {
            child: Box::new(child),
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
        })
    }
}

impl LspSession {
    pub(super) fn start(spec: &LspServerSpec, root: &Path) -> Result<Self, ToolError> {
        Self::start_with_starter(spec, root, &RealLspProcessStarter)
    }

    pub(super) fn start_with_starter(
        spec: &LspServerSpec,
        root: &Path,
        starter: &dyn LspProcessStarter,
    ) -> Result<Self, ToolError> {
        let process = starter.start(spec, root)?;
        let mut session = Self {
            child: process.child,
            stdin: process.stdin,
            stdout: BufReader::new(process.stdout),
            next_id: 1,
            root: root.to_path_buf(),
            diagnostics: BTreeMap::new(),
        };

        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": file_uri_from_path(root),
            "workspaceFolders": [{
                "name": "workspace",
                "uri": file_uri_from_path(root),
            }],
            "capabilities": {
                "window": { "workDoneProgress": true },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "workspaceEdit": {
                        "documentChanges": true,
                        "resourceOperations": ["create", "rename", "delete"],
                    },
                },
                "textDocument": {
                    "publishDiagnostics": {
                        "relatedInformation": true,
                    },
                    "rename": {
                        "prepareSupport": true,
                    },
                    "synchronization": {
                        "didOpen": true,
                        "didChange": true,
                    }
                }
            }
        });
        if let Some(initialization) = &spec.initialization {
            params["initializationOptions"] = initialization.clone();
        }

        let initialize_id = session.next_request_id();
        let initialize_result = session.request_raw(initialize_id, "initialize", params)?;
        if initialize_result.is_null() {
            return Err(ToolError::Execution(
                "language server failed to initialize".to_string(),
            ));
        }

        session.notify("initialized", json!({}))?;
        thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
        Ok(session)
    }

    pub(super) fn open_file(
        &mut self,
        file_path: &Path,
        server_name: &str,
    ) -> Result<(), ToolError> {
        let text = fs::read_to_string(file_path).tool_err("failed to read source file")?;
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": file_uri_from_path(file_path),
                    "languageId": language_id(file_path, server_name),
                    "version": 0,
                    "text": text,
                }
            }),
        )?;
        thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_request_id();
        self.request_raw(id, method, params)
    }

    fn request_raw(&mut self, id: u64, method: &str, params: Value) -> Result<Value, ToolError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        loop {
            let message = self.read_message()?;
            if let Some(response_id) = message.get("id").and_then(Value::as_u64) {
                if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                    self.respond_to_server_request(response_id, server_method, &message)?;
                    continue;
                }
                if response_id != id {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Err(ToolError::Execution(format!(
                        "language server request failed: {}",
                        error_message(error)
                    )));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }

            if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                self.handle_server_notification(server_method, &message);
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_message(&mut self, message: Value) -> Result<(), ToolError> {
        let body = serde_json::to_vec(&message).tool_err("failed to encode lsp request")?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())
            .tool_err("failed to write lsp header")?;
        self.stdin
            .write_all(&body)
            .tool_err("failed to write lsp body")?;
        self.stdin.flush().tool_err("failed to flush lsp request")
    }

    fn read_message(&mut self) -> Result<Value, ToolError> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .tool_err("failed to read lsp header")?;
            if read == 0 {
                return Err(ToolError::Execution(
                    "language server closed the connection".to_string(),
                ));
            }
            if line == "\r\n" {
                break;
            }
            let lowercase = line.to_ascii_lowercase();
            if let Some(value) = lowercase.strip_prefix("content-length:") {
                let parsed = value.trim().parse::<usize>().map_err(|err| {
                    ToolError::Execution(format!("invalid lsp content length: {err}"))
                })?;
                content_length = Some(parsed);
            }
        }

        let length = content_length.ok_or_else(|| {
            ToolError::Execution("language server response missing content length".to_string())
        })?;
        let mut body = vec![0_u8; length];
        self.stdout
            .read_exact(&mut body)
            .tool_err("failed to read lsp body")?;
        serde_json::from_slice(&body).tool_err("failed to decode lsp message")
    }

    fn respond_to_server_request(
        &mut self,
        id: u64,
        method: &str,
        _message: &Value,
    ) -> Result<(), ToolError> {
        let result = match method {
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability" => Value::Null,
            "workspace/configuration" => json!([{}]),
            "workspace/workspaceFolders" => json!([{
                "name": "workspace",
                "uri": file_uri_from_path(&self.root),
            }]),
            _ => Value::Null,
        };
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    fn handle_server_notification(&mut self, method: &str, message: &Value) {
        if method != "textDocument/publishDiagnostics" {
            return;
        }

        let Some(params) = message.get("params") else {
            return;
        };
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return;
        };
        let Some(path) = uri_to_workspace_path(uri, &self.root) else {
            return;
        };
        let diagnostics = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.diagnostics
            .insert(path.display().to_string(), diagnostics);
    }

    pub(super) fn diagnostics(&self) -> Vec<LspDiagnosticReport> {
        self.diagnostics
            .iter()
            .map(|(file_path, diagnostics)| LspDiagnosticReport {
                file_path: file_path.clone(),
                diagnostics: diagnostics.clone(),
            })
            .collect()
    }

    pub(super) fn diagnostics_for(&self, file_path: &Path) -> LspDiagnosticReport {
        let canonical = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf())
            .display()
            .to_string();
        LspDiagnosticReport {
            file_path: canonical.clone(),
            diagnostics: self
                .diagnostics
                .get(&canonical)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn request_with_retry(
    session: &mut LspSession,
    method: &str,
    params: Value,
) -> Result<Value, ToolError> {
    request_with_retry_mode(session, method, params, true)
}

pub(super) fn request_with_retry_no_empty_retry(
    session: &mut LspSession,
    method: &str,
    params: Value,
) -> Result<Value, ToolError> {
    request_with_retry_mode(session, method, params, false)
}

fn request_with_retry_mode(
    session: &mut LspSession,
    method: &str,
    params: Value,
    retry_on_empty: bool,
) -> Result<Value, ToolError> {
    for attempt in 0..DEFAULT_LSP_RETRY_ATTEMPTS {
        match session.request(method, params.clone()) {
            Ok(value)
                if !lsp_value_is_empty(&value)
                    || !retry_on_empty
                    || attempt + 1 == DEFAULT_LSP_RETRY_ATTEMPTS =>
            {
                return Ok(value)
            }
            Ok(_) => {
                thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
            }
            Err(ToolError::Execution(message)) if message.contains("content modified") => {
                thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
            }
            Err(err) => return Err(err),
        }
    }

    std::process::abort()
}

fn error_message(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn lsp_value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn uri_to_workspace_path(uri: &str, root: &Path) -> Option<PathBuf> {
    let path = file_path_from_uri(uri)?;
    path.starts_with(root).then_some(path)
}

fn language_id(path: &Path, server_name: &str) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("rs") => "rust".to_string(),
        Some("ts") => "typescript".to_string(),
        Some("tsx") => "typescriptreact".to_string(),
        Some("py") | Some("pyi") => "python".to_string(),
        Some("go") => "go".to_string(),
        Some("json") | Some("jsonc") => "json".to_string(),
        Some("yaml") | Some("yml") => "yaml".to_string(),
        Some("js") => "javascript".to_string(),
        Some("jsx") => "javascriptreact".to_string(),
        Some("mjs") | Some("cjs") | Some("mts") | Some("cts") => "javascript".to_string(),
        Some(other) => other.to_string(),
        None => server_name.to_string(),
    }
}
