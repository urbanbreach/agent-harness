use crate::openai::prepare_tool_result;
use crate::{CompletionRequest, MessageRole, ProviderRequestCostError};

const BASE_FRAMING_TOKENS: u32 = 3;
const MESSAGE_FRAMING_TOKENS: u32 = 4;
const TOOL_FRAMING_TOKENS: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) enum OpenAiBudgetMode {
    Chat,
    Responses,
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FramingKind {
    Generic,
    Chat,
    Responses,
    Auto,
    Anthropic,
}

pub(super) fn framing_tokens(
    request: &CompletionRequest,
    framing: FramingKind,
) -> Result<u32, ProviderRequestCostError> {
    let message_units = match framing {
        FramingKind::Generic => request.messages.len(),
        FramingKind::Chat => request
            .messages
            .len()
            .checked_add(chat_media_message_count(request)?)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?,
        FramingKind::Responses => responses_message_count(request)?,
        FramingKind::Auto => request
            .messages
            .len()
            .checked_add(chat_media_message_count(request)?)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?
            .max(responses_message_count(request)?),
        FramingKind::Anthropic => {
            let messages = request
                .messages
                .iter()
                .filter(|message| match message.role {
                    MessageRole::System => false,
                    MessageRole::User | MessageRole::Assistant => true,
                    MessageRole::Tool => message.tool_call_id.is_some(),
                })
                .count();
            messages
                .checked_add(usize::from(
                    request
                        .messages
                        .iter()
                        .any(|message| message.role == MessageRole::System),
                ))
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)?
        }
    };
    let message_units =
        u32::try_from(message_units).map_err(|_| ProviderRequestCostError::ArithmeticOverflow)?;
    let tool_units = u32::try_from(request.tools.as_ref().map_or(0, Vec::len))
        .map_err(|_| ProviderRequestCostError::ArithmeticOverflow)?;
    BASE_FRAMING_TOKENS
        .checked_add(
            message_units
                .checked_mul(MESSAGE_FRAMING_TOKENS)
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)?,
        )
        .and_then(|total| total.checked_add(tool_units.checked_mul(TOOL_FRAMING_TOKENS)?))
        .ok_or(ProviderRequestCostError::ArithmeticOverflow)
}

fn responses_message_count(request: &CompletionRequest) -> Result<usize, ProviderRequestCostError> {
    request.messages.iter().try_fold(0_usize, |total, message| {
        let units = match message.role {
            MessageRole::Assistant => {
                let tool_calls = message.assistant_tool_calls.as_ref().map_or(0, Vec::len);
                usize::from(!message.content.is_empty() || tool_calls == 0)
                    .checked_add(tool_calls)
                    .ok_or(ProviderRequestCostError::ArithmeticOverflow)?
            }
            MessageRole::System | MessageRole::User | MessageRole::Tool => 1,
        };
        total
            .checked_add(units)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)
    })
}

fn chat_media_message_count(
    request: &CompletionRequest,
) -> Result<usize, ProviderRequestCostError> {
    let mut pending_images = false;
    let mut media_messages = 0_usize;
    for message in &request.messages {
        if message.role == MessageRole::Tool {
            pending_images |= prepare_tool_result(&message.content)
                .is_some_and(|result| !result.images.is_empty());
        } else if pending_images {
            media_messages = media_messages
                .checked_add(1)
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
            pending_images = false;
        }
    }
    media_messages
        .checked_add(usize::from(pending_images))
        .ok_or(ProviderRequestCostError::ArithmeticOverflow)
}
