use super::*;
use harness_core::config::RemoteSearchConfig;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebArgs {
    query: String,
    #[serde(rename = "numResults", alias = "num_results")]
    #[schemars(range(min = 1, max = 20))]
    num_results: Option<u32>,
    livecrawl: Option<Crawl>,
    r#type: Option<SearchType>,
    #[serde(rename = "contextMaxCharacters", alias = "context_max_characters")]
    #[schemars(range(min = 1, max = 50_000))]
    context_max_characters: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeArgs {
    query: String,
    /// Approximate output budget; four characters per requested token.
    #[serde(rename = "tokensNum", alias = "tokens_num")]
    tokens_num: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Crawl {
    Fallback,
    Preferred,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SearchType {
    Auto,
    Fast,
    Instant,
}

pub fn register_remote_search_tools(registry: &mut ToolRegistry, config: RemoteSearchConfig) {
    let mut endpoint = config.endpoint.clone();
    if let Ok(mut url) = reqwest::Url::parse(&endpoint)
        && url.host_str() == Some("mcp.exa.ai")
        && !url.query_pairs().any(|(k, _)| k == "tools")
    {
        url.query_pairs_mut()
            .append_pair("tools", "web_search_advanced_exa");
        endpoint = url.into();
    }
    let headers = config
        .auth_token
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .map(|token| ("x-api-key".into(), token.clone()))
        .into_iter()
        .collect();
    let server = Arc::new(Server {
        name: "@remote-search".into(),
        config: McpServerConfig::Http {
            endpoint,
            headers,
            timeout_secs: config.timeout_secs.clamp(1, 120),
            enabled: true,
        },
        sessions: Mutex::new(BTreeMap::new()),
        catalog: Weak::new(),
    });
    for (id, description) in [
        ("websearch", "Search public web pages with Exa. Queries leave the local machine."),
        ("codesearch", "Find public code examples and library documentation with Exa. Use grep for local files."),
    ] {
        registry.register(Arc::new(Search {
            inner: McpTool { id: id.into(), method: "tools/call", description, server: Arc::clone(&server) },
            config: config.clone(),
        }));
    }
}

struct Search {
    inner: McpTool,
    config: RemoteSearchConfig,
}
#[async_trait::async_trait]
impl Tool for Search {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }
    fn parameters_json_schema(&self) -> Value {
        if self.id() == "websearch" {
            json!(schemars::schema_for!(WebArgs))
        } else {
            json!(schemars::schema_for!(CodeArgs))
        }
    }
    fn secret_values(&self) -> Vec<String> {
        self.inner.secret_values()
    }
    async fn close_run(&self, run: &str) -> Result<(), ToolError> {
        self.inner.close_run(run).await
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let (query, arguments, limit) = arguments(self.id(), args)?;
        if self.config.require_auth && self.inner.secret_values().is_empty() {
            return Err(failure("remote search requires an API key"));
        }
        let wire = json!({"tool":"web_search_advanced_exa","arguments":arguments});
        let mut retries = 0;
        let result = loop {
            match self.inner.call(ctx.clone(), wire.clone()).await {
                Err(ToolError::HttpStatus {
                    status: 429 | 500..=599,
                    retry_after_ms,
                }) if retries < self.config.max_retries.min(5) => {
                    retries += 1;
                    let delay = retry_after_ms
                        .unwrap_or(self.config.retry_backoff_ms)
                        .min(5000);
                    tokio::select! {
                        () = ctx.cancellation.cancelled() => return Err(ToolError::Cancelled),
                        () = tokio::time::sleep(Duration::from_millis(delay)) => {},
                    }
                }
                result => break result?,
            }
        };
        if result.is_error() {
            return Ok(result);
        }
        let (mut text, sources) = render_search(&ctx.redactor.redact_text(&result.display_text));
        let truncated = text.chars().count() > limit;
        if truncated {
            text.truncate(
                text.char_indices()
                    .nth(limit)
                    .map_or(text.len(), |(i, _)| i),
            );
            text.push_str("\n[Search output truncated to the requested budget.]");
        }
        if text.trim().is_empty() {
            text = "No search results found.".into();
        }
        Ok(ToolResult::structured(
            text,
            json!({"provider":"exa","query":query,"sources":sources,"truncated":truncated}),
        ))
    }
}

fn arguments(id: &str, args: Value) -> Result<(String, Value, usize), ToolError> {
    let invalid = |e: serde_json::Error| ToolError::InvalidArguments(e.to_string());
    let (query, mut arguments, limit) = if id == "websearch" {
        let args: WebArgs = serde_json::from_value(args).map_err(invalid)?;
        let (count, limit) = (
            args.num_results.unwrap_or(8),
            args.context_max_characters.unwrap_or(10_000),
        );
        if !(1..=20).contains(&count) || !(1..=50_000).contains(&limit) {
            return Err(ToolError::InvalidArguments(
                "numResults must be 1–20 and contextMaxCharacters 1–50000".into(),
            ));
        }
        let kind = match args.r#type.unwrap_or(SearchType::Auto) {
            SearchType::Auto => "auto",
            SearchType::Fast => "fast",
            SearchType::Instant => "instant",
        };
        let mut wire = json!({"numResults":count,"type":kind,"contextMaxCharacters":limit,"textMaxCharacters":limit});
        if matches!(args.livecrawl, Some(Crawl::Preferred)) {
            wire["maxAgeHours"] = 0.into();
        }
        (args.query, wire, limit as usize)
    } else {
        let args: CodeArgs = serde_json::from_value(args).map_err(invalid)?;
        let limit = args.tokens_num.unwrap_or(5000).clamp(1000, 50_000) * 4;
        (
            args.query,
            json!({"numResults":8,"type":"auto","textMaxCharacters":limit,"contextMaxCharacters":limit}),
            limit as usize,
        )
    };
    if query.trim().is_empty() || query.len() > 4096 {
        return Err(ToolError::InvalidArguments(
            "query must contain 1–4096 bytes of nonblank text".into(),
        ));
    }
    arguments["query"] = query.clone().into();
    Ok((query, arguments, limit))
}

fn render_search(text: &str) -> (String, Vec<String>) {
    let parsed = serde_json::from_str::<Value>(text).ok();
    let Some(results) = parsed.as_ref().and_then(|v| v["results"].as_array()) else {
        return (text.into(), vec![]);
    };
    let mut sources = Vec::new();
    let mut output = Vec::new();
    for result in results {
        let url = result["url"].as_str().unwrap_or_default();
        if !url.is_empty() {
            sources.push(url.to_owned());
        }
        let mut lines = vec![
            result["title"].as_str().unwrap_or("Untitled").to_owned(),
            url.to_owned(),
        ];
        if let Some(text) = result["text"].as_str() {
            lines.push(text.into());
        } else if let Some(highlights) = result["highlights"].as_array() {
            lines.extend(
                highlights
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
        output.push(lines.join("\n"));
    }
    (output.join("\n\n"), sources)
}
