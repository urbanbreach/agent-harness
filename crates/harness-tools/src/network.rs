use std::fs;
use std::time::Duration;

use harness_core::config::{
    DEFAULT_REMOTE_SEARCH_ENDPOINT, DEFAULT_REMOTE_SEARCH_MAX_RETRIES,
    DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS, DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS,
};
use harness_core::tool::{ArtifactRef, ToolContext, ToolError, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::env_vars::{first_env_entry, first_non_empty_env_value};
use crate::http_client;
use crate::text::trimmed_non_empty;

const DEFAULT_WEBSEARCH_LIMIT: usize = 8;
const DEFAULT_WEB_TIMEOUT_SECS: u64 = 30;
const MAX_WEB_TIMEOUT_SECS: u64 = 120;
const DEFAULT_CODE_SEARCH_TOKENS: u32 = 5_000;
const MIN_CODE_SEARCH_TOKENS: u32 = 1_000;
const MAX_CODE_SEARCH_TOKENS: u32 = 50_000;
const MAX_FETCH_BYTES: usize = 5 * 1024 * 1024;
const EXA_WEB_SEARCH_TOOL_NAME: &str = "web_search_exa";
const EXA_CODE_SEARCH_TOOL_NAME: &str = "web_search_exa";
const CODE_SEARCH_TIMEOUT_MESSAGE: &str = "Code search request timed out";
const EMPTY_CODE_SEARCH_MESSAGE: &str = "No code snippets or documentation found. Please try a different query, be more specific about the library or programming concept, or check the spelling of framework names.";
const HARNESS_WEBFETCH_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";
const HARNESS_WEBFETCH_FALLBACK_USER_AGENT: &str = "agent-harness";
const HARNESS_WEBFETCH_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const MAX_REMOTE_SEARCH_RETRY_BACKOFF_MS: u64 = 5_000;
const REMOTE_SEARCH_ENDPOINT_ENV_VARS: &[&str] =
    &["HARNESS_REMOTE_SEARCH_ENDPOINT", "HARNESS_EXA_MCP_ENDPOINT"];
const REMOTE_SEARCH_AUTH_TOKEN_ENV_VARS: &[&str] = &[
    "HARNESS_REMOTE_SEARCH_AUTH_TOKEN",
    "HARNESS_EXA_MCP_AUTH_TOKEN",
    "EXA_API_KEY",
];
const REMOTE_SEARCH_REQUIRE_AUTH_ENV_VARS: &[&str] = &["HARNESS_REMOTE_SEARCH_REQUIRE_AUTH"];
const REMOTE_SEARCH_TIMEOUT_SECS_ENV_VARS: &[&str] = &["HARNESS_REMOTE_SEARCH_TIMEOUT_SECS"];
const REMOTE_SEARCH_MAX_RETRIES_ENV_VARS: &[&str] = &["HARNESS_REMOTE_SEARCH_MAX_RETRIES"];
const REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV_VARS: &[&str] =
    &["HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS"];

pub(crate) struct NetworkExecutor {
    client: reqwest::Client,
    remote_search: RemoteSearchClient,
}

impl NetworkExecutor {
    pub(crate) fn new() -> Self {
        let client = http_client::redirect_limited_client(10, "http client");
        Self {
            remote_search: RemoteSearchClient::from_env(client.clone()),
            client,
        }
    }

    pub(crate) async fn web_fetch(
        &self,
        ctx: &ToolContext,
        request: WebFetchRequest,
    ) -> Result<ToolResult, ToolError> {
        let url = normalize_url(&request.url)?;
        let timeout_secs = request
            .timeout_secs
            .unwrap_or(DEFAULT_WEB_TIMEOUT_SECS)
            .clamp(1, MAX_WEB_TIMEOUT_SECS);
        let response = self
            .send_web_fetch_request(&url, request.format, timeout_secs)
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ToolError::Execution(format!(
                "request failed with status code: {}",
                status
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let mime = media_type(&content_type);
        let bytes = read_web_fetch_body(response).await?;
        let kind = classify_web_fetch_body(&mime, &bytes);

        match kind {
            WebFetchBodyKind::Text => {
                let text = String::from_utf8(bytes).map_err(|_| {
                    ToolError::Execution(format!(
                        "received non-text response with content type {}",
                        display_content_type(&content_type, &mime)
                    ))
                })?;
                Ok(crate::text_json_tool_result(
                    render_textual_content(request.format, &mime, &text),
                    json!({
                        "url": url.as_str(),
                        "content_type": content_type,
                        "media_type": mime,
                        "requested_format": request.format.as_str(),
                        "timeout_secs": timeout_secs,
                        "response_kind": "text",
                        "byte_len": text.len(),
                    }),
                ))
            }
            WebFetchBodyKind::Image | WebFetchBodyKind::Pdf | WebFetchBodyKind::Binary => {
                let artifact = write_web_fetch_artifact(ctx, &bytes, &mime, kind)?;
                Ok(crate::text_json_artifacts_tool_result(
                    format!(
                        "Fetched {} artifact ({} bytes, {}).\nArtifact: {}",
                        kind.label(),
                        bytes.len(),
                        display_content_type(&content_type, &mime),
                        artifact.path
                    ),
                    json!({
                        "url": url.as_str(),
                        "content_type": content_type,
                        "media_type": mime,
                        "requested_format": request.format.as_str(),
                        "timeout_secs": timeout_secs,
                        "response_kind": "artifact",
                        "artifact_kind": kind.label(),
                        "byte_len": bytes.len(),
                        "artifact": {
                            "path": artifact.path,
                            "digest": artifact.digest,
                        },
                    }),
                    vec![artifact],
                ))
            }
        }
    }

    async fn send_web_fetch_request(
        &self,
        url: &reqwest::Url,
        format: WebFetchFormat,
        timeout_secs: u64,
    ) -> Result<reqwest::Response, ToolError> {
        let initial = self
            .execute_web_fetch_request(url, format, timeout_secs, HARNESS_WEBFETCH_USER_AGENT)
            .await?;
        if initial.status() == reqwest::StatusCode::FORBIDDEN
            && initial
                .headers()
                .get("cf-mitigated")
                .and_then(|value| value.to_str().ok())
                == Some("challenge")
        {
            return self
                .execute_web_fetch_request(
                    url,
                    format,
                    timeout_secs,
                    HARNESS_WEBFETCH_FALLBACK_USER_AGENT,
                )
                .await;
        }
        Ok(initial)
    }

    async fn execute_web_fetch_request(
        &self,
        url: &reqwest::Url,
        format: WebFetchFormat,
        timeout_secs: u64,
        user_agent: &str,
    ) -> Result<reqwest::Response, ToolError> {
        self.client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::ACCEPT, accept_header(format))
            .header(
                reqwest::header::ACCEPT_LANGUAGE,
                HARNESS_WEBFETCH_ACCEPT_LANGUAGE,
            )
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    ToolError::Execution(format!("request timed out after {timeout_secs}s"))
                } else {
                    ToolError::Execution(format!("request failed: {err}"))
                }
            })
    }

    pub(crate) async fn web_search(
        &self,
        request: WebSearchRequest,
    ) -> Result<ToolResult, ToolError> {
        let request = NormalizedWebSearchRequest::from(request);
        let response = self.remote_search.web_search(&request).await?;
        let empty = response.is_empty();
        let display_text = if empty {
            "No search results found".to_string()
        } else {
            response.text
        };
        Ok(crate::text_json_tool_result(
            display_text,
            json!({
                "query": request.query,
                "numResults": request.num_results,
                "livecrawl": request.livecrawl,
                "type": request.search_type,
                "contextMaxCharacters": request.context_max_characters,
                "empty": empty,
            }),
        ))
    }

    pub(crate) async fn code_search(
        &self,
        request: CodeSearchRequest,
    ) -> Result<ToolResult, ToolError> {
        let request = NormalizedCodeSearchRequest::from(request);
        let response = self
            .remote_search
            .code_search(&request)
            .await
            .map_err(normalize_code_search_error)?;
        let empty = response.is_empty();
        let display_text = if empty {
            EMPTY_CODE_SEARCH_MESSAGE.to_string()
        } else {
            response.text
        };
        Ok(crate::text_json_tool_result(
            display_text,
            json!({
                "query": request.query,
                "tokensNum": request.tokens_num,
                "empty": empty,
            }),
        ))
    }
}

