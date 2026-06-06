use serde::Serialize;

use super::non_empty_string;
use crate::{
    CacheRetention, CompletionMessage, CompletionRequest, MessageRole, ProviderRequestContext,
    ToolChoice, ToolDef,
};

const OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH: usize = 64;

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
            tools,
            tool_choice,
            context,
            stream,
        } = request;
        let cache = openai_prompt_cache_params(&context, supports_long_cache_retention);

        Self {
            model: model_id,
            messages: messages.into_iter().map(Into::into).collect(),
            prompt_cache_key: cache.key,
            prompt_cache_retention: cache.retention,
            temperature,
            max_tokens,
            reasoning_effort,
            text_verbosity,
            tools: map_tools(tools),
            tool_choice,
            stream,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
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
            content,
            name,
            tool_call_id,
            tool_calls,
        }
    }
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
            tools: map_tools(tools),
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
        output: String,
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
                output: content,
            }];
        }

        let item_type = match role {
            MessageRole::Assistant => "output_text",
            MessageRole::System | MessageRole::User => "input_text",
            MessageRole::Tool => unreachable!("tool messages handled above"),
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
struct OpenAiResponsesContentItem {
    #[serde(rename = "type")]
    item_type: String,
    text: String,
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

fn map_tools<T>(tools: Option<Vec<ToolDef>>) -> Option<Vec<T>>
where
    T: From<ToolDef>,
{
    tools.map(|tools| tools.into_iter().map(Into::into).collect())
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
