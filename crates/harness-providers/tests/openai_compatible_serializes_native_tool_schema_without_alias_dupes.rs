use std::collections::BTreeMap;

use harness_providers::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderStreamEvent, ToolChoice, ToolDef,
};
use serde_json::json;
use tokio_stream::StreamExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn openai_compatible_serializes_native_tool_schema_without_alias_dupes() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(responses_done_sse_transcript(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: format!("{}/v1", server.uri()),
        api_key: "test-key".to_string(),
        api_mode: OpenAiApiMode::Responses,
        timeout_ms: 60_000,
        headers: BTreeMap::new(),
    })
    .expect("provider should build");

    let events = provider
        .stream_completion(native_surface_request())
        .await
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            },
        ]
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(requests.len(), 1);

    let body: serde_json::Value = requests[0].body_json().expect("request body must be JSON");
    let tools = body
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("responses request should serialize tools");
    assert_eq!(tools.len(), 2);

    let names = tools
        .iter()
        .map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .expect("responses tool name")
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["fs_read", "shell_run"]);
    assert!(!names.iter().any(|name| matches!(*name, "read" | "bash")));
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
        tools: Some(vec![
            ToolDef {
                tool_id: "fs.read".to_string(),
                function_name: "fs_read".to_string(),
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
                tool_id: "shell.run".to_string(),
                function_name: "shell_run".to_string(),
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
