// allow: SIZE_OK — GitHub tool wrapper (API client + search)
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use harness_core::ToolResultExt;
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::env_vars::first_env_value;
use crate::http_client;
use crate::json_schema_for;
use crate::parse_tool_args;
use crate::text::has_trimmed_content;
use crate::text_json_tool_result;

const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";
const HARNESS_GITHUB_API_BASE_URL_ENV_VARS: &[&str] = &["HARNESS_GITHUB_API_BASE_URL"];
const GITHUB_TOKEN_ENV_VARS: &[&str] = &["HARNESS_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"];
const GITHUB_REPOSITORY_ENV_VARS: &[&str] = &["HARNESS_GITHUB_REPOSITORY", "GITHUB_REPOSITORY"];
const DEFAULT_LIST_PER_PAGE: u8 = 20;
const MAX_LIST_PER_PAGE: u8 = 100;
const USER_AGENT: &str = concat!("agent-harness/", env!("CARGO_PKG_VERSION"));

pub(crate) struct GitHubExecutor {
    transport: Arc<dyn GitHubHttpTransport>,
    api_base_url: String,
    auth_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitHubHttpRequest {
    pub method: Method,
    pub url: String,
    pub auth_token: Option<String>,
    pub body: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct GitHubHttpResponse {
    pub status: u16,
    pub body: String,
}

impl GitHubHttpResponse {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }
}

#[async_trait]
pub trait GitHubHttpTransport: Send + Sync {
    async fn send(&self, request: GitHubHttpRequest) -> Result<GitHubHttpResponse, ToolError>;
}

#[derive(Debug, Clone)]
struct ReqwestGitHubHttpTransport {
    client: reqwest::Client,
}

#[async_trait]
impl GitHubHttpTransport for ReqwestGitHubHttpTransport {
    async fn send(&self, request: GitHubHttpRequest) -> Result<GitHubHttpResponse, ToolError> {
        let mut builder = self
            .client
            .request(request.method, request.url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .header(reqwest::header::USER_AGENT, USER_AGENT);
        if let Some(token) = request.auth_token {
            builder = builder.bearer_auth(token);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }

        let response = builder.send().await.tool_err("GitHub request failed")?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| {
            ToolError::Execution(format!("failed to read GitHub response: {err}"))
        })?;
        Ok(GitHubHttpResponse { status, body })
    }
}

struct ListedItems {
    items: Vec<Value>,
    query: BTreeMap<String, String>,
}

impl GitHubExecutor {
    pub(crate) fn new() -> Self {
        Self {
            transport: Arc::new(ReqwestGitHubHttpTransport {
                client: http_client::default_client("GitHub client should build"),
            }),
            api_base_url: first_env_value(HARNESS_GITHUB_API_BASE_URL_ENV_VARS)
                .unwrap_or_else(|| DEFAULT_GITHUB_API_BASE_URL.to_string()),
            auth_token: first_env_value(GITHUB_TOKEN_ENV_VARS),
        }
    }

    pub(crate) fn with_transport(
        api_base_url: impl Into<String>,
        auth_token: Option<String>,
        transport: Arc<dyn GitHubHttpTransport>,
    ) -> Self {
        Self {
            transport,
            api_base_url: api_base_url.into(),
            auth_token,
        }
    }

