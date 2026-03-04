use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::{self as stream, StreamExt};

use crate::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub api_mode: OpenAiApiMode,
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

#[derive(Debug, Error)]
pub enum OpenAiCompatibleProviderError {
    #[error("failed to build HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),
    #[error("invalid header name `{header}`: {source}")]
    InvalidHeaderName {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderName,
    },
    #[error("invalid header value for `{header}`: {source}")]
    InvalidHeaderValue {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderValue,
    },
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_mode: OpenAiApiMode,
    headers: HeaderMap,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        config: OpenAiCompatibleProviderConfig,
    ) -> Result<Self, OpenAiCompatibleProviderError> {
        let headers = parse_headers(&config.headers)?;

        let timeout = if config.timeout_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(config.timeout_ms))
        };

        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }

        let client = builder
            .build()
            .map_err(OpenAiCompatibleProviderError::BuildHttpClient)?;

        Ok(Self {
            client,
            base_url: config.base_url,
            api_key: config.api_key,
            api_mode: config.api_mode,
            headers,
        })
    }

    fn chat_completions_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    fn responses_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/responses")
    }

    async fn send_chat_request(
        &self,
        request: &OpenAiChatCompletionsRequest,
    ) -> Result<reqwest::Response, String> {
        self.client
            .post(self.chat_completions_endpoint())
            .headers(self.headers.clone())
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(|_| "openai_compatible request failed before receiving response".to_string())
    }

    async fn send_responses_request(
        &self,
        request: &OpenAiResponsesRequest,
    ) -> Result<reqwest::Response, String> {
        self.client
            .post(self.responses_endpoint())
            .headers(self.headers.clone())
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(|_| "openai_compatible request failed before receiving response".to_string())
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let chat_request = OpenAiChatCompletionsRequest::from(req.clone());
        let responses_request = OpenAiResponsesRequest::from(req);

        let strategy = match self.api_mode {
            OpenAiApiMode::ChatCompletions => RequestStrategy::ChatCompletions,
            OpenAiApiMode::Responses => RequestStrategy::Responses,
            OpenAiApiMode::Auto => RequestStrategy::Auto,
        };

        let (mode, response) = match strategy {
            RequestStrategy::ChatCompletions => match self.send_chat_request(&chat_request).await {
                Ok(response) => (OpenAiApiMode::ChatCompletions, response),
                Err(message) => {
                    return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                }
            },
            RequestStrategy::Responses => {
                match self.send_responses_request(&responses_request).await {
                    Ok(response) => (OpenAiApiMode::Responses, response),
                    Err(message) => {
                        return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                    }
                }
            }
            RequestStrategy::Auto => {
                let response = match self.send_responses_request(&responses_request).await {
                    Ok(response) => response,
                    Err(message) => {
                        return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                    }
                };

                if matches!(response.status().as_u16(), 404 | 405) {
                    match self.send_chat_request(&chat_request).await {
                        Ok(fallback_response) => {
                            (OpenAiApiMode::ChatCompletions, fallback_response)
                        }
                        Err(message) => {
                            return Box::pin(stream::iter(vec![ProviderStreamEvent::Error {
                                message,
                            }]))
                        }
                    }
                } else {
                    (OpenAiApiMode::Responses, response)
                }
            }
        };

        let status = response.status();
        if !status.is_success() {
            return Box::pin(stream::iter(vec![ProviderStreamEvent::Error {
                message: format!(
                    "openai_compatible request failed with status {}",
                    status.as_u16()
                ),
            }]));
        }

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            match mode {
                OpenAiApiMode::ChatCompletions => consume_chat_sse_stream(response, tx).await,
                OpenAiApiMode::Responses | OpenAiApiMode::Auto => {
                    consume_responses_sse_stream(response, tx).await
                }
            }
        });

        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestStrategy {
    ChatCompletions,
    Responses,
    Auto,
}

async fn consume_chat_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<ProviderStreamEvent>,
) {
    if tx.send(ProviderStreamEvent::Start).await.is_err() {
        return;
    }

    let mut usage = zero_usage();
    let mut done_emitted = false;
    let mut sse_stream = response.bytes_stream().eventsource();

    while let Some(next_event) = sse_stream.next().await {
        let event = match next_event {
            Ok(event) => event,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible SSE stream transport error".to_string(),
                    })
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            if !done_emitted {
                let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
            }
            return;
        }

        let chunk: OpenAiChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(err) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: format!(
                            "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                            summarize_sse_data(data)
                        ),
                    })
                    .await;
                return;
            }
        };

        if let Some(chunk_usage) = chunk.usage {
            usage = chunk_usage;
        }

        let mut finish_seen = false;
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                if !content.is_empty()
                    && tx
                        .send(ProviderStreamEvent::TextDelta(content))
                        .await
                        .is_err()
                {
                    return;
                }
            }

            if choice.finish_reason.is_some() {
                finish_seen = true;
            }
        }

        if finish_seen && !done_emitted {
            done_emitted = true;
            if tx
                .send(ProviderStreamEvent::Done {
                    usage: usage.clone(),
                })
                .await
                .is_err()
            {
                return;
            }
        }
    }

    if !done_emitted {
        let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
    }
}

