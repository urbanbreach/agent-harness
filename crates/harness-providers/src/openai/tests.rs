use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use serde_json::json;
use tokio::time::timeout;
use tokio_stream::StreamExt;

use super::{
    OpenAiApiMode, OpenAiAuthProfile, OpenAiChatCompletionsRequest, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig, OpenAiHttpResponse, OpenAiHttpTransport,
    OpenAiResponsesRequest, CODEX_API_ENDPOINT, COPILOT_API_BASE,
};
use crate::{
    CacheRetention, CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderBearerToken, ProviderCredentialKind, ProviderCredentialSource, ProviderErrorCategory,
    ProviderRequestInitiator, ProviderStreamEvent, ProviderStreamFinishedMetadata, ToolChoice,
    ToolDef,
};

#[derive(Debug, Clone)]
struct RecordedOpenAiRequest {
    endpoint: String,
    headers: HeaderMap,
    bearer_token: String,
    body: serde_json::Value,
}

#[derive(Debug, Clone)]
struct ScriptedOpenAiResponse {
    status: u16,
    chunks: Vec<Result<Vec<u8>, String>>,
}

impl ScriptedOpenAiResponse {
    fn sse(body: String) -> Self {
        Self {
            status: 200,
            chunks: vec![Ok(body.into_bytes())],
        }
    }

    fn sse_chunks(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            status: 200,
            chunks: chunks.into_iter().map(Ok).collect(),
        }
    }

    fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            chunks: vec![Ok(body.into().into_bytes())],
        }
    }
}

#[derive(Debug)]
struct ScriptedOpenAiTransport {
    responses: Mutex<VecDeque<ScriptedOpenAiResponse>>,
    requests: Mutex<Vec<RecordedOpenAiRequest>>,
}

#[derive(Debug)]
struct StaticCredentialSource {
    token: String,
    account_id: Option<String>,
    enterprise_url: Option<String>,
}

#[async_trait]
impl ProviderCredentialSource for StaticCredentialSource {
    async fn bearer_token(&self) -> Result<ProviderBearerToken, crate::ProviderCredentialError> {
        Ok(ProviderBearerToken {
            token: self.token.clone(),
            kind: ProviderCredentialKind::StoredOauth,
            account_id: self.account_id.clone(),
            enterprise_url: self.enterprise_url.clone(),
        })
    }
}

impl ScriptedOpenAiTransport {
    fn new(responses: impl IntoIterator<Item = ScriptedOpenAiResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<RecordedOpenAiRequest> {
        self.requests
            .lock()
            .expect("scripted transport requests lock")
            .clone()
    }
}

#[async_trait]
impl OpenAiHttpTransport for ScriptedOpenAiTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        self.requests
            .lock()
            .expect("scripted transport requests lock")
            .push(RecordedOpenAiRequest {
                endpoint,
                headers,
                bearer_token,
                body,
            });
        let response = self
            .responses
            .lock()
            .expect("scripted transport responses lock")
            .pop_front()
            .expect("scripted OpenAI response");
        let mut headers = HeaderMap::new();
        if response.status == 200 {
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("text/event-stream"),
            );
        }
        Ok(OpenAiHttpResponse::new(
            response.status,
            headers,
            Box::pin(tokio_stream::iter(response.chunks)),
        ))
    }
}

#[derive(Debug)]
struct FailingOpenAiTransport;

#[async_trait]
impl OpenAiHttpTransport for FailingOpenAiTransport {
    async fn post_json(
        &self,
        _endpoint: String,
        _headers: HeaderMap,
        _bearer_token: String,
        _body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        Err("socket closed before response".to_string())
    }
}

fn assert_single_error_category(events: &[ProviderStreamEvent], expected: ProviderErrorCategory) {
    let error_events = events
        .iter()
        .filter(|event| matches!(event, ProviderStreamEvent::Error { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        error_events.len(),
        1,
        "expected exactly one error event: {events:?}"
    );
    let ProviderStreamEvent::Error {
        message,
        category,
        remediation,
    } = error_events[0]
    else {
        panic!("expected provider error event: {events:?}");
    };
    assert_eq!(
        *category,
        Some(expected),
        "unexpected category in {message}"
    );
    assert!(
        message.contains(expected.as_str()),
        "message should render category {expected:?}: {message}"
    );
    assert!(
        remediation
            .as_deref()
            .is_some_and(|hint| !hint.trim().is_empty()),
        "error category {expected:?} should include remediation hint"
    );
}

fn provider_for_transport(
    transport: Arc<ScriptedOpenAiTransport>,
    api_key: &str,
) -> OpenAiCompatibleProvider {
    provider_for_transport_with_mode(transport, api_key, OpenAiApiMode::ChatCompletions)
}

fn provider_for_transport_with_mode(
    transport: Arc<ScriptedOpenAiTransport>,
    api_key: &str,
    api_mode: OpenAiApiMode,
) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "http://127.0.0.1/v1".to_string(),
            api_key: api_key.to_string(),
            api_mode,
            timeout_ms: 15_000,
            headers: std::collections::BTreeMap::new(),
        },
        transport,
    )
    .expect("build provider")
}

fn basic_request(model_id: &str) -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: model_id.to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "Say hello from test".to_string(),
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
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    }
}

fn request_with_single_tool(model_id: &str) -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: model_id.to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "Read /tmp/demo.txt".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: Some(64),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: Some(vec![ToolDef {
            tool_id: "fs.read".to_string(),
            function_name: "filesystem_read".to_string(),
            description: Some("Read file content".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filePath": {"type": "string"}
                },
                "required": ["filePath"],
                "additionalProperties": false
            }),
        }]),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
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

fn tool_call_sse_transcript() -> String {
    concat!(
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"/tmp/demo.txt\\\"}\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"total_tokens\":16}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
}

fn malformed_tool_call_sse_transcript() -> String {
    concat!(
            "data: {\"id\":\"chatcmpl-tool-2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_bad\",\"type\":\"function\",\"function\":{\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":3,\"total_tokens\":14}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
}

fn responses_tool_call_sse_transcript() -> String {
    concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_1\",\"call_id\":\"call_resp_1\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_resp_1\",\"delta\":\"/demo.txt\\\"}\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_1\",\"call_id\":\"call_resp_1\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp/demo.txt\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-tool-1\",\"status\":\"completed\",\"provider_session_id\":\"session-tool-1\",\"provider_cache_id\":\"cache-tool-1\",\"usage\":{\"input_tokens\":9,\"output_tokens\":3,\"total_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":5},\"cache_creation_input_tokens\":2}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
}

fn responses_done_sse_transcript() -> String {
    concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cache-1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
}

fn responses_malformed_tool_args_sse_transcript() -> String {
    concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_bad\",\"call_id\":\"call_resp_bad\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_resp_bad\",\"delta\":\"/demo.txt\\\"\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":9,\"output_tokens\":3,\"total_tokens\":12}}}\n\n",
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
    resolve_env_reference_with(value, |key| env::var(key).ok())
}

fn resolve_env_reference_with(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
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
        return lookup(key)
            .filter(|resolved| !resolved.is_empty())
            .unwrap_or_else(|| fallback.to_string());
    }

    lookup(reference).unwrap_or_else(|| value.to_string())
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

mod auth_profiles_test;
mod live_smoke_test;
mod request_serialization_test;
mod responses_cache_test;
mod tool_errors_test;
