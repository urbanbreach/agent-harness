use super::*;

#[tokio::test]
async fn chat_sse_stream_without_usage_chunk_emits_done_with_usage_none() {
    // arrange: an SSE transcript whose chunks contain no usage object
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(no_usage_sse_transcript())]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");

    // act: collect the provider stream events
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    // assert: the terminating DoneWithMetadata event has usage: None
    let done = events
        .iter()
        .find(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }));
    let usage =
        match done.unwrap_or_else(|| panic!("expected DoneWithMetadata event, got: {events:?}")) {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => usage,
            other => panic!("expected DoneWithMetadata event, got: {other:?}"),
        };
    assert_eq!(
        *usage, None,
        "usage should be None when no usage chunk is emitted"
    );
}

#[tokio::test]
async fn chat_sse_stream_with_usage_chunk_emits_done_with_usage_some() {
    // arrange: an SSE transcript whose final chunk contains usage
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(deterministic_sse_transcript())]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");

    // act: collect the provider stream events
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    // assert: the terminating DoneWithMetadata event carries the reported usage
    let done = events
        .iter()
        .find(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }));
    let usage =
        match done.unwrap_or_else(|| panic!("expected DoneWithMetadata event, got: {events:?}")) {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => usage,
            other => panic!("expected DoneWithMetadata event, got: {other:?}"),
        };
    assert_eq!(
        *usage,
        Some(CompletionUsage {
            prompt_tokens: 4,
            completion_tokens: 2,
            total_tokens: 6,
        }),
        "usage should be Some when a usage chunk is emitted"
    );
}

#[tokio::test]
async fn chat_sse_stream_with_zero_total_tokens_falls_back_to_prompt_plus_completion() {
    // arrange: an SSE transcript whose final chunk reports total_tokens: 0
    // but non-zero prompt_tokens and completion_tokens
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        zero_total_tokens_sse_transcript(),
    )]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");

    // act: collect the provider stream events
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    // assert: total_tokens falls back to prompt_tokens + completion_tokens
    let done = events
        .iter()
        .find(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }));
    let usage =
        match done.unwrap_or_else(|| panic!("expected DoneWithMetadata event, got: {events:?}")) {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => usage,
            other => panic!("expected DoneWithMetadata event, got: {other:?}"),
        };
    assert_eq!(
        *usage,
        Some(CompletionUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        "total_tokens should fall back to prompt + completion when provider reports 0"
    );
}

#[tokio::test]
async fn chat_sse_stream_with_usage_chunk_after_finish_reason_emits_usage() {
    // arrange: an SSE transcript following the standard OpenAI streaming protocol
    // where usage arrives in a separate chunk AFTER the finish_reason chunk.
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        usage_after_finish_sse_transcript(),
    )]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");

    // act: collect the provider stream events
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    // assert: the terminating DoneWithMetadata event carries the reported usage
    let done = events
        .iter()
        .find(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }));
    let usage =
        match done.unwrap_or_else(|| panic!("expected DoneWithMetadata event, got: {events:?}")) {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => usage,
            other => panic!("expected DoneWithMetadata event, got: {other:?}"),
        };
    assert_eq!(
        *usage,
        Some(CompletionUsage {
            prompt_tokens: 4,
            completion_tokens: 2,
            total_tokens: 6,
        }),
        "usage should be Some when a separate usage chunk arrives after finish_reason"
    );
}

#[tokio::test]
async fn chat_sse_stream_with_usage_chunk_after_finish_reason_no_done_sentinel_emits_usage() {
    // arrange: same separate-usage shape but the stream ends without a [DONE] sentinel
    let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(
        usage_after_finish_no_done_sse_transcript(),
    )]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");

    // act: collect the provider stream events
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    // assert: the terminating DoneWithMetadata event carries the reported usage
    let done = events
        .iter()
        .find(|event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. }));
    let usage =
        match done.unwrap_or_else(|| panic!("expected DoneWithMetadata event, got: {events:?}")) {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => usage,
            other => panic!("expected DoneWithMetadata event, got: {other:?}"),
        };
    assert_eq!(
        *usage,
        Some(CompletionUsage {
            prompt_tokens: 3,
            completion_tokens: 1,
            total_tokens: 4,
        }),
        "usage should be Some when usage chunk arrives after finish_reason without [DONE]"
    );
}

fn zero_total_tokens_sse_transcript() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-zero\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-zero\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"total_tokens\":0}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn no_usage_sse_transcript() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-nousage\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-nousage\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-nousage\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn usage_after_finish_sse_transcript() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-after\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-after\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-after\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn usage_after_finish_no_done_sse_transcript() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-nodone\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-nodone\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-nodone\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n"
    )
    .to_string()
}