async fn consume_responses_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<ProviderStreamEvent>,
) {
    if tx.send(ProviderStreamEvent::Start).await.is_err() {
        return;
    }

    let mut usage = zero_usage();
    let mut done_emitted = false;
    let mut sse_stream = response.bytes_stream().eventsource();

    while let Some(next_event) = sse_stream.next().await {
        let event = match next_event {
            Ok(event) => event,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible SSE stream transport error".to_string(),
                    })
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            if !done_emitted {
                let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
            }
            return;
        }

        let parsed: OpenAiResponsesEvent = match serde_json::from_str(data) {
            Ok(parsed) => parsed,
            Err(err) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: format!(
                            "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                            summarize_sse_data(data)
                        ),
                    })
                    .await;
                return;
            }
        };

        match parsed.event_type.as_str() {
            "response.output_text.delta" => {
                if let Some(delta) = parsed.delta {
                    if !delta.is_empty()
                        && tx
                            .send(ProviderStreamEvent::TextDelta(delta))
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            }
            "response.completed" => {
                if let Some(response) = parsed.response {
                    if let Some(completion_usage) = response
                        .usage
                        .map(OpenAiResponsesUsage::into_completion_usage)
                    {
                        usage = completion_usage;
                    }
                }

                if !done_emitted {
                    done_emitted = true;
                    if tx
                        .send(ProviderStreamEvent::Done {
                            usage: usage.clone(),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            "response.error" => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible responses stream returned error event"
                            .to_string(),
                    })
                    .await;
                return;
            }
            _ => {}
        }
    }

    if !done_emitted {
        let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
    }
}

fn summarize_sse_data(data: &str) -> String {
    let mut snippet = data
        .chars()
        .take(160)
        .collect::<String>()
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    if data.chars().count() > 160 {
        snippet.push('…');
    }
    snippet
}

fn parse_headers(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, OpenAiCompatibleProviderError> {
    let mut parsed = HeaderMap::new();

    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderName {
                header: name.clone(),
                source,
            }
        })?;

        let header_value = HeaderValue::from_str(value).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderValue {
                header: name.clone(),
                source,
            }
        })?;

        parsed.insert(header_name, header_value);
    }

    Ok(parsed)
}

