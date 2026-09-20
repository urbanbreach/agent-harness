// allow: SIZE_OK — MCP tool integration (server registration + rendering)
use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::McpServerConfig;
use harness_core::tool::ToolError;
use harness_core::ToolResultExt;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::mcp_render::{
    jsonrpc_error_message, render_mcp_http_parse_error, render_mcp_http_status_error,
};
use crate::text::has_trimmed_content;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
// Internal read budgets: reject oversized protocol input before retaining it.
const MCP_RESPONSE_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const MCP_HEADER_BYTE_LIMIT: usize = 8 * 1024;

fn response_limit_error() -> ToolError {
    ToolError::Execution(format!(
        "MCP response exceeded {MCP_RESPONSE_BYTE_LIMIT}-byte limit"
    ))
}

fn header_limit_error() -> ToolError {
    ToolError::Execution(format!(
        "MCP header exceeded {MCP_HEADER_BYTE_LIMIT}-byte limit"
    ))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct McpSessionMetadata {
    pub(crate) protocol_version: Option<String>,
    pub(crate) server_info: Option<Value>,
}

pub(crate) enum McpSession {
    Stdio(StdioMcpSession),
    Http(HttpMcpSession),
}

impl McpSession {
    pub(crate) async fn start(
        server_id: &str,
        config: &McpServerConfig,
        http_client: reqwest::Client,
    ) -> Result<Self, ToolError> {
        match config {
            McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                ..
            } => Ok(Self::Stdio(
                StdioMcpSession::start(server_id, command, env, cwd.as_ref(), *timeout_secs)
                    .await?,
            )),
            McpServerConfig::Http {
                endpoint,
                headers,
                timeout_secs,
                ..
            } => Ok(Self::Http(
                HttpMcpSession::start(server_id, endpoint, headers, *timeout_secs, http_client)
                    .await?,
            )),
        }
    }

    pub(crate) async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, ToolError> {
        match self {
            Self::Stdio(session) => session.request(method, params).await,
            Self::Http(session) => session.request(method, params).await,
        }
    }

    pub(crate) fn metadata(&self) -> &McpSessionMetadata {
        match self {
            Self::Stdio(session) => session.metadata(),
            Self::Http(session) => &session.metadata,
        }
    }

    pub(crate) async fn close(self) -> Result<(), ToolError> {
        match self {
            Self::Stdio(session) => session.close().await,
            Self::Http(session) => session.close().await,
        }
    }
}

pub(crate) struct StdioMcpSession {
    child: Box<dyn StdioMcpChild>,
    stdin: Box<dyn AsyncWrite + Send + Unpin>,
    stdout: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    next_id: u64,
    timeout: Duration,
    metadata: McpSessionMetadata,
}

struct StdioMcpProcess {
    child: Box<dyn StdioMcpChild>,
    stdin: Box<dyn AsyncWrite + Send + Unpin>,
    stdout: Box<dyn AsyncRead + Send + Unpin>,
}

#[async_trait]
trait StdioMcpChild: Send {
    async fn kill(&mut self) -> io::Result<()>;
    async fn wait(&mut self) -> io::Result<()>;
}

#[async_trait]
impl StdioMcpChild for Child {
    async fn kill(&mut self) -> io::Result<()> {
        Child::kill(self).await
    }

    async fn wait(&mut self) -> io::Result<()> {
        Child::wait(self).await.map(|_| ())
    }
}

trait StdioMcpProcessStarter: Sync {
    fn start(
        &self,
        server_id: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
    ) -> Result<StdioMcpProcess, ToolError>;
}

#[derive(Debug, Default)]
struct RealStdioMcpProcessStarter;

impl StdioMcpProcessStarter for RealStdioMcpProcessStarter {
    fn start(
        &self,
        server_id: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
    ) -> Result<StdioMcpProcess, ToolError> {
        if command.is_empty() {
            return Err(ToolError::Execution(format!(
                "MCP server `{server_id}` has empty stdio command"
            )));
        }

        let mut process = Command::new(&command[0]);
        process
            .kill_on_drop(true)
            .args(command.iter().skip(1))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }
        if !env.is_empty() {
            process.envs(env.iter());
        }

