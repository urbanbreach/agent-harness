use super::*;
use crate::{CompletionUsage, MessageRole};
use serde_json::json;

#[test]
fn provider_streams_preserve_tool_calls_and_terminal_usage() -> Result<(), &'static str> {
    // Each protocol bills 4 prompt tokens, 2 of them cache reads; Anthropic reports
    // cached tokens outside `input_tokens`, the others inside it.
    let cases = [
        (
            Protocol::Chat,
            vec![
                json!({"choices":[{"delta":{"content":"hello","tool_calls":[{"index":0,"id":"call-1","function":{"name":"read","arguments":"{\"path\":"}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read","arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}]}),
                json!({"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6,"prompt_tokens_details":{"cached_tokens":2}}}),
            ],
        ),
        (
            Protocol::Responses,
            vec![
                json!({"type":"response.output_text.delta","delta":"hello"}),
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"read","arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\":\"x\"}"}),
                json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":2,"total_tokens":6,"input_tokens_details":{"cached_tokens":2}}}}),
            ],
        ),
        (
            Protocol::Anthropic,
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":2,"cache_read_input_tokens":2,"output_tokens":0}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call-1","name":"read","input":{}}}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"x\"}"}}),
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":2}}),
                json!({"type":"message_stop"}),
            ],
        ),
    ];
    for (protocol, frames) in cases {
        let mut decoder = StreamDecoder::new(protocol);
        let mut events = Vec::new();
        for frame in frames {
            events.extend(decoder.frame(&frame.to_string())?);
        }
        if protocol == Protocol::Chat {
            events.extend(decoder.frame("[DONE]")?);
        }
        assert!(events.contains(&ProviderStreamEvent::TextDelta("hello".into())));
        assert!(events.contains(&ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "call-1".into(),
            function_name: "read".into(),
            arguments_json: "{\"path\":\"x\"}".into(),
        }));
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                    input_tokens: 2,
                    output_tokens: 2,
                    cache_read_tokens: 2,
                    ..
                }),
                metadata: Some(crate::ProviderStreamFinishedMetadata {
                    cache_read_tokens: Some(2),
                    ..
                }),
            })
        ));
        assert!(decoder.frame("[DONE]")?.is_empty());
    }
    let mut broken = StreamDecoder::new(Protocol::Chat);
    broken.frame(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read","arguments":"{partial"}}]},"finish_reason":"tool_calls"}]}).to_string())?;
    assert!(broken.frame("[DONE]").is_err());
    Ok(())
}

#[test]
fn stream_error_frames_keep_status_and_classification() -> Result<(), &'static str> {
    let mut decoder = StreamDecoder::new(Protocol::Responses);
    let frame = json!({"type":"error","status":429,"error":{"code":"rate_limit_exceeded","message":"slow down sk-abcdefghijklmnopqrstuvwxyz"}});
    assert!(decoder.frame(&frame.to_string()).is_err());
    let Some(ProviderStreamEvent::Error {
        category, envelope, ..
    }) = decoder.take_failure()
    else {
        return Err("the failed frame left no classified error");
    };
    assert_eq!(category, Some(crate::ProviderErrorCategory::RateLimited));
    let envelope = envelope.ok_or("no envelope")?;
    assert_eq!(envelope.status, Some(429));
    assert!(envelope
        .body
        .is_some_and(|body| body.contains("slow down") && !body.contains("abcdefghijklmnop")));
    Ok(())
}

