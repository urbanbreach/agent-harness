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

const REMOTE_SEARCH_ENDPOINT_ENV: &str = "HARNESS_REMOTE_SEARCH_ENDPOINT";
const REMOTE_SEARCH_AUTH_TOKEN_ENV: &str = "HARNESS_REMOTE_SEARCH_AUTH_TOKEN";
const REMOTE_SEARCH_REQUIRE_AUTH_ENV: &str = "HARNESS_REMOTE_SEARCH_REQUIRE_AUTH";
const REMOTE_SEARCH_TIMEOUT_SECS_ENV: &str = "HARNESS_REMOTE_SEARCH_TIMEOUT_SECS";
const REMOTE_SEARCH_MAX_RETRIES_ENV: &str = "HARNESS_REMOTE_SEARCH_MAX_RETRIES";
const REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV: &str = "HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS";
const EMPTY_CODE_SEARCH_MESSAGE: &str = "No code snippets or documentation found. Please try a different query, be more specific about the library or programming concept, or check the spelling of framework names.";

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
        run_id: "run-native-code-search-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
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
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_code_search_uses_shared_client_and_respects_tokens_contract() {
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
                "text/event-stream; charset=utf-8".to_string(),
            )],
            body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Tokio JoinSet examples\\nspawn multiple tasks\"}]}}\n\n".to_string(),
            delay: Duration::ZERO,
        }
    }));
    let _search_env = EnvGuard::set(&[
        (REMOTE_SEARCH_ENDPOINT_ENV, Some(base_url.as_str())),
        (REMOTE_SEARCH_AUTH_TOKEN_ENV, Some("fixture-token")),
        (REMOTE_SEARCH_REQUIRE_AUTH_ENV, Some("1")),
        (REMOTE_SEARCH_TIMEOUT_SECS_ENV, Some("5")),
        (REMOTE_SEARCH_MAX_RETRIES_ENV, Some("0")),
        (REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV, Some("1")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let codesearch = registry.get("codesearch").expect("codesearch tool");

    let min_result = codesearch
        .call(
            test_context(&workspace, "codesearch-min"),
            json!({
                "query": "Tokio JoinSet rust example",
                "tokensNum": 25,
            }),
        )
        .await
        .expect("codesearch min clamp");
    let default_result = codesearch
        .call(
            test_context(&workspace, "codesearch-default"),
            json!({
                "query": "Tokio JoinSet rust example default"
            }),
        )
        .await
        .expect("codesearch default");
    let max_result = codesearch
        .call(
            test_context(&workspace, "codesearch-max"),
            json!({
                "query": "Tokio JoinSet rust example max",
                "tokensNum": 90_000,
            }),
        )
        .await
        .expect("codesearch max clamp");

    assert_eq!(
        min_result.display_text,
        "Tokio JoinSet examples\nspawn multiple tasks"
    );
    assert_eq!(min_result.display_text, default_result.display_text);
    assert_eq!(min_result.display_text, max_result.display_text);

    let min_json = min_result.structured_json.expect("min structured json");
    let default_json = default_result
        .structured_json
        .expect("default structured json");
    let max_json = max_result.structured_json.expect("max structured json");
    assert_eq!(min_json["tokensNum"], json!(1000));
    assert_eq!(default_json["tokensNum"], json!(5000));
    assert_eq!(max_json["tokensNum"], json!(50000));
    assert_eq!(min_json["empty"], json!(false));
    assert_eq!(default_json["empty"], json!(false));
    assert_eq!(max_json["empty"], json!(false));

    let requests = requests.lock().expect("request log");
    assert_eq!(
        requests.len(),
        3,
        "codesearch should hit the shared backend path for each request"
    );
    for (request, (query, tokens_num)) in requests.iter().zip([
        ("Tokio JoinSet rust example", 1000),
        ("Tokio JoinSet rust example default", 5000),
        ("Tokio JoinSet rust example max", 50000),
    ]) {
        assert_eq!(request.path, "/");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer fixture-token")
        );
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("application/json, text/event-stream")
        );
        let payload: Value = serde_json::from_str(&request.body).expect("jsonrpc payload");
        assert_eq!(payload["method"], json!("tools/call"));
        assert_eq!(payload["params"]["name"], json!("get_code_context_exa"));
        assert_eq!(payload["params"]["arguments"]["query"], json!(query));
        assert_eq!(
            payload["params"]["arguments"]["tokensNum"],
            json!(tokens_num)
        );
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide search env mutation across awaits"
)]
async fn native_code_search_handles_timeout_and_empty_context_cleanly() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let timeout_url = spawn_http_server(Arc::new(move |_request| TestResponse {
        status: "200 OK",
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        )],
        body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"slow result\"}]}}\n\n"
            .to_string(),
        delay: Duration::from_secs(2),
    }));
    let _timeout_env = EnvGuard::set(&[
        (REMOTE_SEARCH_ENDPOINT_ENV, Some(timeout_url.as_str())),
        (REMOTE_SEARCH_AUTH_TOKEN_ENV, Some("fixture-token")),
        (REMOTE_SEARCH_REQUIRE_AUTH_ENV, Some("1")),
        (REMOTE_SEARCH_TIMEOUT_SECS_ENV, Some("1")),
        (REMOTE_SEARCH_MAX_RETRIES_ENV, Some("0")),
        (REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV, Some("1")),
    ]);
    let timeout_registry = coordinator_registry(ShellAllowlist::default());
    let timeout_error = timeout_registry
        .get("codesearch")
        .expect("codesearch tool")
        .call(
            test_context(&workspace, "timeout-code-search"),
            json!({
                "query": "Tokio JoinSet timeout"
            }),
        )
        .await
        .expect_err("timeout should fail deterministically");
    expect_execution_error(timeout_error, "Code search request timed out");
    drop(_timeout_env);

    let empty_url = spawn_http_server(Arc::new(move |_request| TestResponse {
        status: "200 OK",
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream; charset=utf-8".to_string(),
        )],
        body: "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"   \"}]}}\n\n"
            .to_string(),
        delay: Duration::ZERO,
    }));
    let _empty_env = EnvGuard::set(&[
        (REMOTE_SEARCH_ENDPOINT_ENV, Some(empty_url.as_str())),
        (REMOTE_SEARCH_AUTH_TOKEN_ENV, None),
        (REMOTE_SEARCH_REQUIRE_AUTH_ENV, Some("0")),
        (REMOTE_SEARCH_TIMEOUT_SECS_ENV, Some("5")),
        (REMOTE_SEARCH_MAX_RETRIES_ENV, Some("0")),
        (REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV, Some("1")),
    ]);
    let empty_registry = coordinator_registry(ShellAllowlist::default());
    let empty_result = empty_registry
        .get("codesearch")
        .expect("codesearch tool")
        .call(
            test_context(&workspace, "empty-code-search"),
            json!({
                "query": "no matches fixture"
            }),
        )
        .await
        .expect("empty context should be handled");
    assert_eq!(empty_result.display_text, EMPTY_CODE_SEARCH_MESSAGE);
    let empty_json = empty_result.structured_json.expect("empty structured json");
    assert_eq!(empty_json["query"], json!("no matches fixture"));
    assert_eq!(empty_json["tokensNum"], json!(5000));
    assert_eq!(empty_json["empty"], json!(true));
}
