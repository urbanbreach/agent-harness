use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::tool::{ArtifactRef, ToolContext, ToolError, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::http_client;
use crate::text::trimmed_non_empty;

mod remote_search;

use remote_search::{
    NormalizedCodeSearchRequest, NormalizedWebSearchRequest, RemoteSearchClient,
    EMPTY_CODE_SEARCH_MESSAGE,
};
pub use remote_search::{
    RemoteSearchHttpRequest, RemoteSearchHttpResponse, RemoteSearchHttpTransport,
    RemoteSearchTestConfig,
};

const DEFAULT_WEB_TIMEOUT_SECS: u64 = 30;
const MAX_WEB_TIMEOUT_SECS: u64 = 120;
const MAX_FETCH_BYTES: usize = 5 * 1024 * 1024;
const HARNESS_WEBFETCH_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";
const HARNESS_WEBFETCH_FALLBACK_USER_AGENT: &str = "agent-harness";
const HARNESS_WEBFETCH_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

pub(crate) struct NetworkExecutor {
    web_fetch_transport: Arc<dyn WebFetchHttpTransport>,
    remote_search: RemoteSearchClient,
}

impl NetworkExecutor {
    pub(crate) fn new() -> Self {
        let client = http_client::redirect_limited_client(10, "http client");
        Self {
            remote_search: RemoteSearchClient::from_env(client.clone()),
            web_fetch_transport: Arc::new(ReqwestWebFetchHttpTransport { client }),
        }
    }

    #[doc(hidden)]
    pub(crate) fn with_remote_search_transport(
        config: RemoteSearchTestConfig,
        transport: Arc<dyn RemoteSearchHttpTransport>,
    ) -> Result<Self, String> {
        let client = http_client::redirect_limited_client(10, "http client");
        Ok(Self {
            remote_search: RemoteSearchClient::from_test_config(config, transport)?,
            web_fetch_transport: Arc::new(ReqwestWebFetchHttpTransport { client }),
        })
    }

    #[doc(hidden)]
    pub(crate) fn with_web_fetch_transport(transport: Arc<dyn WebFetchHttpTransport>) -> Self {
        let client = http_client::redirect_limited_client(10, "http client");
        Self {
            remote_search: RemoteSearchClient::from_env(client),
            web_fetch_transport: transport,
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
        let status = response.status;
        if !(200..300).contains(&status) {
            return Err(ToolError::Execution(format!(
                "request failed with status code: {}",
                status
            )));
        }
        let content_type = response.header(reqwest::header::CONTENT_TYPE.as_str());
        let mime = media_type(&content_type);
        let bytes = read_web_fetch_body(response)?;
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
    ) -> Result<WebFetchHttpResponse, ToolError> {
        let initial = self
            .execute_web_fetch_request(url, format, timeout_secs, HARNESS_WEBFETCH_USER_AGENT)
            .await?;
        if initial.status == 403 && initial.header("cf-mitigated") == "challenge" {
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
    ) -> Result<WebFetchHttpResponse, ToolError> {
        self.web_fetch_transport
            .execute(WebFetchHttpRequest {
                url: url.as_str().to_string(),
                user_agent: user_agent.to_string(),
                accept: accept_header(format).to_string(),
                accept_language: HARNESS_WEBFETCH_ACCEPT_LANGUAGE.to_string(),
                timeout_secs,
            })
            .await
            .map_err(|err| normalize_web_fetch_transport_error(err, timeout_secs))
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
        let response = self.remote_search.code_search(&request).await?;
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
pub struct WebFetchHttpRequest {
    pub url: String,
    pub user_agent: String,
    pub accept: String,
    pub accept_language: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct WebFetchHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub content_length: Option<u64>,
    pub body: Vec<u8>,
}

impl WebFetchHttpResponse {
    #[doc(hidden)]
    pub fn new(
        status: u16,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            status,
            headers: normalize_web_fetch_headers(headers),
            content_length: None,
            body: body.into(),
        }
    }

    #[doc(hidden)]
    pub fn with_content_length(mut self, content_length: u64) -> Self {
        self.content_length = Some(content_length);
        self
    }

    fn header(&self, name: &str) -> String {
        self.headers
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }
}

fn normalize_web_fetch_headers(
    headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .map(|(name, value)| (name.into().to_ascii_lowercase(), value.into()))
        .collect()
}

#[async_trait]
pub trait WebFetchHttpTransport: Send + Sync {
    async fn execute(
        &self,
        request: WebFetchHttpRequest,
    ) -> Result<WebFetchHttpResponse, ToolError>;
}

#[derive(Debug, Clone)]
struct ReqwestWebFetchHttpTransport {
    client: reqwest::Client,
}

#[async_trait]
impl WebFetchHttpTransport for ReqwestWebFetchHttpTransport {
    async fn execute(
        &self,
        request: WebFetchHttpRequest,
    ) -> Result<WebFetchHttpResponse, ToolError> {
        let url = reqwest::Url::parse(&request.url)
            .map_err(|err| ToolError::InvalidArguments(format!("invalid URL: {err}")))?;
        let mut response = self
            .client
            .get(url)
            .header(reqwest::header::USER_AGENT, request.user_agent)
            .header(reqwest::header::ACCEPT, request.accept)
            .header(reqwest::header::ACCEPT_LANGUAGE, request.accept_language)
            .timeout(Duration::from_secs(request.timeout_secs))
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    ToolError::Execution("timeout".to_string())
                } else {
                    ToolError::Execution(format!("request failed: {err}"))
                }
            })?;
        let status = response.status().as_u16();
        let content_length = response.content_length();
        if content_length.unwrap_or_default() > MAX_FETCH_BYTES as u64 {
            return Ok(WebFetchHttpResponse {
                status,
                headers: BTreeMap::new(),
                content_length,
                body: Vec::new(),
            });
        }
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
            })
            .collect::<BTreeMap<_, _>>();
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
        Ok(WebFetchHttpResponse {
            status,
            headers,
            content_length,
            body,
        })
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

fn read_web_fetch_body(response: WebFetchHttpResponse) -> Result<Vec<u8>, ToolError> {
    if response.content_length.unwrap_or_default() > MAX_FETCH_BYTES as u64 {
        return Err(ToolError::Execution(
            "response too large (exceeds 5MB limit)".to_string(),
        ));
    }

    if response.body.len() > MAX_FETCH_BYTES {
        return Err(ToolError::Execution(
            "response too large (exceeds 5MB limit)".to_string(),
        ));
    }
    Ok(response.body)
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

static HTML_TAGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap_or_abort());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap_or_abort());
static H1_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").unwrap_or_abort());
static H2_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>").unwrap_or_abort());
static H3_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<h3[^>]*>(.*?)</h3>").unwrap_or_abort());
static LI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<li[^>]*>(.*?)</li>").unwrap_or_abort());
static BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)</?(p|div|section|article|main|header|footer|ul|ol)[^>]*>").unwrap_or_abort()
});
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap_or_abort()
});
static MARKDOWN_WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]+").unwrap_or_abort());
static MARKDOWN_BLANK_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").unwrap_or_abort());

