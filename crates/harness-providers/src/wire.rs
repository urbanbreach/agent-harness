use crate::{CompletionRequest, ProviderStreamEvent};
use serde_json::Value;
mod request;
mod stream;
pub(crate) use request::encode;
pub(crate) use stream::StreamDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Chat,
    Responses,
    Anthropic,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssistantToolCall, CompletionMessage, CompletionUsage, MessageRole};
    use serde_json::json;

    #[test]
    fn attachments_reach_each_protocol_and_missing_bytes_fail(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::attachment_protocol::AttachmentMetadata;
        use base64::Engine;
        let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
        let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let mut request = CompletionRequest {
            model_id: "test".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "Describe this.")],
            attachments: [(
                0,
                vec![
                    AttachmentMetadata::from_bytes("image", "image/png", None, &png, None),
                    AttachmentMetadata::from_bytes(
                        "text",
                        "text/plain",
                        None,
                        b"Attached note.",
                        None,
                    ),
                ],
            )]
            .into(),
            ..Default::default()
        };
        for protocol in [Protocol::Chat, Protocol::Responses, Protocol::Anthropic] {
            let body = encode(&request, protocol)?;
            let key = if protocol == Protocol::Responses {
                "input"
            } else {
                "messages"
            };
            let content = &body[key][0]["content"];
            assert_eq!(content[0]["text"], "Describe this.");
            assert_eq!(content[2]["text"], "Attached note.");
            match protocol {
                Protocol::Chat => assert_eq!(
                    content[1]["image_url"]["url"],
                    format!("data:image/png;base64,{encoded}")
                ),
                Protocol::Responses => assert_eq!(
                    content[1]["image_url"],
                    format!("data:image/png;base64,{encoded}")
                ),
                Protocol::Anthropic => {
                    assert_eq!(content[1]["source"]["data"], encoded);
                    assert_eq!(content[1]["source"]["media_type"], "image/png");
                }
            }
        }
        let json = serde_json::to_string(&request)?;
        assert!(!json.contains(encoded) && !json.contains("Attached note."));
        assert!(!format!("{request:?}").contains("Attached note."));
        let restored: CompletionRequest = serde_json::from_str(&json)?;
        assert_eq!(request, restored);
        assert!(encode(&restored, Protocol::Chat).is_err());
        request.messages[0].role = MessageRole::Assistant;
        assert!(encode(&request, Protocol::Chat).is_err());
        request.messages[0].role = MessageRole::User;
        request
            .attachments
            .get_mut(&0)
            .ok_or("missing attachment")?[0]
            .content_ref
            .push('0');
        assert!(encode(&request, Protocol::Chat).is_err());
        Ok(())
    }

    #[test]
    fn image_limits_are_checked_before_base64_encoding() -> Result<(), Box<dyn std::error::Error>> {
        use crate::attachment_protocol::AttachmentMetadata;
        use image::ImageEncoder;
        let mut request = CompletionRequest {
            model_id: "gpt-6-astra".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "Look")],
            ..Default::default()
        };
        for (protocol, width, height, padding, expected) in [
            (Protocol::Anthropic, 8001u32, 1u32, 0, "8000"),
            (Protocol::Anthropic, 1, 1, 7_500_001, "encoded"),
            (Protocol::Responses, 6000, 6000, 0, "30,000"),
            (Protocol::Chat, 6000, 6000, 0, "30,000"),
        ] {
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png).write_image(
                &vec![0; width as usize * height as usize],
                width,
                height,
                image::ExtendedColorType::L8,
            )?;
            if padding > 0 {
                png.resize(padding, 0);
            }
            request.attachments.insert(
                0,
                vec![AttachmentMetadata::from_bytes(
                    "picture",
                    "image/png",
                    None,
                    &png,
                    None,
                )],
            );
            let error = encode(&request, protocol)
                .err()
                .ok_or("oversized image was accepted")?;
            assert!(error.contains(expected), "{error}");
            assert!(
                crate::request_budget::request_budget_semantics(&request, 0, protocol).is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn provider_protocols_preserve_tool_results_and_media_order() -> Result<(), &'static str> {
        let mut request = CompletionRequest {
            model_id: "test-model".into(),
            stream: true,
            max_tokens: Some(100),
            ..Default::default()
        };
        request.messages = vec![
            CompletionMessage::text(MessageRole::System, "Be concise."),
            CompletionMessage::text(MessageRole::User, "Read the file."),
            CompletionMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: Some(vec![AssistantToolCall {
                    tool_call_id: "call-1".into(),
                    function_name: "mcp.fixture.echo".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "contents".into(),
                name: None,
                tool_call_id: Some("call-1".into()),
                assistant_tool_calls: None,
            },
        ];
        request.tools = Some(vec![crate::ToolDef {
            tool_id: "mcp.fixture.echo".into(),
            function_name: "mcp_fixture_echo".into(),
            description: None,
            parameters: json!({"type":"object"}),
        }]);
        let chat = encode(&request, Protocol::Chat)?;
        assert_eq!(
            chat["messages"][2]["tool_calls"][0]["function"]["name"],
            "mcp_fixture_echo"
        );
        assert_eq!(chat["messages"][2]["tool_calls"][0]["id"], "call-1");
        assert_eq!(chat["messages"][3]["tool_call_id"], "call-1");
        let responses = encode(&request, Protocol::Responses)?;
        assert_eq!(responses["input"][2]["type"], "function_call");
        assert_eq!(responses["input"][2]["name"], "mcp_fixture_echo");
        assert_eq!(responses["input"][3]["type"], "function_call_output");
        let anthropic = encode(&request, Protocol::Anthropic)?;
        assert_eq!(anthropic["system"], "Be concise.");
        assert_eq!(
            anthropic["messages"][1]["content"][0]["name"],
            "mcp_fixture_echo"
        );
        assert_eq!(anthropic["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(
            anthropic["messages"][2]["content"][0]["tool_use_id"],
            "call-1"
        );

        use base64::Engine;
        let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "PNG fixture")?;
        request.attachments.insert(
            3,
            vec![crate::attachment_protocol::AttachmentMetadata::from_bytes(
                "tool-image",
                "image/png",
                None,
                &png,
                None,
            )],
        );
        request.messages[2]
            .assistant_tool_calls
            .as_mut()
            .ok_or("missing calls")?
            .push(AssistantToolCall {
                tool_call_id: "call-2".into(),
                function_name: "other".into(),
                arguments_json: "{}".into(),
            });
        request.messages.push(CompletionMessage {
            role: MessageRole::Tool,
            content: "second result".into(),
            tool_call_id: Some("call-2".into()),
            name: None,
            assistant_tool_calls: None,
        });
        let chat = encode(&request, Protocol::Chat)?;
        assert_eq!(chat["messages"][3]["tool_call_id"], "call-1");
        assert_eq!(chat["messages"][4]["tool_call_id"], "call-2");
        assert_eq!(chat["messages"][5]["role"], "user");
        assert_eq!(
            chat["messages"][5]["content"][1]["image_url"]["url"],
            format!("data:image/png;base64,{encoded}")
        );
        let responses = encode(&request, Protocol::Responses)?;
        assert_eq!(responses["input"][4]["call_id"], "call-1");
        assert_eq!(responses["input"][4]["output"][1]["type"], "input_image");
        assert_eq!(
            responses["input"][4]["output"][1]["image_url"],
            format!("data:image/png;base64,{encoded}")
        );
        let anthropic = encode(&request, Protocol::Anthropic)?;
        assert_eq!(
            anthropic["messages"][2]["content"][0]["content"][1]["source"]["data"],
            encoded
        );
        assert_eq!(
            anthropic["messages"][2]["content"][1]["tool_use_id"],
            "call-2"
        );

        Ok(())
    }

    #[test]
    fn tool_definitions_stay_within_the_request_limit() {
        let mut request = CompletionRequest {
            model_id: "test".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "hello")],
            tools: Some(
                (0..129)
                    .map(|n| crate::ToolDef {
                        tool_id: format!("tool_{n}"),
                        function_name: format!("tool_{n}"),
                        description: None,
                        parameters: json!({"type":"object"}),
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        assert!(encode(&request, Protocol::Chat).is_err_and(|e| e.contains("128")));
        assert!(
            crate::request_budget::generic_request_budget_semantics(&request, 0)
                .is_err_and(|e| e.to_string().contains("128"))
        );
        if let Some(tools) = &mut request.tools {
            tools.truncate(128);
        }
        assert!(encode(&request, Protocol::Chat).is_ok());
        assert!(crate::request_budget::generic_request_budget_semantics(&request, 0).is_ok());
    }

    #[test]
    fn provider_streams_preserve_tool_calls_and_terminal_usage() -> Result<(), &'static str> {
        let cases = [
            (
                Protocol::Chat,
                vec![
                    json!({"choices":[{"delta":{"content":"hello","tool_calls":[{"index":0,"id":"call-1","function":{"name":"read","arguments":"{\"path\":"}}]}}]}),
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read","arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}]}),
                    json!({"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}),
                ],
            ),
            (
                Protocol::Responses,
                vec![
                    json!({"type":"response.output_text.delta","delta":"hello"}),
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"read","arguments":""}}),
                    json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\":\"x\"}"}),
                    json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":2,"total_tokens":6}}}),
                ],
            ),
            (
                Protocol::Anthropic,
                vec![
                    json!({"type":"message_start","message":{"usage":{"input_tokens":4,"output_tokens":0}}}),
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
                        total_tokens: 6
                    }),
                    ..
                })
            ));
            assert!(decoder.frame("[DONE]")?.is_empty());
        }
        let mut broken = StreamDecoder::new(Protocol::Chat);
        broken.frame(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read","arguments":"{partial"}}]},"finish_reason":"tool_calls"}]}).to_string())?;
        assert!(broken.frame("[DONE]").is_err());
        Ok(())
    }
}