        let mut child = process.spawn().map_err(|err| {
            ToolError::Execution(format!(
                "failed to start MCP stdio server `{server_id}`: {err}"
            ))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            ToolError::Execution(format!("MCP stdio server `{server_id}` stdin unavailable"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolError::Execution(format!("MCP stdio server `{server_id}` stdout unavailable"))
        })?;
        Ok(StdioMcpProcess {
            child: Box::new(child),
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
        })
    }
}

impl StdioMcpSession {
    pub(crate) async fn start(
        server_id: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
        timeout_secs: u64,
    ) -> Result<Self, ToolError> {
        Self::start_with_starter(
            server_id,
            command,
            env,
            cwd,
            timeout_secs,
            &RealStdioMcpProcessStarter,
        )
        .await
    }

    async fn start_with_starter(
        server_id: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
        timeout_secs: u64,
        starter: &dyn StdioMcpProcessStarter,
    ) -> Result<Self, ToolError> {
        let process = starter.start(server_id, command, env, cwd)?;
        let mut session = Self {
            child: process.child,
            stdin: process.stdin,
            stdout: BufReader::new(process.stdout),
            next_id: 1,
            timeout: Duration::from_secs(timeout_secs.max(1)),
            metadata: McpSessionMetadata::default(),
        };
        let initialized = async {
            let initialize = session
                .request(
                    "initialize",
                    json!({
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": {
                            "name": "agent-harness",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                    }),
                )
                .await?;
            session.metadata = parse_session_metadata(&initialize);
            session.notify("notifications/initialized", json!({})).await
        }
        .await;
        if let Err(err) = initialized {
            // Tokio's kill also waits; bound cleanup and preserve the startup error.
            let _ = timeout(session.timeout, session.child.kill()).await;
            return Err(err);
        }
        Ok(session)
    }

    pub(crate) async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, ToolError> {
        let request_id = self.next_request_id();
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let message = match self.read_message().await {
                Ok(message) => message,
                Err(error) => {
                    // A failed read leaves the protocol unusable; terminate and reap now.
                    let _ = timeout(self.timeout, self.child.kill()).await;
                    return Err(error);
                }
            };
            if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                if let Some(message_id) = message.get("id").cloned() {
                    self.respond_method_not_found(message_id, server_method)
                        .await?;
                }
                continue;
            }

            if message.get("id") != Some(&Value::String(request_id.clone())) {
                continue;
            }

