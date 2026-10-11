use super::*;

pub(super) async fn serve(
    listener: TcpListener,
    seen: Arc<Mutex<Vec<Value>>>,
    notify: Arc<Notify>,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await?;
        let method = line
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        let mut length = 0;
        loop {
            line.clear();
            stream.read_line(&mut line).await?;
            if line == "\r\n" {
                break;
            }
            match line.split_once(':') {
                Some((key, value)) if key.eq_ignore_ascii_case("content-length") => {
                    length = value
                        .trim()
                        .parse::<usize>()
                        .map_err(std::io::Error::other)?;
                }
                _ => {}
            }
        }
        let mut body = vec![0; length];
        stream.read_exact(&mut body).await?;
        let mut sse = false;
        let mut changed_tools = false;
        let (status, body) = if method == "POST" {
            let input: Value = serde_json::from_slice(&body)?;
            seen.lock().await.push(input.clone());
            notify.notify_one();
            sse = input["params"]["name"]
                .as_str()
                .is_some_and(|s| s.ends_with("sse"));
            changed_tools = input["params"]["name"] == "refresh-sse";
            let result = reply(&input);
            if input.get("id").is_some() && input["params"]["name"] != "slow" {
                (
                    "200 OK",
                    json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string(),
                )
            } else {
                ("202 Accepted", String::new())
            }
        } else if method == "DELETE" {
            ("200 OK", String::new())
        } else {
            ("405 Method Not Allowed", String::new())
        };
        let (body, content_type) = if sse {
            (
                format!("event: message\ndata: {body}\n\n"),
                "text/event-stream",
            )
        } else {
            (body, "application/json")
        };
        let body = if changed_tools {
            format!("event: message\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}}\n\n{body}")
        } else {
            body
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nMcp-Session-Id: fixture-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes()).await?;
    }
    #[allow(
        unreachable_code,
        reason = "fixture serves until its owning task is aborted"
    )]
    Ok::<(), std::io::Error>(())
}

fn reply(input: &Value) -> Value {
    match input["method"].as_str().unwrap_or_default() {
        "initialize" => {
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{},"resources":{},"prompts":{}},"serverInfo":{"name":"fixture","version":"1"}})
        }
        "tools/list" if input["params"]["cursor"].is_null() => {
            json!({"tools":[{"name":"echo","inputSchema":{"type":"object"}}],"nextCursor":"second"})
        }
        "tools/list" => {
            let tools: Vec<_> = std::iter::once("other".to_owned())
                .chain((0..130).map(|n| format!("zz_{n:03}")))
                .map(|name| json!({"name":name,"inputSchema":{"type":"object"}}))
                .collect();
            json!({"tools":tools})
        }
        "tools/call"
            if input["params"]["name"]
                .as_str()
                .is_some_and(|s| s.starts_with("large-")) =>
        {
            json!({"content":[{"type":"text","text":"x".repeat(1_048_800)}]})
        }
        "tools/call" if input["params"]["name"] == "structured" => {
            json!({"content":[],"structuredContent":{"status":"structured-only"}})
        }
        "tools/call" if input["params"]["name"] == "image" => {
            json!({"content":[{"type":"image","mimeType":"image/png","data":IMAGE}]})
        }
        "tools/call" => {
            json!({"content":[{"type":"text","text":"answer opaque-mcp-fixture"}],"structuredContent":{"received":input["params"]["arguments"]},"isError":false})
        }
        "resources/list" => {
            json!({"resources":[{"name":"notes","uri":"fixture://notes"}]})
        }
        "resources/read" if input["params"]["uri"] == "fixture://image" => {
            json!({"contents":[{"uri":"fixture://image","mimeType":"image/png","blob":IMAGE}]})
        }
        "resources/read" => {
            json!({"contents":[{"uri":"fixture://notes","text":"resource body"}]})
        }
        "prompts/list" => json!({"prompts":[{"name":"review"}]}),
        "prompts/get" if input["params"]["name"] == "image" => {
            json!({"messages":[{"role":"user","content":{"type":"image","mimeType":"image/png","data":IMAGE}}]})
        }
        "prompts/get" => {
            json!({"messages":[{"role":"user","content":{"type":"text","text":"Review this change."}}]})
        }
        _ => json!({}),
    }
}
