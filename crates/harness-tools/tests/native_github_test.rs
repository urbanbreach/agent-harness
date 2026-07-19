use harness_tools::UnwrapOrAbort;
use std::sync::{Arc, Mutex};

mod common;

use async_trait::async_trait;
use common::{
    expect_execution_error, setup_workspace_fixture, test_context as common_test_context,
};
use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolError;
use harness_tools::{
    coordinator_registry_with_github_transport, GitHubHttpRequest, GitHubHttpResponse,
    GitHubHttpTransport,
};
use reqwest::Method;
use serde_json::{json, Value};

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-github-tests", tool_call_id)
}

#[derive(Debug)]
struct ScriptedGitHubTransport {
    requests: Mutex<Vec<GitHubHttpRequest>>,
    responses: Mutex<Vec<GitHubHttpResponse>>,
}

impl ScriptedGitHubTransport {
    fn new(responses: Vec<GitHubHttpResponse>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses),
        })
    }

    fn requests(&self) -> Vec<GitHubHttpRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

#[async_trait]
impl GitHubHttpTransport for ScriptedGitHubTransport {
    async fn send(&self, request: GitHubHttpRequest) -> Result<GitHubHttpResponse, ToolError> {
        self.requests.lock().unwrap_or_abort().push(request);
        Ok(self.responses.lock().unwrap_or_abort().remove(0))
    }
}

fn github_registry(
    auth_token: Option<&str>,
    transport: Arc<ScriptedGitHubTransport>,
) -> harness_core::tool::ToolRegistry {
    coordinator_registry_with_github_transport(
        ShellAllowlist::default(),
        "https://api.github.test",
        auth_token.map(str::to_string),
        transport,
    )
}

fn request_path(request: &GitHubHttpRequest) -> String {
    let url = reqwest::Url::parse(&request.url).unwrap_or_abort();
    match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_string(),
    }
}

#[tokio::test]
async fn github_issue_get_uses_env_repository_and_auth_headers() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedGitHubTransport::new(vec![GitHubHttpResponse::json(
        200,
        json!({
                "number": 19,
                "title": "Add first-class GitHub issue and PR integration",
                "state": "open",
                "body": "Tracked body",
                "html_url": "https://github.com/urbanbreach/agent-harness/issues/19"
        }),
    )]);

    let registry = github_registry(Some("fixture-token"), Arc::clone(&transport));
    let tool = registry.get("github.issue").unwrap_or_abort();
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-issue-get"),
            json!({
                "operation": "get",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "issue_number": 19
            }),
        )
        .await
        .unwrap_or_abort();

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

    let requests = transport.requests();
    let request = requests.first().unwrap_or_abort();
    assert_eq!(request.method, Method::GET);
    assert_eq!(
        request_path(request),
        "/repos/urbanbreach/agent-harness/issues/19"
    );
    assert_eq!(request.auth_token.as_deref(), Some("fixture-token"));
    assert!(
        request.body.is_none(),
        "get request should not include a body"
    );
}

#[tokio::test]
async fn github_issue_list_filters_pull_requests_and_preserves_query_parameters() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedGitHubTransport::new(vec![GitHubHttpResponse::json(
        200,
        json!([
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
        ]),
    )]);

    let registry = github_registry(None, Arc::clone(&transport));
    let tool = registry.get("github.issue").unwrap_or_abort();
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-issue-list"),
            json!({
                "operation": "list",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "state": "closed",
                "per_page": 2
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Real issue"));
    assert!(!result.display_text.contains("Actually a pull request"));
    let items = result
        .structured_json
        .as_ref()
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .unwrap_or_abort();
    assert_eq!(items.len(), 1);

    let requests = transport.requests();
    let request = requests.first().unwrap_or_abort();
    assert_eq!(
        request_path(request),
        "/repos/urbanbreach/agent-harness/issues?per_page=2&state=closed"
    );
    assert!(
        request.auth_token.is_none(),
        "read-only list call should not require auth"
    );
}

#[tokio::test]
async fn github_pull_request_list_preserves_query_parameters_and_renders_refs() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedGitHubTransport::new(vec![GitHubHttpResponse::json(
        200,
        json!([
                {
                    "number": 11,
                    "title": "Simplify GitHub rendering",
                    "state": "open",
                    "html_url": "https://github.com/urbanbreach/agent-harness/pull/11",
                    "head": {"ref": "cleanup/github-rendering"},
                    "base": {"ref": "dev"}
                }
        ]),
    )]);

    let registry = github_registry(None, Arc::clone(&transport));
    let tool = registry.get("github.pull_request").unwrap_or_abort();
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-pr-list"),
            json!({
                "operation": "list",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "state": "all",
                "per_page": 1
            }),
        )
        .await
        .unwrap_or_abort();

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

    let requests = transport.requests();
    let request = requests.first().unwrap_or_abort();
    assert_eq!(
        request_path(request),
        "/repos/urbanbreach/agent-harness/pulls?per_page=1&state=all"
    );
    assert!(
        request.auth_token.is_none(),
        "read-only pull request list call should not require auth"
    );
}