            return extract_jsonrpc_result(message, method);
        }
    }

    pub(crate) fn metadata(&self) -> &McpSessionMetadata {
        &self.metadata
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn respond_method_not_found(
        &mut self,
        request_id: Value,
        method: &str,
    ) -> Result<(), ToolError> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32601,
                "message": format!("agent-harness MCP client does not implement `{method}`"),
            },
        }))
        .await
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), ToolError> {
        let body = serde_json::to_vec(message).tool_err("failed to encode MCP message")?;
        timeout(self.timeout, async {
            self.stdin.write_all(&body).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| ToolError::Execution("MCP stdio write timed out".to_string()))?
        .tool_err("failed to write MCP message")
        .tool_err("failed to flush MCP message")
    }

    async fn read_line(&mut self, mut byte_limit: usize) -> Result<String, ToolError> {
        timeout(self.timeout, async {
            let mut line = Vec::new();
            loop {
                let available = self
                    .stdout
                    .fill_buf()
                    .await
                    .tool_err("failed to read MCP output")?;
                if available.is_empty() {
                    break;
                }
                let read = available
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(available.len(), |index| index + 1);
                if line
                    .iter()
                    .chain(available[..read].iter())
                    .take(b"content-length:".len())
                    .map(u8::to_ascii_lowercase)
                    .eq(b"content-length:".iter().copied())
                {
                    byte_limit = byte_limit.min(MCP_HEADER_BYTE_LIMIT);
                }
                if read > byte_limit - line.len() && byte_limit <= MCP_HEADER_BYTE_LIMIT {
                    return Err(header_limit_error());
                }
                if read > byte_limit - line.len() {
                    return Err(response_limit_error());
                }
                line.extend_from_slice(&available[..read]);
                self.stdout.consume(read);
                if line.last() == Some(&b'\n') {
                    break;
                }
            }
            String::from_utf8(line).map_err(|_| {
                ToolError::Execution("MCP stdio output is not valid UTF-8".to_string())
            })
        })
        .await
        .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
    }

    async fn read_message(&mut self) -> Result<Value, ToolError> {
        loop {
            let line = self.read_line(MCP_RESPONSE_BYTE_LIMIT).await?;
            if line.is_empty() {
                return Err(ToolError::Execution(
                    "MCP stdio server closed the connection".to_string(),
                ));
            }

            if !has_trimmed_content(&line) {
                continue;
            }

            if line
                .as_bytes()
                .get(..b"content-length:".len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"content-length:"))
            {
                let length = parse_content_length(&line)?;
                return self.read_framed_message(length, line.len()).await;
            }

            return serde_json::from_str(line.trim()).map_err(|err| {
                ToolError::Execution(format!("failed to parse MCP stdio message: {err}"))
            });
        }
    }

    async fn read_framed_message(
        &mut self,
        length: usize,
        mut header_bytes: usize,
    ) -> Result<Value, ToolError> {
        if length > MCP_RESPONSE_BYTE_LIMIT {
            return Err(response_limit_error());
        }
        loop {
            let header_line = self.read_line(MCP_HEADER_BYTE_LIMIT - header_bytes).await?;
            header_bytes += header_line.len();
            if header_line.is_empty() {
                return Err(ToolError::Execution(
                    "MCP stdio server closed before message body".to_string(),
                ));
            }
            if header_line == "\n" || header_line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0_u8; length];
        timeout(self.timeout, self.stdout.read_exact(&mut body))
            .await
            .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
            .map_err(|err| {
                ToolError::Execution(format!("failed to read MCP message body: {err}"))
            })?;
        serde_json::from_slice(&body)
            .map_err(|err| ToolError::Execution(format!("failed to parse MCP message body: {err}")))
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("{}", self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) async fn close(mut self) -> Result<(), ToolError> {
        drop(self.stdin);
        match timeout(self.timeout, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(ToolError::Execution(format!(
                "failed to wait for MCP stdio server shutdown: {err}"
            ))),
            Err(_) => {
                let _ = self.child.kill().await;
                let _ = self.child.wait().await;
                Ok(())
            }
        }
    }
}

pub(crate) struct HttpMcpSession {
    client: reqwest::Client,
    endpoint: String,
    headers: BTreeMap<String, String>,
    timeout: Duration,
    next_id: u64,
    session_id: Option<String>,
    metadata: McpSessionMetadata,
}

impl HttpMcpSession {
    async fn start(
        server_id: &str,
        endpoint: &str,
        headers: &BTreeMap<String, String>,
        timeout_secs: u64,
        client: reqwest::Client,
    ) -> Result<Self, ToolError> {
        let mut session = Self {
            client,
            endpoint: endpoint.to_string(),
            headers: headers.clone(),
            timeout: Duration::from_secs(timeout_secs.max(1)),
            next_id: 1,
            session_id: None,
            metadata: McpSessionMetadata::default(),
        };

        let initialize = session
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "agent-harness",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
            )
            .await
            .map_err(|err| {
                ToolError::Execution(format!(
                    "failed to initialize MCP HTTP server `{server_id}`: {err}"
                ))
            })?;
        session.metadata = parse_session_metadata(&initialize);
        session
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(session)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_request_id();
        let message = self
            .post_jsonrpc(
                Some(id.clone()),
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        extract_jsonrpc_result(message, method)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        let _ = self
            .post_jsonrpc(
                None,
                json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                }),
            )
            .await?;
        Ok(())
    }

    async fn post_jsonrpc(
        &mut self,
        request_id: Option<String>,
        payload: Value,
    ) -> Result<Value, ToolError> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .timeout(self.timeout)
            .json(&payload);
        if let Some(session_id) = &self.session_id {
            request = request.header(MCP_SESSION_ID_HEADER, session_id);
        }
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }

        let response = request.send().await.map_err(|err| {
            if err.is_timeout() {
                ToolError::Execution("MCP HTTP request timed out".to_string())
            } else {
                ToolError::Execution(format!("MCP HTTP request failed: {err}"))
            }
        })?;
        if let Some(header_value) = response
            .headers()
            .get(MCP_SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
        {
            self.session_id = Some(header_value);
        }

        let status = response.status();
        if !status.is_success() {
            let headers = response.headers().clone();
            let bytes = read_http_body(response).await?;
            let body = String::from_utf8_lossy(&bytes);
            return Err(ToolError::Execution(render_mcp_http_status_error(
                status, &headers, &body,
            )));
        }

        if request_id.is_none() {
            return Ok(Value::Null);
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            return read_sse_response(response, request_id.as_deref()).await;
        }

        let bytes = read_http_body(response).await?;
        let body = String::from_utf8_lossy(&bytes);
        if !has_trimmed_content(&body) {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body)
            .map_err(|err| ToolError::Execution(render_mcp_http_parse_error(&body, &err)))
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("{}", self.next_id);
        self.next_id += 1;
        id
    }

    async fn close(self) -> Result<(), ToolError> {
        if let Some(session_id) = self.session_id {
            let mut request = self
                .client
                .delete(&self.endpoint)
                .header(MCP_SESSION_ID_HEADER, session_id)
                .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
                .timeout(self.timeout);
            for (name, value) in &self.headers {
                request = request.header(name, value);
            }
            let _ = request.send().await;
        }
        Ok(())
    }
}

