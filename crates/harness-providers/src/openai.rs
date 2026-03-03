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
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
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
            headers,
        })
    }

    fn chat_completions_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    async fn send_request(
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
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let request = OpenAiChatCompletionsRequest::from(req);
        let response = match self.send_request(&request).await {
            Ok(response) => response,
            Err(message) => {
                return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
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
            consume_sse_stream(response, tx).await;
        });

        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
}

async fn consume_sse_stream(response: reqwest::Response, tx: mpsc::Sender<ProviderStreamEvent>) {
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
        if data == "[DONE]" {
            if !done_emitted {
                let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
            }
            return;
        }

        let chunk: OpenAiChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible returned invalid SSE JSON chunk".to_string(),
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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, env, fs, path::PathBuf, time::Duration};

    use serde::Deserialize;
    use tokio::time::timeout;
    use tokio_stream::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig};
    use crate::{
        CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
        ProviderStreamEvent,
    };

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
            api_key: resolve_env_reference(&provider_config.api_key),
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
        OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
            base_url,
            api_key: api_key.to_string(),
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

    fn resolve_env_reference(value: &str) -> String {
        if !(value.starts_with("${") && value.ends_with('}')) {
            return value.to_string();
        }

        let key = &value[2..value.len() - 1];
        if key.is_empty() {
            return value.to_string();
        }

        env::var(key).unwrap_or_else(|_| value.to_string())
    }

    fn default_timeout_ms() -> u64 {
        60_000
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
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        models: BTreeMap<String, serde_json::Value>,
    }
}
