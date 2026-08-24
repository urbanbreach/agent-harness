use super::*;
use crate::UnwrapOrAbort;

#[test]
fn request_budget_media_and_framing_match_lowered_chat_and_responses_shapes() {
    // arrange
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::User,
                content: "look".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: harness_tool_result_content(json!([
                    { "type": "text", "text": "abcd" },
                    { "type": "file", "uri": (["data", ":", "image/png", ";base64,", "AAAA"].concat()), "mime": "image/png" },
                ])),
                name: Some("read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(128),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };
    let chat_provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: "https://example.test/v1".to_string(),
        api_key: "test-key".to_string(),
        api_mode: OpenAiApiMode::ChatCompletions,
        timeout_ms: 1,
        headers: BTreeMap::new(),
    })
    .unwrap_or_abort();
    let responses_provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: "https://example.test/v1".to_string(),
        api_key: "test-key".to_string(),
        api_mode: OpenAiApiMode::Responses,
        timeout_ms: 1,
        headers: BTreeMap::new(),
    })
    .unwrap_or_abort();

    // act
    let chat_cost = chat_provider
        .request_budget_semantics(&request, 0)
        .unwrap_or_abort()
        .request_cost;
    let responses_cost = responses_provider
        .request_budget_semantics(&request, 0)
        .unwrap_or_abort()
        .request_cost;
    let chat =
        serde_json::to_value(OpenAiChatCompletionsRequest::from(request.clone())).unwrap_or_abort();
    let responses = serde_json::to_value(OpenAiResponsesRequest::from(request)).unwrap_or_abort();

    // assert
    assert_eq!(chat_cost.attachments_tokens, 7);
    assert_eq!(responses_cost.attachments_tokens, 7);
    assert_eq!(chat_cost.framing_tokens, 15);
    assert_eq!(responses_cost.framing_tokens, 11);
    assert_eq!(chat["messages"].as_array().map(Vec::len), Some(3));
    assert_eq!(responses["input"].as_array().map(Vec::len), Some(2));
}

#[test]
fn openai_chat_request_extracts_tool_result_images_to_user_message() {
    // arrange
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::Tool,
            content: harness_tool_result_content(json!([
                { "type": "text", "text": "Image read successfully" },
                {
                    "type": "file",
                    "uri": "data:image/png;base64,AAAA",
                    "mime": "image/png",
                    "name": "pixel.png",
                },
            ])),
            name: Some("read".to_string()),
            tool_call_id: Some("call_1".to_string()),
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let body = serde_json::to_value(OpenAiChatCompletionsRequest::from(request)).unwrap_or_abort();

    // assert
    let messages = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[0].get("content"),
        Some(&serde_json::Value::String(
            "Image read successfully".to_string()
        ))
    );
    assert_eq!(
        messages[1]
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("image_url"))
            .and_then(|image_url| image_url.get("url")),
        Some(&serde_json::Value::String(
            "data:image/png;base64,AAAA".to_string()
        ))
    );
}

#[test]
fn openai_chat_request_batches_consecutive_tool_result_images() {
    // arrange
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::Tool,
                content: harness_tool_result_content(json!([
                    { "type": "text", "text": "First image read successfully" },
                    {
                        "type": "file",
                        "uri": "data:image/png;base64,AAAA",
                        "mime": "image/png",
                        "name": "first.png",
                    },
                ])),
                name: Some("read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: harness_tool_result_content(json!([
                    { "type": "text", "text": "Second image read successfully" },
                    {
                        "type": "file",
                        "uri": "data:image/jpeg;base64,BBBB",
                        "mime": "image/jpeg",
                        "name": "second.jpg",
                    },
                ])),
                name: Some("read".to_string()),
                tool_call_id: Some("call_2".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let body = serde_json::to_value(OpenAiChatCompletionsRequest::from(request)).unwrap_or_abort();

    // assert
    let messages = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].get("role"), Some(&json!("tool")));
    assert_eq!(messages[1].get("role"), Some(&json!("tool")));
    assert_eq!(messages[2].get("role"), Some(&json!("user")));
    let content = messages[2]
        .get("content")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(content.len(), 2);
    assert_eq!(
        content[0]
            .get("image_url")
            .and_then(|image_url| image_url.get("url")),
        Some(&json!("data:image/png;base64,AAAA"))
    );
    assert_eq!(
        content[1]
            .get("image_url")
            .and_then(|image_url| image_url.get("url")),
        Some(&json!("data:image/jpeg;base64,BBBB"))
    );
}

