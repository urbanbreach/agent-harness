use harness_providers::UnwrapOrAbort;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_providers::cassette::{
    CassetteMode, OpenAiHttpCassette, OpenAiHttpInteraction, OpenAiHttpRecordedRequest,
    OpenAiHttpRecordedResponse, RecordedOpenAiHttpTransport,
};
use harness_providers::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiHttpResponse,
    OpenAiHttpTransport,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderStreamEvent, ProviderStreamFinishedMetadata,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::json;
use tempfile::tempdir;
use tokio_stream::StreamExt;

#[tokio::test]
async fn replayed_http_cassette_drives_openai_parser_without_inner_transport() {
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("openai-http.json");
    OpenAiHttpCassette::new(vec![OpenAiHttpInteraction {
        request: recorded_request("/v1/chat/completions", basic_chat_body()),
        response: recorded_response(200, deterministic_chat_sse()),
    }])
    .write_to(&path)
    .unwrap_or_abort();

    let inner = Arc::new(ScriptedTransport::default());
    let inner_clone = Arc::clone(&inner);
    let transport = Arc::new(
        RecordedOpenAiHttpTransport::with_ci(inner_clone, &path, CassetteMode::Replay, false)
            .unwrap_or_abort(),
    );
    let provider = provider_for_transport(transport, OpenAiApiMode::ChatCompletions);

    let events = provider
        .stream_completion(basic_request())
        .await
        .collect::<Vec<_>>()
        .await;

    assert_eq!(inner.calls(), 0, "replay must not call live transport");
    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::TextDelta("Hello".to_string()),
            ProviderStreamEvent::TextDelta(" cassette".to_string()),
            ProviderStreamEvent::DoneWithMetadata {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_response_id: Some("chatcmpl-cassette".to_string()),
                    provider_stop_reason: Some("stop".to_string()),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );
}

#[tokio::test]
async fn ci_forces_http_replay_and_missing_cassette_fails_closed() {
    let temp = tempdir().unwrap_or_abort();
    let missing = temp.path().join("missing.json");

    let err = match RecordedOpenAiHttpTransport::with_ci(
        Arc::new(ScriptedTransport::default()),
        &missing,
        CassetteMode::Auto,
        true,
    ) {
        Ok(_) => panic!("CI must force replay"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("missing cassette"));
    assert!(!missing.exists());
}

#[tokio::test]
async fn record_mode_writes_redacted_path_headers_body_and_replays() {
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("recorded-http.json");
    let inner = Arc::new(ScriptedTransport::new([ScriptedResponse::sse(
        deterministic_chat_sse(),
    )]));
    let inner_clone = Arc::clone(&inner);
    let transport =
        RecordedOpenAiHttpTransport::with_ci(inner_clone, &path, CassetteMode::Record, false)
            .unwrap_or_abort();

    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer sk-secret123456"),
    );
    headers.insert(
        "openai-organization",
        HeaderValue::from_static("org-public"),
    );
    let response = transport
        .post_json(
            "https://api.example.test/v1/chat/completions?api_key=sk-querysecret123".to_string(),
            headers,
            "sk-secret123456".to_string(),
            basic_chat_body(),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(response.status, 200);

    let cassette = OpenAiHttpCassette::read_from(&path).unwrap_or_abort();
    assert_eq!(cassette.interactions.len(), 1);
    let interaction = &cassette.interactions[0];
    assert_eq!(interaction.request.endpoint_path, "/v1/chat/completions");
    assert_eq!(
        interaction.request.headers.get("openai-organization"),
        Some(&"org-public".to_string())
    );
    assert!(!interaction.request.headers.contains_key("authorization"));
    let raw = std::fs::read_to_string(&path).unwrap_or_abort();
    assert!(!raw.contains("sk-secret"));
    assert!(!raw.contains("api_key"));
    assert_eq!(inner.calls(), 1);
}

#[tokio::test]
async fn unsafe_http_recording_refuses_to_write_secret_body() {
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("unsafe-http.json");
    let inner = Arc::new(ScriptedTransport::new([ScriptedResponse::sse(
        "data: {\"type\":\"response.completed\",\"leak\":\"sk-leakedsecret123\"}\n\ndata: [DONE]\n\n",
    )]));
    let transport = RecordedOpenAiHttpTransport::with_ci(inner, &path, CassetteMode::Record, false)
        .unwrap_or_abort();

    let err = match transport
        .post_json(
            "https://api.example.test/v1/responses".to_string(),
            HeaderMap::new(),
            "placeholder-key".to_string(),
            json!({"model":"gpt-test","stream":true}),
        )
        .await
    {
        Ok(_) => panic!("unsafe cassette should fail"),
        Err(err) => err,
    };

    assert!(err.contains("unsafe cassette secret detected"));
    assert!(!path.exists(), "unsafe cassette must not be written");
}

#[derive(Debug, Default)]
struct ScriptedTransport {
    responses: Mutex<VecDeque<ScriptedResponse>>,
    calls: Mutex<usize>,
}

impl ScriptedTransport {
    fn new(responses: impl IntoIterator<Item = ScriptedResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap_or_abort()
    }
}

#[derive(Debug, Clone)]
struct ScriptedResponse {
    status: u16,
    body: String,
}

impl ScriptedResponse {
    fn sse(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }
}

#[async_trait]
impl OpenAiHttpTransport for ScriptedTransport {
    async fn post_json(
        &self,
        _endpoint: String,
        _headers: HeaderMap,
        _bearer_token: String,
        _body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        *self.calls.lock().unwrap_or_abort() += 1;
        let response = self
            .responses
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .unwrap_or_abort();
        let mut headers = HeaderMap::new();
        if response.status == 200 {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        }
        Ok(OpenAiHttpResponse::text(
            response.status,
            headers,
            response.body,
        ))
    }
}

fn provider_for_transport(
    transport: Arc<dyn OpenAiHttpTransport>,
    api_mode: OpenAiApiMode,
) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.example.test/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode,
            timeout_ms: 15_000,
            headers: BTreeMap::new(),
        },
        transport,
    )
    .unwrap_or_abort()
}

fn basic_request() -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: "gpt-test".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "Say hello from cassette".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: Some(32),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    }
}

fn basic_chat_body() -> serde_json::Value {
    json!({
        "model": "gpt-test",
        "messages": [{"role":"user","content":"Say hello from cassette"}],
        "temperature": 0.0,
        "max_tokens": 32,
        "stream": true
    })
}

fn recorded_request(endpoint_path: &str, body: serde_json::Value) -> OpenAiHttpRecordedRequest {
    OpenAiHttpRecordedRequest {
        endpoint_path: endpoint_path.to_string(),
        headers: BTreeMap::new(),
        body,
    }
}

fn recorded_response(status: u16, body: impl Into<String>) -> OpenAiHttpRecordedResponse {
    OpenAiHttpRecordedResponse {
        status,
        headers: BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())]),
        body: body.into(),
    }
}

fn deterministic_chat_sse() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-cassette\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-cassette\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" cassette\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-cassette\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}
