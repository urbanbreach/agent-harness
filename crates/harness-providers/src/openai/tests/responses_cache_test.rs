use super::*;
use crate::UnwrapOrAbort;

#[tokio::test]
async fn openai_responses_offline_transport_streams_tool_call_complete() {
    // arrange
    // act
    // assert
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        responses_tool_call_sse_transcript(),
    )]);
    let provider = provider_for_transport_with_mode(
        Arc::clone(&transport),
        "test-secret-key",
        OpenAiApiMode::Responses,
    );
    let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_resp_1".to_string(),
                function_name: Some("filesystem_read".to_string()),
                arguments_delta: "{\"filePath\":\"/tmp".to_string(),
            },
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_resp_1".to_string(),
                function_name: None,
                arguments_delta: "/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_resp_1".to_string(),
                function_name: "filesystem_read".to_string(),
                arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 9,
                    completion_tokens: 3,
                    total_tokens: 12,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_response_id: Some("resp-tool-1".to_string()),
                    provider_session_id: Some("session-tool-1".to_string()),
                    provider_cache_id: Some("cache-tool-1".to_string()),
                    provider_stop_reason: Some("completed".to_string()),
                    cache_read_tokens: Some(5),
                    cache_write_tokens: Some(2),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);

    assert!(requests[0].endpoint.ends_with("/v1/responses"));
    assert_eq!(requests[0].bearer_token, "test-secret-key");

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
        tools[0].get("name"),
        Some(&serde_json::Value::String("filesystem_read".to_string()))
    );
    assert!(tools[0].get("function").is_none());
}

#[tokio::test]
async fn openai_responses_sse_parser_handles_multibyte_utf8_split_across_chunks() {
    // arrange
    // act
    // assert
    let transcript = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi €\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );
    let euro = transcript.find('€').unwrap_or_abort();
    let split = euro + 1;
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse_chunks(vec![
        transcript.as_bytes()[..split].to_vec(),
        transcript.as_bytes()[split..].to_vec(),
    ])]);
    let provider = provider_for_transport_with_mode(
        Arc::clone(&transport),
        "test-secret-key",
        OpenAiApiMode::Responses,
    );

    let events = collect_events(&provider, basic_request("gpt-5.5")).await;

    assert!(events.contains(&ProviderStreamEvent::TextDelta("hi €".to_string())));
    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata { .. })
    ));
}

#[tokio::test]
async fn openai_responses_waits_for_late_terminal_usage_before_finishing() {
    // arrange
    let transcript = concat!(
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        "data: {\"type\":\"response.done\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":8000,\"output_tokens\":840,\"total_tokens\":8840}}}\n\n",
        "data: [DONE]\n\n",
    );
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(transcript.to_string())]);
    let provider = provider_for_transport_with_mode(
        Arc::clone(&transport),
        "test-secret-key",
        OpenAiApiMode::Responses,
    );

    // act
    let events = collect_events(&provider, basic_request("gpt-5.5")).await;

    // assert
    let terminal_events = events
        .iter()
        .filter(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }))
        .collect::<Vec<_>>();
    assert_eq!(terminal_events.len(), 1, "events: {events:?}");
    assert!(matches!(
        terminal_events.first(),
        Some(ProviderStreamEvent::DoneWithMetadata {
            usage: Some(CompletionUsage {
                prompt_tokens: 8_000,
                completion_tokens: 840,
                total_tokens: 8_840,
            }),
            ..
        })
    ));
}