#[test]
fn openai_chat_request_skips_invalid_tool_result_images() {
    // arrange
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::Tool,
            content: harness_tool_result_content(json!([
                { "type": "text", "text": "Image read successfully" },
                {
                    "type": "file",
                    "uri": "data:image/png;base64,AAAA",
                    "mime": "image/png",
                    "name": "valid.png",
                },
                {
                    "type": "file",
                    "uri": "data:image/png;base64,not canonical",
                    "mime": "image/png",
                    "name": "invalid.png",
                },
            ])),
            name: Some("read".to_string()),
            tool_call_id: Some("call_1".to_string()),
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let body = serde_json::to_value(OpenAiChatCompletionsRequest::from(request)).unwrap_or_abort();

    // assert
    let content = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .and_then(|messages| messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(content.len(), 1);
    assert_eq!(
        content[0]
            .get("image_url")
            .and_then(|image_url| image_url.get("url")),
        Some(&json!("data:image/png;base64,AAAA"))
    );
}

#[test]
fn openai_responses_request_skips_oversized_tool_result_images() {
    // arrange
    let oversized_base64 = "A".repeat(28 * 1024 * 1024 + 4);
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::Tool,
            content: harness_tool_result_content(json!([
                { "type": "text", "text": "Image read successfully" },
                {
                    "type": "file",
                    "uri": "data:image/png;base64,AAAA",
                    "mime": "image/png",
                    "name": "valid.png",
                },
                {
                    "type": "file",
                    "uri": format!("data:image/png;base64,{oversized_base64}"),
                    "mime": "image/png",
                    "name": "too-large.png",
                },
            ])),
            name: Some("read".to_string()),
            tool_call_id: Some("call_1".to_string()),
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let body = serde_json::to_value(OpenAiResponsesRequest::from(request)).unwrap_or_abort();

    // assert
    let output = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .and_then(|input| input.first())
        .and_then(|item| item.get("output"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0]["type"], json!("input_text"));
    assert_eq!(output[1]["image_url"], json!("data:image/png;base64,AAAA"));
}

#[test]
fn openai_responses_request_serializes_tool_result_images_and_skips_pdfs() {
    // arrange
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::Tool,
            content: harness_tool_result_content(json!([
                { "type": "text", "text": "PDF read successfully" },
                {
                    "type": "file",
                    "uri": "data:image/png;base64,AAAA",
                    "mime": "image/png",
                    "name": "pixel.png",
                },
                {
                    "type": "file",
                    "uri": "data:application/pdf;base64,JVBERi0=",
                    "mime": "application/pdf",
                    "name": "doc.pdf",
                },
            ])),
            name: Some("read".to_string()),
            tool_call_id: Some("call_1".to_string()),
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let body = serde_json::to_value(OpenAiResponsesRequest::from(request)).unwrap_or_abort();

    // assert
    let output = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .and_then(|input| input.first())
        .and_then(|item| item.get("output"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(output[0]["type"], json!("input_text"));
    assert_eq!(output[0]["text"], json!("PDF read successfully"));
    assert_eq!(output[1]["type"], json!("input_image"));
    assert_eq!(output[1]["image_url"], json!("data:image/png;base64,AAAA"));
    assert_eq!(output.len(), 2);
}

fn harness_tool_result_content(content: serde_json::Value) -> String {
    json!({
        "_harness_tool_result": {
            "text": "tool media summary",
            "content": content,
        }
    })
    .to_string()
}
