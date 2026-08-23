use harness_providers::attachment_protocol::{
    serialize_attachments, AttachmentDimensions, AttachmentMetadata, AttachmentPayload,
    AttachmentProtocol,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderRequestContext,
    ToolChoice, ToolDef, UnwrapOrAbort,
};
use serde_json::json;

pub(crate) fn ordinary_request_fixture() -> CompletionRequest {
    CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-fixture".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "ordinary deterministic request".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: Some(128),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: ProviderRequestContext {
            session_id: Some("session-root".to_string()),
            request_id: Some("request-physical-1".to_string()),
            ..ProviderRequestContext::default()
        },
        stream: true,
    }
}

pub(crate) fn tool_request_fixture() -> CompletionRequest {
    CompletionRequest {
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "read the fixture file".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        tools: Some(vec![ToolDef {
            tool_id: "fs.read".to_string(),
            function_name: "filesystem_read".to_string(),
            description: Some("Read a file by path".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {"filePath": {"type": "string"}},
                "required": ["filePath"],
                "additionalProperties": false
            }),
        }]),
        tool_choice: Some(ToolChoice::Auto),
        max_tokens: Some(64),
        ..ordinary_request_fixture()
    }
}

pub(crate) fn attachment_request_fixture() -> CompletionRequest {
    attachment_request_fixture_with("attachment-image", &[137, 80, 78, 71])
}

pub(crate) fn attachment_request_fixture_with(id: &str, bytes: &[u8]) -> CompletionRequest {
    let metadata = AttachmentMetadata::new(
        id,
        "image/png",
        bytes.len() as u64,
        Some(AttachmentDimensions {
            width: 1,
            height: 1,
        }),
        format!("attachment://{id}"),
    );
    let serialized = serialize_attachments(
        &AttachmentProtocol::openai(),
        &[AttachmentPayload::new(metadata, bytes.to_vec())],
    )
    .unwrap_or_abort();
    let attachment = &serialized[0];
    let metadata = attachment.metadata();
    let mut request = ordinary_request_fixture();
    request.messages[0].content = json!({
        "attachment": {
            "id": metadata.id,
            "mime": metadata.mime,
            "size": metadata.size,
            "dimensions": metadata.dimensions,
            "content_ref": metadata.content_ref,
            "data_url": attachment.data_url()
        }
    })
    .to_string();
    request.context.has_media = true;
    request
}

pub(crate) fn physical_retry_request_fixture() -> CompletionRequest {
    let mut request = ordinary_request_fixture();
    request.context.request_id = Some("request-physical-2".to_string());
    request
}

pub(crate) fn child_request_fixture() -> CompletionRequest {
    let mut request = ordinary_request_fixture();
    request.context.session_id = Some("session-child".to_string());
    request
}

pub(crate) fn ordinary_usage_fixture() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 21,
        completion_tokens: 8,
        total_tokens: 29,
    }
}

pub(crate) fn tool_usage_fixture() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 34,
        completion_tokens: 13,
        total_tokens: 47,
    }
}
