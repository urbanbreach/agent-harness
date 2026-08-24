use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::openai::{prepare_tool_result, PreparedImage};
use crate::{CompletionMessage, CompletionRequest, MessageRole, ToolDef};

mod framing;

pub(crate) use framing::OpenAiBudgetMode;
use framing::{framing_tokens, FramingKind};

const BYTES_PER_TOKEN: u32 = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestCost {
    pub system_tokens: u32,
    pub tools_tokens: u32,
    pub history_tokens: u32,
    pub attachments_tokens: u32,
    pub framing_tokens: u32,
    pub pending_prompt_tokens: u32,
}

impl ProviderRequestCost {
    pub fn total_input_tokens(self) -> Result<u32, ProviderRequestCostError> {
        [
            self.system_tokens,
            self.tools_tokens,
            self.history_tokens,
            self.attachments_tokens,
            self.framing_tokens,
            self.pending_prompt_tokens,
        ]
        .into_iter()
        .try_fold(0_u32, |total, component| {
            total
                .checked_add(component)
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutputCapDisposition {
    Emitted(u32),
    ProviderDefaulted(u32),
    ProviderControlled,
    UnspecifiedUnknownLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBudgetSemantics {
    pub request_cost: ProviderRequestCost,
    pub output_cap_disposition: ProviderOutputCapDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderRequestCostError {
    #[error("pending prompt index {pending_prompt_index} is outside {message_count} messages")]
    PendingPromptOutOfBounds {
        pending_prompt_index: usize,
        message_count: usize,
    },
    #[error("message at pending prompt index {pending_prompt_index} is not a user message")]
    PendingPromptNotUser { pending_prompt_index: usize },
    #[error("provider cannot represent attachment at message {message_index} with MIME {mime}")]
    UnsupportedAttachment { message_index: usize, mime: String },
    #[error("provider request cost arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("provider router could not resolve the requested provider")]
    ProviderUnavailable,
    #[error("tool schema could not be serialized for request costing")]
    ToolSchemaSerialization,
}

pub fn generic_request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    request_budget_semantics(request, pending_prompt_index, BudgetProtocol::Generic)
}

pub(crate) fn openai_request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
    mode: OpenAiBudgetMode,
    provider_controlled_output: bool,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    request_budget_semantics(
        request,
        pending_prompt_index,
        BudgetProtocol::OpenAi {
            mode,
            provider_controlled_output,
        },
    )
}

pub(crate) fn anthropic_request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    request_budget_semantics(request, pending_prompt_index, BudgetProtocol::Anthropic)
}

#[derive(Debug, Clone, Copy)]
enum BudgetProtocol {
    Generic,
    OpenAi {
        mode: OpenAiBudgetMode,
        provider_controlled_output: bool,
    },
    Anthropic,
}

fn request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
    protocol: BudgetProtocol,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    let pending = request.messages.get(pending_prompt_index).ok_or(
        ProviderRequestCostError::PendingPromptOutOfBounds {
            pending_prompt_index,
            message_count: request.messages.len(),
        },
    )?;
    if pending.role != MessageRole::User {
        return Err(ProviderRequestCostError::PendingPromptNotUser {
            pending_prompt_index,
        });
    }

    let (framing, attachments_supported, provider_controlled_output, provider_default_output) =
        match protocol {
            BudgetProtocol::Generic => (FramingKind::Generic, true, false, None),
            BudgetProtocol::OpenAi {
                mode,
                provider_controlled_output,
            } => (
                match mode {
                    OpenAiBudgetMode::Chat => FramingKind::Chat,
                    OpenAiBudgetMode::Responses => FramingKind::Responses,
                    OpenAiBudgetMode::Auto => FramingKind::Auto,
                },
                true,
                provider_controlled_output,
                None,
            ),
            BudgetProtocol::Anthropic => (FramingKind::Anthropic, false, false, Some(4_096)),
        };
    let mut system_bytes = 0_usize;
    let mut history_bytes = 0_usize;
    let mut pending_bytes = 0_usize;
    let mut attachment_bytes = 0_usize;
    for (index, message) in request.messages.iter().enumerate() {
        let (text_bytes, images) = message_cost_shape(message)?;
        if !attachments_supported {
            if let Some(image) = images.first() {
                return Err(ProviderRequestCostError::UnsupportedAttachment {
                    message_index: index,
                    mime: image.mime.clone(),
                });
            }
        }
        for image in &images {
            attachment_bytes = checked_add(attachment_bytes, image.url.len())?;
        }
        if message.role == MessageRole::System {
            system_bytes = checked_add(system_bytes, text_bytes)?;
        } else if index == pending_prompt_index {
            pending_bytes = checked_add(pending_bytes, text_bytes)?;
        } else {
            history_bytes = checked_add(history_bytes, text_bytes)?;
        }
    }

    let output_cap_disposition = if provider_controlled_output {
        ProviderOutputCapDisposition::ProviderControlled
    } else {
        match (request.max_tokens, provider_default_output) {
            (Some(tokens), _) => ProviderOutputCapDisposition::Emitted(tokens),
            (None, Some(tokens)) => ProviderOutputCapDisposition::ProviderDefaulted(tokens),
            (None, None) => ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
        }
    };
    Ok(ProviderBudgetSemantics {
        request_cost: ProviderRequestCost {
            system_tokens: estimate_bytes(system_bytes)?,
            tools_tokens: estimate_tools(request.tools.as_deref().unwrap_or_default())?,
            history_tokens: estimate_bytes(history_bytes)?,
            attachments_tokens: estimate_bytes(attachment_bytes)?,
            framing_tokens: framing_tokens(request, framing)?,
            pending_prompt_tokens: estimate_bytes(pending_bytes)?,
        },
        output_cap_disposition,
    })
}

fn message_cost_shape(
    message: &CompletionMessage,
) -> Result<(usize, Vec<PreparedImage>), ProviderRequestCostError> {
    let prepared = (message.role == MessageRole::Tool)
        .then(|| prepare_tool_result(&message.content))
        .flatten();
    let mut bytes = prepared
        .as_ref()
        .map_or(message.content.len(), |result| result.text.len());
    bytes = checked_add(bytes, message.name.as_deref().map_or(0, str::len))?;
    bytes = checked_add(bytes, message.tool_call_id.as_deref().map_or(0, str::len))?;
    if let Some(tool_calls) = &message.assistant_tool_calls {
        for tool_call in tool_calls {
            bytes = checked_add(bytes, tool_call.tool_call_id.len())?;
            bytes = checked_add(bytes, tool_call.function_name.len())?;
            bytes = checked_add(bytes, tool_call.arguments_json.len())?;
        }
    }
    Ok((
        bytes,
        prepared.map_or_else(Vec::new, |result| result.images),
    ))
}

fn estimate_tools(tools: &[ToolDef]) -> Result<u32, ProviderRequestCostError> {
    let mut bytes = 0_usize;
    for tool in tools {
        bytes = checked_add(bytes, tool.function_name.len())?;
        bytes = checked_add(bytes, tool.description.as_deref().map_or(0, str::len))?;
        bytes = checked_add(
            bytes,
            serde_json::to_vec(&tool.parameters)
                .map_err(|_| ProviderRequestCostError::ToolSchemaSerialization)?
                .len(),
        )?;
    }
    estimate_bytes(bytes)
}

fn estimate_bytes(bytes: usize) -> Result<u32, ProviderRequestCostError> {
    let bytes = u32::try_from(bytes).map_err(|_| ProviderRequestCostError::ArithmeticOverflow)?;
    Ok(bytes.div_ceil(BYTES_PER_TOKEN))
}

fn checked_add(left: usize, right: usize) -> Result<usize, ProviderRequestCostError> {
    left.checked_add(right)
        .ok_or(ProviderRequestCostError::ArithmeticOverflow)
}
