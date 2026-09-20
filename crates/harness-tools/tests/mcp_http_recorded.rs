//! Recorded loopback HTTP evidence through the public MCP tool registry.
//! Pure byte-framing cases remain in the deterministic mcp_session unit tests.

use std::collections::BTreeMap;
use std::time::Duration;

use harness_core::config::{McpConfig, McpServerConfig, ShellAllowlist};
use harness_core::tool::{ToolError, ToolResult};
use harness_tools::{coordinator_registry_with_mcp, UnwrapOrAbort};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

mod common;

async fn call_http_fixture(
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
) -> Result<ToolResult, ToolError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_abort();
    let endpoint = format!("http://{}", listener.local_addr().unwrap_or_abort());
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap_or_abort();
            let mut stream = tokio::io::BufReader::new(stream);
            let mut body_length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(
                    stream.read_line(&mut line).await.unwrap_or_abort(),
                    0,
                    "MCP client closed before sending request headers"
                );
                if line == "\r\n" {
                    break;
                }
                if let Some(length) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    body_length = length.trim().parse::<usize>().unwrap_or_abort();
                }
            }
            let mut request = vec![0; body_length];
            stream.read_exact(&mut request).await.unwrap_or_abort();
            let request: Value = serde_json::from_slice(&request).unwrap_or_abort();
            let method = request["method"].as_str().unwrap_or_abort();
            let mut stream = stream.into_inner();
            if method == "notifications/initialized" {
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap_or_abort();
                continue;
            }
            let is_tool_call = method == "tools/call";
            let handshake = serde_json::to_vec(&json!({
                "jsonrpc": "2.0", "id": request["id"],
                "result": if method == "initialize" {
                    json!({"protocolVersion":"2025-06-18", "capabilities":{}})
                } else {
                    json!({"tools":[]})
                },
            }))
            .unwrap_or_abort();
            let (status, content_type, bytes) = if is_tool_call {
                (status, content_type, body.as_slice())
            } else {
                ("200 OK", "application/json", handshake.as_slice())
            };
            stream
                .write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n").as_bytes())
                .await
                .unwrap_or_abort();
            for chunk in bytes.chunks(8192) {
                if stream.write_all(chunk).await.is_err() {
                    break;
                }
            }
            if is_tool_call {
                return;
            }
        }
    });
    let config = McpConfig {
        servers: BTreeMap::from([(
            "fixture".to_string(),
            McpServerConfig::Http {
                endpoint,
                headers: BTreeMap::new(),
                timeout_secs: 10,
                enabled: true,
            },
        )]),
    };
    // Registry discovery is synchronous; let the loopback server keep running.
    let registry = tokio::task::spawn_blocking(move || {
        coordinator_registry_with_mcp(ShellAllowlist::default(), config)
    })
    .await
    .unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let result = registry
        .get("mcp.fixture.tool.call")
        .unwrap_or_abort()
        .call(
            common::test_context(temp.path(), "run-mcp-http", "call-mcp-http"),
            json!({"tool":"echo", "arguments":{}}),
        )
        .await;
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    result
}

#[tokio::test]
async fn http_response_limits_cover_success_errors_and_sse() {
    const LIMIT: usize = 16 * 1024 * 1024;
    for (status, content_type, extra) in [
        ("200 OK", "application/json", 0),
        ("200 OK", "application/json", 1),
        ("500 Internal Server Error", "text/plain", 1),
        ("200 OK", "text/event-stream", 0),
        ("200 OK", "text/event-stream", 1),
    ] {
        let (prefix, suffix, padding) = if content_type == "text/event-stream" {
            (
                "data: {\"id\":\"2\",\"result\":{},\"padding\":\"",
                "\"}\n\n",
                b'x',
            )
        } else {
            ("{\"id\":\"2\",\"result\":{}}", "", b' ')
        };
        let mut body = prefix.as_bytes().to_vec();
        body.resize(LIMIT + extra - suffix.len(), padding);
        body.extend_from_slice(suffix.as_bytes());
        let result = call_http_fixture(status, content_type, body).await;
        if extra == 0 {
            assert_eq!(
                result.unwrap_or_abort().display_text,
                "MCP tool returned no content"
            );
        } else {
            assert!(matches!(result, Err(ToolError::Execution(message))
                if message == "MCP response exceeded 16777216-byte limit"));
        }
    }
}

#[tokio::test]
async fn sse_rejects_invalid_payloads_and_incomplete_or_unmatched_eof() {
    for (body, expected_error) in [
        (
            &b"data: <html>private-payload</html>\n\n"[..],
            "failed to parse MCP SSE data: invalid JSON",
        ),
        (
            &b"data: {\"id\":\"2\",\"result\":\"private-\xff\"}\n\n"[..],
            "MCP SSE frame is not valid UTF-8",
        ),
        (
            &b"data: {\"id\":\"2\",\"result\":\"private-\xc3"[..],
            "MCP SSE stream ended before the request response arrived",
        ),
        (
            &b"data: {\"method\":\"notifications/progress\"}\n\n"[..],
            "MCP SSE stream ended before the request response arrived",
        ),
    ] {
        let error = call_http_fixture("200 OK", "text/event-stream", body.to_vec())
            .await
            .expect_err("malformed or unmatched SSE");
        assert!(matches!(error, ToolError::Execution(message) if message == expected_error));
    }
}
