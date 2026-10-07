//! Claude Code stream-json control protocol (the SDK `query()` subset the lane uses).
//!
//! Mirrors `@anthropic-ai/claude-agent-sdk` `query()` (0.3.289): spawns the `claude` binary with
//! the SDK's argv, answers its control requests (permission prompts, hook callbacks, the
//! in-process `custom-tools` MCP server), and yields every other stdout message.
use super::tools::{
    CUSTOM_TOOLS_MCP_SERVER_NAME, HOST_CAPTURED_SDK_TOOL_MATCHER, TOOL_EXECUTION_DENIED_MESSAGE,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
};
mod reader;
pub use reader::*;
mod argv;
pub use argv::*;

pub const SDK_VERSION: &str = "0.3.289";
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_MCP_VERSIONS: [&str; 5] = [
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
    "2024-10-07",
];
const STDERR_TAIL: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemPrompt {
    Custom(String),
    /// The `claude_code` preset with an optional append.
    Preset {
        append: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Thinking {
    Adaptive {
        display: String,
    },
    /// `maxThinkingTokens`; zero disables thinking.
    Budget(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// SDK `Options` as the lane sets them; `canUseTool` and the host-tool denial hooks are always on.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryOptions {
    pub cwd: PathBuf,
    pub model: String,
    pub tools: Vec<String>,
    pub permission_mode: String,
    pub include_partial_messages: bool,
    pub system_prompt: SystemPrompt,
    pub settings: Value,
    pub setting_sources: Vec<String>,
    pub executable: PathBuf,
    pub max_turns: Option<u32>,
    /// Ordered `extraArgs`; `None` is a bare flag.
    pub extra_args: Vec<(String, Option<String>)>,
    pub thinking: Option<Thinking>,
    pub effort: Option<String>,
    pub env: BTreeMap<String, String>,
    pub custom_tools: Vec<CustomTool>,
    pub resume: Option<String>,
    pub resume_session_at: Option<String>,
    pub fork_session: bool,
    pub session_id: Option<String>,
}

fn mcp_initialize_result(version: &str) -> Value {
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": true}},
        "serverInfo": {"name": CUSTOM_TOOLS_MCP_SERVER_NAME, "version": "1.0.0"},
    })
}

pub fn host_tool_denial_output() -> Value {
    json!({
        "continue": false,
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": TOOL_EXECUTION_DENIED_MESSAGE,
        },
    })
}

fn append_tail(tail: &mut String, text: &str) {
    tail.push_str(text);
    if tail.len() > 2 * STDERR_TAIL {
        let cut = tail.len() - STDERR_TAIL;
        let cut = (cut..tail.len())
            .find(|i| tail.is_char_boundary(*i))
            .unwrap_or(tail.len());
        tail.drain(..cut);
    }
}

