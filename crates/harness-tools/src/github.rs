use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::env_vars::first_env_value;
use crate::http_client;
use crate::text::has_trimmed_content;

const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";
const HARNESS_GITHUB_API_BASE_URL_ENV_VARS: &[&str] = &["HARNESS_GITHUB_API_BASE_URL"];
const GITHUB_TOKEN_ENV_VARS: &[&str] = &["HARNESS_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"];
const GITHUB_REPOSITORY_ENV_VARS: &[&str] = &["HARNESS_GITHUB_REPOSITORY", "GITHUB_REPOSITORY"];
const DEFAULT_LIST_PER_PAGE: u8 = 20;
const MAX_LIST_PER_PAGE: u8 = 100;
const USER_AGENT: &str = concat!("agent-harness/", env!("CARGO_PKG_VERSION"));

pub(crate) struct GitHubExecutor {
    client: reqwest::Client,
    api_base_url: String,
    auth_token: Option<String>,
}

impl GitHubExecutor {
    pub(crate) fn new() -> Self {
        Self {
            client: http_client::default_client("GitHub client should build"),
            api_base_url: first_env_value(HARNESS_GITHUB_API_BASE_URL_ENV_VARS)
                .unwrap_or_else(|| DEFAULT_GITHUB_API_BASE_URL.to_string()),
            auth_token: first_env_value(GITHUB_TOKEN_ENV_VARS),
        }
    }

