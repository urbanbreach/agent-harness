use super::*;

#[tokio::test]
async fn chat_sse_stream_reports_usage_for_supported_shapes() {
    // arrange
    let cases = [
        ("no usage chunk", no_usage_sse_transcript(), None),
        (
            "final usage chunk",
            deterministic_sse_transcript(),
            Some(CompletionUsage {
                prompt_tokens: 4,
                completion_tokens: 2,
                total_tokens: 6,
            }),
        ),
        (
            "zero total token fallback",
            zero_total_tokens_sse_transcript(),
            Some(CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
        ),
        (
            "usage after finish reason",
            usage_after_finish_sse_transcript(),
            Some(CompletionUsage {
                prompt_tokens: 4,
                completion_tokens: 2,
                total_tokens: 6,
            }),
        ),
        (
            "usage after finish reason without sentinel",
            usage_after_finish_no_done_sse_transcript(),
            Some(CompletionUsage {
                prompt_tokens: 3,
                completion_tokens: 1,
                total_tokens: 4,
            }),
        ),
    ];

    for (name, transcript, expected) in cases {
        let transport = ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(transcript)]);
        let provider = provider_for_transport(transport, "test-secret-key");

        // act
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        // assert
        let usage = events.iter().find_map(|event| match event {
            ProviderStreamEvent::DoneWithMetadata { usage, .. } => Some(usage),
            _ => None,
        });
        assert_eq!(usage, Some(&expected), "{name}: events={events:?}");
    }
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
