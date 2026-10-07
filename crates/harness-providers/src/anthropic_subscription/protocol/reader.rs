//! Stdout reader and control-request answers.
use super::*;

pub(super) struct ReaderContext {
    pub(super) writer: mpsc::UnboundedSender<Write>,
    pub(super) pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>,
    pub(super) messages: mpsc::UnboundedSender<Result<Value, QueryError>>,
    pub(super) closed: Arc<AtomicBool>,
    pub(super) state: Arc<Mutex<RunState>>,
    pub(super) single: bool,
    pub(super) custom_tools: bool,
    pub(super) tools_list: Value,
}

pub(super) async fn read_messages(
    ctx: ReaderContext,
    stdout: tokio::process::ChildStdout,
    mut child: tokio::process::Child,
    stderr_tail: Arc<Mutex<String>>,
) {
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match message.get("type").and_then(Value::as_str) {
            Some("control_response") => {
                let response = &message["response"];
                let id = response["request_id"].as_str().unwrap_or("");
                let reply = ctx.pending.lock().ok().and_then(|mut p| p.remove(id));
                if let Some(reply) = reply {
                    let _ = reply.send(if response["subtype"] == "success" {
                        Ok(response.get("response").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(response["error"]
                            .as_str()
                            .unwrap_or("control request failed")
                            .into())
                    });
                }
                continue;
            }
            Some("control_request") => {
                if let Some(reply) = handle_control_request(&ctx, &message) {
                    let _ = ctx.writer.send(Write::Line(reply.to_string()));
                }
                continue;
            }
            Some("control_cancel_request" | "keep_alive" | "transcript_mirror") => continue,
            Some("result") => {
                let end = ctx.state.lock().map_or(true, |mut state| {
                    state.last_error_result = error_result_text(&message);
                    state.result_received = true;
                    state.session_state.as_deref().is_none_or(|s| s == "idle")
                });
                if end && ctx.single {
                    let _ = ctx.writer.send(Write::End);
                }
            }
            Some("system") if message["subtype"] == "session_state_changed" => {
                let idle = message["state"] == "idle";
                let end = ctx.state.lock().is_ok_and(|mut state| {
                    state.session_state = message["state"].as_str().map(str::to_owned);
                    idle && state.result_received
                });
                if end && ctx.single {
                    let _ = ctx.writer.send(Write::End);
                }
                if message["sdk_host_only"] == true {
                    continue;
                }
            }
            _ => {}
        }
        if ctx.messages.send(Ok(message)).is_err() {
            return;
        }
    }
    let status = child.wait().await;
    if ctx.closed.load(Ordering::SeqCst) {
        return;
    }
    let tail = stderr_tail.lock().map(|t| t.clone()).unwrap_or_default();
    let tail = tail.trim();
    let tail = if tail.is_empty() {
        String::new()
    } else {
        let start = tail.len().saturating_sub(STDERR_TAIL);
        let start = (start..tail.len())
            .find(|i| tail.is_char_boundary(*i))
            .unwrap_or(0);
        format!(". stderr: {}", tail[start..].trim())
    };
    let failure = match status {
        Ok(status) if status.success() => None,
        Ok(status) => Some(match status.code() {
            Some(code) => format!("Claude Code process exited with code {code}{tail}"),
            None => format!(
                "Claude Code process terminated by signal {}{tail}",
                signal_name(&status)
            ),
        }),
        Err(e) => Some(format!("Claude Code process error: {e}")),
    };
    let failure = failure.map(|failure| {
        ctx.state
            .lock()
            .ok()
            .and_then(|state| state.last_error_result.clone())
            .map_or(failure, |text| {
                format!("Claude Code returned an error result: {text}")
            })
    });
    // In-flight control requests (initialize, interrupt) reject with the cleanup cause.
    let pending: Vec<_> = ctx
        .pending
        .lock()
        .map(|mut p| p.drain().collect())
        .unwrap_or_default();
    for (_, reply) in pending {
        let _ = reply.send(Err(failure
            .clone()
            .unwrap_or_else(|| "Query closed before response received".into())));
    }
    if let Some(failure) = failure {
        let _ = ctx.messages.send(Err(QueryError(failure)));
    }
}

#[cfg(unix)]
fn signal_name(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        Some(1) => "SIGHUP".into(),
        Some(2) => "SIGINT".into(),
        Some(9) => "SIGKILL".into(),
        Some(15) => "SIGTERM".into(),
        Some(signal) => format!("SIG{signal}"),
        None => "unknown".into(),
    }
}
#[cfg(not(unix))]
fn signal_name(_status: &std::process::ExitStatus) -> String {
    "unknown".into()
}

fn handle_control_request(ctx: &ReaderContext, message: &Value) -> Option<Value> {
    let request = &message["request"];
    let id = message["request_id"].as_str().unwrap_or("");
    let success = |response: Value| json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": response}});
    let failure = |error: String| json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": error}});
    match request["subtype"].as_str().unwrap_or("") {
        "can_use_tool" => Some(success(json!({
            "behavior": "deny",
            "message": TOOL_EXECUTION_DENIED_MESSAGE,
            "toolUseID": request["tool_use_id"],
        }))),
        "hook_callback" => Some(if request["callback_id"] == "hook_0" {
            success(host_tool_denial_output())
        } else {
            failure(format!(
                "No hook callback found for ID: {}",
                request["callback_id"].as_str().unwrap_or("")
            ))
        }),
        "mcp_message" => {
            let server = request["server_name"].as_str().unwrap_or("");
            if server != CUSTOM_TOOLS_MCP_SERVER_NAME || !ctx.custom_tools {
                return Some(failure(format!("SDK MCP server not found: {server}")));
            }
            Some(success(
                json!({"mcp_response": mcp_response(&request["message"], &ctx.tools_list)}),
            ))
        }
        "elicitation" => Some(success(json!({"action": "decline"}))),
        "request_user_dialog"
        | "remote_tool_call"
        | "remote_plumbing_call"
        | "remote_tools_probe"
        | "remote_tools_reannounce" => None,
        other => Some(failure(format!(
            "Unsupported control request subtype: {other}"
        ))),
    }
}

/// The in-process server's JSON-RPC answer; a notification gets the SDK's empty receipt.
fn mcp_response(message: &Value, tools_list: &Value) -> Value {
    if message.get("method").is_none() || message.get("id").is_none_or(Value::is_null) {
        return json!({"jsonrpc": "2.0", "result": {}, "id": 0});
    }
    let id = message["id"].clone();
    let result = match message["method"].as_str().unwrap_or("") {
        "initialize" => {
            let requested = message["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = if SUPPORTED_MCP_VERSIONS.contains(&requested) {
                requested
            } else {
                MCP_PROTOCOL_VERSION
            };
            mcp_initialize_result(version)
        }
        "tools/list" => tools_list.clone(),
        "tools/call" => json!({
            "content": [{"type": "text", "text": TOOL_EXECUTION_DENIED_MESSAGE}],
            "isError": true,
        }),
        "ping" => json!({}),
        method => {
            return json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Method not found: {method}")}});
        }
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
