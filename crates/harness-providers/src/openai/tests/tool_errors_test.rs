use super::*;
use crate::UnwrapOrAbort;

#[tokio::test]
async fn openai_stream_terminal_failures_discard_pending_tools() {
    use OpenAiApiMode::{ChatCompletions as Chat, Responses};
    use ProviderErrorCategory::{MalformedStream, Other};

    let chat_text = r#"{"choices":[{"delta":{"content":"partial answer"}}]}"#;
    let chat_tool = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"filesystem_read","arguments":"{}"}}]}}]}"#;
    let chat_finish = r#"{"choices":[{"finish_reason":"tool_calls"}]}"#;
    let response_text = r#"{"type":"response.output_text.delta","delta":"partial answer"}"#;
    let response_tool = r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"item_1","call_id":"call_1","name":"filesystem_read","arguments":""}}"#;
    let response_item_done = r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"item_1","call_id":"call_1","name":"filesystem_read","arguments":"{}"}}"#;
    let response_complete = r#"{"type":"response.completed","response":{"status":"completed"}}"#;
    let response_failed = r#"{"type":"response.failed","response":{"error":{"message":"private-response-sentinel"}}}"#;
    let cases = [
        (Chat, vec![chat_text], MalformedStream, 0),
        (Chat, vec![chat_text, chat_tool], MalformedStream, 0),
        (
            Chat,
            vec![
                chat_text,
                r#"{"choices":[{"finish_reason":"private-response-sentinel"}]}"#,
            ],
            MalformedStream,
            0,
        ),
        (
            Chat,
            vec![
                chat_text,
                chat_tool,
                chat_finish,
                r#"{"error":{"message":"private-response-sentinel"}}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (Responses, vec![response_text], MalformedStream, 0),
        (
            Responses,
            vec![response_text, response_tool],
            MalformedStream,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.unknown"}"#,
            ],
            MalformedStream,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"error","message":"private-response-sentinel"}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                response_complete,
                response_failed,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.error","error":{"message":"private-response-sentinel"}}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.incomplete"}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.completed","response":{"status":"failed"}}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.done","response":{"status":"incomplete"}}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.in_progress","response":{"status":"cancelled"}}"#,
                "[DONE]",
            ],
            Other,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.completed","response":{"status":"in_progress"}}"#,
                "[DONE]",
            ],
            MalformedStream,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                r#"{"type":"response.done","response":{"status":"private-response-sentinel"}}"#,
                "[DONE]",
            ],
            MalformedStream,
            0,
        ),
        (
            Responses,
            vec![
                response_text,
                response_tool,
                response_item_done,
                response_failed,
                "[DONE]",
            ],
            Other,
            1,
        ),
    ];

    for (mode, frames, category, completed_tools) in cases {
        let transcript = format!("data: {}\n\n", frames.join("\n\ndata: "));
        let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(transcript)]);
        let provider = provider_for_transport_with_mode(transport, "test-secret-key", mode);
        let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

        assert_single_error_category(&events, category);
        assert!(events.contains(&ProviderStreamEvent::TextDelta(
            "partial answer".to_string()
        )));
        assert!(
            !events.iter().any(|event| matches!(
                event,
                ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. }
            )),
            "{events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderStreamEvent::ToolCallComplete { .. }))
                .count(),
            completed_tools,
            "{events:?}"
        );
        assert!(!format!("{events:?}").contains("private-response-sentinel"));
    }
}

#[tokio::test]
async fn openai_responses_offline_transport_malformed_args_fail_closed() {
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

    let private_payload = "private-response-sentinel";
    for data in [
        format!(
            r#"{{"type":"response.reasoning_summary_text.delta","summary_index":"{private_payload}"}}"#
        ),
        format!(r#"{{"type":"response.output_text.delta","delta":"{private_payload}""#),
    ] {
        let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(format!(
            "data: {data}\n\ndata: [DONE]\n\n"
        ))]);
        let provider =
            provider_for_transport_with_mode(transport, api_key, OpenAiApiMode::Responses);
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Started { metadata: None },
                ProviderStreamEvent::categorized_error(
                    "openai_compatible returned invalid SSE JSON chunk",
                    ProviderErrorCategory::MalformedStream,
                ),
            ]
        );
    }
}

#[tokio::test]
async fn openai_compatible_errors_include_response_body_detail() {
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

    for (mode, terminal) in [
        (
            OpenAiApiMode::ChatCompletions,
            r#"{"choices":[{"finish_reason":"stop"}]}"#,
        ),
        (OpenAiApiMode::Responses, r#"{"type":"response.completed"}"#),
    ] {
        let mut response = ScriptedOpenAiResponse::sse(format!("data: {terminal}\n\n"));
        response.chunks.push(Err("body read failed".to_string()));
        let transport = ScriptedOpenAiTransport::new([response]);
        let provider = provider_for_transport_with_mode(transport, "test-secret-key", mode);
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_single_error_category(&events, ProviderErrorCategory::TransportFailure);
        assert!(!events.iter().any(|event| matches!(
            event,
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. }
        )));
    }
}