#[tokio::test]
async fn openai_compatible_request_uses_stable_clamped_prompt_cache_key() {
    // arrange
    // act
    // assert
    let session_a = "session-abcdefghijklmnopqrstuvwxyz-ABCDEFGHIJKLMNOPQRSTUVWXYZ-0123456789";
    let expected_clamped = session_a.chars().take(64).collect::<String>();
    assert_eq!(expected_clamped.chars().count(), 64);

    let mut first = basic_request("gpt-4o-mini");
    first.context.session_id = Some(session_a.to_string());
    let first_body = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        first.clone(),
        false,
        false,
    ))
    .unwrap_or_abort();
    let second_body = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        first, false, false,
    ))
    .unwrap_or_abort();
    assert_eq!(
        first_body.get("prompt_cache_key"),
        Some(&serde_json::Value::String(expected_clamped.clone()))
    );
    assert_eq!(
        second_body.get("prompt_cache_key"),
        Some(&serde_json::Value::String(expected_clamped.clone()))
    );

    let mut other_session = basic_request("gpt-4o-mini");
    other_session.context.session_id = Some("session-b".to_string());
    let other_body = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        other_session,
        false,
        false,
    ))
    .unwrap_or_abort();
    assert_ne!(
        other_body.get("prompt_cache_key"),
        first_body.get("prompt_cache_key")
    );

    let no_session = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        basic_request("gpt-4o-mini"),
        false,
        false,
    ))
    .unwrap_or_abort();
    assert!(no_session.get("prompt_cache_key").is_none());

    let mut disabled = basic_request("gpt-4o-mini");
    disabled.context.session_id = Some("session-disabled".to_string());
    disabled.context.cache_retention = CacheRetention::None;
    let disabled_body = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        disabled, true, false,
    ))
    .unwrap_or_abort();
    assert!(disabled_body.get("prompt_cache_key").is_none());
    assert!(disabled_body.get("prompt_cache_retention").is_none());
}

#[tokio::test]
async fn openai_compatible_long_cache_retention_is_direct_openai_only() {
    // arrange
    // act
    // assert
    let transport =
        ScriptedOpenAiTransport::new([
            ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ]);
    let direct_provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "test-secret-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 15_000,
            headers: std::collections::BTreeMap::new(),
        },
        Arc::clone(&transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort();
    let mut direct_request = basic_request("gpt-4o-mini");
    direct_request.context.session_id = Some("session-direct".to_string());
    direct_request.context.cache_retention = CacheRetention::Long;
    let _ = collect_events(&direct_provider, direct_request).await;
    let direct_body = &transport.requests()[0].body;
    assert_eq!(
        direct_body.get("prompt_cache_key"),
        Some(&serde_json::Value::String("session-direct".to_string()))
    );
    assert_eq!(
        direct_body.get("prompt_cache_retention"),
        Some(&serde_json::Value::String("24h".to_string()))
    );

    let proxy_transport =
        ScriptedOpenAiTransport::new([
            ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ]);
    let proxy_provider = provider_for_transport_with_mode(
        Arc::clone(&proxy_transport),
        "test-secret-key",
        OpenAiApiMode::Responses,
    );
    let mut proxy_request = basic_request("gpt-4o-mini");
    proxy_request.context.session_id = Some("session-proxy".to_string());
    proxy_request.context.cache_retention = CacheRetention::Long;
    let _ = collect_events(&proxy_provider, proxy_request).await;
    let proxy_body = &proxy_transport.requests()[0].body;
    assert_eq!(
        proxy_body.get("prompt_cache_key"),
        Some(&serde_json::Value::String("session-proxy".to_string()))
    );
    assert!(proxy_body.get("prompt_cache_retention").is_none());
}

#[tokio::test]
async fn openai_auto_loopback_falls_back_to_chat_completions_on_400() {
    // arrange
    // act
    // assert
    let transport = ScriptedOpenAiTransport::new([
        ScriptedOpenAiResponse::text(400, "unsupported responses"),
        ScriptedOpenAiResponse::sse(deterministic_sse_transcript()),
    ]);
    let provider = provider_for_transport_with_mode(
        Arc::clone(&transport),
        "test-secret-key",
        OpenAiApiMode::Auto,
    );
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::TextDelta("Hello".to_string()),
            ProviderStreamEvent::TextDelta(" world".to_string()),
            ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_response_id: Some("chatcmpl-1".to_string()),
                    provider_stop_reason: Some("stop".to_string()),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].endpoint.ends_with("/v1/responses"));
    assert!(requests[1].endpoint.ends_with("/v1/chat/completions"));
}

#[tokio::test]
async fn openai_transport_failure_keeps_sanitized_context() {
    // arrange
    // act
    // assert
    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: "http://127.0.0.1:9/v1?api_key=should-not-leak".to_string(),
        api_key: "test-secret-key".to_string(),
        api_mode: OpenAiApiMode::ChatCompletions,
        timeout_ms: 1_000,
        headers: BTreeMap::new(),
    })
    .unwrap_or_abort();

    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;
    let [ProviderStreamEvent::Error { message, .. }] = events.as_slice() else {
        panic!("expected one provider error, got {events:?}");
    };

    assert!(message.contains("before receiving response"));
    assert!(message.contains("connection") || message.contains("transport"));
    assert!(!message.contains("should-not-leak"));
    assert!(!message.contains("test-secret-key"));
}