#[derive(Debug, Clone)]
struct NormalizedWebSearchRequest {
    query: String,
    num_results: usize,
    livecrawl: String,
    search_type: String,
    context_max_characters: Option<u32>,
}

impl From<WebSearchRequest> for NormalizedWebSearchRequest {
    fn from(request: WebSearchRequest) -> Self {
        Self {
            query: request.query,
            num_results: request
                .num_results
                .unwrap_or(DEFAULT_WEBSEARCH_LIMIT as u32)
                .clamp(1, 20) as usize,
            livecrawl: request.livecrawl.unwrap_or_else(|| "fallback".to_string()),
            search_type: request.search_type.unwrap_or_else(|| "auto".to_string()),
            context_max_characters: request.context_max_characters,
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedCodeSearchRequest {
    query: String,
    tokens_num: u32,
}

impl From<CodeSearchRequest> for NormalizedCodeSearchRequest {
    fn from(request: CodeSearchRequest) -> Self {
        Self {
            query: request.query,
            tokens_num: request
                .tokens_num
                .unwrap_or(DEFAULT_CODE_SEARCH_TOKENS)
                .clamp(MIN_CODE_SEARCH_TOKENS, MAX_CODE_SEARCH_TOKENS),
        }
    }
}

#[derive(Debug, Clone)]
struct RemoteSearchTextResult {
    text: String,
}

impl RemoteSearchTextResult {
    fn is_empty(&self) -> bool {
        trimmed_non_empty(&self.text).is_none()
    }
}

#[derive(Debug, Clone)]
struct RemoteSearchClient {
    client: reqwest::Client,
    backend: Result<RemoteSearchBackend, String>,
}

impl RemoteSearchClient {
    fn from_env(client: reqwest::Client) -> Self {
        Self {
            client,
            backend: RemoteSearchBackend::from_env(),
        }
    }

    async fn web_search(
        &self,
        request: &NormalizedWebSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.backend()?.web_search(&self.client, request).await
    }

    async fn code_search(
        &self,
        request: &NormalizedCodeSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.backend()?.code_search(&self.client, request).await
    }

    fn backend(&self) -> Result<&RemoteSearchBackend, ToolError> {
        self.backend
            .as_ref()
            .map_err(|message| ToolError::Execution(message.clone()))
    }
}

#[derive(Debug, Clone)]
enum RemoteSearchBackend {
    ExaMcp(ExaRemoteSearchBackend),
}

impl RemoteSearchBackend {
    fn from_env() -> Result<Self, String> {
        Ok(Self::ExaMcp(ExaRemoteSearchBackend {
            config: RemoteSearchConfig::from_env()?,
        }))
    }

    async fn web_search(
        &self,
        client: &reqwest::Client,
        request: &NormalizedWebSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        match self {
            Self::ExaMcp(backend) => {
                backend
                    .call_text_tool(
                        client,
                        EXA_WEB_SEARCH_TOOL_NAME,
                        json!({
                            "query": request.query,
                            "numResults": request.num_results,
                            "livecrawl": request.livecrawl,
                            "type": request.search_type,
                            "contextMaxCharacters": request.context_max_characters,
                        }),
                        "search.web",
                    )
                    .await
            }
        }
    }

    async fn code_search(
        &self,
        client: &reqwest::Client,
        request: &NormalizedCodeSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        match self {
            Self::ExaMcp(backend) => {
                backend
                    .call_text_tool(
                        client,
                        EXA_CODE_SEARCH_TOOL_NAME,
                        json!({
                            "query": request.query,
                            "tokensNum": request.tokens_num,
                        }),
                        "search.code",
                    )
                    .await
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ExaRemoteSearchBackend {
    config: RemoteSearchConfig,
}

impl ExaRemoteSearchBackend {
    async fn call_text_tool(
        &self,
        client: &reqwest::Client,
        tool_name: &str,
        arguments: Value,
        operation: &str,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.config.ensure_auth_configured()?;

        let mut attempt = 0_u32;
        loop {
            let response = self
                .execute_call(client, tool_name, arguments.clone(), operation)
                .await?;
            let status = response.status();
            let retry_after_secs = retry_after_secs(response.headers());

            if should_retry_remote_search(status) && attempt < self.config.max_retries {
                tokio::time::sleep(remote_search_retry_delay(
                    retry_after_secs,
                    self.config.retry_backoff_ms,
                ))
                .await;
                attempt += 1;
                continue;
            }

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let attempts = attempt + 1;
                let retry_hint = retry_after_secs
                    .map(|seconds| format!("; retry after {seconds}s"))
                    .unwrap_or_default();
                return Err(ToolError::Execution(format!(
                    "remote search rate limit exceeded after {attempts} attempt{}{}",
                    if attempts == 1 { "" } else { "s" },
                    retry_hint,
                )));
            }

            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                let message = if self.config.auth_token.is_some() {
                    "remote search authentication failed"
                } else {
                    "remote search authentication is not configured"
                };
                return Err(ToolError::Execution(message.to_string()));
            }

            if !status.is_success() {
                let body = response.text().await.map_err(|err| {
                    ToolError::Execution(format!(
                        "remote search request failed with status {status}; failed to read error body: {err}"
                    ))
                })?;
                return Err(ToolError::Execution(format!(
                    "remote search request failed with status {status}{}",
                    backend_error_suffix(&body),
                )));
            }

            let body = response.text().await.map_err(|err| {
                ToolError::Execution(format!("failed to read remote search response body: {err}"))
            })?;
            let text = parse_sse_text_result(&body)?.unwrap_or_default();
            return Ok(RemoteSearchTextResult { text });
        }
    }

    async fn execute_call(
        &self,
        client: &reqwest::Client,
        tool_name: &str,
        arguments: Value,
        operation: &str,
    ) -> Result<reqwest::Response, ToolError> {
        let mut request = client
            .post(self.config.endpoint.clone())
            .header(reqwest::header::USER_AGENT, "agent-harness")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": tool_name,
                    "arguments": arguments,
                }
            }))
            .timeout(Duration::from_secs(self.config.timeout_secs));

        if let Some(token) = &self.config.auth_token {
            request = request.bearer_auth(token);
        }

        request.send().await.map_err(|err| {
            if err.is_timeout() {
                ToolError::Execution(format!(
                    "{operation} request timed out after {}s",
                    self.config.timeout_secs
                ))
            } else {
                ToolError::Execution(format!("remote search request failed: {err}"))
            }
        })
    }
}

#[derive(Debug, Clone)]
struct RemoteSearchConfig {
    endpoint: reqwest::Url,
    auth_token: Option<String>,
    require_auth: bool,
    timeout_secs: u64,
    max_retries: u32,
    retry_backoff_ms: u64,
}

impl RemoteSearchConfig {
    fn from_env() -> Result<Self, String> {
        let endpoint = first_non_empty_env_value(REMOTE_SEARCH_ENDPOINT_ENV_VARS)
            .unwrap_or_else(|| DEFAULT_REMOTE_SEARCH_ENDPOINT.to_string());
        let endpoint = parse_remote_search_endpoint(&endpoint)?;
        let auth_token = first_non_empty_env_value(REMOTE_SEARCH_AUTH_TOKEN_ENV_VARS);
        let require_auth = read_bool_env(REMOTE_SEARCH_REQUIRE_AUTH_ENV_VARS)?.unwrap_or(false);
        let timeout_secs = read_parsed_env::<u64>(REMOTE_SEARCH_TIMEOUT_SECS_ENV_VARS)?
            .unwrap_or(DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS)
            .clamp(1, MAX_WEB_TIMEOUT_SECS);
        let max_retries = read_parsed_env::<u32>(REMOTE_SEARCH_MAX_RETRIES_ENV_VARS)?
            .unwrap_or(DEFAULT_REMOTE_SEARCH_MAX_RETRIES);
        let retry_backoff_ms = read_parsed_env::<u64>(REMOTE_SEARCH_RETRY_BACKOFF_MS_ENV_VARS)?
            .unwrap_or(DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS)
            .min(MAX_REMOTE_SEARCH_RETRY_BACKOFF_MS);

        Ok(Self {
            endpoint,
            auth_token,
            require_auth,
            timeout_secs,
            max_retries,
            retry_backoff_ms,
        })
    }