    async fn issue(&self, args: GitHubIssueArgs) -> Result<ToolResult, ToolError> {
        let repo = RepoRef::resolve(args.owner.as_deref(), args.repo.as_deref())?;
        match args.operation {
            GitHubIssueOperation::Get => {
                let issue_number = required_issue_number(args.issue_number)?;
                let issue = self
                    .send_json_request(
                        Method::GET,
                        &format!("/repos/{}/{}/issues/{issue_number}", repo.owner, repo.repo),
                        None,
                        false,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: render_issue(&issue),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "get",
                        "issue": issue,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubIssueOperation::List => {
                let query = query_map([
                    (
                        "state",
                        Some(args.state.unwrap_or(GitHubListState::Open).as_api_value()),
                    ),
                    ("per_page", Some(list_per_page_param(args.per_page))),
                ]);
                let issues = self
                    .send_json_request(
                        Method::GET,
                        &path_with_query(
                            &format!("/repos/{}/{}/issues", repo.owner, repo.repo),
                            &query,
                        ),
                        None,
                        false,
                    )
                    .await?;
                let items = issues
                    .as_array()
                    .ok_or_else(|| {
                        ToolError::Execution("GitHub returned a non-array issue list".to_string())
                    })?
                    .iter()
                    .filter(|item| item.get("pull_request").is_none())
                    .cloned()
                    .collect::<Vec<_>>();
                Ok(ToolResult {
                    display_text: render_issue_list(&repo, &items),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "list",
                        "items": items,
                        "query": query,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubIssueOperation::Comment => {
                let issue_number = required_issue_number(args.issue_number)?;
                let body = required_non_empty(args.body, "body")?;
                let comment = self
                    .send_json_request(
                        Method::POST,
                        &format!(
                            "/repos/{}/{}/issues/{issue_number}/comments",
                            repo.owner, repo.repo
                        ),
                        Some(json!({ "body": body })),
                        true,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: format!(
                        "Commented on issue #{} in {}/{}.\nURL: {}",
                        issue_number,
                        repo.owner,
                        repo.repo,
                        string_field(&comment, "html_url").unwrap_or("<unknown>")
                    ),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "comment",
                        "issue_number": issue_number,
                        "comment": comment,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubIssueOperation::Close | GitHubIssueOperation::Reopen => {
                let issue_number = required_issue_number(args.issue_number)?;
                let state = match args.operation {
                    GitHubIssueOperation::Close => "closed",
                    GitHubIssueOperation::Reopen => "open",
                    _ => unreachable!(),
                };
                let issue = self
                    .send_json_request(
                        Method::PATCH,
                        &format!("/repos/{}/{}/issues/{issue_number}", repo.owner, repo.repo),
                        Some(json!({ "state": state })),
                        true,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: format!(
                        "Issue #{} in {}/{} is now {}.\nURL: {}",
                        issue_number,
                        repo.owner,
                        repo.repo,
                        state,
                        string_field(&issue, "html_url").unwrap_or("<unknown>")
                    ),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": state,
                        "issue": issue,
                    })),
                    artifacts: Vec::new(),
                })
            }
        }
    }

    async fn pull_request(&self, args: GitHubPullRequestArgs) -> Result<ToolResult, ToolError> {
        let repo = RepoRef::resolve(args.owner.as_deref(), args.repo.as_deref())?;
        match args.operation {
            GitHubPullRequestOperation::Get => {
                let pull_number = required_pull_number(args.pull_number)?;
                let pull_request = self
                    .send_json_request(
                        Method::GET,
                        &format!("/repos/{}/{}/pulls/{pull_number}", repo.owner, repo.repo),
                        None,
                        false,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: render_pull_request(&pull_request),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "get",
                        "pull_request": pull_request,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubPullRequestOperation::List => {
                let query = query_map([
                    (
                        "state",
                        Some(args.state.unwrap_or(GitHubListState::Open).as_api_value()),
                    ),
                    ("per_page", Some(list_per_page_param(args.per_page))),
                ]);
                let pull_requests = self
                    .send_json_request(
                        Method::GET,
                        &path_with_query(
                            &format!("/repos/{}/{}/pulls", repo.owner, repo.repo),
                            &query,
                        ),
                        None,
                        false,
                    )
                    .await?;
                let items = pull_requests
                    .as_array()
                    .ok_or_else(|| {
                        ToolError::Execution(
                            "GitHub returned a non-array pull request list".to_string(),
                        )
                    })?
                    .clone();
                Ok(ToolResult {
                    display_text: render_pull_request_list(&repo, &items),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "list",
                        "items": items,
                        "query": query,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubPullRequestOperation::Comment => {
                let pull_number = required_pull_number(args.pull_number)?;
                let body = required_non_empty(args.body, "body")?;
                let comment = self
                    .send_json_request(
                        Method::POST,
                        &format!(
                            "/repos/{}/{}/issues/{pull_number}/comments",
                            repo.owner, repo.repo
                        ),
                        Some(json!({ "body": body })),
                        true,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: format!(
                        "Commented on pull request #{} in {}/{}.\nURL: {}",
                        pull_number,
                        repo.owner,
                        repo.repo,
                        string_field(&comment, "html_url").unwrap_or("<unknown>")
                    ),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "comment",
                        "pull_number": pull_number,
                        "comment": comment,
                    })),
                    artifacts: Vec::new(),
                })
            }
            GitHubPullRequestOperation::Create => {
                let title = required_non_empty(args.title, "title")?;
                let head = required_non_empty(args.head, "head")?;
                let base = required_non_empty(args.base, "base")?;
                let mut payload = Map::new();
                payload.insert("title".to_string(), Value::String(title));
                payload.insert("head".to_string(), Value::String(head));
                payload.insert("base".to_string(), Value::String(base));
                if let Some(body) = args.body.filter(|value| has_trimmed_content(value)) {
                    payload.insert("body".to_string(), Value::String(body));
                }
                if let Some(draft) = args.draft {
                    payload.insert("draft".to_string(), Value::Bool(draft));
                }
                let pull_request = self
                    .send_json_request(
                        Method::POST,
                        &format!("/repos/{}/{}/pulls", repo.owner, repo.repo),
                        Some(Value::Object(payload)),
                        true,
                    )
                    .await?;
                Ok(ToolResult {
                    display_text: format!(
                        "Created pull request #{} in {}/{}.\nURL: {}",
                        pull_request
                            .get("number")
                            .and_then(Value::as_u64)
                            .unwrap_or_default(),
                        repo.owner,
                        repo.repo,
                        string_field(&pull_request, "html_url").unwrap_or("<unknown>")
                    ),
                    structured_json: Some(json!({
                        "repository": repo.as_json(),
                        "operation": "create",
                        "pull_request": pull_request,
                    })),
                    artifacts: Vec::new(),
                })
            }
        }
    }

    async fn send_json_request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        require_auth: bool,
    ) -> Result<Value, ToolError> {
        let mut url = self.api_base_url.trim_end_matches('/').to_string();
        url.push_str(path);
        let mut request = self
            .client
            .request(method, &url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .header(reqwest::header::USER_AGENT, USER_AGENT);

        if let Some(token) = self
            .auth_token
            .as_deref()
            .filter(|value| has_trimmed_content(value))
        {
            request = request.bearer_auth(token);
        } else if require_auth {
            return Err(ToolError::Execution(
                "GitHub authentication is required for this operation; set HARNESS_GITHUB_TOKEN, GITHUB_TOKEN, or GH_TOKEN".to_string(),
            ));
        }

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|err| ToolError::Execution(format!("GitHub request failed: {err}")))?;
        let status = response.status();
        let text = response.text().await.map_err(|err| {
            ToolError::Execution(format!("failed to read GitHub response: {err}"))
        })?;

        if !status.is_success() {
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| text.clone());
            return Err(ToolError::Execution(format!(
                "GitHub API request failed with status {}: {}",
                status.as_u16(),
                message
            )));
        }

        serde_json::from_str(&text)
            .map_err(|err| ToolError::Execution(format!("GitHub returned invalid JSON: {err}")))
    }
}

pub(crate) struct GitHubIssueTool {
    executor: Arc<GitHubExecutor>,
}

impl GitHubIssueTool {
    pub(crate) fn new(executor: Arc<GitHubExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct GitHubPullRequestTool {
    executor: Arc<GitHubExecutor>,
}

impl GitHubPullRequestTool {
    pub(crate) fn new(executor: Arc<GitHubExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GitHubIssueOperation {
    Get,
    List,
    Comment,
    Close,
    Reopen,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GitHubPullRequestOperation {
    Get,
    List,
    Comment,
    Create,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GitHubListState {
    Open,
    Closed,
    All,
}

impl GitHubListState {
    fn as_api_value(self) -> String {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
        .to_string()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GitHubIssueArgs {
    operation: GitHubIssueOperation,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    issue_number: Option<u64>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    state: Option<GitHubListState>,
    #[serde(default)]
    per_page: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GitHubPullRequestArgs {
    operation: GitHubPullRequestOperation,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    pull_number: Option<u64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    head: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    state: Option<GitHubListState>,
    #[serde(default)]
    per_page: Option<u8>,
    #[serde(default)]
    draft: Option<bool>,
}

#[derive(Debug, Clone)]
struct RepoRef {
    owner: String,
    repo: String,
}

impl RepoRef {
    fn resolve(owner: Option<&str>, repo: Option<&str>) -> Result<Self, ToolError> {
        match (
            owner.filter(|value| has_trimmed_content(value)),
            repo.filter(|value| has_trimmed_content(value)),
        ) {
            (Some(owner), Some(repo)) => Ok(Self {
                owner: owner.to_string(),
                repo: repo.to_string(),
            }),
            (None, None) => repository_from_env(),
            _ => Err(ToolError::InvalidArguments(
                "owner and repo must be provided together, or omit both and set HARNESS_GITHUB_REPOSITORY / GITHUB_REPOSITORY".to_string(),
            )),
        }
    }

    fn as_json(&self) -> Value {
        json!({ "owner": self.owner, "repo": self.repo })
    }
}

fn repository_from_env() -> Result<RepoRef, ToolError> {
    let repository = first_env_value(GITHUB_REPOSITORY_ENV_VARS).ok_or_else(|| {
        ToolError::InvalidArguments(
            "owner/repo not provided and no HARNESS_GITHUB_REPOSITORY or GITHUB_REPOSITORY is set"
                .to_string(),
        )
    })?;
    let (owner, repo) = repository.split_once('/').ok_or_else(|| {
        ToolError::InvalidArguments(format!(
            "repository must be in owner/repo form, got {repository:?}"
        ))
    })?;
    if !has_trimmed_content(owner) || !has_trimmed_content(repo) {
        return Err(ToolError::InvalidArguments(format!(
            "repository must be in owner/repo form, got {repository:?}"
        )));
    }
    Ok(RepoRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

fn required_issue_number(issue_number: Option<u64>) -> Result<u64, ToolError> {
    issue_number.ok_or_else(|| {
        ToolError::InvalidArguments("issue_number is required for this operation".to_string())
    })
}

fn required_pull_number(pull_number: Option<u64>) -> Result<u64, ToolError> {
    pull_number.ok_or_else(|| {
        ToolError::InvalidArguments("pull_number is required for this operation".to_string())
    })
}

fn required_non_empty(value: Option<String>, field: &str) -> Result<String, ToolError> {
    let value = value.ok_or_else(|| {
        ToolError::InvalidArguments(format!("{field} is required for this operation"))
    })?;
    if !has_trimmed_content(&value) {
        return Err(ToolError::InvalidArguments(format!(
            "{field} must not be empty"
        )));
    }
    Ok(value)
}

fn list_per_page_param(per_page: Option<u8>) -> String {
    per_page
        .unwrap_or(DEFAULT_LIST_PER_PAGE)
        .clamp(1, MAX_LIST_PER_PAGE)
        .to_string()
}

fn query_map<const N: usize>(
    entries: [(impl Into<String>, Option<String>); N],
) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.into(), value)))
        .collect()
}

fn path_with_query(path: &str, query: &BTreeMap<String, String>) -> String {
    if query.is_empty() {
        return path.to_string();
    }

    let query = query
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("{path}?{query}")
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn render_issue(issue: &Value) -> String {
    format!(
        "Issue #{}: {}\nState: {}\nURL: {}\n{}",
        issue
            .get("number")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        string_field(issue, "title").unwrap_or("<untitled>"),
        string_field(issue, "state").unwrap_or("<unknown>"),
        string_field(issue, "html_url").unwrap_or("<unknown>"),
        render_body(issue.get("body").and_then(Value::as_str))
    )
}

fn render_issue_list(repo: &RepoRef, issues: &[Value]) -> String {
    if issues.is_empty() {
        return format!("No issues found for {}/{}.", repo.owner, repo.repo);
    }

    let lines = issues
        .iter()
        .map(|issue| {
            format!(
                "- #{} [{}] {}",
                issue
                    .get("number")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                string_field(issue, "state").unwrap_or("unknown"),
                string_field(issue, "title").unwrap_or("<untitled>")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Issues for {}/{}:\n{}", repo.owner, repo.repo, lines)
}

fn render_pull_request(pull_request: &Value) -> String {
    format!(
        "Pull request #{}: {}\nState: {}\nURL: {}\nBase: {}\nHead: {}\n{}",
        pull_request
            .get("number")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        string_field(pull_request, "title").unwrap_or("<untitled>"),
        string_field(pull_request, "state").unwrap_or("<unknown>"),
        string_field(pull_request, "html_url").unwrap_or("<unknown>"),
        pull_request
            .pointer("/base/ref")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        pull_request
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>"),
        render_body(pull_request.get("body").and_then(Value::as_str))
    )
}

fn render_pull_request_list(repo: &RepoRef, pull_requests: &[Value]) -> String {
    if pull_requests.is_empty() {
        return format!("No pull requests found for {}/{}.", repo.owner, repo.repo);
    }

    let lines = pull_requests
        .iter()
        .map(|pull_request| {
            format!(
                "- #{} [{}] {} ({} -> {})",
                pull_request
                    .get("number")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                string_field(pull_request, "state").unwrap_or("unknown"),
                string_field(pull_request, "title").unwrap_or("<untitled>"),
                pull_request
                    .pointer("/head/ref")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>"),
                pull_request
                    .pointer("/base/ref")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Pull requests for {}/{}:\n{}", repo.owner, repo.repo, lines)
}

fn render_body(body: Option<&str>) -> String {
    let body = body.unwrap_or("").trim();
    if body.is_empty() {
        "Body: <empty>".to_string()
    } else {
        format!("Body:\n{}", body)
    }
}

#[async_trait]
impl Tool for GitHubIssueTool {
    fn id(&self) -> &str {
        "github.issue"
    }

    fn description(&self) -> &str {
        "Reads, comments on, and closes or reopens GitHub issues through the GitHub REST API."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GitHubIssueArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GitHubIssueArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.issue(args).await
    }
}

#[async_trait]
impl Tool for GitHubPullRequestTool {
    fn id(&self) -> &str {
        "github.pull_request"
    }

    fn description(&self) -> &str {
        "Reads, comments on, lists, and creates GitHub pull requests through the GitHub REST API."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GitHubPullRequestArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GitHubPullRequestArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.pull_request(args).await
    }
}
