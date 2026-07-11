// allow: SIZE_OK — network tool wrapper (web fetch + search)
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::{
    DEFAULT_REMOTE_SEARCH_ENDPOINT, DEFAULT_REMOTE_SEARCH_MAX_RETRIES,
    DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS, DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS,
};
use harness_core::tool::ToolError;
use harness_core::ToolResultExt;
use serde_json::{json, Value};

use crate::env_vars::{first_env_entry, first_non_empty_env_value};
use crate::text::trimmed_non_empty;

use super::{normalize_url, CodeSearchRequest, WebSearchRequest, MAX_WEB_TIMEOUT_SECS};

const DEFAULT_WEBSEARCH_LIMIT: usize = 8;
const DEFAULT_CODE_SEARCH_TOKENS: u32 = 5_000;
const MIN_CODE_SEARCH_TOKENS: u32 = 1_000;
const MAX_CODE_SEARCH_TOKENS: u32 = 50_000;
const EXA_WEB_SEARCH_TOOL_NAME: &str = "web_search_exa";
const EXA_CODE_SEARCH_TOOL_NAME: &str = "web_search_exa";
const CODE_SEARCH_TIMEOUT_MESSAGE: &str = "Code search request timed out";
pub(super) const EMPTY_CODE_SEARCH_MESSAGE: &str = "No code snippets or documentation found. Please try a different query, be more specific about the library or programming concept, or check the spelling of framework names.";
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

#[derive(Debug, Clone)]
pub(super) struct NormalizedWebSearchRequest {
    pub(super) query: String,
    pub(super) num_results: usize,
    pub(super) livecrawl: String,
    pub(super) search_type: String,
    pub(super) context_max_characters: Option<u32>,
}

impl From<WebSearchRequest> for NormalizedWebSearchRequest {
    fn from(request: WebSearchRequest) -> Self {
        Self {
            query: request.query,
            num_results: request
                .num_results
                .unwrap_or(u32::try_from(DEFAULT_WEBSEARCH_LIMIT).unwrap_or(u32::MAX))
                .clamp(1, 20) as usize,
            livecrawl: request.livecrawl.unwrap_or_else(|| "fallback".to_string()),
            search_type: request.search_type.unwrap_or_else(|| "auto".to_string()),
            context_max_characters: request.context_max_characters,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedCodeSearchRequest {
    pub(super) query: String,
    pub(super) tokens_num: u32,
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
pub(super) struct RemoteSearchTextResult {
    pub(super) text: String,
}

impl RemoteSearchTextResult {
    pub(super) fn is_empty(&self) -> bool {
        trimmed_non_empty(&self.text).is_none()
    }
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RemoteSearchTestConfig {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub require_auth: bool,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
}

impl Default for RemoteSearchTestConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_REMOTE_SEARCH_ENDPOINT.to_string(),
            auth_token: Some("fixture-token".to_string()),
            require_auth: true,
            timeout_secs: DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS,
            max_retries: DEFAULT_REMOTE_SEARCH_MAX_RETRIES,
            retry_backoff_ms: DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteSearchHttpRequest {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub tool_name: String,
    pub arguments: Value,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct RemoteSearchHttpResponse {
    pub status: u16,
    pub retry_after_secs: Option<u64>,
    pub body: String,
}

impl RemoteSearchHttpResponse {
    #[doc(hidden)]
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            retry_after_secs: None,
            body: body.into(),
        }
    }

    #[doc(hidden)]
    pub fn with_retry_after_secs(mut self, retry_after_secs: u64) -> Self {
        self.retry_after_secs = Some(retry_after_secs);
        self
    }
}

#[async_trait]
pub trait RemoteSearchHttpTransport: Send + Sync {
    async fn execute(
        &self,
        request: RemoteSearchHttpRequest,
    ) -> Result<RemoteSearchHttpResponse, ToolError>;
}

#[derive(Debug, Clone)]
struct ReqwestRemoteSearchTransport {
    client: reqwest::Client,
}

#[async_trait]
impl RemoteSearchHttpTransport for ReqwestRemoteSearchTransport {
    async fn execute(
        &self,
        request: RemoteSearchHttpRequest,
    ) -> Result<RemoteSearchHttpResponse, ToolError> {
        let endpoint = reqwest::Url::parse(&request.endpoint).map_err(|err| {
            ToolError::Execution(format!("invalid remote search endpoint: {err}"))
        })?;
        let mut builder = self
            .client
            .post(endpoint)
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
                    "name": request.tool_name,
                    "arguments": request.arguments,
                }
            }))
            .timeout(Duration::from_secs(request.timeout_secs));

        if let Some(token) = &request.auth_token {
            builder = builder.bearer_auth(token);
        }

        let response = builder.send().await.map_err(|err| {
            if err.is_timeout() {
                ToolError::Execution(format!(
                    "remote search request timed out after {}s",
                    request.timeout_secs
                ))
            } else {
                ToolError::Execution(format!("remote search request failed: {err}"))
            }
        })?;

        let status = response.status().as_u16();
        let retry_after_secs = retry_after_secs(response.headers());
        let body = response.text().await.map_err(|err| {
            ToolError::Execution(format!("failed to read remote search response body: {err}"))
        })?;
        Ok(RemoteSearchHttpResponse {
            status,
            retry_after_secs,
            body,
        })
    }
}

