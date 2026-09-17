// allow: SIZE_OK — LSP support (client connection + message handling)
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::tool::ToolError;
use harness_core::ToolResultExt;
use serde_json::{json, Value};

use crate::workspace_paths::{file_path_from_uri, file_uri_from_path};

use super::{LspDiagnosticReport, LspServerSpec};

#[cfg(test)]
mod tests;

const DEFAULT_LSP_BOOT_DELAY_MS: u64 = 150;
const DEFAULT_LSP_RETRY_ATTEMPTS: usize = 8;
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OPEN_DOCUMENTS: usize = 200;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);
const PUBLISH_QUIESCENCE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct SessionControl {
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) shutdown: Arc<AtomicBool>,
}

impl SessionControl {
    fn check(&self) -> Result<(), ToolError> {
        if self.cancelled.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return Err(ToolError::Execution(
                "language server operation cancelled".to_string(),
            ));
        }
        Ok(())
    }
}

struct OpenDocument {
    text: String,
    version: i64,
}

pub(super) struct LspSession {
    child: Box<dyn LspChild>,
    stdin: mpsc::SyncSender<Vec<u8>>,
    messages: Receiver<Result<Value, ToolError>>,
    pub(super) next_id: u64,
    root: PathBuf,
    diagnostics: BTreeMap<String, Vec<Value>>,
    published: BTreeMap<String, (Instant, bool)>,
    opened: BTreeMap<String, OpenDocument>,
    next_document_version: i64,
    pull_results: BTreeMap<String, (String, Vec<Value>)>,
    control: SessionControl,
    diagnostic_provider: Option<Value>,
    save_options: Option<Value>,
    initialized: bool,
    server_quiescent: Option<bool>,
    server_error: Option<String>,
    pub(super) deadline: Instant,
}

pub(super) struct LspProcess {
    pub(super) child: Box<dyn LspChild>,
    pub(super) stdin: Box<dyn Write + Send>,
    pub(super) stdout: Box<dyn Read + Send>,
}

pub(super) trait LspChild: Send {
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<()>;
    fn has_exited(&mut self) -> io::Result<bool> {
        Ok(false)
    }
}

impl LspChild for Child {
    fn has_exited(&mut self) -> io::Result<bool> {
        Child::try_wait(self).map(|status| status.is_some())
    }
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
    pub(super) fn start(
        spec: &LspServerSpec,
        root: &Path,
        control: SessionControl,
        deadline: Instant,
    ) -> Result<Self, ToolError> {
        Self::start_inner(spec, root, &RealLspProcessStarter, control, deadline)
    }

    #[cfg(test)]
    pub(super) fn start_with_starter(
        spec: &LspServerSpec,
        root: &Path,
        starter: &dyn LspProcessStarter,
    ) -> Result<Self, ToolError> {
        Self::start_inner(
            spec,
            root,
            starter,
            SessionControl::default(),
            Instant::now() + REQUEST_TIMEOUT,
        )
    }

    fn start_inner(
        spec: &LspServerSpec,
        root: &Path,
        starter: &dyn LspProcessStarter,
        control: SessionControl,
        deadline: Instant,
    ) -> Result<Self, ToolError> {
        control.check()?;
        let process = starter.start(spec, root)?;
        let (sender, messages) = mpsc::sync_channel(64);
        let (stdin, writes) = mpsc::sync_channel::<Vec<u8>>(16);
        let mut session = Self {
            child: process.child,
            stdin,
            messages,
            next_id: 1,
            root: root.to_path_buf(),
            diagnostics: BTreeMap::new(),
            published: BTreeMap::new(),
            opened: BTreeMap::new(),
            next_document_version: 0,
            pull_results: BTreeMap::new(),
            control,
            diagnostic_provider: None,
            save_options: None,
            initialized: false,
            server_quiescent: None,
            server_error: None,
            deadline,
        };
        // Keep blocking pipe reads off the caller so a silent server cannot hang a tool forever.
        // Dropping the session kills the child and closes the bounded reader channel.
        let read_sender = sender.clone();
        let _reader = thread::Builder::new()
            .name("harness-lsp-reader".to_string())
            .spawn(move || {
                let mut stdout = BufReader::new(process.stdout);
                loop {
                    let message = read_message(&mut stdout);
                    let failed = message.is_err();
                    if read_sender.send(message).is_err() || failed {
                        break;
                    }
                }
            })
            .tool_err("failed to start lsp reader")?;

        // A server that stops reading stdin must not prevent cancellation or child cleanup.
        let _writer = thread::Builder::new()
            .name("harness-lsp-writer".to_string())
            .spawn(move || {
                let mut stdin = process.stdin;
                for frame in writes {
                    if let Err(error) = stdin.write_all(&frame).and_then(|()| stdin.flush()) {
                        let _ = sender.send(Err(ToolError::Execution(format!(
                            "failed to write lsp request: {error}"
                        ))));
                        break;
                    }
                }
            })
            .tool_err("failed to start lsp writer")?;

        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": file_uri_from_path(root),
            "workspaceFolders": [{
                "name": "workspace",
                "uri": file_uri_from_path(root),
            }],
            "capabilities": {
                "experimental": { "serverStatusNotification": true },
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
                        "versionSupport": true,
                    },
                    "diagnostic": {},
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
        session.save_options = initialize_result
            .pointer("/capabilities/textDocumentSync/save")
            .filter(|save| save.is_object() || **save == Value::Bool(true))
            .cloned();
        session.diagnostic_provider = initialize_result
            .pointer("/capabilities/diagnosticProvider")
            .filter(|provider| provider.is_object() || **provider == Value::Bool(true))
            .cloned();
        if initialize_result
            .pointer("/serverInfo/name")
            .and_then(Value::as_str)
            == Some("rust-analyzer")
        {
            session.server_quiescent = Some(false);
        }

        session.notify("initialized", json!({}))?;
        session.initialized = true;
        // A cold rust-analyzer can return an empty full report before loading the project.
        // Its status notification is the readiness barrier, not an arbitrary startup sleep.
        session.wait_until_ready()?;
        Ok(session)
    }

