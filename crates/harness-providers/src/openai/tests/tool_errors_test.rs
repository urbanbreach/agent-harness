use super::*;
use crate::UnwrapOrAbort;

#[tokio::test]
async fn openai_responses_offline_transport_malformed_args_fail_closed() {
    // arrange
    // act
    // assert
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        responses_malformed_tool_args_sse_transcript(),
    )]);
    let provider =
        provider_for_transport_with_mode(transport, "test-secret-key", OpenAiApiMode::Responses);
    let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

    assert!(matches!(
        events.first(),
        Some(ProviderStreamEvent::Started { .. })
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. })));
    assert!(events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Error { message, .. } if message.contains("malformed arguments JSON"))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::ToolCallComplete { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. })));
}

#[tokio::test]
async fn openai_compatible_offline_transport_streams_chat_tool_calls() {
    // arrange
    // act
    // assert
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(tool_call_sse_transcript())]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");
    let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_1".to_string(),
                function_name: Some("filesystem_read".to_string()),
                arguments_delta: "{\"filePath\":\"".to_string(),
            },
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_1".to_string(),
                function_name: None,
                arguments_delta: "/tmp/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_1".to_string(),
                function_name: "filesystem_read".to_string(),
                arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 4,
                    total_tokens: 16,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_response_id: Some("chatcmpl-tool-1".to_string()),
                    provider_stop_reason: Some("tool_calls".to_string()),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);

    let body = &requests[0].body;
    assert_eq!(
        body.get("tool_choice"),
        Some(&serde_json::Value::String("auto".to_string()))
    );

    let tools = body
        .get("tools")
        .and_then(|value| value.as_array())
        .unwrap_or_abort();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("type"),
        Some(&serde_json::Value::String("function".to_string()))
    );
    assert_eq!(
        tools[0].get("function").and_then(|value| value.get("name")),
        Some(&serde_json::Value::String("filesystem_read".to_string()))
    );
}

#[tokio::test]
async fn openai_compatible_offline_transport_chat_tool_calls_fail_closed_on_invalid_arguments() {
    // arrange
    // act
    // assert
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        malformed_tool_call_sse_transcript(),
    )]);
    let provider = provider_for_transport(transport, "test-secret-key");
    let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

    assert!(matches!(
        events.first(),
        Some(ProviderStreamEvent::Started { .. })
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::Error { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::ToolCallComplete { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. })));
}

#[tokio::test]
async fn openai_compatible_errors_do_not_leak_auth_secrets() {
    // arrange
    // act
    // assert
    let api_key = "test-secret-key";

    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::text(
        401,
        format!("Authorization: Bearer {api_key} should never leak"),
    )]);
    let provider = provider_for_transport(transport, api_key);
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    assert_eq!(events.len(), 1);
    let ProviderStreamEvent::Error { message, .. } = &events[0] else {
        panic!("expected an error event for non-success response")
    };

    assert!(message.contains("status 401"));
    assert!(!message.contains(api_key));
    assert!(!message.to_ascii_lowercase().contains("authorization"));
}

#[tokio::test]
async fn openai_compatible_errors_include_response_body_detail() {
    // arrange
    // act
    // assert
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::text(
            400,
            json!({
                "error": {
                    "message": "Invalid schema for function 'question': object schema missing properties"
                }
            })
            .to_string(),
        )]);

    let provider = provider_for_transport(transport, "test-secret-key");
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    assert_eq!(events.len(), 1);
    let ProviderStreamEvent::Error { message, .. } = &events[0] else {
        panic!("expected an error event for non-success response")
    };

    assert!(message.contains("status 400"));
    assert!(message.contains("Invalid schema for function 'question'"));
    assert!(message.contains("object schema missing properties"));
}

#[tokio::test]
async fn openai_non_success_responses_map_to_stable_error_categories() {
    // arrange
    let cases = [
        (
            401,
            json!({"error": {"message": "missing API key"}}).to_string(),
            "",
            ProviderErrorCategory::MissingCredentials,
        ),
        (
            401,
            json!({"error": {"message": "invalid_api_key"}}).to_string(),
            "test-secret-key",
            ProviderErrorCategory::InvalidCredentials,
        ),
        (
            429,
            json!({"error": {"message": "rate limit exceeded"}}).to_string(),
            "test-secret-key",
            ProviderErrorCategory::RateLimited,
        ),
        (
            400,
            json!({"error": {"message": "context_length_exceeded: maximum context window"}})
                .to_string(),
            "test-secret-key",
            ProviderErrorCategory::ContextWindowExceeded,
        ),
        (
            400,
            json!({"error": {"message": "unsupported tool call shape"}}).to_string(),
            "test-secret-key",
            ProviderErrorCategory::UnsupportedToolCall,
        ),
        (
            500,
            json!({"error": {"message": "provider server exploded"}}).to_string(),
            "test-secret-key",
            ProviderErrorCategory::Other,
        ),
    ];

    for (status, body, api_key, expected_category) in cases {
        let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::text(status, body)]);
        let provider = provider_for_transport(transport, api_key);
        // act
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;
        // assert
        assert_single_error_category(&events, expected_category);
    }
}

#[tokio::test]
async fn openai_rate_limit_error_includes_retry_after_ms_metadata() {
    // arrange
    // act
    // assert
    let mut response = ScriptedOpenAiResponse::text(
        429,
        json!({"error": {"message": "rate limit exceeded"}}).to_string(),
    );
    response.headers.insert(
        reqwest::header::RETRY_AFTER,
        reqwest::header::HeaderValue::from_static("2"),
    );
    let transport = ScriptedOpenAiTransport::new([response]);
    let provider = provider_for_transport(transport, "test-secret-key");

    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    let [ProviderStreamEvent::Error {
        category,
        retry_after_ms,
        ..
    }] = events.as_slice()
    else {
        panic!("expected one provider error event: {events:?}");
    };
    assert_eq!(*category, Some(ProviderErrorCategory::RateLimited));
    assert_eq!(*retry_after_ms, Some(2_000));
}

#[tokio::test]
async fn openai_malformed_stream_and_transport_failures_have_stable_categories() {
    // arrange
    let malformed_transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        "data: {not json}\n\n".to_string(),
    )]);
    let malformed_provider = provider_for_transport(malformed_transport, "test-secret-key");
    let malformed_events =
            // act
            collect_events(&malformed_provider, basic_request("gpt-4o-mini")).await;
    // assert
    assert_single_error_category(&malformed_events, ProviderErrorCategory::MalformedStream);

    let transport_provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "http://127.0.0.1/v1".to_string(),
            api_key: "test-secret-key".to_string(),
            api_mode: OpenAiApiMode::ChatCompletions,
            timeout_ms: 15_000,
            headers: std::collections::BTreeMap::new(),
        },
        Arc::new(FailingOpenAiTransport),
    )
    .unwrap_or_abort();
    let transport_events = collect_events(&transport_provider, basic_request("gpt-4o-mini")).await;
    assert_single_error_category(&transport_events, ProviderErrorCategory::TransportFailure);
}