#[tokio::test]
async fn github_issue_close_requires_authentication() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();

    let registry = github_registry(None, ScriptedGitHubTransport::new(Vec::new()));
    let tool = registry.get("github.issue").unwrap_or_abort();
    let error = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-issue-close"),
            json!({
                "operation": "close",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "issue_number": 19
            }),
        )
        .await
        .expect_err("close without auth should fail");
    expect_execution_error(error, "GitHub authentication is required");
}

#[tokio::test]
async fn github_issue_comment_posts_body_and_renders_comment_url() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedGitHubTransport::new(vec![GitHubHttpResponse::json(
        201,
        json!({
                "id": 55,
                "body": "Looks good from here.",
                "html_url": "https://github.com/urbanbreach/agent-harness/issues/19#issuecomment-55"
        }),
    )]);

    let registry = github_registry(Some("fixture-token"), Arc::clone(&transport));
    let tool = registry.get("github.issue").unwrap_or_abort();
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-issue-comment"),
            json!({
                "operation": "comment",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "issue_number": 19,
                "body": "Looks good from here."
            }),
        )
        .await
        .unwrap_or_abort();

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

    let requests = transport.requests();
    let request = requests.first().unwrap_or_abort();
    assert_eq!(request.method, Method::POST);
    assert_eq!(
        request_path(request),
        "/repos/urbanbreach/agent-harness/issues/19/comments"
    );
    assert_eq!(request.auth_token.as_deref(), Some("fixture-token"));
    let payload = request.body.as_ref().unwrap_or_abort();
    assert_eq!(payload.get("body"), Some(&json!("Looks good from here.")));
}

#[tokio::test]
async fn github_pull_request_create_posts_expected_payload() {
    // arrange
    // act
    // assert
    let workspace = setup_workspace_fixture();
    let transport = ScriptedGitHubTransport::new(vec![GitHubHttpResponse::json(
        201,
        json!({
                "number": 42,
                "title": "Add GitHub tool docs",
                "state": "open",
                "html_url": "https://github.com/urbanbreach/agent-harness/pull/42",
                "body": "This adds docs.",
                "head": {"ref": "feature/github-docs"},
                "base": {"ref": "main"}
        }),
    )]);

    let registry = github_registry(Some("fixture-token"), Arc::clone(&transport));
    let tool = registry.get("github.pull_request").unwrap_or_abort();
    let result = tool
        .call(
            test_context(workspace.workspace(), "toolcall-github-pr-create"),
            json!({
                "operation": "create",
                "owner": "urbanbreach",
                "repo": "agent-harness",
                "title": "Add GitHub tool docs",
                "body": "This adds docs.",
                "head": "feature/github-docs",
                "base": "main",
                "draft": true
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Created pull request #42"));
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.pointer("/pull_request/number")),
        Some(&json!(42))
    );

    let requests = transport.requests();
    let request = requests.first().unwrap_or_abort();
    assert_eq!(request.method, Method::POST);
    assert_eq!(
        request_path(request),
        "/repos/urbanbreach/agent-harness/pulls"
    );
    assert_eq!(request.auth_token.as_deref(), Some("fixture-token"));
    let payload = request.body.as_ref().unwrap_or_abort();
    assert_eq!(payload.get("title"), Some(&json!("Add GitHub tool docs")));
    assert_eq!(payload.get("head"), Some(&json!("feature/github-docs")));
    assert_eq!(payload.get("base"), Some(&json!("main")));
    assert_eq!(payload.get("body"), Some(&json!("This adds docs.")));
    assert_eq!(payload.get("draft"), Some(&json!(true)));
}
