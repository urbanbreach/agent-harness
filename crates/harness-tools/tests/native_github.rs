use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use harness_core::clock::RealClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolError};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

const GITHUB_API_BASE_URL_ENV: &str = "HARNESS_GITHUB_API_BASE_URL";
const GITHUB_TOKEN_ENV: &str = "HARNESS_GITHUB_TOKEN";
const GITHUB_REPOSITORY_ENV: &str = "HARNESS_GITHUB_REPOSITORY";

#[derive(Debug, Clone)]
struct TestRequest {
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

#[derive(Debug, Clone)]
struct TestResponse {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: String,
    delay: Duration,
}

fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: "run-native-github-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp_dir.path().join("workspace")).expect("workspace");
    temp_dir
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    previous: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(entries: &[(&'static str, Option<&str>)]) -> Self {
        let previous = entries
            .iter()
            .map(|(key, value)| {
                let previous = std::env::var(key).ok();
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
                (*key, previous)
            })
            .collect();
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn spawn_http_server(
    handler: Arc<dyn Fn(TestRequest) -> TestResponse + Send + Sync + 'static>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test http server");
    let addr = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let request = read_request(&mut stream);
            let response = handler(request);
            if !response.delay.is_zero() {
                thread::sleep(response.delay);
            }

            let mut header_text = format!("HTTP/1.1 {}\r\nConnection: close\r\n", response.status);
            let has_content_length = response
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
            if !has_content_length {
                header_text.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
            }
            for (name, value) in &response.headers {
                header_text.push_str(&format!("{name}: {value}\r\n"));
            }
            header_text.push_str("\r\n");

            let _ = stream.write_all(header_text.as_bytes());
            let _ = stream.write_all(response.body.as_bytes());
        }
    });
    format!("http://{addr}")
}

fn read_request(stream: &mut std::net::TcpStream) -> TestRequest {
    let mut bytes = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut buf = [0_u8; 1024];
        let read = stream.read(&mut buf).expect("read request bytes");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..read]);
        if let Some(headers_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_text = String::from_utf8_lossy(&bytes[..headers_end + 4]);
            content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let body_start = headers_end + 4;
            while bytes.len() < body_start + content_length {
                let read = stream.read(&mut buf).expect("read request body");
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..read]);
            }
            break;
        }
    }

    parse_request(&bytes, content_length)
}