fn html_to_text(html: &str) -> String {
    let without_tags = HTML_TAGS_RE.replace_all(html, " ");
    let decoded = decode_basic_html_entities(&without_tags);
    WHITESPACE_RE.replace_all(decoded.trim(), " ").to_string()
}

fn html_to_markdown(html: &str) -> String {
    let markdown = html
        .replace("\r\n", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    let markdown = H1_RE.replace_all(&markdown, "# $1\n\n").into_owned();
    let markdown = H2_RE.replace_all(&markdown, "## $1\n\n").into_owned();
    let markdown = H3_RE.replace_all(&markdown, "### $1\n\n").into_owned();
    let markdown = LI_RE.replace_all(&markdown, "- $1\n").into_owned();
    let markdown = BLOCK_RE.replace_all(&markdown, "\n\n").into_owned();
    let markdown = LINK_RE.replace_all(&markdown, "[$2]($1)").into_owned();
    let markdown = HTML_TAGS_RE.replace_all(&markdown, " ").into_owned();
    let decoded = decode_basic_html_entities(&markdown);
    let collapsed_spaces = MARKDOWN_WHITESPACE_RE
        .replace_all(decoded.trim(), " ")
        .to_string();
    MARKDOWN_BLANK_LINE_RE
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

fn normalize_web_fetch_transport_error(error: ToolError, timeout_secs: u64) -> ToolError {
    match error {
        ToolError::Execution(message) if message == "timeout" => {
            ToolError::Execution(format!("request timed out after {timeout_secs}s"))
        }
        other => other,
    }
}
