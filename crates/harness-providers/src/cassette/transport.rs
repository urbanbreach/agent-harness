use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use tokio_stream::StreamExt;

use crate::cassette::{
    CassetteError, CassetteMode, OpenAiHttpCassette, OpenAiHttpInteraction,
    OpenAiHttpRecordedRequest, OpenAiHttpRecordedResponse, CASSETTE_VERSION,
};
use crate::openai::{OpenAiHttpResponse, OpenAiHttpTransport};

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
                return Err(CassetteError::MissingReplayCassette { path });
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let index = state.cursor;
        let Some(interaction) = state.cassette.interactions.get(index) else {
            return Err(CassetteError::Exhausted {
                count: state.cassette.interactions.len(),
            });
        };
        if interaction.request != request {
            return Err(CassetteError::RequestMismatch {
                index,
                expected: crate::cassette::compact_json(&interaction.request),
                actual: crate::cassette::compact_json(&request),
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
        let record = self.state.lock().unwrap_or_else(|e| e.into_inner()).record;
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
    let headers = crate::cassette::recorded_headers_to_header_map(&response.headers)
        .unwrap_or_else(|_| HeaderMap::new());
    OpenAiHttpResponse::text(response.status, headers, response.body)
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
