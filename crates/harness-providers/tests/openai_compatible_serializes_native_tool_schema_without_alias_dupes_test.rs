use harness_providers::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiHttpResponse,
    OpenAiHttpTransport,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderStreamEvent, ProviderStreamFinishedMetadata, ToolChoice, ToolDef,
};
use harness_testkit::fakes::{
    FakeHttpClient, HttpClient, HttpInvocation, HttpOutput, ScriptedHttpCall,
};
use serde_json::json;
use tokio_stream::StreamExt;

#[derive(Debug)]
struct FakeOpenAiTransport {
    http: Arc<FakeHttpClient>,
}

impl FakeOpenAiTransport {
    fn new(http: Arc<FakeHttpClient>) -> Self {
        Self { http }
    }
}

#[async_trait]
impl OpenAiHttpTransport for FakeOpenAiTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: reqwest::header::HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        let headers = headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect();
        let output = self
            .http
            .send(
                HttpInvocation::new("POST", endpoint)
                    .headers(headers)
                    .bearer_token(bearer_token)
                    .body(body),
            )
            .map_err(|err| err.to_string())?;
        Ok(OpenAiHttpResponse::text(
            output.status,
            reqwest::header::HeaderMap::new(),
            output.body_text(),
        ))
    }
}

#[tokio::test]
async fn openai_compatible_serializes_native_tool_schema_without_alias_dupes() {
    // arrange
    // act
    // assert
    let http = Arc::new(FakeHttpClient::new([ScriptedHttpCall::new(
        "POST",
        "http://127.0.0.1/v1/responses",
        HttpOutput::text(200, responses_done_sse_transcript()),
    )]));
    let transport = Arc::new(FakeOpenAiTransport::new(Arc::clone(&http)));

    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "http://127.0.0.1/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 60_000,
            headers: BTreeMap::new(),
        },
        Arc::<FakeOpenAiTransport>::clone(&transport),
    )
    .unwrap_or_abort();

    let events = provider
        .stream_completion(native_surface_request())
        .await
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_stop_reason: Some("response.completed".to_string()),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );

    let requests = http.calls();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.ends_with("/v1/responses"));
    assert_eq!(requests[0].bearer_token.as_deref(), Some("test-key"));

    let body = &requests[0].body;
    let tools = body
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(tools.len(), 2);

    let names = tools
        .iter()
        .map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_abort()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["read", "bash"]);
    assert!(!names
        .iter()
        .any(|name| matches!(*name, "fs_read" | "shell_run")));
}

fn native_surface_request() -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "Use native tools only".to_string(),
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
        thinking: None,
        tools: Some(vec![
            ToolDef {
                tool_id: "read".to_string(),
                function_name: "read".to_string(),
                description: Some("Read a file".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "filePath": {"type": "string"}
                    },
                    "required": ["filePath"],
                    "additionalProperties": false
                }),
            },
            ToolDef {
                tool_id: "bash".to_string(),
                function_name: "bash".to_string(),
                description: Some("Run a shell command".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }),
            },
        ]),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
        stream: true,
    }
}

fn responses_done_sse_transcript() -> String {
    concat!(
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}
