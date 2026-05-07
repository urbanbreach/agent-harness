use std::sync::{Arc, Mutex};
use std::time::Duration;

mod common;

use common::{
    env_test_lock, expect_execution_error, setup_workspace_fixture, spawn_http_server,
    test_context as common_test_context, EnvGuard, TestRequest, TestResponse,
};
use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

const GITHUB_API_BASE_URL_ENV: &str = "HARNESS_GITHUB_API_BASE_URL";
const GITHUB_TOKEN_ENV: &str = "HARNESS_GITHUB_TOKEN";
const GITHUB_REPOSITORY_ENV: &str = "HARNESS_GITHUB_REPOSITORY";

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-github-tests", tool_call_id)
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_issue_get_uses_env_repository_and_auth_headers() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
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
            test_context(workspace.workspace(), "toolcall-github-issue-get"),
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
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
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
            test_context(workspace.workspace(), "toolcall-github-issue-list"),
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
async fn github_pull_request_list_preserves_query_parameters_and_renders_refs() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
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
                    "number": 11,
                    "title": "Simplify GitHub rendering",
                    "state": "open",
                    "html_url": "https://github.com/urbanbreach/agent-harness/pull/11",
                    "head": {"ref": "cleanup/github-rendering"},
                    "base": {"ref": "dev"}
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
    let tool = registry
        .get("github.pull_request")
        .expect("github.pull_request tool");
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-pr-list"),
            json!({
                "operation": "list",
                "state": "all",
                "per_page": 1
            }),
        )
        .await
        .expect("github.pull_request list");

    assert_eq!(
        result.display_text,
        "Pull requests for urbanbreach/agent-harness:\n- #11 [open] Simplify GitHub rendering (cleanup/github-rendering -> dev)"
    );
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.pointer("/items/0/number")),
        Some(&json!(11))
    );

    let requests = requests.lock().expect("request log");
    let request = requests.first().expect("request captured");
    assert_eq!(
        request.path,
        "/repos/urbanbreach/agent-harness/pulls?per_page=1&state=all"
    );
    assert!(
        !request.headers.contains_key("authorization"),
        "read-only pull request list call should not require auth"
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_issue_close_requires_authentication() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
    let _env = EnvGuard::set(&[
        (GITHUB_API_BASE_URL_ENV, Some("http://127.0.0.1:9")),
        (GITHUB_TOKEN_ENV, None),
        (GITHUB_REPOSITORY_ENV, Some("urbanbreach/agent-harness")),
    ]);

    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("github.issue").expect("github.issue tool");
    let error = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-issue-close"),
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
async fn github_issue_comment_posts_body_and_renders_comment_url() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
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
                "id": 55,
                "body": "Looks good from here.",
                "html_url": "https://github.com/urbanbreach/agent-harness/issues/19#issuecomment-55"
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
            test_context(workspace.workspace(), "toolcall-github-issue-comment"),
            json!({
                "operation": "comment",
                "issue_number": 19,
                "body": "Looks good from here."
            }),
        )
        .await
        .expect("github.issue comment");

    assert_eq!(
        result.display_text,
        "Commented on issue #19 in urbanbreach/agent-harness.\nURL: https://github.com/urbanbreach/agent-harness/issues/19#issuecomment-55"
    );
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.pointer("/comment/id")),
        Some(&json!(55))
    );

    let requests = requests.lock().expect("request log");
    let request = requests.first().expect("request captured");
    assert_eq!(
        request.path,
        "/repos/urbanbreach/agent-harness/issues/19/comments"
    );
    assert_eq!(
        request.headers.get("authorization"),
        Some(&"Bearer fixture-token".to_string())
    );
    let payload: Value = serde_json::from_str(&request.body).expect("request json");
    assert_eq!(payload.get("body"), Some(&json!("Looks good from here.")));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide GitHub env mutation across awaits"
)]
async fn github_pull_request_create_posts_expected_payload() {
    let _env_guard = env_test_lock();
    let workspace = setup_workspace_fixture();
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
            test_context(workspace.workspace(), "toolcall-github-pr-create"),
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
    assert_eq!(payload.get("body"), Some(&json!("This adds docs.")));
    assert_eq!(payload.get("draft"), Some(&json!(true)));
}