/// The text an `is_error` result carries (`lastErrorResultText`).
fn error_result_text(message: &Value) -> Option<String> {
    if message["is_error"] != true {
        return None;
    }
    let text = if message["subtype"] == "success" {
        message["result"].as_str().unwrap_or("").to_owned()
    } else {
        message["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    };
    (!text.is_empty()).then_some(text)
}

fn request_id() -> String {
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    bytes
        .iter()
        .take(11)
        .map(|b| char::from(alphabet[usize::from(*b) % 36]))
        .collect()
}

/// How the prompt reaches stdin: one message, then stdin closes when the run ends
/// (bidirectional needs keep it open until the result), or a resident stream the caller feeds.
pub enum Prompt {
    Single(Value),
    Streaming,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct QueryError(pub String);

enum Write {
    Line(String),
    End,
}

#[derive(Default)]
struct RunState {
    result_received: bool,
    session_state: Option<String>,
    last_error_result: Option<String>,
}

pub struct ClaudeQuery {
    writer: mpsc::UnboundedSender<Write>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>,
    messages: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Value, QueryError>>>,
    init: watch::Receiver<Option<Result<Value, String>>>,
    closed: Arc<AtomicBool>,
    pid: Option<u32>,
    reader: tokio::task::JoinHandle<()>,
}

impl ClaudeQuery {
    pub fn spawn(options: &QueryOptions, prompt: Prompt) -> Result<Self, QueryError> {
        let mut command = tokio::process::Command::new(&options.executable);
        command
            .args(options.argv())
            .current_dir(&options.cwd)
            .env_clear()
            .envs(options.child_env())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                QueryError(format!(
                    "Claude Code native binary not found at {}. Please ensure Claude Code is installed via native installer or specify a valid path with options.pathToClaudeCodeExecutable.",
                    options.executable.display()
                ))
            } else {
                QueryError(format!("Failed to spawn Claude Code process: {e}"))
            }
        })?;
        let pid = child.id();
        let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(QueryError("Claude Code process has no stdio".into()));
        };
        let (writer, mut lines) = mpsc::unbounded_channel::<Write>();
        tokio::spawn(async move {
            while let Some(Write::Line(line)) = lines.recv().await {
                let line = line + "\n";
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let tail = Arc::clone(&stderr_tail);
        tokio::spawn(async move {
            let mut buffer = [0u8; 4096];
            while let Ok(read @ 1..) = stderr.read(&mut buffer).await {
                if let Ok(mut tail) = tail.lock() {
                    append_tail(&mut tail, &String::from_utf8_lossy(&buffer[..read]));
                }
            }
        });
        let pending = Arc::new(Mutex::new(HashMap::<_, oneshot::Sender<_>>::new()));
        let (messages_tx, messages_rx) = mpsc::unbounded_channel();
        let (init_tx, init_rx) = watch::channel(None);
        let closed = Arc::new(AtomicBool::new(false));
        let single = matches!(prompt, Prompt::Single(_));

        let init_id = request_id();
        let (init_reply, init_wait) = oneshot::channel();
        if let Ok(mut pending) = pending.lock() {
            pending.insert(init_id.clone(), init_reply);
        }
        let _ = writer.send(Write::Line(
            json!({"request_id": init_id, "type": "control_request", "request": options.initialize_request()})
                .to_string(),
        ));
        tokio::spawn(async move {
            let result = init_wait
                .await
                .unwrap_or_else(|_| Err("Query closed before response received".into()));
            let _ = init_tx.send(Some(result));
        });
        if let Prompt::Single(message) = prompt {
            let _ = writer.send(Write::Line(message.to_string()));
        }

        let state = Arc::new(Mutex::new(RunState::default()));
        let ctx = ReaderContext {
            writer: writer.clone(),
            pending: Arc::clone(&pending),
            messages: messages_tx,
            closed: Arc::clone(&closed),
            state,
            single,
            custom_tools: !options.custom_tools.is_empty(),
            tools_list: options.tools_list_result(),
        };
        let reader = tokio::spawn(read_messages(ctx, stdout, child, stderr_tail));
        Ok(Self {
            writer,
            pending,
            messages: tokio::sync::Mutex::new(messages_rx),
            init: init_rx,
            closed,
            pid,
            reader,
        })
    }

    /// Next SDK message; `None` once the process ended or the query closed.
    pub async fn next(&self) -> Option<Result<Value, QueryError>> {
        self.messages.lock().await.recv().await
    }

    pub fn push_user(&self, message: Value) -> Result<(), QueryError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(QueryError(
                "Cannot push to a closed session input controller".into(),
            ));
        }
        self.writer
            .send(Write::Line(message.to_string()))
            .map_err(|_| QueryError("ProcessTransport is not ready for writing".into()))
    }

    async fn request(&self, request: Value) -> Result<Value, QueryError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(QueryError("Query closed before response received".into()));
        }
        let id = request_id();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), tx);
        }
        self.writer
            .send(Write::Line(
                json!({"request_id": id, "type": "control_request", "request": request})
                    .to_string(),
            ))
            .map_err(|_| QueryError("ProcessTransport is not ready for writing".into()))?;
        rx.await
            .map_err(|_| QueryError("Query closed before response received".into()))?
            .map_err(QueryError)
    }

    /// The interrupt receipt: `{still_queued}` when Claude Code reports its queue.
    pub async fn interrupt(&self) -> Result<Option<Value>, QueryError> {
        let response = self.request(json!({"subtype": "interrupt"})).await?;
        let Some(queued) = response.get("still_queued").and_then(Value::as_array) else {
            return Ok(None);
        };
        let queued: Vec<_> = queued.iter().filter(|v| v.is_string()).cloned().collect();
        let mut receipt = json!({"still_queued": queued});
        if let Some(cancelled) = response.get("cancelled").and_then(Value::as_array) {
            receipt["cancelled"] = json!(cancelled
                .iter()
                .filter(|v| v.is_string())
                .cloned()
                .collect::<Vec<_>>());
        }
        Ok(Some(receipt))
    }

    pub async fn set_model(&self, model: &str) -> Result<(), QueryError> {
        self.request(json!({"subtype": "set_model", "model": model}))
            .await
            .map(|_| ())
    }

    pub async fn initialization_result(&self) -> Result<Value, QueryError> {
        let mut init = self.init.clone();
        let result = init
            .wait_for(Option::is_some)
            .await
            .map_err(|_| QueryError("Query closed before response received".into()))?
            .clone();
        result
            .unwrap_or_else(|| Err("Query closed before response received".into()))
            .map_err(QueryError)
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.writer.send(Write::End);
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        terminate(self.pid);
        self.reader.abort();
    }
}

impl Drop for ClaudeQuery {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(unix)]
fn terminate(pid: Option<u32>) {
    let pid = pid
        .and_then(|pid| i32::try_from(pid).ok())
        .and_then(rustix::process::Pid::from_raw);
    if let Some(pid) = pid {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
    }
}
#[cfg(not(unix))]
fn terminate(_pid: Option<u32>) {}
