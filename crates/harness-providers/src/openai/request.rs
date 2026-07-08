// allow: SIZE_OK — OpenAI API request builder (message shaping + tool schema + cache key + streaming params)
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use super::non_empty_string;
use crate::{
    CacheRetention, CompletionMessage, CompletionRequest, MessageRole, ProviderRequestContext,
    ToolChoice, ToolDef,
};

const OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH: usize = 64;
const MAX_MEDIA_ENCODED_BYTES: usize = 28 * 1024 * 1024;
const MAX_MEDIA_DECODED_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(super) struct OpenAiChatCompletionsRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_retention: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiChatCompletionsRequest {
    fn from(request: CompletionRequest) -> Self {
        Self::from_completion_request(request, false)
    }
}

impl OpenAiChatCompletionsRequest {
    pub(super) fn from_completion_request(
        request: CompletionRequest,
        supports_long_cache_retention: bool,
    ) -> Self {
        let CompletionRequest {
            provider_id: _,
            model_id,
            messages,
            temperature,
            max_tokens,
            variant: _,
            reasoning_effort,
            text_verbosity,
            reasoning_summary: _,
            thinking,
            tools,
            tool_choice,
            context,
            stream,
        } = request;
        let cache = openai_prompt_cache_params(&context, supports_long_cache_retention);

        Self {
            model: model_id,
            messages: serialize_chat_messages(messages),
            prompt_cache_key: cache.key,
            prompt_cache_retention: cache.retention,
            temperature,
            max_tokens,
            reasoning_effort,
            text_verbosity,
            thinking,
            tools: tools.map(|tools| tools.into_iter().map(Into::into).collect()),
            tool_choice,
            stream,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: OpenAiChatMessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiChatMessageToolCall>>,
}

impl From<CompletionMessage> for OpenAiChatMessage {
    fn from(message: CompletionMessage) -> Self {
        let CompletionMessage {
            role,
            content,
            name,
            tool_call_id,
            assistant_tool_calls,
        } = message;

        let tool_calls = assistant_tool_calls
            .filter(|calls| !calls.is_empty())
            .map(|calls| {
                calls
                    .into_iter()
                    .map(|call| OpenAiChatMessageToolCall {
                        id: call.tool_call_id,
                        kind: "function",
                        function: OpenAiChatMessageToolCallFunction {
                            name: call.function_name,
                            arguments: call.arguments_json,
                        },
                    })
                    .collect()
            });

        Self {
            role: role_to_openai(&role).to_string(),
            content: OpenAiChatMessageContent::Text(content),
            name,
            tool_call_id,
            tool_calls,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiChatMessageContent {
    Text(String),
    Parts(Vec<OpenAiChatUserContent>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OpenAiChatUserContent {
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Serialize)]
struct OpenAiImageUrl {
    url: String,
}

fn serialize_chat_messages(messages: Vec<CompletionMessage>) -> Vec<OpenAiChatMessage> {
    let mut serialized = Vec::new();
    let mut pending_images = Vec::new();
    for message in messages {
        if matches!(message.role, MessageRole::Tool) {
            let (tool_message, images) = chat_message_and_images_from_tool_message(message);
            serialized.push(tool_message);
            pending_images.extend(images);
            continue;
        }

        if !pending_images.is_empty() {
            serialized.push(OpenAiChatMessage {
                role: "user".to_string(),
                content: OpenAiChatMessageContent::Parts(std::mem::take(&mut pending_images)),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
        }
        serialized.push(message.into());
    }

    if !pending_images.is_empty() {
        serialized.push(OpenAiChatMessage {
            role: "user".to_string(),
            content: OpenAiChatMessageContent::Parts(pending_images),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        });
    }

    serialized
}

fn chat_message_and_images_from_tool_message(
    message: CompletionMessage,
) -> (OpenAiChatMessage, Vec<OpenAiChatUserContent>) {
    let tool_call_id = message.tool_call_id.clone();
    let payload = parse_harness_tool_result(&message.content);
    let text = payload
        .as_ref()
        .map(provider_tool_result_text)
        .unwrap_or_else(|| message.content.clone());
    let tool_message = OpenAiChatMessage {
        role: role_to_openai(&message.role).to_string(),
        content: OpenAiChatMessageContent::Text(text),
        name: message.name,
        tool_call_id,
        tool_calls: None,
    };

    let images = payload
        .as_ref()
        .map(provider_tool_result_images)
        .unwrap_or_default();
    (tool_message, images)
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessageToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiChatMessageToolCallFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessageToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatTool {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiChatToolFunction,
}

impl From<ToolDef> for OpenAiChatTool {
    fn from(tool: ToolDef) -> Self {
        Self {
            kind: "function",
            function: OpenAiChatToolFunction {
                name: tool.function_name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatToolFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(super) struct OpenAiResponsesRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<OpenAiResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_retention: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenAiResponsesReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<OpenAiResponsesText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiResponsesRequest {
    fn from(request: CompletionRequest) -> Self {
        Self::from_completion_request(request, false, false)
    }
}

impl OpenAiResponsesRequest {
    pub(super) fn from_completion_request(
        request: CompletionRequest,
        supports_long_cache_retention: bool,
        system_as_instructions: bool,
    ) -> Self {
        let CompletionRequest {
            provider_id: _,
            model_id,
            messages,
            temperature,
            max_tokens,
            variant: _,
            reasoning_effort,
            text_verbosity,
            reasoning_summary,
            thinking,
            tools,
            tool_choice,
            context,
            stream,
        } = request;
        let cache = openai_prompt_cache_params(&context, supports_long_cache_retention);
        let (instructions, messages) = if system_as_instructions {
            responses_instructions_and_messages(messages)
        } else {
            (None, messages)
        };

        Self {
            model: model_id,
            instructions,
            input: serialize_responses_input(messages),
            prompt_cache_key: cache.key,
            prompt_cache_retention: cache.retention,
            temperature,
            max_output_tokens: max_tokens,
            reasoning: (reasoning_effort.is_some() || reasoning_summary.is_some()).then_some(
                OpenAiResponsesReasoning {
                    effort: reasoning_effort,
                    summary: reasoning_summary,
                },
            ),
            text: text_verbosity.map(|verbosity| OpenAiResponsesText { verbosity }),
            thinking,
            tools: tools.map(|tools| tools.into_iter().map(Into::into).collect()),
            tool_choice,
            stream,
        }
    }
}

fn responses_instructions_and_messages(
    messages: Vec<CompletionMessage>,
) -> (Option<String>, Vec<CompletionMessage>) {
    let mut instructions = Vec::new();
    let mut input_messages = Vec::new();
    for message in messages {
        if matches!(message.role, MessageRole::System) {
            if let Some(content) = non_empty_string(&message.content) {
                instructions.push(content.to_string());
            }
        } else {
            input_messages.push(message);
        }
    }

    let instructions = (!instructions.is_empty()).then(|| instructions.join("\n\n"));
    (instructions, input_messages)
}

fn serialize_responses_input(messages: Vec<CompletionMessage>) -> Vec<OpenAiResponsesInputItem> {
    messages
        .into_iter()
        .flat_map(OpenAiResponsesInputItem::from_completion_message)
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesInputItem {
    Message {
        role: String,
        content: Vec<OpenAiResponsesContentItem>,
    },
    FunctionCall {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        output: OpenAiResponsesFunctionCallOutput,
    },
}

impl OpenAiResponsesInputItem {
    fn from_completion_message(message: CompletionMessage) -> Vec<Self> {
        let CompletionMessage {
            role,
            content,
            name: _,
            tool_call_id,
            assistant_tool_calls,
        } = message;

        if matches!(role, MessageRole::Tool) {
            return vec![Self::FunctionCallOutput {
                item_type: "function_call_output",
                call_id: tool_call_id.unwrap_or_default(),
                output: responses_tool_result_output(&content),
            }];
        }

        let item_type = match role {
            MessageRole::Assistant => "output_text",
            MessageRole::System | MessageRole::User => "input_text",
            MessageRole::Tool => std::process::abort(),
        };

        let has_assistant_tool_calls = assistant_tool_calls
            .as_ref()
            .is_some_and(|tool_calls| !tool_calls.is_empty());
        let omit_assistant_message = matches!(role, MessageRole::Assistant)
            && has_assistant_tool_calls
            && non_empty_string(&content).is_none();

        let mut items = Vec::new();
        if !omit_assistant_message {
            items.push(Self::Message {
                role: role_to_openai(&role).to_string(),
                content: vec![OpenAiResponsesContentItem {
                    item_type: item_type.to_string(),
                    text: content,
                }],
            });
        }

        if matches!(role, MessageRole::Assistant) {
            if let Some(tool_calls) = assistant_tool_calls {
                for tool_call in tool_calls {
                    items.push(Self::FunctionCall {
                        item_type: "function_call",
                        call_id: tool_call.tool_call_id,
                        name: tool_call.function_name,
                        arguments: tool_call.arguments_json,
                    });
                }
            }
        }

        items
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesFunctionCallOutput {
    Text(String),
    Content(Vec<OpenAiResponsesFunctionCallOutputContent>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OpenAiResponsesFunctionCallOutputContent {
    #[serde(rename = "input_text")]
    Text { text: String },
    #[serde(rename = "input_image")]
    Image { image_url: String },
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesContentItem {
    #[serde(rename = "type")]
    item_type: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct HarnessToolResultEnvelope {
    #[serde(rename = "_harness_tool_result")]
    result: HarnessToolResultPayload,
}

#[derive(Debug, Deserialize)]
struct HarnessToolResultPayload {
    text: String,
    content: Vec<HarnessToolResultContent>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HarnessToolResultContent {
    Text { text: String },
    File { uri: String, mime: String },
}

fn parse_harness_tool_result(content: &str) -> Option<HarnessToolResultPayload> {
    serde_json::from_str::<HarnessToolResultEnvelope>(content)
        .ok()
        .map(|envelope| envelope.result)
}

fn provider_tool_result_text(payload: &HarnessToolResultPayload) -> String {
    payload
        .content
        .iter()
        .filter_map(|item| match item {
            HarnessToolResultContent::Text { text } => Some(text.as_str()),
            HarnessToolResultContent::File { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
        .if_empty_then(|| payload.text.clone())
}

fn provider_tool_result_images(payload: &HarnessToolResultPayload) -> Vec<OpenAiChatUserContent> {
    payload
        .content
        .iter()
        .filter_map(|item| match item {
            HarnessToolResultContent::File { uri, mime } => validated_openai_image_url(uri, mime)
                .map(|url| OpenAiChatUserContent::ImageUrl {
                    image_url: OpenAiImageUrl { url },
                }),
            HarnessToolResultContent::Text { .. } => None,
        })
        .collect()
}

fn responses_tool_result_output(content: &str) -> OpenAiResponsesFunctionCallOutput {
    let Some(payload) = parse_harness_tool_result(content) else {
        return OpenAiResponsesFunctionCallOutput::Text(content.to_string());
    };

    OpenAiResponsesFunctionCallOutput::Content(
        payload
            .content
            .into_iter()
            .filter_map(|item| match item {
                HarnessToolResultContent::Text { text } => {
                    Some(OpenAiResponsesFunctionCallOutputContent::Text { text })
                }
                HarnessToolResultContent::File { uri, mime } => {
                    validated_openai_image_url(&uri, &mime).map(|image_url| {
                        OpenAiResponsesFunctionCallOutputContent::Image { image_url }
                    })
                }
            })
            .collect(),
    )
}

fn is_openai_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

fn validated_openai_image_url(uri: &str, mime: &str) -> Option<String> {
    let normalized_mime = mime.to_ascii_lowercase();
    if !is_openai_image_mime(&normalized_mime) {
        return None;
    }

    let base64 = if let Some(data_url) = uri.strip_prefix("data:") {
        let (data_url_mime, base64) = data_url.split_once(";base64,")?;
        if data_url_mime.is_empty()
            || data_url_mime.contains([';', ','])
            || data_url_mime.to_ascii_lowercase() != normalized_mime
        {
            return None;
        }
        base64
    } else {
        uri
    };

    if base64.is_empty() || base64.len() % 4 != 0 || base64.len() > MAX_MEDIA_ENCODED_BYTES {
        return None;
    }

    let bytes = STANDARD.decode(base64).ok()?;
    if bytes.len() > MAX_MEDIA_DECODED_BYTES || STANDARD.encode(&bytes) != base64 {
        return None;
    }

    Some(format!("data:{normalized_mime};base64,{base64}"))
}

trait EmptyStringExt {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String;
}

impl EmptyStringExt for String {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesText {
    verbosity: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesTool {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

impl From<ToolDef> for OpenAiResponsesTool {
    fn from(tool: ToolDef) -> Self {
        Self {
            kind: "function",
            name: tool.function_name,
            description: tool
                .description
                .filter(|value| non_empty_string(value).is_some()),
            parameters: tool.parameters,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenAiPromptCacheParams {
    key: Option<String>,
    retention: Option<&'static str>,
}

fn openai_prompt_cache_params(
    context: &ProviderRequestContext,
    supports_long_cache_retention: bool,
) -> OpenAiPromptCacheParams {
    let key = match context.cache_retention {
        CacheRetention::None => None,
        CacheRetention::Short | CacheRetention::Long => context
            .session_id
            .as_deref()
            .and_then(non_empty_string)
            .map(clamp_openai_prompt_cache_key),
    };
    let retention = (key.is_some()
        && context.cache_retention == CacheRetention::Long
        && supports_long_cache_retention)
        .then_some("24h");

    OpenAiPromptCacheParams { key, retention }
}

fn clamp_openai_prompt_cache_key(key: &str) -> String {
    key.chars()
        .take(OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH)
        .collect()
}

fn role_to_openai(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}
