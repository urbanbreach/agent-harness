use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use reqwest::{Method, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

pub fn register_github_tools(registry: &mut ToolRegistry, lookup: &dyn Fn(&str) -> Option<String>) {
    let first = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            lookup(key)
                .filter(|value| !value.trim().is_empty())
                .map(|s| s.trim().to_owned())
        })
    };
    let config = Arc::new(Config {
        base: first(&["HARNESS_GITHUB_API_BASE_URL"])
            .unwrap_or_else(|| "https://api.github.com".into()),
        token: first(&["HARNESS_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]),
        repository: first(&["HARNESS_GITHUB_REPOSITORY", "GITHUB_REPOSITORY"]),
    });
    for pull in [false, true] {
        registry.register(Arc::new(GitHub {
            pull,
            config: Arc::clone(&config),
        }));
    }
}
struct Config {
    base: String,
    token: Option<String>,
    repository: Option<String>,
}
struct GitHub {
    pull: bool,
    config: Arc<Config>,
}
#[derive(Clone, Copy, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Get,
    List,
    Comment,
    Close,
    Reopen,
    Create,
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum State {
    #[default]
    Open,
    Closed,
    All,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    operation: Operation,
    owner: Option<String>,
    repo: Option<String>,
    issue_number: Option<u64>,
    pull_number: Option<u64>,
    title: Option<String>,
    body: Option<String>,
    head: Option<String>,
    base: Option<String>,
    draft: Option<bool>,
    #[serde(default)]
    state: State,
    per_page: Option<u32>,
    page: Option<u32>,
}
fn invalid(message: &str) -> ToolError {
    ToolError::InvalidArguments(message.into())
}
impl GitHub {
    fn unsupported_fields(&self) -> &[&str] {
        if self.pull {
            &["issue_number"]
        } else {
            &["pull_number", "title", "head", "base", "draft"]
        }
    }
    fn parse(&self, value: Value) -> Result<Args, ToolError> {
        if self
            .unsupported_fields()
            .iter()
            .any(|key| value.get(*key).is_some())
        {
            return Err(invalid("field is not supported by this GitHub tool"));
        }
        let mut args: Args = serde_json::from_value(value)
            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        if (self.pull && matches!(args.operation, Operation::Close | Operation::Reopen))
            || (!self.pull && args.operation == Operation::Create)
        {
            return Err(invalid("operation is not supported by this GitHub tool"));
        }
        if args.owner.is_none() && args.repo.is_none() {
            let (owner, repo) = self
                .config
                .repository
                .as_deref()
                .and_then(|s| s.split_once('/'))
                .ok_or_else(|| {
                    invalid(
                        "provide owner and repo, or HARNESS_GITHUB_REPOSITORY / GITHUB_REPOSITORY",
                    )
                })?;
            args.owner = Some(owner.into());
            args.repo = Some(repo.into());
        }
        for name in [&mut args.owner, &mut args.repo] {
            let name = name
                .as_mut()
                .ok_or_else(|| invalid("owner and repo must be provided together"))?;
            if name.is_empty()
                || name.len() > 100
                || matches!(name.as_str(), "." | "..")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(invalid("invalid GitHub owner or repository name"));
            }
            name.make_ascii_lowercase();
        }
        if args.body.as_ref().is_some_and(|s| s.len() > 64 * 1024) {
            return Err(invalid("GitHub body exceeds 64 KiB"));
        }
        Ok(args)
    }
    fn request(&self, args: &Args) -> Result<(Method, Url, Option<Value>), ToolError> {
        let mut url =
            Url::parse(&self.config.base).map_err(|_| invalid("invalid GitHub API base URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid(
                "GitHub API base must be an HTTP(S) URL without credentials, query, or fragment",
            ));
        }
        let owner = args
            .owner
            .as_deref()
            .ok_or_else(|| invalid("owner is required"))?;
        let repo = args
            .repo
            .as_deref()
            .ok_or_else(|| invalid("repo is required"))?;
        let collection = if self.pull && args.operation != Operation::Comment {
            "pulls"
        } else {
            "issues"
        };
        url.path_segments_mut()
            .map_err(|_| invalid("GitHub API URL cannot be a base"))?
            .pop_if_empty()
            .extend(["repos", owner, repo, collection]);
        if !matches!(args.operation, Operation::List | Operation::Create) {
            let number = if self.pull {
                args.pull_number
            } else {
                args.issue_number
            }
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("a positive issue_number or pull_number is required"))?;
            url.path_segments_mut()
                .map_err(|_| invalid("invalid GitHub URL"))?
                .push(&number.to_string());
        }
        let required = |value: &Option<String>, name: &str| {
            value
                .as_ref()
                .filter(|s| !s.trim().is_empty() && s.len() <= 64 * 1024)
                .cloned()
                .ok_or_else(|| {
                    ToolError::InvalidArguments(format!(
                        "{name} is required and must fit within 64 KiB"
                    ))
                })
        };
        let (method, body) = match args.operation {
            Operation::List => {
                let state = match args.state {
                    State::Open => "open",
                    State::Closed => "closed",
                    State::All => "all",
                };
                url.query_pairs_mut()
                    .append_pair("state", state)
                    .append_pair(
                        "per_page",
                        &args.per_page.unwrap_or(20).clamp(1, 100).to_string(),
                    )
                    .append_pair("page", &args.page.unwrap_or(1).max(1).to_string());
                (Method::GET, None)
            }
            Operation::Get => (Method::GET, None),
            Operation::Comment => {
                url.path_segments_mut()
                    .map_err(|_| invalid("invalid GitHub URL"))?
                    .push("comments");
                (
                    Method::POST,
                    Some(json!({"body":required(&args.body,"body")?})),
                )
            }
            Operation::Close | Operation::Reopen => (
                Method::PATCH,
                Some(json!({"state":if args.operation==Operation::Close {"closed"} else {"open"}})),
            ),
            Operation::Create => {
                let mut body = json!({"title":required(&args.title,"title")?,"head":required(&args.head,"head")?,"base":required(&args.base,"base")?});
                if let Some(text) = &args.body {
                    body["body"] = text.clone().into();
                }
                if let Some(draft) = args.draft {
                    body["draft"] = draft.into();
                }
                (Method::POST, Some(body))
            }
        };
        if method != Method::GET && self.config.token.is_none() {
            return Err(ToolError::Execution("GitHub authentication is required; set HARNESS_GITHUB_TOKEN, GITHUB_TOKEN, or GH_TOKEN".into()));
        }
        Ok((method, url, body))
    }
}
#[async_trait::async_trait]
impl Tool for GitHub {
    fn id(&self) -> &str {
        if self.pull {
            "github.pull_request"
        } else {
            "github.issue"
        }
    }
    fn description(&self) -> &str {
        if self.pull {
            "Get, list, comment on, or create GitHub pull requests. Pass owner/repo or configure GITHUB_REPOSITORY; mutations require an API token. Supports page, per_page (1–100), and state for lists."
        } else {
            "Get, list, comment on, close, or reopen GitHub issues. Pass owner/repo or configure GITHUB_REPOSITORY; mutations require an API token. Supports page, per_page (1–100), and state for lists."
        }
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }
    fn parameters_json_schema(&self) -> Value {
        let mut schema = schemars::schema_for!(Args).to_value();
        if let Some(properties) = schema["properties"].as_object_mut() {
            for key in self.unsupported_fields() {
                properties.remove(*key);
            }
        }
        schema["$defs"]["Operation"]["enum"] = if self.pull {
            json!(["get", "list", "comment", "create"])
        } else {
            json!(["get", "list", "comment", "close", "reopen"])
        };
        schema
    }
    fn permission_requests(&self, value: &Value) -> Vec<(String, String)> {
        let selector = self
            .parse(value.clone())
            .map(|args| {
                format!(
                    "{}/{}:{}",
                    args.owner.unwrap_or_default(),
                    args.repo.unwrap_or_default(),
                    value["operation"].as_str().unwrap_or_default()
                )
            })
            .unwrap_or_else(|_| "invalid".into());
        vec![(self.id().into(), selector)]
    }
    fn secret_values(&self) -> Vec<String> {
        self.config.token.iter().cloned().collect()
    }
    async fn call(&self, ctx: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let args = self.parse(value)?;
        let (method, url, body) = self.request(&args)?;
        let client = crate::web::CLIENT
            .as_ref()
            .map_err(|_| ToolError::Execution("HTTP client initialization failed".into()))?;
        let mut request = client
            .request(method, url)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .header(
                "user-agent",
                concat!("agent-harness/", env!("CARGO_PKG_VERSION")),
            );
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let fetch = async {
            let response = request.send().await.map_err(|e| {
                ToolError::Execution(format!("GitHub request failed: {}", e.without_url()))
            })?;
            if !response.status().is_success() {
                return Err(ToolError::HttpStatus {
                    status: response.status().as_u16(),
                    retry_after_ms: None,
                });
            }
            let more = response
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.split(',').any(|part| part.contains("rel=\"next\"")));
            let bytes = crate::web::response_bytes(response, 2 * 1024 * 1024).await?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| ToolError::Execution("GitHub returned invalid JSON".into()))?;
            Ok((value, more))
        };
        let (mut value, more) = tokio::select! {
            biased;
            ()=ctx.cancellation.cancelled()=>return Err(ToolError::Cancelled),
            result=tokio::time::timeout(Duration::from_secs(30),fetch)=>result.map_err(|_|ToolError::Execution("GitHub request timed out".into()))??,
        };
        let key = if args.operation == Operation::List {
            let items = value
                .as_array_mut()
                .ok_or_else(|| ToolError::Execution("GitHub returned a non-array list".into()))?;
            if !items.iter().all(Value::is_object) {
                return Err(ToolError::Execution(
                    "GitHub returned an invalid list item".into(),
                ));
            }
            if !self.pull {
                items.retain(|item| item.get("pull_request").is_none());
            }
            "items"
        } else {
            if !value.is_object() {
                return Err(ToolError::Execution(
                    "GitHub returned a non-object result".into(),
                ));
            }
            if args.operation == Operation::Comment {
                "comment"
            } else if self.pull {
                "pull_request"
            } else {
                "issue"
            }
        };
        let display = if let Some(items) = value.as_array() {
            items.iter().map(render).collect::<Vec<_>>().join("\n")
        } else {
            render(&value)
        };
        let mut metadata = json!({"repository":{"owner":args.owner,"repo":args.repo},"operation":args.operation,"has_more":more,
            "query":{"state":args.state,"per_page":args.per_page.unwrap_or(20).clamp(1,100),"page":args.page.unwrap_or(1).max(1)}});
        metadata[key] = value;
        Ok(ToolResult::structured(display, metadata))
    }
}
fn render(value: &Value) -> String {
    format!(
        "#{} {} [{}]\n{}\n{}",
        value["number"].as_u64().unwrap_or(0),
        value["title"].as_str().unwrap_or_default(),
        value["state"].as_str().unwrap_or("unknown"),
        value["html_url"].as_str().unwrap_or_default(),
        value["body"].as_str().unwrap_or_default()
    )
}