#[derive(Clone)]
pub(super) struct RemoteSearchClient {
    backend: Result<RemoteSearchBackend, String>,
}

impl RemoteSearchClient {
    pub(super) fn from_env(client: reqwest::Client) -> Self {
        Self {
            backend: RemoteSearchBackend::from_env(client),
        }
    }

    pub(super) fn from_test_config(
        config: RemoteSearchTestConfig,
        transport: Arc<dyn RemoteSearchHttpTransport>,
    ) -> Result<Self, String> {
        Ok(Self {
            backend: Ok(RemoteSearchBackend::ExaMcp(ExaRemoteSearchBackend {
                config: RemoteSearchConfig::from_test_config(config)?,
                transport,
            })),
        })
    }

    pub(super) async fn web_search(
        &self,
        request: &NormalizedWebSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.backend()?.web_search(request).await
    }

    pub(super) async fn code_search(
        &self,
        request: &NormalizedCodeSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.backend()?
            .code_search(request)
            .await
            .map_err(normalize_code_search_error)
    }

    fn backend(&self) -> Result<&RemoteSearchBackend, ToolError> {
        self.backend
            .as_ref()
            .map_err(|message| ToolError::Execution(message.clone()))
    }
}

#[derive(Clone)]
enum RemoteSearchBackend {
    ExaMcp(ExaRemoteSearchBackend),
}

impl RemoteSearchBackend {
    fn from_env(client: reqwest::Client) -> Result<Self, String> {
        Ok(Self::ExaMcp(ExaRemoteSearchBackend {
            config: RemoteSearchConfig::from_env()?,
            transport: Arc::new(ReqwestRemoteSearchTransport { client }),
        }))
    }

    async fn web_search(
        &self,
        request: &NormalizedWebSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        match self {
            Self::ExaMcp(backend) => {
                backend
                    .call_text_tool(
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
        request: &NormalizedCodeSearchRequest,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        match self {
            Self::ExaMcp(backend) => {
                backend
                    .call_text_tool(
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

#[derive(Clone)]
struct ExaRemoteSearchBackend {
    config: RemoteSearchConfig,
    transport: Arc<dyn RemoteSearchHttpTransport>,
}

impl ExaRemoteSearchBackend {
    async fn call_text_tool(
        &self,
        tool_name: &str,
        arguments: Value,
        operation: &str,
    ) -> Result<RemoteSearchTextResult, ToolError> {
        self.config.ensure_auth_configured()?;

        let mut attempt = 0_u32;
        loop {
            let response = self
                .execute_call(tool_name, arguments.clone(), operation)
                .await?;
            let status = response.status;
            let retry_after_secs = response.retry_after_secs;

            if should_retry_remote_search(status) && attempt < self.config.max_retries {
                tokio::time::sleep(remote_search_retry_delay(
                    retry_after_secs,
                    self.config.retry_backoff_ms,
                ))
                .await;
                attempt += 1;
                continue;
            }

            if status == 429 {
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

            if matches!(status, 401 | 403) {
                let message = if self.config.auth_token.is_some() {
                    "remote search authentication failed"
                } else {
                    "remote search authentication is not configured"
                };
                return Err(ToolError::Execution(message.to_string()));
            }

            if !(200..300).contains(&status) {
                return Err(ToolError::Execution(format!(
                    "remote search request failed with status {status}{}",
                    backend_error_suffix(&response.body),
                )));
            }

            let text = parse_sse_text_result(&response.body)?.unwrap_or_default();
            return Ok(RemoteSearchTextResult { text });
        }
    }

    async fn execute_call(
        &self,
        tool_name: &str,
        arguments: Value,
        operation: &str,
    ) -> Result<RemoteSearchHttpResponse, ToolError> {
        self.transport
            .execute(RemoteSearchHttpRequest {
                endpoint: self.config.endpoint.as_str().to_string(),
                auth_token: self.config.auth_token.clone(),
                tool_name: tool_name.to_string(),
                arguments,
                timeout_secs: self.config.timeout_secs,
            })
            .await
            .map_err(|err| normalize_remote_search_transport_error(err, operation))
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

    fn from_test_config(config: RemoteSearchTestConfig) -> Result<Self, String> {
        Ok(Self {
            endpoint: parse_remote_search_endpoint(&config.endpoint)?,
            auth_token: config.auth_token,
            require_auth: config.require_auth,
            timeout_secs: config.timeout_secs.clamp(1, MAX_WEB_TIMEOUT_SECS),
            max_retries: config.max_retries,
            retry_backoff_ms: config
                .retry_backoff_ms
                .min(MAX_REMOTE_SEARCH_RETRY_BACKOFF_MS),
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

fn should_retry_remote_search(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
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

fn normalize_remote_search_transport_error(error: ToolError, operation: &str) -> ToolError {
    match error {
        ToolError::Execution(message) if message == "timeout" => ToolError::Execution(format!(
            "{operation} request timed out after transport timeout"
        )),
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
        let value: Value = serde_json::from_str(data).tool_err("failed to parse sse payload")?;
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
