use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
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

#[tokio::test]
async fn github_operations_keep_auth_policy_bodies_and_pagination_at_the_http_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/api/v3", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let started = Arc::new(Semaphore::new(0));
    let fixture = Server(tokio::spawn(serve(
        listener,
        Arc::clone(&requests),
        Arc::clone(&started),
    )));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_github_tools(&mut registry, &|key| match key {
        "HARNESS_GITHUB_API_BASE_URL" => Some(base.clone()),
        "HARNESS_GITHUB_TOKEN" => Some("opaque-github-credential".into()),
        "HARNESS_GITHUB_REPOSITORY" => Some("acme/project".into()),
        _ => None,
    });
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "github.issue".into(),
            pattern: "acme/denied:*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("github", root.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    assert!(
        requests.lock().await.is_empty(),
        "registration must stay offline"
    );
    for args in [
        json!({"operation":"list","owner":"ACME","repo":"DENIED"}),
        json!({"operation":"get","owner":"../escape","repo":"project","issue_number":1}),
        json!({"operation":"get","issue_number":0}),
        json!({"operation":"comment","issue_number":1,"body":" "}),
        json!({"operation":"create","title":"invalid for issues"}),
        json!({"operation":"list","extra":true}),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor(), None, "github.issue", args)
            .await
            .is_err());
    }
    assert!(requests.lock().await.is_empty());
    for (id, args, method, path) in [
        (
            "github.issue",
            json!({"operation":"get","issue_number":7}),
            "GET",
            "issues/7",
        ),
        (
            "github.issue",
            json!({"operation":"list","per_page":500,"page":2,"state":"all"}),
            "GET",
            "issues?state=all&per_page=100&page=2",
        ),
        (
            "github.issue",
            json!({"operation":"comment","issue_number":7,"body":"line one\nline two"}),
            "POST",
            "issues/7/comments",
        ),
        (
            "github.issue",
            json!({"operation":"close","issue_number":7}),
            "PATCH",
            "issues/7",
        ),
        (
            "github.issue",
            json!({"operation":"reopen","issue_number":7}),
            "PATCH",
            "issues/7",
        ),
        (
            "github.pull_request",
            json!({"operation":"get","pull_number":8}),
            "GET",
            "pulls/8",
        ),
        (
            "github.pull_request",
            json!({"operation":"list"}),
            "GET",
            "pulls?state=open&per_page=20&page=1",
        ),
        (
            "github.pull_request",
            json!({"operation":"comment","pull_number":8,"body":"review"}),
            "POST",
            "issues/8/comments",
        ),
        (
            "github.pull_request",
            json!({"operation":"create","title":"Change","head":"feature","base":"main","body":"details","draft":true}),
            "POST",
            "pulls",
        ),
    ] {
        let result = handle
            .execute_agent_tool_call(actor(), None, id, args.clone())
            .await?;
        assert!(!result.is_error());
        assert!(!result.display_text.contains("opaque-github-credential"));
        let metadata = result.structured_json.ok_or("missing GitHub metadata")?;
        if args["operation"] == "list" {
            assert_eq!(
                metadata["items"].as_array().ok_or("missing list")?.len(),
                if id == "github.issue" { 1 } else { 2 }
            );
            assert_eq!(metadata["has_more"], true);
        }
        let observed = requests
            .lock()
            .await
            .last()
            .cloned()
            .ok_or("no HTTP request")?;
        assert_eq!(observed["method"], method);
        assert_eq!(
            observed["path"],
            format!("/api/v3/repos/acme/project/{path}")
        );
        assert_eq!(
            observed["headers"]["authorization"],
            "Bearer opaque-github-credential"
        );
        assert_eq!(observed["headers"]["accept"], "application/vnd.github+json");
        assert_eq!(observed["headers"]["x-github-api-version"], "2022-11-28");
        match args["operation"].as_str() {
            Some("comment") => assert_eq!(observed["body"], json!({"body":args["body"]})),
            Some("close") => assert_eq!(observed["body"], json!({"state":"closed"})),
            Some("reopen") => assert_eq!(observed["body"], json!({"state":"open"})),
            Some("create") => assert_eq!(
                observed["body"],
                json!({"title":"Change","head":"feature","base":"main","body":"details","draft":true})
            ),
            _ => assert!(observed["body"].is_null()),
        }
    }
    for repo in ["redirect", "limits", "invalid", "fail"] {
        assert!(handle
            .execute_agent_tool_call(
                actor(),
                None,
                "github.issue",
                json!({"operation":"list","owner":"acme","repo":repo})
            )
            .await
            .is_err());
    }
    assert_eq!(
        requests.lock().await.len(),
        13,
        "do not retry mutations or follow redirects"
    );
    let task = handle
        .request_tool_call(
            actor(),
            None,
            "github.issue",
            json!({"operation":"get","owner":"acme","repo":"slow","issue_number":1}),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(2), started.acquire())
        .await??
        .forget();
    handle.cancel_task(task, "cancel GitHub request").await?;
    tokio::time::timeout(Duration::from_secs(2), handle.stop_run()).await??;
    let journal = std::fs::read_to_string(run.events_path)?;
    assert!(
        !journal.contains("opaque-github-credential") && !journal.contains("PRIVATE_ERROR_BODY")
    );

    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_github_tools(&mut registry, &|key| {
        (key == "HARNESS_GITHUB_API_BASE_URL").then(|| base.clone())
    });
    let mut config = CoordinatorConfig::new(root.path().join("anonymous"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let anonymous = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    anonymous.start_run("public", root.path()).await?;
    assert!(anonymous.execute_agent_tool_call(actor(),None,"github.issue",json!({"operation":"comment","owner":"acme","repo":"project","issue_number":1,"body":"no"})).await.is_err_and(|e|e.contains("authentication")));
    anonymous
        .execute_agent_tool_call(
            actor(),
            None,
            "github.issue",
            json!({"operation":"get","owner":"acme","repo":"project","issue_number":1}),
        )
        .await?;
    anonymous.stop_run().await?;
    assert_eq!(requests.lock().await.len(), 15);
    assert!(requests
        .lock()
        .await
        .last()
        .ok_or("missing public request")?["headers"]["authorization"]
        .is_null());
    drop(fixture);
    Ok(())
}

struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(
    listener: TcpListener,
    requests: Arc<Mutex<Vec<Value>>>,
    started: Arc<Semaphore>,
) -> std::io::Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let mut words = line.split_whitespace();
        let method = words.next().unwrap_or_default().to_owned();
        let path = words.next().unwrap_or_default().to_owned();
        let mut headers = serde_json::Map::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await? == 0 || line == "\r\n" {
                break;
            }
            if let Some((key, value)) = line.split_once(':') {
                headers.insert(key.to_ascii_lowercase(), value.trim().into());
            }
        }
        let size = headers
            .get("content-length")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let mut bytes = vec![0; size];
        reader.read_exact(&mut bytes).await?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        };
        requests
            .lock()
            .await
            .push(json!({"method":method,"path":path,"headers":headers,"body":body}));
        let mut socket = reader.into_inner();
        if path.contains("/slow/") {
            started.add_permits(1);
            let _ = socket.read_u8().await;
            continue;
        }
        if path.contains("/limits/") {
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 3000000\r\nConnection: close\r\n\r\n",
                )
                .await?;
            continue;
        }
        let (status, extra, response) = if path.contains("/redirect/") {
            ("302 Found", "Location: /leaked\r\n", json!({}))
        } else if path.contains("/fail/") {
            ("403 Forbidden", "", json!({"message":"PRIVATE_ERROR_BODY"}))
        } else if path.contains("/invalid/") {
            ("200 OK", "", json!({}))
        } else if path.contains('?') {
            (
                "200 OK",
                "Link: </next>; rel=\"next\"\r\n",
                json!([{"number":7,"title":"Issue","body":"opaque-github-credential"},{"number":8,"title":"Pull","pull_request":{}}]),
            )
        } else {
            (
                "200 OK",
                "",
                json!({"number":7,"title":"Response","body":"opaque-github-credential","html_url":"https://github.com/acme/project/issues/7"}),
            )
        };
        let body = response.to_string();
        socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
    }
}