    pub(super) fn open_file(
        &mut self,
        file_path: &Path,
        server_name: &str,
    ) -> Result<(), ToolError> {
        let text = fs::read_to_string(file_path).tool_err("failed to read source file")?;
        let key = file_path.display().to_string();
        let previous = self.opened.get(&key);
        if previous.is_some_and(|document| document.text == text) {
            return Ok(());
        }
        let is_open = previous.is_some();
        // Session-wide versions also reject delayed pushes after a document is closed/reopened.
        let version = self.next_document_version;
        self.next_document_version += 1;
        if !is_open && self.opened.len() >= MAX_OPEN_DOCUMENTS {
            // ponytail: bounded buffers with deterministic eviction; use LRU if navigation churn warrants it.
            if let Some(oldest) = self.opened.keys().next().cloned() {
                self.close_document(&oldest)?;
            }
        }
        // Consume queued old pushes before assigning the new document version.
        self.drain_messages()?;
        self.diagnostics.remove(&key);
        self.published.remove(&key);
        self.pull_results.clear();
        if !is_open {
            self.notify("textDocument/didOpen", json!({"textDocument": {
                "uri": file_uri_from_path(file_path), "languageId": language_id(file_path, server_name),
                "version": version, "text": text,
            }}))?;
        } else {
            // A full replacement is valid for both full and incremental synchronization.
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": file_uri_from_path(file_path), "version": version},
                    "contentChanges": [{"text": text}],
                }),
            )?;
        }
        if let Some(options) = &self.save_options {
            let mut params = json!({"textDocument": {"uri": file_uri_from_path(file_path)}});
            if options.get("includeText").and_then(Value::as_bool) == Some(true) {
                params["text"] = json!(text);
            }
            self.notify("textDocument/didSave", params)?;
        }
        self.opened.insert(key, OpenDocument { text, version });
        Ok(())
    }

    pub(super) fn sync_documents(&mut self, server_name: &str) -> Result<(), ToolError> {
        // ponytail: rescan at most 200 open buffers; add a filesystem watcher only if measured I/O warrants it.
        for key in self.opened.keys().cloned().collect::<Vec<_>>() {
            let path = Path::new(&key);
            if path.try_exists().tool_err("failed to inspect lsp source")?
                && path
                    .canonicalize()
                    .tool_err("failed to resolve lsp source")?
                    == path
            {
                self.open_file(path, server_name)?;
            } else {
                self.close_document(&key)?;
            }
        }
        Ok(())
    }

    fn close_document(&mut self, key: &str) -> Result<(), ToolError> {
        // `key` is the path we opened. Re-canonicalizing a replaced symlink would close the wrong URI.
        let uri = reqwest::Url::from_file_path(key)
            .map_err(|()| ToolError::Execution("invalid open document path".to_string()))?;
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri.as_str()}}),
        )?;
        self.opened.remove(key);
        self.diagnostics.remove(key);
        self.published.remove(key);
        self.pull_results.clear();
        Ok(())
    }

    pub(super) fn begin_operation(&mut self, control: SessionControl, deadline: Instant) {
        self.control = control;
        self.deadline = deadline;
    }

    pub(super) fn drain_messages(&mut self) -> Result<(), ToolError> {
        for _ in 0..64 {
            match self.messages.try_recv() {
                Ok(message) => {
                    self.handle_server_message(&message?)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(ToolError::Execution(
                        "language server closed the connection".to_string(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_request_id();
        self.request_raw(id, method, params)
    }

    fn request_raw(&mut self, id: u64, method: &str, params: Value) -> Result<Value, ToolError> {
        self.request_raw_until(id, method, params, self.deadline)
    }

    fn request_raw_until(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, ToolError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        loop {
            let message = self.receive_until(deadline)?.ok_or_else(|| {
                ToolError::Execution(format!("language server timed out waiting for {method}"))
            })?;
            if self.handle_server_message(&message)? {
                continue;
            }
            if let Some(response_id) = message.get("id").and_then(Value::as_u64) {
                if response_id != id {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Err(ToolError::Execution(format!(
                        "language server request failed ({}): {}",
                        error.get("code").unwrap_or(&Value::Null),
                        error_message(error)
                    )));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn handle_server_message(&mut self, message: &Value) -> Result<bool, ToolError> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(false);
        };
        if let Some(id) = message.get("id") {
            self.respond_to_server_request(id, method, message)?;
        } else {
            self.handle_server_notification(method, message);
        }
        Ok(true)
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
        self.control.check()?;
        let body = serde_json::to_vec(&message).tool_err("failed to encode lsp request")?;
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend(body);
        loop {
            match self.stdin.try_send(frame) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(ToolError::Execution(
                        "language server write queue closed".to_string(),
                    ))
                }
                Err(mpsc::TrySendError::Full(pending)) => frame = pending,
            }
            self.control.check()?;
            if Instant::now() >= self.deadline {
                return Err(ToolError::Execution(
                    "language server timed out writing request".to_string(),
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn receive_until(&self, deadline: Instant) -> Result<Option<Value>, ToolError> {
        loop {
            self.control.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            match self
                .messages
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                Ok(message) => return message.map(Some),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ToolError::Execution(
                        "language server closed the connection".to_string(),
                    ))
                }
            }
        }
    }

    fn respond_to_server_request(
        &mut self,
        id: &Value,
        method: &str,
        message: &Value,
    ) -> Result<(), ToolError> {
        let result = match method {
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability" => Value::Null,
            "workspace/configuration" => json!(vec![
                json!({});
                message
                    .pointer("/params/items")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            ]),
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
        if method == "experimental/serverStatus" {
            if let Some(quiescent) = message
                .pointer("/params/quiescent")
                .and_then(Value::as_bool)
            {
                self.server_quiescent = Some(quiescent);
            }
            self.server_error = match message.pointer("/params/health").and_then(Value::as_str) {
                Some("error" | "warning") => Some(
                    message
                        .pointer("/params/message")
                        .and_then(Value::as_str)
                        .unwrap_or("language server is not fully operational")
                        .to_string(),
                ),
                _ => None,
            };
            return;
        }
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
        let key = path.display().to_string();
        if params
            .get("version")
            .is_some_and(|version| !version.is_null() && !version.is_i64())
        {
            return;
        }
        let version = params.get("version").and_then(Value::as_i64);
        let Some(document) = self.opened.get(&key) else {
            return;
        };
        if version.is_some_and(|version| version != document.version) {
            return;
        }
        let Some(diagnostics) = params.get("diagnostics").and_then(Value::as_array) else {
            return;
        };
        self.diagnostics.insert(key.clone(), diagnostics.clone());
        self.published
            .insert(key, (Instant::now(), version.is_some()));
    }

    fn wait_until_ready(&mut self) -> Result<(), ToolError> {
        while self.server_quiescent == Some(false) {
            let message = self.receive_until(self.deadline)?.ok_or_else(|| {
                ToolError::Execution(
                    "language server timed out loading the project; diagnostics are unavailable"
                        .to_string(),
                )
            })?;
            self.handle_server_message(&message)?;
        }
        if let Some(error) = &self.server_error {
            return Err(ToolError::Execution(format!(
                "language server is not ready: {error}"
            )));
        }
        Ok(())
    }

    pub(super) fn collect_diagnostics(&mut self, file_path: &Path) -> Result<(), ToolError> {
        self.drain_messages()?;
        self.wait_until_ready()?;
        let key = file_path.display().to_string();
        let deadline = self.deadline.min(Instant::now() + DIAGNOSTICS_TIMEOUT);
        if let Some(provider) = self.diagnostic_provider.clone() {
            let mut params = json!({"textDocument": {"uri": file_uri_from_path(file_path)}});
            if let Some(identifier) = provider.get("identifier").and_then(Value::as_str) {
                params["identifier"] = json!(identifier);
            }
            if let Some((result_id, _)) = self.pull_results.get(&key) {
                params["previousResultId"] = json!(result_id);
            }
            loop {
                let id = self.next_request_id();
                match self.request_raw_until(
                    id,
                    "textDocument/diagnostic",
                    params.clone(),
                    deadline,
                ) {
                    Ok(report) => {
                        self.apply_pull_report(&key, &params, &report)?;
                        return self.ensure_current_document(file_path);
                    }
                    Err(ToolError::Execution(message)) if unsupported_diagnostics(&message) => {
                        self.diagnostic_provider = None;
                        break;
                    }
                    Err(ToolError::Execution(message))
                        if (message.contains("(-32801)")
                            || message.contains("(-32802)")
                            || message.to_ascii_lowercase().contains("content modified"))
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        loop {
            let ready_at = self.published.get(&key).map(|(received, versioned)| {
                if *versioned {
                    *received
                } else {
                    *received + PUBLISH_QUIESCENCE
                }
            });
            if ready_at.is_some_and(|ready| Instant::now() >= ready) {
                return self.ensure_current_document(file_path);
            }
            if Instant::now() >= deadline {
                return Err(ToolError::Execution(format!(
                    "timed out waiting for fresh diagnostics for {}; diagnostics are unavailable",
                    file_path.display()
                )));
            }
            if let Some(message) =
                self.receive_until(ready_at.map_or(deadline, |ready| ready.min(deadline)))?
            {
                self.handle_server_message(&message)?;
            }
        }
    }

    fn apply_pull_report(
        &mut self,
        key: &str,
        params: &Value,
        report: &Value,
    ) -> Result<(), ToolError> {
        let items = match report["kind"].as_str() {
    Some("full") => report.get("items").and_then(Value::as_array).cloned(),
    Some("unchanged") => self.pull_results.get(key)
        .filter(|(id, _)| params["previousResultId"].as_str() == Some(id.as_str()) && report["resultId"].is_string())
        .map(|(_, items)| items.clone()),
    _ => None,
}.ok_or_else(|| ToolError::Execution("language server returned diagnostics without a valid full report or cached result; freshness is unknown".to_string()))?;
        if let Some(id) = report["resultId"].as_str() {
            self.pull_results
                .insert(key.to_string(), (id.to_string(), items.clone()));
        } else {
            self.pull_results.remove(key);
        }
        self.diagnostics.insert(key.to_string(), items);
        Ok(())
    }

    fn ensure_current_document(&self, path: &Path) -> Result<(), ToolError> {
        let current =
            fs::read_to_string(path).tool_err("failed to verify diagnostic source freshness")?;
        if self
            .opened
            .get(&path.display().to_string())
            .map(|document| &document.text)
            != Some(&current)
        {
            return Err(ToolError::Execution(
                "source changed while collecting diagnostics; run fileDiagnostics again"
                    .to_string(),
            ));
        }
        Ok(())
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
        if self.initialized {
            self.control = SessionControl::default();
            self.deadline = Instant::now() + SHUTDOWN_TIMEOUT;
            let acknowledged = self.request("shutdown", Value::Null).is_ok()
                && self.notify("exit", Value::Null).is_ok();
            while acknowledged && Instant::now() < self.deadline {
                if self.child.has_exited().unwrap_or(false) {
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn request_with_retry(
    session: &mut LspSession,
    method: &str,
    params: Value,
) -> Result<Value, ToolError> {
    for attempt in 0..DEFAULT_LSP_RETRY_ATTEMPTS {
        match session.request(method, params.clone()) {
            Err(ToolError::Execution(message))
                if (message.contains("(-32801)")
                    || message.contains("(-32802)")
                    || message.to_ascii_lowercase().contains("content modified"))
                    && attempt + 1 < DEFAULT_LSP_RETRY_ATTEMPTS =>
            {
                thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
            }
            result => return result,
        }
    }
    Err(ToolError::Execution(
        "language server did not stabilize before the retry limit".to_string(),
    ))
}

fn unsupported_diagnostics(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("(-32601)")
        || message.contains("method not found")
        || message.contains("unknown request")
        || message.contains("not implemented")
}

fn read_message(reader: &mut impl BufRead) -> Result<Value, ToolError> {
    let mut content_length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let read = reader
            .take(8193 - header_bytes)
            .read_line(&mut line)
            .tool_err("failed to read lsp header")?;
        header_bytes += read as u64;
        if header_bytes > 8192 {
            return Err(ToolError::Execution(
                "language server header exceeds 8 KiB".to_string(),
            ));
        }
        if read == 0 {
            return Err(ToolError::Execution(
                "language server closed the connection".to_string(),
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .tool_err("invalid lsp content length")?,
            );
        }
    }
    let length = content_length
        .filter(|length| *length <= 16 * 1024 * 1024)
        .ok_or_else(|| {
            ToolError::Execution(
                "language server response has missing or excessive content length".to_string(),
            )
        })?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .tool_err("failed to read lsp body")?;
    serde_json::from_slice(&body).tool_err("failed to decode lsp message")
}

fn error_message(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
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
