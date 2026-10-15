use super::*;
use crate::{AssistantToolCall, CompletionMessage, CompletionUsage, MessageRole};
use serde_json::json;

fn assert_ordered_archive_content(request: &mut CompletionRequest) -> Result<(), &'static str> {
    use crate::MessageContentPart::{Attachment, Text};
    request.message_content.insert(
        0,
        vec![
            Text("Guide\nHISTORY\nHead".into()),
            Attachment(0),
            Text("Unavailable middle frame".into()),
            Attachment(1),
            Text("Tail".into()),
        ],
    );
    for protocol in [Protocol::Chat, Protocol::Responses, Protocol::Anthropic] {
        let body = encode(request, protocol)?;
        let key = if protocol == Protocol::Responses {
            "input"
        } else {
            "messages"
        };
        let content = body[key][0]["content"]
            .as_array()
            .ok_or("ordered content is not an array")?;
        assert_eq!(content.len(), 5);
        assert_eq!(content[0]["text"], "Guide\nHISTORY\nHead");
        assert_eq!(
            content[1]["type"],
            match protocol {
                Protocol::Chat => "image_url",
                Protocol::Responses => "input_image",
                Protocol::Anthropic => "image",
            }
        );
        assert_eq!(content[2]["text"], "Unavailable middle frame");
        assert_eq!(content[3]["text"], "Attached note.");
        assert_eq!(content[4]["text"], "Tail");
    }
    request.message_content.insert(0, vec![Attachment(1)]);
    assert!(encode(request, Protocol::Chat).is_err());
    request.message_content.clear();
    Ok(())
}

#[test]
fn attachments_reach_each_protocol_and_missing_bytes_fail() -> Result<(), Box<dyn std::error::Error>>
{
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
                AttachmentMetadata::from_bytes("text", "text/plain", None, b"Attached note.", None),
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
    assert_ordered_archive_content(&mut request)?;
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
fn tool_results_keep_ordered_text_and_images() -> Result<(), Box<dyn std::error::Error>> {
    use crate::attachment_protocol::AttachmentMetadata;
    use crate::MessageContentPart::{Attachment, Text};
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
    )?;
    let mut call = CompletionMessage::text(MessageRole::Assistant, "");
    call.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id: "c1".into(),
        function_name: "read".into(),
        arguments_json: "{}".into(),
    }]);
    let mut result = CompletionMessage::text(MessageRole::Tool, "ignored body");
    result.tool_call_id = Some("c1".into());
    let image = |id: &str| AttachmentMetadata::from_bytes(id, "image/png", None, &png, None);
    let request = CompletionRequest {
        model_id: "test".into(),
        messages: vec![call, result],
        attachments: [(1, vec![image("frame"), image("original")])].into(),
        message_content: [(
            1,
            vec![
                Text("note".into()),
                Attachment(0),
                Text("[image 1]".into()),
                Attachment(1),
            ],
        )]
        .into(),
        ..Default::default()
    };
    for protocol in [Protocol::Responses, Protocol::Anthropic] {
        let body = encode(&request, protocol)?;
        let content = if protocol == Protocol::Responses {
            &body["input"][1]["output"]
        } else {
            &body["messages"][1]["content"][0]["content"]
        };
        let kinds: Vec<_> = content
            .as_array()
            .ok_or("tool content is not an array")?
            .iter()
            .map(|part| part["text"].as_str().unwrap_or("<image>"))
            .collect();
        assert_eq!(kinds, ["note", "<image>", "[image 1]", "<image>"]);
    }
    // Chat tool messages are text only: the leading note stays, the rest follows.
    let body = encode(&request, Protocol::Chat)?;
    assert_eq!(body["messages"][1]["content"], "note");
    let media = body["messages"][2]["content"]
        .as_array()
        .ok_or("no media")?;
    assert_eq!(media[1]["type"], "image_url");
    assert_eq!(media[2]["text"], "[image 1]");
    assert_eq!(media[3]["type"], "image_url");
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
        assert!(crate::request_budget::request_budget_semantics(&request, 0, protocol).is_err());
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
            continuation: None,
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
            continuation: None,
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

    check_continuation_scope(&mut request)?;

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
        continuation: None,
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

fn check_continuation_scope(request: &mut CompletionRequest) -> Result<(), &'static str> {
    let mut state = crate::ProviderContinuation::new(
        Protocol::Chat,
        json!({
            "tool_calls":{"call-1":{"google":{"thought_signature":"private-continuation"}}}
        }),
    )?;
    state.model_id.clone_from(&request.model_id);
    request.messages[2].continuation = Some(Box::new(state));
    let budget = crate::request_budget::request_budget_semantics(request, 1, Protocol::Chat)
        .map_err(|_| "budget failed")?
        .request_cost;
    assert!(encode(request, Protocol::Chat)?
        .to_string()
        .contains("private-continuation"));
    assert!(!serde_json::to_string(request)
        .map_err(|_| "serialization failed")?
        .contains("private-continuation"));
    assert!(!format!("{request:?}").contains("private-continuation"));
    request.provider_id = Some("different-provider".into());
    assert!(!encode(request, Protocol::Chat)?
        .to_string()
        .contains("private-continuation"));
    request.provider_id = None;
    request.model_id = "different-model".into();
    assert!(!encode(request, Protocol::Chat)?
        .to_string()
        .contains("private-continuation"));
    assert!(!encode(request, Protocol::Anthropic)?
        .to_string()
        .contains("private-continuation"));
    assert!(
        crate::request_budget::request_budget_semantics(request, 1, Protocol::Chat)
            .map_err(|_| "budget failed")?
            .request_cost
            .history_tokens
            < budget.history_tokens
    );
    request.messages[2].continuation = None;
    Ok(())
}

mod streams;
