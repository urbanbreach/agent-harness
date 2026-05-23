#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use harness_core::config::PermissionMode;
use harness_core::event::{ActorKind, EventActor};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::ToolError;

mod env_guard;
mod event_log;
mod event_reader;
mod question_events;
pub(crate) mod remote_search_env;
mod repo_root;
mod tool_context;
mod workspace;

#[allow(unused_imports)]
pub use env_guard::EnvGuard;
#[allow(unused_imports)]
pub(crate) use event_log::{
    find_finished, read_events, wait_for_request_terminal, wait_for_succeeded_tool_call_finish,
    wait_for_tool_call_finish,
};
#[allow(unused_imports)]
pub(crate) use question_events::wait_for_question_permission;
#[allow(unused_imports)]
pub(crate) use repo_root::repo_root;
#[allow(unused_imports)]
pub use tool_context::{test_context, test_context_with_tool_state};
#[allow(unused_imports)]
pub use workspace::{setup_workspace, setup_workspace_fixture};

#[derive(Debug, Clone)]
pub struct TestRequest {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct TestResponse {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub delay: Duration,
}

#[derive(Debug, Clone)]
pub struct TestBinaryResponse {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}

pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn env_test_lock() -> MutexGuard<'static, ()> {
    env_lock().lock().expect("env lock")
}

pub fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

pub fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

pub fn anonymous_supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}

pub fn allow_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
}

pub fn edit_only_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
}

pub fn ask_edit_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Ask,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000)
}

pub fn expect_invalid_arguments(error: ToolError, expected: &str) {
    match error {
        ToolError::InvalidArguments(message) => assert!(
            message.contains(expected),
            "expected invalid-arguments error containing {expected:?}, got {message:?}"
        ),
        other => panic!("expected invalid-arguments error, got {other:?}"),
    }
}

pub fn expect_execution_error(error: ToolError, expected: &str) {
    match error {
        ToolError::Execution(message) => assert!(
            message.contains(expected),
            "expected execution error containing {expected:?}, got {message:?}"
        ),
        other => panic!("expected execution error, got {other:?}"),
    }
}

pub fn spawn_http_server(
    handler: std::sync::Arc<dyn Fn(TestRequest) -> TestResponse + Send + Sync + 'static>,
) -> String {
    spawn_server(handler)
}

pub fn spawn_binary_http_server(
    handler: std::sync::Arc<dyn Fn(TestRequest) -> TestBinaryResponse + Send + Sync + 'static>,
) -> String {
    spawn_server(handler)
}

trait ServerResponse {
    fn status(&self) -> &'static str;
    fn headers(&self) -> &[(String, String)];
    fn body(&self) -> &[u8];
    fn delay(&self) -> Duration;
}

impl ServerResponse for TestResponse {
    fn status(&self) -> &'static str {
        self.status
    }

    fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    fn body(&self) -> &[u8] {
        self.body.as_bytes()
    }

    fn delay(&self) -> Duration {
        self.delay
    }
}

impl ServerResponse for TestBinaryResponse {
    fn status(&self) -> &'static str {
        self.status
    }

    fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    fn body(&self) -> &[u8] {
        &self.body
    }

    fn delay(&self) -> Duration {
        self.delay
    }
}

fn spawn_server<R>(
    handler: std::sync::Arc<dyn Fn(TestRequest) -> R + Send + Sync + 'static>,
) -> String
where
    R: ServerResponse + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test http server");
    let addr = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let request = read_request(&mut stream);
            let response = handler(request);
            if !response.delay().is_zero() {
                thread::sleep(response.delay());
            }

            let body = response.body();
            let mut header_text =
                format!("HTTP/1.1 {}\r\nConnection: close\r\n", response.status());
            let has_content_length = response
                .headers()
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
            if !has_content_length {
                header_text.push_str(&format!("Content-Length: {}\r\n", body.len()));
            }
            for (name, value) in response.headers() {
                header_text.push_str(&format!("{name}: {value}\r\n"));
            }
            header_text.push_str("\r\n");

            let _ = stream.write_all(header_text.as_bytes());
            let _ = stream.write_all(body);
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