async fn read_http_body(mut response: reqwest::Response) -> Result<Vec<u8>, ToolError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .tool_err("failed to read MCP HTTP response body")?
    {
        if chunk.len() > MCP_RESPONSE_BYTE_LIMIT - body.len() {
            return Err(response_limit_error());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn read_sse_response(
    mut response: reqwest::Response,
    request_id: Option<&str>,
) -> Result<Value, ToolError> {
    let mut buffer = Vec::new();
    let mut response_bytes = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .tool_err("failed to read MCP SSE chunk")?
    {
        if chunk.len() > MCP_RESPONSE_BYTE_LIMIT - response_bytes {
            return Err(response_limit_error());
        }
        response_bytes += chunk.len();
        // Recheck only the suffix that could begin a delimiter split across chunks.
        let scan_offset = buffer.len().saturating_sub(3);
        buffer.extend_from_slice(&chunk);
        if let Some(message) = parse_sse_buffer(&mut buffer, scan_offset, request_id)? {
            return Ok(message);
        }
    }
    Err(ToolError::Execution(
        "MCP SSE stream ended before the request response arrived".to_string(),
    ))
}

fn parse_sse_buffer(
    buffer: &mut Vec<u8>,
    mut scan_offset: usize,
    request_id: Option<&str>,
) -> Result<Option<Value>, ToolError> {
    while let Some(relative_end) = find_sse_event_boundary(&buffer[scan_offset..]) {
        let end = scan_offset + relative_end;
        let event = std::str::from_utf8(&buffer[..end])
            .map_err(|_| ToolError::Execution("MCP SSE frame is not valid UTF-8".to_string()))?;
        let message = parse_sse_event(event)?;
        buffer.drain(..end);
        scan_offset = 0;
        if let Some(message) = message {
            if request_id.is_none() || message.get("id").and_then(Value::as_str) == request_id {
                return Ok(Some(message));
            }
        }
    }
    Ok(None)
}

fn find_sse_event_boundary(buffer: &[u8]) -> Option<usize> {
    for index in 0..buffer.len() {
        match buffer[index..] {
            [b'\r', b'\n', b'\r', b'\n', ..] => return Some(index + 4),
            [b'\n', b'\n', ..] => return Some(index + 2),
            _ => {}
        }
    }
    None
}

fn parse_sse_event(event: &str) -> Result<Option<Value>, ToolError> {
    let mut data_lines = Vec::new();
    for line in event.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let trimmed = data.trim();
            if trimmed == "[DONE]" {
                return Ok(None);
            }
            data_lines.push(trimmed);
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let joined = data_lines.join("\n");
    serde_json::from_str(&joined)
        .map(Some)
        .map_err(|_| ToolError::Execution("failed to parse MCP SSE data: invalid JSON".to_string()))
}

fn extract_jsonrpc_result(message: Value, method: &str) -> Result<Value, ToolError> {
    if let Some(error) = message.get("error") {
        return Err(ToolError::Execution(format!(
            "MCP `{method}` failed: {}",
            jsonrpc_error_message(error)
        )));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_session_metadata(result: &Value) -> McpSessionMetadata {
    McpSessionMetadata {
        protocol_version: result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        server_info: result.get("serverInfo").cloned(),
    }
}

fn parse_content_length(line: &str) -> Result<usize, ToolError> {
    line.split_once(':')
        .map(|(_, value)| value.trim())
        .ok_or_else(|| ToolError::Execution("invalid MCP content-length header".to_string()))?
        .parse::<usize>()
        .tool_err("invalid MCP content length")
}

#[cfg(test)]
mod tests {
    use super::{StdioMcpChild, StdioMcpProcess, StdioMcpProcessStarter, StdioMcpSession};
    use crate::UnwrapOrAbort;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Mutex;

    struct FakeStdioMcpChild;

    #[async_trait]
    impl StdioMcpChild for FakeStdioMcpChild {
        async fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }

        async fn wait(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FakeStdioMcpStart {
        server_id: String,
        command: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
    }

    struct FakeStdioMcpStarter {
        started: Mutex<Vec<FakeStdioMcpStart>>,
    }

    impl FakeStdioMcpStarter {
        fn new() -> Self {
            Self {
                started: Mutex::new(Vec::new()),
            }
        }
    }

    impl StdioMcpProcessStarter for FakeStdioMcpStarter {
        fn start(
            &self,
            server_id: &str,
            command: &[String],
            env: &BTreeMap<String, String>,
            cwd: Option<&PathBuf>,
        ) -> Result<StdioMcpProcess, harness_core::tool::ToolError> {
            self.started
                .lock()
                .unwrap_or_abort()
                .push(FakeStdioMcpStart {
                    server_id: server_id.to_string(),
                    command: command.to_vec(),
                    env: env.clone(),
                    cwd: cwd.cloned(),
                });
            Ok(StdioMcpProcess {
                child: Box::new(FakeStdioMcpChild),
                stdin: Box::new(Vec::<u8>::new()),
                stdout: Box::new(std::io::Cursor::new(mcp_stdio_startup_responses())),
            })
        }
    }

    fn mcp_stdio_startup_responses() -> Vec<u8> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "protocolVersion": "2025-06-18",
                "serverInfo": {
                    "name": "fake-mcp",
                    "version": "1.0.0"
                }
            }
        });
        let mut bytes = serde_json::to_vec(&body).unwrap_or_abort();
        bytes.push(b'\n');
        bytes
    }

    fn session_with_output(output: &str) -> StdioMcpSession {
        StdioMcpSession {
            child: Box::new(FakeStdioMcpChild),
            stdin: Box::new(Vec::<u8>::new()),
            stdout: tokio::io::BufReader::new(Box::new(io::Cursor::new(
                output.as_bytes().to_vec(),
            ))),
            next_id: 1,
            timeout: std::time::Duration::from_secs(1),
            metadata: super::McpSessionMetadata::default(),
        }
    }

    #[tokio::test]
    async fn stdio_framed_message_skips_extra_headers_and_preserves_next_message() {
        let mut session = session_with_output(
            "\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}\n[]\n",
        );

        let first = session.read_message().await.unwrap_or_abort();
        let second = session.read_message().await.unwrap_or_abort();

        assert_eq!(
            (first, second),
            (serde_json::json!({}), serde_json::json!([]))
        );
    }

    #[tokio::test]
    async fn stdio_framed_message_rejects_eof_before_header_separator() {
        let mut session = session_with_output("Content-Length: 2\n");

        let error = session.read_message().await.expect_err("header EOF");

        assert!(matches!(
            error,
            harness_core::tool::ToolError::Execution(message)
                if message == "MCP stdio server closed before message body"
        ));
    }

    #[tokio::test]
    async fn stdio_framed_message_rejects_truncated_body() {
        let mut session = session_with_output("Content-Length: 2\n\n{");

        let error = session.read_message().await.expect_err("body EOF");

        assert!(matches!(
            error,
            harness_core::tool::ToolError::Execution(message)
                if message.starts_with("failed to read MCP message body:")
        ));
    }

    #[tokio::test]
    async fn stdio_response_and_header_limits_are_inclusive() {
        const LIMIT: usize = 16 * 1024 * 1024;
        let framed = format!(
            "Content-Length: {LIMIT}\r\n\r\n\"{}\"",
            "x".repeat(LIMIT - 2)
        );
        let line = format!("\"{}\"\n", "x".repeat(LIMIT - 3));
        let header = "Content-Length: 2\r\nX-Fixture: ";
        let headers = format!(
            "{header}{}\r\n\r\n{{}}",
            "x".repeat(8192 - header.len() - 4)
        );
        for input in [framed, line, headers] {
            session_with_output(&input)
                .read_message()
                .await
                .unwrap_or_abort();
        }
        for (input, expected) in [
            (
                format!("Content-Length: {}\r\n", LIMIT + 1),
                "MCP response exceeded 16777216-byte limit",
            ),
            (
                format!("Content-Length: 2{}\n", " ".repeat(8192)),
                "MCP header exceeded 8192-byte limit",
            ),
            (
                format!("Content-Length: 2\r\nX-Fixture: {}", "x".repeat(8192)),
                "MCP header exceeded 8192-byte limit",
            ),
            (
                "x".repeat(LIMIT + 1),
                "MCP response exceeded 16777216-byte limit",
            ),
        ] {
            let error = session_with_output(&input)
                .read_message()
                .await
                .expect_err("oversized input");
            assert!(
                matches!(error, harness_core::tool::ToolError::Execution(message) if message == expected)
            );
        }
    }

    #[test]
    fn sse_frames_preserve_utf8_and_delimiter_order() {
        let expected = serde_json::json!({"id":"1", "result":"sécond 😀"});
        for (first, second) in [("\r\n", "\n"), ("\n", "\r\n")] {
            let input = format!(
                ": heartbeat{first}{first}data: {{\"method\":\"notifications/progress\"}}{first}{first}\
                 data: {{\"id\":\"other\",\"result\":0}}{second}{second}\
                 data: {{\"id\":\"1\",{second}data: \"result\":\"sécond 😀\"}}{second}{second}"
            );
            for chunk_size in 1..=input.len() {
                let mut buffer = Vec::new();
                let message = input.as_bytes().chunks(chunk_size).find_map(|chunk| {
                    let scan_offset = buffer.len().saturating_sub(3);
                    buffer.extend_from_slice(chunk);
                    super::parse_sse_buffer(&mut buffer, scan_offset, Some("1")).unwrap_or_abort()
                });
                assert_eq!(message, Some(expected.clone()), "chunk size {chunk_size}");
            }
        }
    }

    #[tokio::test]
    async fn stdio_mcp_session_start_can_use_injected_process_starter_without_spawning() {
        let starter = FakeStdioMcpStarter::new();
        let command = vec!["fake-mcp".to_string(), "--stdio".to_string()];
        let env = BTreeMap::from([("TOKEN".to_string(), "redacted".to_string())]);
        let cwd = PathBuf::from("/tmp/fake-mcp-root");

        let session = StdioMcpSession::start_with_starter(
            "fake-server",
            &command,
            &env,
            Some(&cwd),
            1,
            &starter,
        )
        .await
        .unwrap_or_abort();

        assert_eq!(session.next_id, 2);
        assert_eq!(
            session.metadata.protocol_version.as_deref(),
            Some("2025-06-18")
        );
        assert_eq!(
            session
                .metadata
                .server_info
                .as_ref()
                .and_then(|info| info.get("name"))
                .and_then(Value::as_str),
            Some("fake-mcp")
        );
        let started = starter.started.lock().unwrap_or_abort();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].server_id, "fake-server");
        assert_eq!(started[0].command, command);
        assert_eq!(started[0].env, env);
        assert_eq!(started[0].cwd.as_ref(), Some(&cwd));
    }
}