    async fn issue(&self, args: GitHubIssueArgs) -> Result<ToolResult, ToolError> {
        let repo = RepoRef::resolve(args.owner.as_deref(), args.repo.as_deref())?;
        match args.operation {
            GitHubIssueOperation::Get => {
                let issue_number = required_issue_number(args.issue_number)?;
                let issue = self
                    .send_json_request(Method::GET, &repo.issue_path(issue_number), None, false)
                    .await?;
                Ok(text_json_tool_result(
                    render_issue(&issue),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "get",
                        "issue": issue,
                    }),
                ))
            }
            GitHubIssueOperation::List => {
                let ListedItems { items, query } = self
                    .list_items(
                        &repo.issues_path(),
                        args.state,
                        args.per_page,
                        "GitHub returned a non-array issue list",
                    )
                    .await?;
                let items = items
                    .iter()
                    .filter(|item| is_issue_list_item(item))
                    .cloned()
                    .collect::<Vec<_>>();
                Ok(text_json_tool_result(
                    render_issue_list(&repo, &items),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "list",
                        "items": items,
                        "query": query,
                    }),
                ))
            }
            GitHubIssueOperation::Comment => {
                let issue_number = required_issue_number(args.issue_number)?;
                let body = required_non_empty(args.body, "body")?;
                let comment = self.create_issue_comment(&repo, issue_number, body).await?;
                Ok(text_json_tool_result(
                    render_comment_result("issue", issue_number, &repo, &comment),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "comment",
                        "issue_number": issue_number,
                        "comment": comment,
                    }),
                ))
            }
            GitHubIssueOperation::Close => {
                let issue_number = required_issue_number(args.issue_number)?;
                self.update_issue_state(&repo, issue_number, IssueState::Closed)
                    .await
            }
            GitHubIssueOperation::Reopen => {
                let issue_number = required_issue_number(args.issue_number)?;
                self.update_issue_state(&repo, issue_number, IssueState::Open)
                    .await
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
                        &repo.pull_request_path(pull_number),
                        None,
                        false,
                    )
                    .await?;
                Ok(text_json_tool_result(
                    render_pull_request(&pull_request),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "get",
                        "pull_request": pull_request,
                    }),
                ))
            }
            GitHubPullRequestOperation::List => {
                let ListedItems { items, query } = self
                    .list_items(
                        &repo.pull_requests_path(),
                        args.state,
                        args.per_page,
                        "GitHub returned a non-array pull request list",
                    )
                    .await?;
                Ok(text_json_tool_result(
                    render_pull_request_list(&repo, &items),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "list",
                        "items": items,
                        "query": query,
                    }),
                ))
            }
            GitHubPullRequestOperation::Comment => {
                let pull_number = required_pull_number(args.pull_number)?;
                let body = required_non_empty(args.body, "body")?;
                let comment = self.create_issue_comment(&repo, pull_number, body).await?;
                Ok(text_json_tool_result(
                    render_comment_result("pull request", pull_number, &repo, &comment),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "comment",
                        "pull_number": pull_number,
                        "comment": comment,
                    }),
                ))
            }
            GitHubPullRequestOperation::Create => {
                let title = required_non_empty(args.title, "title")?;
                let head = required_non_empty(args.head, "head")?;
                let base = required_non_empty(args.base, "base")?;
                let payload = pull_request_create_payload(title, head, base, args.body, args.draft);
                let pull_request = self
                    .send_json_request(
                        Method::POST,
                        &repo.pull_requests_path(),
                        Some(payload),
                        true,
                    )
                    .await?;
                Ok(text_json_tool_result(
                    render_created_pull_request(&repo, &pull_request),
                    json!({
                        "repository": repo.as_json(),
                        "operation": "create",
                        "pull_request": pull_request,
                    }),
                ))
            }
        }
    }

    async fn list_items(
        &self,
        path: &str,
        state: Option<GitHubListState>,
        per_page: Option<u8>,
        non_array_message: &str,
    ) -> Result<ListedItems, ToolError> {
        let query = list_query(state, per_page);
        let response = self
            .send_json_request(Method::GET, &path_with_query(path, &query), None, false)
            .await?;
        let items = required_json_array(&response, non_array_message)?.clone();
        Ok(ListedItems { items, query })
    }

    async fn create_issue_comment(
        &self,
        repo: &RepoRef,
        issue_number: u64,
        body: String,
    ) -> Result<Value, ToolError> {
        self.send_json_request(
            Method::POST,
            &repo.issue_comments_path(issue_number),
            Some(json!({ "body": body })),
            true,
        )
        .await
    }

    async fn update_issue_state(
        &self,
        repo: &RepoRef,
        issue_number: u64,
        state: IssueState,
    ) -> Result<ToolResult, ToolError> {
        let api_state = state.as_api_value();
        let issue = self
            .send_json_request(
                Method::PATCH,
                &repo.issue_path(issue_number),
                Some(json!({ "state": api_state })),
                true,
            )
            .await?;
        Ok(text_json_tool_result(
            render_updated_issue_state(repo, issue_number, state, &issue),
            json!({
                "repository": repo.as_json(),
                "operation": api_state,
                "issue": issue,
            }),
        ))
    }

    async fn send_json_request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        require_auth: bool,
    ) -> Result<Value, ToolError> {
        let auth_token = self.auth_token().map(str::to_string);
        if auth_token.is_none() && require_auth {
            return Err(ToolError::Execution(
                "GitHub authentication is required for this operation; set HARNESS_GITHUB_TOKEN, GITHUB_TOKEN, or GH_TOKEN".to_string(),
            ));
        }
        let response = self
            .transport
            .send(GitHubHttpRequest {
                method,
                url: self.api_url(path),
                auth_token,
                body,
            })
            .await?;
        read_github_json_response(response)
    }

    fn api_url(&self, path: &str) -> String {
        let mut url = self.api_base_url.trim_end_matches('/').to_string();
        url.push_str(path);
        url
    }

    fn auth_token(&self) -> Option<&str> {
        self.auth_token
            .as_deref()
            .filter(|value| has_trimmed_content(value))
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

#[derive(Debug, Clone, Copy)]
enum IssueState {
    Open,
    Closed,
}

impl IssueState {
    fn as_api_value(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

impl GitHubListState {
    fn as_api_value(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
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

    fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    fn api_path(&self, suffix: &str) -> String {
        format!("/repos/{}/{}/{}", self.owner, self.repo, suffix)
    }

    fn issues_path(&self) -> String {
        self.api_path("issues")
    }

    fn issue_path(&self, issue_number: u64) -> String {
        self.api_path(&format!("issues/{issue_number}"))
    }

    fn issue_comments_path(&self, issue_number: u64) -> String {
        self.api_path(&format!("issues/{issue_number}/comments"))
    }

    fn pull_requests_path(&self) -> String {
        self.api_path("pulls")
    }

    fn pull_request_path(&self, pull_number: u64) -> String {
        self.api_path(&format!("pulls/{pull_number}"))
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

fn github_api_error_message(response_text: &str) -> String {
    serde_json::from_str::<Value>(response_text)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| response_text.to_string())
}

fn read_github_json_response(response: GitHubHttpResponse) -> Result<Value, ToolError> {
    let status = response.status;
    let text = response.body;

    if !(200..300).contains(&status) {
        return Err(ToolError::Execution(format!(
            "GitHub API request failed with status {}: {}",
            status,
            github_api_error_message(&text)
        )));
    }

    parse_github_json_response(&text)
}

fn parse_github_json_response(response_text: &str) -> Result<Value, ToolError> {
    serde_json::from_str(response_text).tool_err("GitHub returned invalid JSON")
}

fn required_issue_number(issue_number: Option<u64>) -> Result<u64, ToolError> {
    required_number(issue_number, "issue_number")
}

fn required_pull_number(pull_number: Option<u64>) -> Result<u64, ToolError> {
    required_number(pull_number, "pull_number")
}

fn required_number(value: Option<u64>, field: &str) -> Result<u64, ToolError> {
    value.ok_or_else(|| {
        ToolError::InvalidArguments(format!("{field} is required for this operation"))
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

fn pull_request_create_payload(
    title: String,
    head: String,
    base: String,
    body: Option<String>,
    draft: Option<bool>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("title".to_string(), Value::String(title));
    payload.insert("head".to_string(), Value::String(head));
    payload.insert("base".to_string(), Value::String(base));
    if let Some(body) = body.filter(|value| has_trimmed_content(value)) {
        payload.insert("body".to_string(), Value::String(body));
    }
    if let Some(draft) = draft {
        payload.insert("draft".to_string(), Value::Bool(draft));
    }
    Value::Object(payload)
}

fn list_per_page_param(per_page: Option<u8>) -> String {
    per_page
        .unwrap_or(DEFAULT_LIST_PER_PAGE)
        .clamp(1, MAX_LIST_PER_PAGE)
        .to_string()
}

fn list_state_param(state: Option<GitHubListState>) -> String {
    state
        .unwrap_or(GitHubListState::Open)
        .as_api_value()
        .to_string()
}

fn list_query(state: Option<GitHubListState>, per_page: Option<u8>) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("state".to_string(), list_state_param(state)),
        ("per_page".to_string(), list_per_page_param(per_page)),
    ])
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

fn number_field(value: &Value) -> u64 {
    value
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

fn html_url_field(value: &Value) -> &str {
    string_field(value, "html_url").unwrap_or("<unknown>")
}

fn title_field(value: &Value) -> &str {
    string_field(value, "title").unwrap_or("<untitled>")
}

fn state_field<'a>(value: &'a Value, fallback: &'static str) -> &'a str {
    string_field(value, "state").unwrap_or(fallback)
}

fn body_field(value: &Value) -> Option<&str> {
    string_field(value, "body")
}

fn is_issue_list_item(item: &Value) -> bool {
    item.get("pull_request").is_none()
}

fn pull_request_ref<'a>(pull_request: &'a Value, path: &str) -> &'a str {
    pull_request
        .pointer(path)
        .and_then(Value::as_str)
        .unwrap_or("<unknown>")
}

fn pull_request_base_ref(pull_request: &Value) -> &str {
    pull_request_ref(pull_request, "/base/ref")
}

fn pull_request_head_ref(pull_request: &Value) -> &str {
    pull_request_ref(pull_request, "/head/ref")
}

fn required_json_array<'a>(value: &'a Value, message: &str) -> Result<&'a Vec<Value>, ToolError> {
    value
        .as_array()
        .ok_or_else(|| ToolError::Execution(message.to_string()))
}

fn render_comment_result(subject: &str, number: u64, repo: &RepoRef, comment: &Value) -> String {
    format!(
        "Commented on {subject} #{} in {}.\nURL: {}",
        number,
        repo.full_name(),
        html_url_field(comment)
    )
}

fn render_issue(issue: &Value) -> String {
    format!(
        "Issue #{}: {}\nState: {}\nURL: {}\n{}",
        number_field(issue),
        title_field(issue),
        state_field(issue, "<unknown>"),
        html_url_field(issue),
        render_body(body_field(issue))
    )
}

fn render_issue_list(repo: &RepoRef, issues: &[Value]) -> String {
    render_list(repo, issues, "issues", "Issues", render_issue_list_item)
}

fn render_issue_list_item(issue: &Value) -> String {
    format!(
        "- #{} [{}] {}",
        number_field(issue),
        state_field(issue, "unknown"),
        title_field(issue)
    )
}

fn render_pull_request(pull_request: &Value) -> String {
    format!(
        "Pull request #{}: {}\nState: {}\nURL: {}\nBase: {}\nHead: {}\n{}",
        number_field(pull_request),
        title_field(pull_request),
        state_field(pull_request, "<unknown>"),
        html_url_field(pull_request),
        pull_request_base_ref(pull_request),
        pull_request_head_ref(pull_request),
        render_body(body_field(pull_request))
    )
}

fn render_created_pull_request(repo: &RepoRef, pull_request: &Value) -> String {
    format!(
        "Created pull request #{} in {}.\nURL: {}",
        number_field(pull_request),
        repo.full_name(),
        html_url_field(pull_request)
    )
}

fn render_updated_issue_state(
    repo: &RepoRef,
    issue_number: u64,
    state: IssueState,
    issue: &Value,
) -> String {
    format!(
        "Issue #{} in {} is now {}.\nURL: {}",
        issue_number,
        repo.full_name(),
        state.as_api_value(),
        html_url_field(issue)
    )
}

fn render_pull_request_list(repo: &RepoRef, pull_requests: &[Value]) -> String {
    render_list(
        repo,
        pull_requests,
        "pull requests",
        "Pull requests",
        render_pull_request_list_item,
    )
}

fn render_pull_request_list_item(pull_request: &Value) -> String {
    format!(
        "- #{} [{}] {} ({} -> {})",
        number_field(pull_request),
        state_field(pull_request, "unknown"),
        title_field(pull_request),
        pull_request_head_ref(pull_request),
        pull_request_base_ref(pull_request)
    )
}

fn render_list(
    repo: &RepoRef,
    items: &[Value],
    empty_label: &str,
    header_label: &str,
    render_item: impl Fn(&Value) -> String,
) -> String {
    if items.is_empty() {
        return format!("No {empty_label} found for {}.", repo.full_name());
    }

    let repo_name = repo.full_name();
    let lines = items.iter().map(render_item).collect::<Vec<_>>().join("\n");
    format!("{header_label} for {repo_name}:\n{lines}")
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
    tool_metadata!(
        "github.issue",
        "Reads, comments on, and closes or reopens GitHub issues through the GitHub REST API.",
        ToolCapability::Network,
        json_schema_for::<GitHubIssueArgs>()
    );

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GitHubIssueArgs = parse_tool_args(args_json)?;
        self.executor.issue(args).await
    }
}

#[async_trait]
impl Tool for GitHubPullRequestTool {
    tool_metadata!(
        "github.pull_request",
        "Reads, comments on, lists, and creates GitHub pull requests through the GitHub REST API.",
        ToolCapability::Network,
        json_schema_for::<GitHubPullRequestArgs>()
    );

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GitHubPullRequestArgs = parse_tool_args(args_json)?;
        self.executor.pull_request(args).await
    }
}