fn parse_request(bytes: &[u8], content_length: usize) -> TestRequest {
    let headers_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(bytes.len());
    let header_text = String::from_utf8_lossy(&bytes[..headers_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let body = String::from_utf8_lossy(
        bytes
            .get(headers_end..headers_end.saturating_add(content_length))
            .unwrap_or_default(),
    )
    .to_string();
    TestRequest {
        path,
        headers,
        body,
    }
}

fn expect_execution_error(error: ToolError, expected: &str) {
    match error {
        ToolError::Execution(message) => assert!(
            message.contains(expected),
            "expected execution error containing {expected:?}, got {message:?}"
        ),
        other => panic!("expected execution error, got {other:?}"),
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_issue_get_uses_env_repository_and_auth_headers() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let requests = Arc::new(Mutex::new(Vec::<TestRequest>::new()));
    let request_log = Arc::clone(&requests);
    let base_url = spawn_http_server(Arc::new(move |request| {
        request_log.lock().expect("request log").push(request);
        TestResponse {
            status: "200 OK",
            headers: vec![(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            )],
            body: json!({
                "number": 19,
                "title": "Add first-class GitHub issue and PR integration",
                "state": "open",
                "body": "Tracked body",
                "html_url": "https://github.com/urbanbreach/agent-harness/issues/19"
            })
            .to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _env = EnvGuard::set(&[
        (GITHUB_API_BASE_URL_ENV, Some(base_url.as_str())),
        (GITHUB_TOKEN_ENV, Some("fixture-token")),
        (GITHUB_REPOSITORY_ENV, Some("urbanbreach/agent-harness")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("github.issue").expect("github.issue tool");
    let result = tool
        .call(
            test_context(&workspace, "toolcall-github-issue-get"),
            json!({
                "operation": "get",
                "issue_number": 19
            }),
        )
        .await
        .expect("github.issue get");

    assert!(result.display_text.contains("Issue #19"));
    assert!(result
        .display_text
        .contains("Add first-class GitHub issue and PR integration"));
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.pointer("/issue/number")),
        Some(&json!(19))
    );

    let requests = requests.lock().expect("request log");
    let request = requests.first().expect("request captured");
    assert_eq!(request.path, "/repos/urbanbreach/agent-harness/issues/19");
    assert_eq!(
        request.headers.get("authorization"),
        Some(&"Bearer fixture-token".to_string())
    );
    assert_eq!(
        request.headers.get("x-github-api-version"),
        Some(&"2022-11-28".to_string())
    );
    assert!(
        request.body.is_empty(),
        "get request should not include a body"
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_issue_list_filters_pull_requests_and_preserves_query_parameters() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let requests = Arc::new(Mutex::new(Vec::<TestRequest>::new()));
    let request_log = Arc::clone(&requests);
    let base_url = spawn_http_server(Arc::new(move |request| {
        request_log.lock().expect("request log").push(request);
        TestResponse {
            status: "200 OK",
            headers: vec![(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            )],
            body: json!([
                {
                    "number": 7,
                    "title": "Real issue",
                    "state": "closed",
                    "html_url": "https://github.com/urbanbreach/agent-harness/issues/7"
                },
                {
                    "number": 8,
                    "title": "Actually a pull request",
                    "state": "closed",
                    "html_url": "https://github.com/urbanbreach/agent-harness/pull/8",
                    "pull_request": {"url": "https://api.github.com/repos/urbanbreach/agent-harness/pulls/8"}
                }
            ])
            .to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _env = EnvGuard::set(&[
        (GITHUB_API_BASE_URL_ENV, Some(base_url.as_str())),
        (GITHUB_TOKEN_ENV, None),
        (GITHUB_REPOSITORY_ENV, Some("urbanbreach/agent-harness")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("github.issue").expect("github.issue tool");
    let result = tool
        .call(
            test_context(&workspace, "toolcall-github-issue-list"),
            json!({
                "operation": "list",
                "state": "closed",
                "per_page": 2
            }),
        )
        .await
        .expect("github.issue list");

    assert!(result.display_text.contains("Real issue"));
    assert!(!result.display_text.contains("Actually a pull request"));
    let items = result
        .structured_json
        .as_ref()
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .expect("items array");
    assert_eq!(items.len(), 1);

    let requests = requests.lock().expect("request log");
    let request = requests.first().expect("request captured");
    assert_eq!(
        request.path,
        "/repos/urbanbreach/agent-harness/issues?per_page=2&state=closed"
    );
    assert!(
        !request.headers.contains_key("authorization"),
        "read-only list call should not require auth"
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_issue_close_requires_authentication() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let _env = EnvGuard::set(&[
        (GITHUB_API_BASE_URL_ENV, Some("http://127.0.0.1:9")),
        (GITHUB_TOKEN_ENV, None),
        (GITHUB_REPOSITORY_ENV, Some("urbanbreach/agent-harness")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("github.issue").expect("github.issue tool");
    let error = tool
        .call(
            test_context(&workspace, "toolcall-github-issue-close"),
            json!({
                "operation": "close",
                "issue_number": 19
            }),
        )
        .await
        .expect_err("close without auth should fail");
    expect_execution_error(error, "GitHub authentication is required");
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_pull_request_create_posts_expected_payload() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let requests = Arc::new(Mutex::new(Vec::<TestRequest>::new()));
    let request_log = Arc::clone(&requests);
    let base_url = spawn_http_server(Arc::new(move |request| {
        request_log.lock().expect("request log").push(request);
        TestResponse {
            status: "201 Created",
            headers: vec![(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            )],
            body: json!({
                "number": 42,
                "title": "Add GitHub tool docs",
                "state": "open",
                "html_url": "https://github.com/urbanbreach/agent-harness/pull/42",
                "body": "This adds docs.",
                "head": {"ref": "feature/github-docs"},
                "base": {"ref": "main"}
            })
            .to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _env = EnvGuard::set(&[
        (GITHUB_API_BASE_URL_ENV, Some(base_url.as_str())),
        (GITHUB_TOKEN_ENV, Some("fixture-token")),
        (GITHUB_REPOSITORY_ENV, Some("urbanbreach/agent-harness")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry
        .get("github.pull_request")
        .expect("github.pull_request tool");
    let result = tool
        .call(
            test_context(&workspace, "toolcall-github-pr-create"),
            json!({
                "operation": "create",
                "title": "Add GitHub tool docs",
                "body": "This adds docs.",
                "head": "feature/github-docs",
                "base": "main",
                "draft": true
            }),
        )
        .await
        .expect("github.pull_request create");

    assert!(result.display_text.contains("Created pull request #42"));
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.pointer("/pull_request/number")),
        Some(&json!(42))
    );

    let requests = requests.lock().expect("request log");
    let request = requests.first().expect("request captured");
    assert_eq!(request.path, "/repos/urbanbreach/agent-harness/pulls");
    assert_eq!(
        request.headers.get("authorization"),
        Some(&"Bearer fixture-token".to_string())
    );
    let payload: Value = serde_json::from_str(&request.body).expect("request json");
    assert_eq!(payload.get("title"), Some(&json!("Add GitHub tool docs")));
    assert_eq!(payload.get("head"), Some(&json!("feature/github-docs")));
    assert_eq!(payload.get("base"), Some(&json!("main")));
    assert_eq!(payload.get("draft"), Some(&json!(true)));
}