    fn ensure_auth_configured(&self) -> Result<(), ToolError> {
        if self.require_auth && self.auth_token.is_none() {
            return Err(ToolError::Execution(format!(
                "remote search authentication is not configured; set one of {}",
                REMOTE_SEARCH_AUTH_TOKEN_ENV_VARS.join(", "),
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WebFetchFormat {
    Text,
    Markdown,
    Html,
}

#[derive(Debug, Clone)]
pub(crate) struct WebFetchRequest {
    pub(crate) url: String,
    pub(crate) format: WebFetchFormat,
    pub(crate) timeout_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct WebSearchRequest {
    pub(crate) query: String,
    pub(crate) num_results: Option<u32>,
    pub(crate) livecrawl: Option<String>,
    pub(crate) search_type: Option<String>,
    pub(crate) context_max_characters: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodeSearchRequest {
    pub(crate) query: String,
    pub(crate) tokens_num: Option<u32>,
}

fn accept_header(format: WebFetchFormat) -> &'static str {
    match format {
        WebFetchFormat::Markdown => {
            "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1"
        }
        WebFetchFormat::Text => {
            "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1"
        }
        WebFetchFormat::Html => {
            "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, text/markdown;q=0.7, */*;q=0.1"
        }
    }
}

fn normalize_url(url: &str) -> Result<reqwest::Url, ToolError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| ToolError::InvalidArguments(format!("invalid URL: {err}")))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err(ToolError::InvalidArguments(
            "URL must start with http:// or https://".to_string(),
        )),
    }
}

async fn read_web_fetch_body(mut response: reqwest::Response) -> Result<Vec<u8>, ToolError> {
    if response.content_length().unwrap_or_default() > MAX_FETCH_BYTES as u64 {
        return Err(ToolError::Execution(
            "response too large (exceeds 5MB limit)".to_string(),
        ));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to read response body: {err}")))?
    {
        if body.len() + chunk.len() > MAX_FETCH_BYTES {
            return Err(ToolError::Execution(
                "response too large (exceeds 5MB limit)".to_string(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebFetchBodyKind {
    Text,
    Image,
    Pdf,
    Binary,
}

impl WebFetchBodyKind {
    fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Binary => "binary",
        }
    }
}

impl WebFetchFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Html => "html",
        }
    }
}

fn classify_web_fetch_body(mime: &str, bytes: &[u8]) -> WebFetchBodyKind {
    if mime == "application/pdf" {
        return WebFetchBodyKind::Pdf;
    }
    if is_binary_image_mime(mime) {
        return WebFetchBodyKind::Image;
    }
    if is_textual_mime(mime) || std::str::from_utf8(bytes).is_ok() {
        return WebFetchBodyKind::Text;
    }
    WebFetchBodyKind::Binary
}

fn is_binary_image_mime(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml"
}

fn is_textual_mime(mime: &str) -> bool {
    mime.is_empty()
        || mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/xhtml+xml"
        || mime == "image/svg+xml"
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
}

fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn display_content_type<'a>(content_type: &'a str, mime: &'a str) -> &'a str {
    trimmed_non_empty(content_type).map_or(mime, |_| content_type)
}

fn render_textual_content(format: WebFetchFormat, mime: &str, text: &str) -> String {
    if is_html_mime(mime) {
        return match format {
            WebFetchFormat::Html => text.to_string(),
            WebFetchFormat::Text => html_to_text(text),
            WebFetchFormat::Markdown => html_to_markdown(text),
        };
    }
    text.to_string()
}

fn is_html_mime(mime: &str) -> bool {
    mime == "text/html" || mime == "application/xhtml+xml"
}

fn write_web_fetch_artifact(
    ctx: &ToolContext,
    bytes: &[u8],
    mime: &str,
    kind: WebFetchBodyKind,
) -> Result<ArtifactRef, ToolError> {
    let relative = format!(
        "toolcalls/{}/web.fetch.{}",
        ctx.tool_call_id,
        artifact_extension(mime, kind)
    );
    let target = ctx.artifacts_dir.join(&relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create artifact directory: {err}"))
        })?;
    }
    fs::write(&target, bytes).map_err(|err| {
        ToolError::Execution(format!("failed to write web.fetch artifact: {err}"))
    })?;
    Ok(ArtifactRef {
        path: format!("artifacts/{relative}"),
        digest: None,
    })
}

fn artifact_extension(mime: &str, kind: WebFetchBodyKind) -> &'static str {
    match (kind, mime) {
        (WebFetchBodyKind::Pdf, _) => "pdf",
        (WebFetchBodyKind::Image, "image/png") => "png",
        (WebFetchBodyKind::Image, "image/jpeg") => "jpg",
        (WebFetchBodyKind::Image, "image/gif") => "gif",
        (WebFetchBodyKind::Image, "image/webp") => "webp",
        (WebFetchBodyKind::Image, "image/bmp") => "bmp",
        (WebFetchBodyKind::Image, "image/tiff") => "tiff",
        (WebFetchBodyKind::Image, "image/avif") => "avif",
        (WebFetchBodyKind::Image, "image/heic") => "heic",
        (WebFetchBodyKind::Image, "image/heif") => "heif",
        (WebFetchBodyKind::Image, "image/x-icon") => "ico",
        (WebFetchBodyKind::Image, _) => "img",
        (WebFetchBodyKind::Binary, _) => "bin",
        (WebFetchBodyKind::Text, _) => "txt",
    }
}

fn html_to_text(html: &str) -> String {
    let without_tags = Regex::new(r"<[^>]+>")
        .expect("html tag regex")
        .replace_all(html, " ");
    let decoded = decode_basic_html_entities(&without_tags);
    Regex::new(r"\s+")
        .expect("whitespace regex")
        .replace_all(decoded.trim(), " ")
        .to_string()
}

fn html_to_markdown(html: &str) -> String {
    let markdown = html
        .replace("\r\n", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    let markdown = Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>")
        .expect("h1 regex")
        .replace_all(&markdown, "# $1\n\n")
        .into_owned();
    let markdown = Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>")
        .expect("h2 regex")
        .replace_all(&markdown, "## $1\n\n")
        .into_owned();
    let markdown = Regex::new(r"(?is)<h3[^>]*>(.*?)</h3>")
        .expect("h3 regex")
        .replace_all(&markdown, "### $1\n\n")
        .into_owned();
    let markdown = Regex::new(r"(?is)<li[^>]*>(.*?)</li>")
        .expect("li regex")
        .replace_all(&markdown, "- $1\n")
        .into_owned();
    let markdown = Regex::new(r"(?is)</?(p|div|section|article|main|header|footer|ul|ol)[^>]*>")
        .expect("block regex")
        .replace_all(&markdown, "\n\n")
        .into_owned();
    let markdown = Regex::new(r#"(?is)<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#)
        .expect("link regex")
        .replace_all(&markdown, "[$2]($1)")
        .into_owned();
    let markdown = Regex::new(r"<[^>]+>")
        .expect("html tag regex")
        .replace_all(&markdown, " ")
        .into_owned();
    let decoded = decode_basic_html_entities(&markdown);
    let collapsed_spaces = Regex::new(r"[ \t]+")
        .expect("markdown whitespace regex")
        .replace_all(decoded.trim(), " ")
        .to_string();
    Regex::new(r"\n{3,}")
        .expect("markdown blank line regex")
        .replace_all(&collapsed_spaces, "\n\n")
        .to_string()
}

fn decode_basic_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#x27;", "'")
        .replace("&quot;", "\"")
}

fn read_bool_env(keys: &[&str]) -> Result<Option<bool>, String> {
    let Some((key, value)) = first_env_entry(keys) else {
        return Ok(None);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        other => Err(format!(
            "invalid boolean value {other:?} in {key}; expected true/false",
        )),
    }
}

fn read_parsed_env<T>(keys: &[&str]) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let Some((key, value)) = first_env_entry(keys) else {
        return Ok(None);
    };
    value
        .trim()
        .parse::<T>()
        .map(Some)
        .map_err(|err| format!("invalid integer value in {key}: {err}"))
}

fn parse_remote_search_endpoint(endpoint: &str) -> Result<reqwest::Url, String> {
    normalize_url(endpoint).map_err(|err| format!("invalid remote search endpoint: {err}"))
}

fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn should_retry_remote_search(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn remote_search_retry_delay(retry_after_secs: Option<u64>, retry_backoff_ms: u64) -> Duration {
    let header_delay_ms = retry_after_secs.unwrap_or_default().saturating_mul(1_000);
    Duration::from_millis(
        header_delay_ms
            .max(retry_backoff_ms)
            .min(MAX_REMOTE_SEARCH_RETRY_BACKOFF_MS),
    )
}

fn backend_error_suffix(body: &str) -> String {
    trimmed_non_empty(body).map_or_else(String::new, |trimmed| format!(": {trimmed}"))
}

fn normalize_code_search_error(error: ToolError) -> ToolError {
    match error {
        ToolError::Execution(message) if is_code_search_timeout(&message) => {
            ToolError::Execution(CODE_SEARCH_TIMEOUT_MESSAGE.to_string())
        }
        other => other,
    }
}

fn is_code_search_timeout(message: &str) -> bool {
    message.starts_with("search.code request timed out after ")
}

fn parse_sse_text_result(body: &str) -> Result<Option<String>, ToolError> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(content) = value["result"]["content"].as_array() {
            return Ok(extract_text_result(content));
        }
    }

    let mut saw_data_frame = false;
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        saw_data_frame = true;
        let value: Value = serde_json::from_str(data)
            .map_err(|err| ToolError::Execution(format!("failed to parse sse payload: {err}")))?;
        if let Some(content) = value["result"]["content"].as_array() {
            return Ok(extract_text_result(content));
        }
    }

    if saw_data_frame {
        Ok(None)
    } else {
        Err(ToolError::Execution(
            "search response did not include a text payload".to_string(),
        ))
    }
}

fn extract_text_result(content: &[Value]) -> Option<String> {
    content.iter().find_map(|item| {
        item["text"]
            .as_str()
            .and_then(trimmed_non_empty)
            .map(ToOwned::to_owned)
    })
}
