use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_stream::{self as stream, StreamExt};

use crate::openai::{OpenAiHttpResponse, OpenAiHttpTransport};
use crate::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};

const CASSETTE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CassetteMode {
    Replay,
    Record,
    Auto,
}

impl CassetteMode {
    pub fn resolve_for_ci(self, ci: bool) -> Self {
        if ci {
            Self::Replay
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderCassette {
    pub version: u32,
    pub interactions: Vec<CassetteInteraction>,
}

impl ProviderCassette {
    pub fn new(interactions: Vec<CassetteInteraction>) -> Self {
        Self {
            version: CASSETTE_VERSION,
            interactions,
        }
    }

    pub fn read_from(path: &Path) -> Result<Self, CassetteError> {
        let body = fs::read_to_string(path).map_err(|source| CassetteError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let cassette: Self =
            serde_json::from_str(&body).map_err(|source| CassetteError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        if cassette.version != CASSETTE_VERSION {
            return Err(CassetteError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: cassette.version,
            });
        }
        Ok(cassette)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), CassetteError> {
        assert_cassette_is_safe(self)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| CassetteError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self).map_err(CassetteError::Serialize)?;
        fs::write(path, format!("{body}\n")).map_err(|source| CassetteError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CassetteInteraction {
    pub request: CompletionRequest,
    pub events: Vec<ProviderStreamEvent>,
}

impl CassetteInteraction {
    pub fn new(request: CompletionRequest, events: Vec<ProviderStreamEvent>) -> Self {
        Self { request, events }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpCassette {
    pub version: u32,
    pub interactions: Vec<OpenAiHttpInteraction>,
}

impl OpenAiHttpCassette {
    pub fn new(interactions: Vec<OpenAiHttpInteraction>) -> Self {
        Self {
            version: CASSETTE_VERSION,
            interactions,
        }
    }

    pub fn read_from(path: &Path) -> Result<Self, CassetteError> {
        let body = fs::read_to_string(path).map_err(|source| CassetteError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let cassette: Self =
            serde_json::from_str(&body).map_err(|source| CassetteError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        if cassette.version != CASSETTE_VERSION {
            return Err(CassetteError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: cassette.version,
            });
        }
        Ok(cassette)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), CassetteError> {
        assert_http_cassette_is_safe(self)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| CassetteError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self).map_err(CassetteError::Serialize)?;
        fs::write(path, format!("{body}\n")).map_err(|source| CassetteError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpInteraction {
    pub request: OpenAiHttpRecordedRequest,
    pub response: OpenAiHttpRecordedResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpRecordedRequest {
    pub endpoint_path: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpRecordedResponse {
    pub status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum CassetteError {
    #[error("failed to read cassette {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse cassette {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported cassette version {version} in {path}")]
    UnsupportedVersion { path: PathBuf, version: u32 },
    #[error("missing cassette {path} in replay mode")]
    MissingReplayCassette { path: PathBuf },
    #[error("failed to serialize cassette: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to create cassette directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write cassette {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cassette request mismatch at interaction {index}: expected {expected}, got {actual}")]
    RequestMismatch {
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("cassette exhausted after {count} interaction(s)")]
    Exhausted { count: usize },
    #[error("unsafe cassette secret detected: {kind}")]
    UnsafeSecret { kind: String },
}

#[derive(Debug)]
struct CassetteState {
    cassette: ProviderCassette,
    cursor: usize,
    record: bool,
}

/// Provider wrapper that replays or records provider-level completion events from a cassette.
///
/// Matching is intentionally sequential: call N must match interaction N. This makes retry,
/// fallback, and polling behavior observable instead of hidden behind content-keyed dispatch.
pub struct RecordedProvider<P> {
    inner: P,
    path: PathBuf,
    state: Mutex<CassetteState>,
}

#[derive(Debug)]
struct OpenAiHttpCassetteState {
    cassette: OpenAiHttpCassette,
    cursor: usize,
    record: bool,
}

/// HTTP transport wrapper that records or replays OpenAI-compatible wire requests.
///
/// Matching is sequential and stores only a safe request shape: URL paths, an
/// explicit header allow-list, and the JSON body. Bearer tokens and URL query
/// strings are never written to cassette files.
pub struct RecordedOpenAiHttpTransport {
    inner: ArcOpenAiHttpTransport,
    path: PathBuf,
    state: Mutex<OpenAiHttpCassetteState>,
}

type ArcOpenAiHttpTransport = std::sync::Arc<dyn OpenAiHttpTransport>;

impl RecordedOpenAiHttpTransport {
    pub fn new(
        inner: ArcOpenAiHttpTransport,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
    ) -> Result<Self, CassetteError> {
        let ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty() && value != "0");
        Self::with_ci(inner, path, mode, ci)
    }

    pub fn with_ci(
        inner: ArcOpenAiHttpTransport,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
        ci: bool,
    ) -> Result<Self, CassetteError> {
        let path = path.into();
        let mode = mode.resolve_for_ci(ci);
        let exists = path.exists();
        let cassette = match mode {
            CassetteMode::Replay if !exists => {
                return Err(CassetteError::MissingReplayCassette { path })
            }
            CassetteMode::Replay => OpenAiHttpCassette::read_from(&path)?,
            CassetteMode::Auto if exists => OpenAiHttpCassette::read_from(&path)?,
            CassetteMode::Record | CassetteMode::Auto => OpenAiHttpCassette::new(Vec::new()),
        };
        let record =
            matches!(mode, CassetteMode::Record) || matches!(mode, CassetteMode::Auto) && !exists;
        Ok(Self {
            inner,
            path,
            state: Mutex::new(OpenAiHttpCassetteState {
                cassette,
                cursor: 0,
                record,
            }),
        })
    }

    fn replay(
        &self,
        request: OpenAiHttpRecordedRequest,
    ) -> Result<OpenAiHttpRecordedResponse, CassetteError> {
        let mut state = self
            .state
            .lock()
            .expect("http cassette state lock poisoned");
        let index = state.cursor;
        let Some(interaction) = state.cassette.interactions.get(index) else {
            return Err(CassetteError::Exhausted {
                count: state.cassette.interactions.len(),
            });
        };
        if interaction.request != request {
            return Err(CassetteError::RequestMismatch {
                index,
                expected: compact_json(&interaction.request),
                actual: compact_json(&request),
            });
        }
        let response = interaction.response.clone();
        state.cursor += 1;
        Ok(response)
    }

    fn append_recording(
        &self,
        request: OpenAiHttpRecordedRequest,
        response: OpenAiHttpRecordedResponse,
    ) -> Result<(), CassetteError> {
        let mut state = self
            .state
            .lock()
            .expect("http cassette state lock poisoned");
        state
            .cassette
            .interactions
            .push(OpenAiHttpInteraction { request, response });
        state.cassette.write_to(&self.path)
    }
}

#[async_trait]
impl OpenAiHttpTransport for RecordedOpenAiHttpTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        let request = OpenAiHttpRecordedRequest {
            endpoint_path: scrub_endpoint_to_path(&endpoint),
            headers: allowed_request_headers(&headers),
            body: body.clone(),
        };
        let record = self
            .state
            .lock()
            .expect("http cassette state lock poisoned")
            .record;
        if !record {
            return self
                .replay(request)
                .map(recorded_response_to_http_response)
                .map_err(|err| err.to_string());
        }

        let response = self
            .inner
            .post_json(endpoint, headers, bearer_token, body)
            .await?;
        let recorded_response = record_http_response(response).await?;
        self.append_recording(request, recorded_response.clone())
            .map_err(|err| err.to_string())?;
        Ok(recorded_response_to_http_response(recorded_response))
    }
}

impl<P> RecordedProvider<P> {
    pub fn new(
        inner: P,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
    ) -> Result<Self, CassetteError> {
        let ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty() && value != "0");
        Self::with_ci(inner, path, mode, ci)
    }

    pub fn with_ci(
        inner: P,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
        ci: bool,
    ) -> Result<Self, CassetteError> {
        let path = path.into();
        let mode = mode.resolve_for_ci(ci);
        let exists = path.exists();
        let cassette = match mode {
            CassetteMode::Replay if !exists => {
                return Err(CassetteError::MissingReplayCassette { path })
            }
            CassetteMode::Replay => ProviderCassette::read_from(&path)?,
            CassetteMode::Auto if exists => ProviderCassette::read_from(&path)?,
            CassetteMode::Record | CassetteMode::Auto => ProviderCassette::new(Vec::new()),
        };
        let record =
            matches!(mode, CassetteMode::Record) || matches!(mode, CassetteMode::Auto) && !exists;
        Ok(Self {
            inner,
            path,
            state: Mutex::new(CassetteState {
                cassette,
                cursor: 0,
                record,
            }),
        })
    }

    fn replay(&self, req: CompletionRequest) -> Result<Vec<ProviderStreamEvent>, CassetteError> {
        let mut state = self.state.lock().expect("cassette state lock poisoned");
        let index = state.cursor;
        let Some(interaction) = state.cassette.interactions.get(index) else {
            return Err(CassetteError::Exhausted {
                count: state.cassette.interactions.len(),
            });
        };
        if interaction.request != req {
            return Err(CassetteError::RequestMismatch {
                index,
                expected: compact_json(&interaction.request),
                actual: compact_json(&req),
            });
        }
        let events = interaction.events.clone();
        state.cursor += 1;
        Ok(events)
    }

    fn append_recording(
        &self,
        request: CompletionRequest,
        events: Vec<ProviderStreamEvent>,
    ) -> Result<(), CassetteError> {
        let mut state = self.state.lock().expect("cassette state lock poisoned");
        state
            .cassette
            .interactions
            .push(CassetteInteraction::new(request, events));
        state.cassette.write_to(&self.path)
    }
}

#[async_trait]
impl<P> Provider for RecordedProvider<P>
where
    P: Provider + Send + Sync,
{
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let record = self
            .state
            .lock()
            .expect("cassette state lock poisoned")
            .record;
        if !record {
            return match self.replay(req) {
                Ok(events) => Box::pin(stream::iter(events)),
                Err(err) => Box::pin(stream::iter(vec![ProviderStreamEvent::Error {
                    message: err.to_string(),
                }])),
            };
        }

        let events = self
            .inner
            .stream_completion(req.clone())
            .await
            .collect::<Vec<_>>()
            .await;
        match self.append_recording(req, events.clone()) {
            Ok(()) => Box::pin(stream::iter(events)),
            Err(err) => Box::pin(stream::iter(vec![ProviderStreamEvent::Error {
                message: err.to_string(),
            }])),
        }
    }
}

async fn record_http_response(
    response: OpenAiHttpResponse,
) -> Result<OpenAiHttpRecordedResponse, String> {
    let status = response.status;
    let headers = allowed_response_headers(&response.headers);
    let mut bytes = Vec::new();
    let mut body = response.body;
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    let body = String::from_utf8(bytes)
        .map_err(|err| format!("openai_compatible cassette response body was not UTF-8: {err}"))?;
    Ok(OpenAiHttpRecordedResponse {
        status,
        headers,
        body,
    })
}

fn recorded_response_to_http_response(response: OpenAiHttpRecordedResponse) -> OpenAiHttpResponse {
    OpenAiHttpResponse::text(
        response.status,
        recorded_headers_to_header_map(&response.headers)
            .expect("recorded OpenAI response headers should be valid"),
        response.body,
    )
}

fn scrub_endpoint_to_path(endpoint: &str) -> String {
    reqwest::Url::parse(endpoint)
        .map(|url| url.path().to_string())
        .unwrap_or_else(|_| endpoint.split('?').next().unwrap_or(endpoint).to_string())
}

fn allowed_request_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    allowed_headers(
        headers,
        &[
            "openai-organization",
            "openai-project",
            "x-provider-session-id",
            "x-session-id",
        ],
    )
}

fn allowed_response_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    allowed_headers(
        headers,
        &[
            "content-type",
            "x-provider-session-id",
            "x-session-id",
            "openai-session-id",
            "session-id",
            "x-provider-cache-id",
            "x-cache-id",
            "openai-cache-id",
            "cache-id",
        ],
    )
}

fn allowed_headers(headers: &HeaderMap, allowlist: &[&str]) -> BTreeMap<String, String> {
    let mut recorded = BTreeMap::new();
    for name in allowlist {
        if let Some(value) = headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            recorded.insert((*name).to_string(), value.to_string());
        }
    }
    recorded
}

fn recorded_headers_to_header_map(headers: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    let mut parsed = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| format!("invalid recorded header name `{name}`: {err}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|err| format!("invalid recorded header value for `{name}`: {err}"))?;
        parsed.insert(name, value);
    }
    Ok(parsed)
}

fn compact_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string())
}

pub fn assert_cassette_is_safe(cassette: &ProviderCassette) -> Result<(), CassetteError> {
    let body = serde_json::to_string(cassette).map_err(CassetteError::Serialize)?;
    assert_serialized_cassette_is_safe(&body)
}

pub fn assert_http_cassette_is_safe(cassette: &OpenAiHttpCassette) -> Result<(), CassetteError> {
    let body = serde_json::to_string(cassette).map_err(CassetteError::Serialize)?;
    assert_serialized_cassette_is_safe(&body)
}

fn assert_serialized_cassette_is_safe(body: &str) -> Result<(), CassetteError> {
    if let Some(kind) = detect_secret(body) {
        return Err(CassetteError::UnsafeSecret { kind });
    }
    for (name, value) in std::env::vars() {
        if !is_credential_env_name(&name) || value.len() < 8 {
            continue;
        }
        if body.contains(&value) {
            return Err(CassetteError::UnsafeSecret {
                kind: format!("env:{name}"),
            });
        }
    }
    Ok(())
}

fn detect_secret(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("bearer ") {
        return Some("authorization_bearer".to_string());
    }
    for (needle, kind) in [
        ("sk-ant-", "anthropic_api_key"),
        ("sk-", "openai_api_key"),
        ("AIza", "google_api_key"),
        ("AKIA", "aws_access_key_id"),
        ("github_pat_", "github_pat"),
        ("ghp_", "github_token"),
        ("-----BEGIN ", "pem_private_key"),
    ] {
        if body.contains(needle) {
            return Some(kind.to_string());
        }
    }
    None
}

fn is_credential_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "CREDENTIAL", "PASSWORD"]
        .iter()
        .any(|part| upper.contains(part))
}

#[cfg(test)]
mod tests {
    use super::{assert_cassette_is_safe, CassetteInteraction, ProviderCassette};
    use crate::{CompletionMessage, CompletionRequest, MessageRole, ProviderStreamEvent};

    #[test]
    fn safety_scan_rejects_common_secret_shapes() {
        let cassette = ProviderCassette::new(vec![CassetteInteraction::new(
            request_with_content("leaked sk-testsecret123"),
            vec![ProviderStreamEvent::TextDelta("never written".to_string())],
        )]);

        let err = assert_cassette_is_safe(&cassette).expect_err("unsafe cassette");
        assert!(err.to_string().contains("openai_api_key"));
    }

    fn request_with_content(content: &str) -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: "test-model".to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: content.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        }
    }
}