fn zero_usage() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatCompletionsRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiChatCompletionsRequest {
    fn from(request: CompletionRequest) -> Self {
        Self {
            model: request.model_id,
            messages: request.messages.into_iter().map(Into::into).collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: true,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesRequest {
    model: String,
    input: Vec<OpenAiResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiResponsesRequest {
    fn from(request: CompletionRequest) -> Self {
        Self {
            model: request.model_id,
            input: request.messages.into_iter().map(Into::into).collect(),
            temperature: request.temperature,
            max_output_tokens: request.max_tokens,
            stream: true,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesInputItem {
    role: String,
    content: Vec<OpenAiResponsesContentItem>,
}

impl From<CompletionMessage> for OpenAiResponsesInputItem {
    fn from(message: CompletionMessage) -> Self {
        Self {
            role: role_to_openai(&message.role).to_string(),
            content: vec![OpenAiResponsesContentItem {
                item_type: "input_text".to_string(),
                text: message.content,
            }],
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesContentItem {
    #[serde(rename = "type")]
    item_type: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

impl From<CompletionMessage> for OpenAiChatMessage {
    fn from(message: CompletionMessage) -> Self {
        Self {
            role: role_to_openai(&message.role).to_string(),
            content: message.content,
        }
    }
}

fn role_to_openai(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionsChunk {
    #[serde(default)]
    choices: Vec<OpenAiChatChoiceChunk>,
    #[serde(default)]
    usage: Option<CompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChoiceChunk {
    #[serde(default)]
    delta: OpenAiChatDeltaChunk,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiChatDeltaChunk {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    response: Option<OpenAiResponsesResponsePayload>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponsePayload {
    #[serde(default)]
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
}

impl OpenAiResponsesUsage {
    fn into_completion_usage(self) -> CompletionUsage {
        let prompt_tokens = self.prompt_tokens.or(self.input_tokens).unwrap_or(0);
        let completion_tokens = self.completion_tokens.or(self.output_tokens).unwrap_or(0);
        let total_tokens = self
            .total_tokens
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens));

        CompletionUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, env, fs, path::PathBuf, time::Duration};

    use serde::Deserialize;
    use tokio::time::timeout;
    use tokio_stream::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{
        OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
        OpenAiResponsesUsage,
    };
    use crate::{
        CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
        ProviderStreamEvent,
    };

    const CLIPROXY_LOOPBACK_DEFAULT_API_KEY: &str = "sk-zerolimit";

    #[tokio::test]
    async fn openai_compatible_offline_wiremock_parses_sse_deltas() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(deterministic_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), "test-secret-key");
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello".to_string()),
                ProviderStreamEvent::TextDelta(" world".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 4,
                        completion_tokens: 2,
                        total_tokens: 6,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 1);

        let request = &requests[0];
        let authorization = request
            .headers
            .get("authorization")
            .expect("authorization header")
            .to_str()
            .expect("authorization header is utf-8");
        assert_eq!(authorization, "Bearer test-secret-key");

        let body: serde_json::Value = request.body_json().expect("request body must be JSON");
        assert_eq!(body.get("stream"), Some(&serde_json::Value::Bool(true)));
        assert_eq!(
            body.get("model"),
            Some(&serde_json::Value::String("gpt-4o-mini".to_string()))
        );
        assert!(body.get("api_key").is_none());
    }

    #[tokio::test]
    async fn openai_responses_offline_wiremock_parses_sse_deltas() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        deterministic_responses_sse_transcript(),
                        "text/event-stream",
                    ),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url_with_mode(
            format!("{}/v1", server.uri()),
            "test-secret-key",
            OpenAiApiMode::Responses,
        );
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 5,
                        completion_tokens: 1,
                        total_tokens: 6,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/v1/responses");
    }

    #[test]
    fn responses_usage_maps_input_output_tokens() {
        let usage: OpenAiResponsesUsage = serde_json::from_value(serde_json::json!({
            "input_tokens": 9,
            "output_tokens": 3,
            "total_tokens": 12
        }))
        .expect("deserialize responses usage");

        assert_eq!(
            usage.into_completion_usage(),
            CompletionUsage {
                prompt_tokens: 9,
                completion_tokens: 3,
                total_tokens: 12,
            }
        );
    }

    #[test]
    fn responses_usage_supports_prompt_completion_token_shape() {
        let usage: OpenAiResponsesUsage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 4,
            "completion_tokens": 2,
            "total_tokens": 6
        }))
        .expect("deserialize responses usage");

        assert_eq!(
            usage.into_completion_usage(),
            CompletionUsage {
                prompt_tokens: 4,
                completion_tokens: 2,
                total_tokens: 6,
            }
        );
    }

    #[tokio::test]
    async fn openai_compatible_errors_do_not_leak_auth_secrets() {
        let server = MockServer::start().await;
        let api_key = "test-secret-key";

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(format!("Authorization: Bearer {api_key} should never leak")),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), api_key);
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(events.len(), 1);
        let ProviderStreamEvent::Error { message } = &events[0] else {
            panic!("expected an error event for non-success response")
        };

        assert!(!message.contains(api_key));
        assert!(!message.to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    #[ignore = "requires HARNESS_LIVE_PROXY=1 and local proxy access"]
    async fn openai_compatible_live_proxy_config_file_smoke() {
        if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
            return;
        }

        let config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_live_config_path());
        let provider_name =
            env::var("HARNESS_LIVE_PROXY_PROVIDER").unwrap_or_else(|_| "default".to_string());

        let live_config = load_live_config(&config_path).unwrap_or_else(|err| panic!("{err}"));

        let provider_config = live_config
            .providers
            .get(&provider_name)
            .unwrap_or_else(|| panic!("provider `{provider_name}` missing in live config"));

        assert_eq!(provider_config.provider_type, "openai_compatible");

        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
            base_url: provider_config.base_url.clone(),
            api_key: resolve_live_api_key(&provider_config.api_key, &provider_config.base_url),
            api_mode: provider_config.api_mode,
            timeout_ms: provider_config.timeout_ms,
            headers: provider_config.headers.clone(),
        })
        .expect("build live provider");

        let model_id = env::var("HARNESS_LIVE_PROXY_MODEL")
            .ok()
            .or_else(|| provider_config.models.keys().next().cloned())
            .expect("HARNESS_LIVE_PROXY_MODEL or at least one configured model is required");

        let mut stream = provider.stream_completion(basic_request(&model_id)).await;

        let mut saw_start = false;
        let mut saw_done = false;
        let mut delta_chars = 0usize;

        timeout(Duration::from_secs(45), async {
            while let Some(event) = stream.next().await {
                match event {
                    ProviderStreamEvent::Start => saw_start = true,
                    ProviderStreamEvent::TextDelta(delta) => {
                        delta_chars += delta.len();
                    }
                    ProviderStreamEvent::Done { .. } => {
                        saw_done = true;
                        break;
                    }
                    ProviderStreamEvent::Error { message } => {
                        panic!("live proxy returned provider error: {message}")
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for live SSE response");

        assert!(saw_start, "expected a start event");
        assert!(saw_done, "expected a done event");
        assert!(delta_chars > 0, "expected at least one text delta");
    }

    fn provider_for_base_url(base_url: String, api_key: &str) -> OpenAiCompatibleProvider {
        provider_for_base_url_with_mode(base_url, api_key, OpenAiApiMode::ChatCompletions)
    }

    fn provider_for_base_url_with_mode(
        base_url: String,
        api_key: &str,
        api_mode: OpenAiApiMode,
    ) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
            base_url,
            api_key: api_key.to_string(),
            api_mode,
            timeout_ms: 15_000,
            headers: std::collections::BTreeMap::new(),
        })
        .expect("build provider")
    }

    fn basic_request(model_id: &str) -> CompletionRequest {
        CompletionRequest {
            model_id: model_id.to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "Say hello from test".to_string(),
            }],
            temperature: Some(0.0),
            max_tokens: Some(32),
            stream: true,
        }
    }

    async fn collect_events(
        provider: &OpenAiCompatibleProvider,
        request: CompletionRequest,
    ) -> Vec<ProviderStreamEvent> {
        provider.stream_completion(request).await.collect().await
    }

    fn deterministic_sse_transcript() -> String {
        concat!(
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn deterministic_responses_sse_transcript() -> String {
        concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn default_live_config_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("harness.example.jsonc")
    }

    fn load_live_config(path: &PathBuf) -> Result<LiveHarnessConfig, String> {
        let raw = fs::read_to_string(path)
            .map_err(|err| format!("failed reading live config {}: {err}", path.display()))?;
        json5::from_str(&raw)
            .map_err(|err| format!("failed parsing live config {}: {err}", path.display()))
    }

    fn resolve_live_api_key(value: &str, base_url: &str) -> String {
        if !(value.starts_with("${") && value.ends_with('}')) {
            return value.to_string();
        }

        let reference = &value[2..value.len() - 1];
        if reference.is_empty() {
            return value.to_string();
        }

        if let Some((key, fallback)) = reference.split_once(":-") {
            if key.is_empty() {
                return value.to_string();
            }
            return env::var(key).unwrap_or_else(|_| fallback.to_string());
        }

        if reference == "OPENAI_API_KEY" && is_loopback_cliproxy_base_url(base_url) {
            return env::var(reference)
                .unwrap_or_else(|_| CLIPROXY_LOOPBACK_DEFAULT_API_KEY.to_string());
        }

        env::var(reference).unwrap_or_else(|_| value.to_string())
    }

    fn is_loopback_cliproxy_base_url(base_url: &str) -> bool {
        let lowered = base_url.trim().to_ascii_lowercase();
        lowered.contains("127.0.0.1:8317")
            || lowered.contains("localhost:8317")
            || lowered.contains("[::1]:8317")
    }

    fn default_timeout_ms() -> u64 {
        60_000
    }

    fn default_api_mode() -> OpenAiApiMode {
        OpenAiApiMode::Auto
    }

    #[derive(Debug, Deserialize)]
    struct LiveHarnessConfig {
        providers: BTreeMap<String, LiveProviderConfig>,
    }

    #[derive(Debug, Deserialize)]
    struct LiveProviderConfig {
        #[serde(rename = "type")]
        provider_type: String,
        #[serde(alias = "baseUrl")]
        base_url: String,
        #[serde(alias = "apiKey")]
        api_key: String,
        #[serde(default = "default_timeout_ms", alias = "timeoutMs")]
        timeout_ms: u64,
        #[serde(default = "default_api_mode", alias = "apiMode")]
        api_mode: OpenAiApiMode,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        models: BTreeMap<String, serde_json::Value>,
    }
}