#[test]
fn image_detail_reaches_both_openai_encoders() -> Result<(), Box<dyn std::error::Error>> {
    use crate::attachment_protocol::AttachmentMetadata;
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
    )?;
    let mut image = AttachmentMetadata::from_bytes("i", "image/png", None, &png, None);
    image.detail = Some("original".into());
    let request = CompletionRequest {
        model_id: "test".into(),
        messages: vec![CompletionMessage::text(MessageRole::User, "look")],
        attachments: [(0, vec![image])].into(),
        ..Default::default()
    };
    let chat = encode(&request, Protocol::Chat)?;
    let responses = encode(&request, Protocol::Responses)?;
    assert_eq!(
        chat["messages"][0]["content"][1]["image_url"]["detail"],
        "original"
    );
    assert_eq!(responses["input"][0]["content"][1]["detail"], "original");
    Ok(())
}
#[test]
fn anthropic_usage_keeps_one_hour_cache_writes() -> Result<(), &'static str> {
    let mut decoder = StreamDecoder::new(Protocol::Anthropic);
    let mut events = Vec::new();
    for frame in [
        json!({"type":"message_start","message":{"usage":{"input_tokens":1,"cache_creation_input_tokens":5,
            "cache_creation":{"ephemeral_5m_input_tokens":2,"ephemeral_1h_input_tokens":3},"output_tokens":0}}}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ] {
        events.extend(decoder.frame(&frame.to_string())?);
    }
    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata {
            metadata: Some(crate::ProviderStreamFinishedMetadata {
                cache_write_tokens: Some(5),
                cache_write_1h_tokens: 3,
                ..
            }),
            ..
        })
    ));
    Ok(())
}

#[test]
fn an_error_after_usage_carries_the_reported_usage() -> Result<(), &'static str> {
    // (protocol, frames before the failing one, failing frame, prompt, input, cache read/write)
    let cases = [
        (
            Protocol::Anthropic,
            vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":200,"cache_creation_input_tokens":30,"output_tokens":1}}}),
            ],
            json!({"type":"error","error":{"type":"overloaded_error","message":"busy"}}),
            240,
            10,
            (200, 30),
        ),
        (
            Protocol::Responses,
            vec![],
            json!({"type":"response.failed","response":{"usage":{"input_tokens":50,"output_tokens":0,"input_tokens_details":{"cached_tokens":20}},"error":{"code":"server_error","message":"x"}}}),
            50,
            30,
            (20, 0),
        ),
        (
            Protocol::Chat,
            vec![json!({"choices":[],"usage":{"prompt_tokens":70,"completion_tokens":0}})],
            json!({"error":{"message":"boom"}}),
            70,
            70,
            (0, 0),
        ),
    ];
    for (protocol, before, failing, prompt, input, (read, write)) in cases {
        let mut decoder = StreamDecoder::new(protocol);
        for frame in before {
            decoder.frame(&frame.to_string())?;
        }
        assert!(decoder.frame(&failing.to_string()).is_err());
        let Some(ProviderStreamEvent::Error {
            envelope: Some(envelope),
            ..
        }) = decoder.take_failure()
        else {
            return Err("the failure carried no envelope");
        };
        let usage = envelope.usage.ok_or("no usage")?;
        assert_eq!(
            (
                usage.prompt_tokens,
                usage.input_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens
            ),
            (prompt, input, read, write),
            "{protocol:?}"
        );
    }
    Ok(())
}

#[test]
fn completed_text_blocks_are_marked_for_consumers_that_join_them() -> Result<(), &'static str> {
    let cases = [
        (
            Protocol::Anthropic,
            vec![
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"a"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"b"}}),
                json!({"type":"content_block_stop","index":1}),
            ],
        ),
        (
            Protocol::Responses,
            vec![
                json!({"type":"response.output_text.delta","delta":"a"}),
                json!({"type":"response.output_text.done"}),
                json!({"type":"response.output_text.delta","delta":"b"}),
                json!({"type":"response.output_text.done"}),
            ],
        ),
    ];
    for (protocol, frames) in cases {
        let mut decoder = StreamDecoder::new(protocol);
        let mut events = Vec::new();
        for frame in frames {
            events.extend(decoder.frame(&frame.to_string())?);
        }
        assert_eq!(
            events,
            [
                ProviderStreamEvent::TextDelta("a".into()),
                ProviderStreamEvent::TextBlockEnd,
                ProviderStreamEvent::TextDelta("b".into()),
                ProviderStreamEvent::TextBlockEnd,
            ],
            "{protocol:?}"
        );
    }
    Ok(())
}
