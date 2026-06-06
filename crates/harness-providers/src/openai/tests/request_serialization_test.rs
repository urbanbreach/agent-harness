use super::*;

#[test]
fn openai_responses_request_sends_system_prompt_as_instructions() {
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-5.5".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "base instructions".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let body = serde_json::to_value(OpenAiResponsesRequest::from_completion_request(
        request, false, true,
    ))
    .expect("serialize responses request");
    assert_eq!(
        body.get("instructions"),
        Some(&serde_json::Value::String("base instructions".to_string()))
    );
    let input = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request input array");
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0].get("role"),
        Some(&serde_json::Value::String("user".to_string()))
    );
}

#[test]
fn openai_responses_request_replays_assistant_tool_call_before_function_call_output() {
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "sys".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "Use a tool".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Assistant,
                content: "calling tool".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: Some(vec![crate::AssistantToolCall {
                    tool_call_id: "call_1".to_string(),
                    function_name: "filesystem_read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                }]),
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "ok".to_string(),
                name: Some("filesystem_read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let body = serde_json::to_value(OpenAiResponsesRequest::from(request))
        .expect("serialize responses request");
    let input = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request input array");
    assert!(body.get("instructions").is_none());

    assert!(input
        .iter()
        .take(3)
        .all(|item| item.get("role").is_some() && item.get("content").is_some()));
    assert_eq!(
        input[0]
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("type")),
        Some(&serde_json::Value::String("input_text".to_string()))
    );
    assert_eq!(
        input[1]
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("type")),
        Some(&serde_json::Value::String("input_text".to_string()))
    );
    assert_eq!(
        input[2]
            .get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("type")),
        Some(&serde_json::Value::String("output_text".to_string()))
    );

    let function_call_index = input
        .iter()
        .position(|item| {
            item.get("type") == Some(&serde_json::Value::String("function_call".to_string()))
        })
        .expect("assistant tool call should replay as function_call item");
    let function_call_output_index = input
        .iter()
        .position(|item| {
            item.get("type")
                == Some(&serde_json::Value::String(
                    "function_call_output".to_string(),
                ))
        })
        .expect("tool result should serialize as function_call_output item");

    assert!(
        function_call_index < function_call_output_index,
        "function_call replay must precede function_call_output"
    );
    assert_eq!(
        input[function_call_index].get("call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(
        input[function_call_index].get("name"),
        Some(&serde_json::Value::String("filesystem_read".to_string()))
    );
    assert_eq!(
        input[function_call_index].get("arguments"),
        Some(&serde_json::Value::String(
            "{\"filePath\":\"/tmp/demo.txt\"}".to_string()
        ))
    );
    assert_eq!(
        input[function_call_output_index].get("call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(
        input[function_call_output_index].get("output"),
        Some(&serde_json::Value::String("ok".to_string()))
    );
}

#[test]
fn openai_responses_request_omits_empty_assistant_output_text_for_tool_only_turns() {
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "sys".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "Use a tool".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: Some(vec![crate::AssistantToolCall {
                    tool_call_id: "call_1".to_string(),
                    function_name: "read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                }]),
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "1: demo".to_string(),
                name: Some("read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let body = serde_json::to_value(OpenAiResponsesRequest::from(request))
        .expect("serialize responses request");
    let input = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request input array");

    let function_call_index = input
        .iter()
        .position(|item| {
            item.get("type") == Some(&serde_json::Value::String("function_call".to_string()))
        })
        .expect("assistant tool call should replay as function_call item");
    let function_call_output_index = input
        .iter()
        .position(|item| {
            item.get("type")
                == Some(&serde_json::Value::String(
                    "function_call_output".to_string(),
                ))
        })
        .expect("tool result should serialize as function_call_output item");

    assert_eq!(
        function_call_index, 2,
        "empty assistant output_text should be omitted"
    );
    assert_eq!(function_call_output_index, 3);
    assert!(input.iter().all(|item| {
        item.get("content")
            .and_then(serde_json::Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            != Some(&serde_json::Value::String(String::new()))
    }));
}

#[test]
fn openai_chat_request_replays_assistant_tool_call_in_tool_calls_field() {
    let request = CompletionRequest {
        provider_id: None,
        model_id: "gpt-4o-mini".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "sys".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "Use a tool".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::Assistant,
                content: "calling tool".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: Some(vec![crate::AssistantToolCall {
                    tool_call_id: "call_1".to_string(),
                    function_name: "filesystem_read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                }]),
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "ok".to_string(),
                name: Some("filesystem_read".to_string()),
                tool_call_id: Some("call_1".to_string()),
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let body = serde_json::to_value(OpenAiChatCompletionsRequest::from(request))
        .expect("serialize chat request");
    let messages = body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .expect("chat request messages array");

    let assistant = &messages[2];
    let tool_calls = assistant
        .get("tool_calls")
        .and_then(serde_json::Value::as_array)
        .expect("assistant message should include tool_calls replay");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls[0].get("id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(
        tool_calls[0].get("type"),
        Some(&serde_json::Value::String("function".to_string()))
    );
    assert_eq!(
        tool_calls[0]
            .get("function")
            .and_then(|value| value.get("name")),
        Some(&serde_json::Value::String("filesystem_read".to_string()))
    );
    assert_eq!(
        tool_calls[0]
            .get("function")
            .and_then(|value| value.get("arguments")),
        Some(&serde_json::Value::String(
            "{\"filePath\":\"/tmp/demo.txt\"}".to_string()
        ))
    );

    let tool_message = &messages[3];
    assert_eq!(
        tool_message.get("tool_call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(
        tool_message.get("content"),
        Some(&serde_json::Value::String("ok".to_string()))
    );
}
