use std::collections::BTreeMap;

use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderErrorCategory,
    ProviderStreamEvent,
};
use tokio_stream::StreamExt;

fn request(model_id: &str, reasoning_effort: &str) -> CompletionRequest {
    CompletionRequest {
        provider_id: Some("public".to_string()),
        model_id: model_id.to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::User,
                content: "read the file".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "{\"ok\":true}".to_string(),
                name: Some("read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        variant: Some("coding".to_string()),
        reasoning_effort: Some(reasoning_effort.to_string()),
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    }
}

#[tokio::test]
async fn provider_model_effort_streaming_and_protocol_errors_are_observable() {
    // arrange — a public request containing a tool result and a scripted provider stream.
    let selected = request("gpt-5.5", "high");
    let switched = request("gpt-5.4-mini", "low");
    let mut scripts = BTreeMap::new();
    scripts.insert(
        request_digest(&selected),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ReasoningDelta("summary only".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_2".to_string(),
                function_name: "read".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::categorized_error(
                "bad frame",
                ProviderErrorCategory::MalformedStream,
            ),
        ],
    );

    // act — the selected request is streamed.
    let events: Vec<_> = MockProvider::new(scripts)
        .stream_completion(selected)
        .await
        .collect()
        .await;

    // assert — model/effort switching changes the provider shape and stream semantics remain typed.
    assert_ne!(
        request_digest(&switched),
        request_digest(&request("gpt-5.5", "high"))
    );
    assert!(matches!(events[1], ProviderStreamEvent::ReasoningDelta(_)));
    assert!(matches!(
        events[2],
        ProviderStreamEvent::ToolCallComplete { .. }
    ));
    assert!(matches!(
        events[3],
        ProviderStreamEvent::Error {
            category: Some(ProviderErrorCategory::MalformedStream),
            ..
        }
    ));
}
