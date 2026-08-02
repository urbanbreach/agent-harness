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
    describe_upstream_non_json_response, jsonrpc_error_message, render_mcp_http_parse_error,
    render_mcp_http_status_error,
};
use crate::text::has_trimmed_content;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

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
        let timeout = Duration::from_secs(timeout_secs.max(1));
        let mut session = Self {
            child: process.child,
            stdin: process.stdin,
            stdout: BufReader::new(process.stdout),
            next_id: 1,
            timeout,
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
            .await?;
        session.metadata = parse_session_metadata(&initialize);
        session
            .notify("notifications/initialized", json!({}))
            .await?;
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
            let message = self.read_message().await?;
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

    async fn read_message(&mut self) -> Result<Value, ToolError> {
        loop {
            let mut line = String::new();
            let read = timeout(self.timeout, self.stdout.read_line(&mut line))
                .await
                .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
                .tool_err("failed to read MCP output")?;
            if read == 0 {
                return Err(ToolError::Execution(
                    "MCP stdio server closed the connection".to_string(),
                ));
            }

            if !has_trimmed_content(&line) {
                continue;
            }

            if line.to_ascii_lowercase().starts_with("content-length:") {
                let length = parse_content_length(&line)?;
                loop {
                    let mut header_line = String::new();
                    let header_read =
                        timeout(self.timeout, self.stdout.read_line(&mut header_line))
                            .await
                            .map_err(|_| {
                                ToolError::Execution("MCP stdio read timed out".to_string())
                            })?
                            .map_err(|err| {
                                ToolError::Execution(format!("failed to read MCP header: {err}"))
                            })?;
                    if header_read == 0 {
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
                return serde_json::from_slice(&body).map_err(|err| {
                    ToolError::Execution(format!("failed to parse MCP message body: {err}"))
                });
            }

            return serde_json::from_str(line.trim()).map_err(|err| {
                ToolError::Execution(format!("failed to parse MCP stdio message: {err}"))
            });
        }
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
            let body = response.text().await.unwrap_or_default();
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

        let body = response.text().await.map_err(|err| {
            ToolError::Execution(format!("failed to read MCP HTTP response body: {err}"))
        })?;
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

async fn read_sse_response(
    mut response: reqwest::Response,
    request_id: Option<&str>,
) -> Result<Value, ToolError> {
    let mut buffer = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .tool_err("failed to read MCP SSE chunk")?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = find_sse_event_boundary(&buffer) {
            let event = buffer[..index].to_string();
            let remainder = buffer[index..].trim_start_matches(['\r', '\n']).to_string();
            buffer = remainder;
            if let Some(message) = parse_sse_event(&event)? {
                if request_id.is_none() {
                    return Ok(message);
                }
                let response_id = message.get("id").and_then(Value::as_str);
                if response_id == request_id {
                    return Ok(message);
                }
            }
        }
    }
    Err(ToolError::Execution(
        "MCP SSE stream ended before the request response arrived".to_string(),
    ))
}

fn find_sse_event_boundary(buffer: &str) -> Option<usize> {
    buffer
        .find("\n\n")
        .or_else(|| buffer.find("\r\n\r\n"))
        .map(|index| {
            if buffer[index..].starts_with("\r\n\r\n") {
                index + 4
            } else {
                index + 2
            }
        })
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
    serde_json::from_str(&joined).map(Some).map_err(|err| {
        ToolError::Execution(format!(
            "failed to parse MCP SSE data: {}",
            describe_upstream_non_json_response(&joined).unwrap_or_else(|| err.to_string())
        ))
    })
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

    #[tokio::test]
    async fn stdio_mcp_session_start_can_use_injected_process_starter_without_spawning() {
        // arrange
        // act
        // assert
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
