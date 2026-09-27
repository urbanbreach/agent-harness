use harness_core::{
    clock::FakeClock,
    config::{
        set_registered_integrations_config, IntegrationsConfig, RemoteSearchConfig, ShellAllowlist,
    },
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Mutex, Semaphore},
};
use tokio_stream::StreamExt;

#[tokio::test]
async fn search_uses_approved_queries_and_current_exa_options_without_leaking_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let backoff = Arc::new(Semaphore::new(0));
    let remote = RemoteSearchConfig {
        endpoint: format!("http://{}/mcp", listener.local_addr()?),
        auth_token: Some("opaque-search-secret".into()),
        require_auth: true,
        timeout_secs: 2,
        max_retries: 1,
        retry_backoff_ms: 1,
    };
    set_registered_integrations_config(IntegrationsConfig {
        remote_search: remote.clone(),
        ..Default::default()
    });
    let server = tokio::spawn(serve(listener, Arc::clone(&requests), Arc::clone(&backoff)));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "websearch".into(),
            pattern: "denied*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("search", root.path()).await?;
    let actor = EventActor::new(ActorKind::User, None);
    assert!(
        requests.lock().await.is_empty(),
        "registration must not contact Exa"
    );
    for args in [
        json!({"query":"denied query"}),
        json!({"query":" "}),
        json!({"query":"q","numResults":0}),
        json!({"query":"q","type":"invalid"}),
        json!({"query":"q","extra":true}),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor.clone(), None, "websearch", args)
            .await
            .is_err());
    }
    assert!(requests.lock().await.is_empty());
    for (id, args) in [
        (
            "websearch",
            json!({"query":"Rust async documentation","numResults":3,"type":"fast","livecrawl":"preferred","contextMaxCharacters":5000}),
        ),
        (
            "codesearch",
            json!({"query":"Tokio JoinSet example","tokensNum":1200}),
        ),
    ] {
        let result = handle
            .execute_agent_tool_call(actor.clone(), None, id, args)
            .await?;
        assert!(result.display_text.contains("https://docs.rs/tokio/"));
        assert!(!result.display_text.contains("opaque-search-secret"));
        assert_eq!(
            result.structured_json.as_ref().ok_or("metadata missing")?["provider"],
            "exa"
        );
    }
    let shortened = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "websearch",
            json!({"query":"short","contextMaxCharacters":27}),
        )
        .await?;
    assert!(
        !shortened.display_text.contains("opaque"),
        "redact before truncating a secret: {}",
        shortened.display_text
    );
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "websearch",
            json!({"query":"transient"}),
        )
        .await?;
    let unauthorized = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "websearch",
            json!({"query":"unauthorized"}),
        )
        .await;
    assert!(unauthorized.is_err_and(|error| error.contains("401")));
    let application_error = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "codesearch",
            json!({"query":"application-error"}),
        )
        .await?;
    assert!(application_error.is_error());
    let mut events = handle.subscribe_new_events().await?;
    let pending = handle
        .request_tool_call(actor.clone(), None, "websearch", json!({"query":"backoff"}))
        .await?;
    tokio::time::timeout(Duration::from_secs(2), backoff.acquire())
        .await??
        .forget();
    handle.cancel_task(&pending, "cancel search retry").await?;
    tokio::time::timeout(Duration::from_secs(2),async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload,EventV1::ToolCallFinished(t) if t.tool_call_id.as_str() == pending) {return Ok::<_,Box<dyn std::error::Error>>(());}
        }
        Err("cancelled search did not finish".into())
    }).await??;
    handle.stop_run().await?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_remote_search_tools(
        &mut registry,
        RemoteSearchConfig {
            auth_token: None,
            ..remote
        },
    );
    let mut config = CoordinatorConfig::new(root.path().join("missing-auth"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let unauthenticated = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    unauthenticated.start_run("auth", root.path()).await?;
    assert!(unauthenticated
        .execute_agent_tool_call(actor, None, "websearch", json!({"query":"no credential"}))
        .await
        .is_err_and(|error| error.contains("requires an API key")));
    unauthenticated.stop_run().await?;
    let requests = requests.lock().await;
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    let calls: Vec<_> = requests
        .iter()
        .filter(|r| r["method"] == "tools/call")
        .collect();
    assert_eq!(calls.len(), 8);
    assert_eq!(
        calls
            .iter()
            .filter(|r| r["params"]["arguments"]["query"] == "backoff")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|r| r["params"]["arguments"]["query"] == "transient")
            .count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|r| r["params"]["arguments"]["query"] == "unauthorized")
            .count(),
        1
    );
    assert_eq!(calls[0]["params"]["name"], "web_search_advanced_exa");
    assert_eq!(calls[0]["params"]["arguments"]["numResults"], 3);
    assert_eq!(calls[0]["params"]["arguments"]["type"], "fast");
    assert_eq!(calls[0]["params"]["arguments"]["maxAgeHours"], 0);
    assert_eq!(
        calls[0]["params"]["arguments"]["contextMaxCharacters"],
        5000
    );
    assert_eq!(calls[1]["params"]["arguments"]["textMaxCharacters"], 4800);
    assert!(!std::fs::read_to_string(run.events_path)?.contains("opaque-search-secret"));
    server.abort();
    Ok(())
}

async fn serve(
    listener: TcpListener,
    requests: Arc<Mutex<Vec<Value>>>,
    backoff: Arc<Semaphore>,
) -> std::io::Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await?;
        let post = line.starts_with("POST ");
        let mut length = 0;
        let mut auth = false;
        loop {
            line.clear();
            if socket.read_line(&mut line).await? == 0 || line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => {
                        length = value.trim().parse().map_err(std::io::Error::other)?
                    }
                    "x-api-key" => auth = value.trim() == "opaque-search-secret",
                    _ => {}
                }
            }
        }
        let mut bytes = vec![0; length];
        socket.read_exact(&mut bytes).await?;
        let (status, body) = if post {
            assert!(auth, "search must send the configured x-api-key header");
            let input: Value = serde_json::from_slice(&bytes)?;
            let mut recorded = requests.lock().await;
            let query = &input["params"]["arguments"]["query"];
            let retry = query == "transient"
                && !recorded
                    .iter()
                    .any(|r| r["params"]["arguments"]["query"] == "transient");
            recorded.push(input.clone());
            drop(recorded);
            if retry || query == "unauthorized" || query == "backoff" {
                let status = if retry || query == "backoff" {
                    "503 Service Unavailable"
                } else {
                    "401 Unauthorized"
                };
                let delay = if query == "backoff" { 5 } else { 0 };
                socket.write_all(format!("HTTP/1.1 {status}\r\nRetry-After: {delay}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;
                if query == "backoff" {
                    backoff.add_permits(1);
                }
                continue;
            }
            if input.get("id").is_none() {
                ("202 Accepted", String::new())
            } else {
                let result = if input["method"] == "initialize" {
                    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
                } else if query == "application-error" {
                    json!({"isError":true,"content":[{"type":"text","text":"Search failed"}]})
                } else {
                    json!({"content":[{"type":"text","text":json!({"results":[{"title":"Tokio documentation opaque-search-secret","url":"https://docs.rs/tokio/","text":"JoinSet example"}]}).to_string()}]})
                };
                (
                    "200 OK",
                    json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string(),
                )
            }
        } else {
            ("405 Method Not Allowed", String::new())
        };
        socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
    }
}
