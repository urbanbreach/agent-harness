use crate::CompletionRequest;
use serde::{Deserialize, Serialize};
mod images;
pub(crate) use images::check_limits as check_image_limits;

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
        .try_fold(0_u32, |sum, n| {
            sum.checked_add(n)
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderRequestCostError {
    #[error("pending prompt index {pending_prompt_index} is outside {message_count} messages")]
    PendingPromptOutOfBounds {
        pending_prompt_index: usize,
        message_count: usize,
    },
    #[error("pending prompt at {pending_prompt_index} is not a user message")]
    PendingPromptNotUser { pending_prompt_index: usize },
    #[error("unsupported attachment at message {message_index}: {mime}")]
    UnsupportedAttachment { message_index: usize, mime: String },
    #[error("image budget is unknown for the selected model")]
    UnknownImageBudget,
    #[error("invalid attachment: {0}")]
    InvalidAttachment(&'static str),
    #[error("request exceeds 128 tool definitions; reduce the profile's toolset")]
    TooManyTools,
    #[error("request cost overflow")]
    ArithmeticOverflow,
    #[error("provider unavailable")]
    ProviderUnavailable,
    #[error("request cannot be serialized for budgeting")]
    ToolSchemaSerialization,
}

pub fn generic_request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    request_budget_semantics(request, pending_prompt_index, crate::Protocol::Chat)
}

pub(crate) fn request_budget_semantics(
    request: &CompletionRequest,
    pending_prompt_index: usize,
    protocol: crate::Protocol,
) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
    use crate::{MessageRole, Protocol};
    if request
        .tools
        .as_ref()
        .is_some_and(|tools| tools.len() > crate::MAX_TOOL_DEFINITIONS)
    {
        return Err(ProviderRequestCostError::TooManyTools);
    }
    match request.messages.get(pending_prompt_index) {
        Some(message) if message.role == MessageRole::User => {}
        Some(_) => {
            return Err(ProviderRequestCostError::PendingPromptNotUser {
                pending_prompt_index,
            })
        }
        None if pending_prompt_index == request.messages.len()
            && request
                .messages
                .last()
                .is_some_and(|m| m.role == MessageRole::Tool) => {}
        None => {
            return Err(ProviderRequestCostError::PendingPromptOutOfBounds {
                pending_prompt_index,
                message_count: request.messages.len(),
            })
        }
    }
    let mut cost = ProviderRequestCost::default();
    for (index, message) in request.messages.iter().enumerate() {
        let bytes = message.assistant_tool_calls.iter().flatten().try_fold(
            message.content.len(),
            |sum, call| {
                sum.checked_add(call.arguments_json.len())
                    .ok_or(ProviderRequestCostError::ArithmeticOverflow)
            },
        )?;
        let component = if message.role == MessageRole::System {
            &mut cost.system_tokens
        } else if index == pending_prompt_index {
            &mut cost.pending_prompt_tokens
        } else {
            &mut cost.history_tokens
        };
        *component = component
            .checked_add(estimate_tokens(bytes)?)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
    }
    if let Some(tools) = &request.tools {
        let bytes = serde_json::to_vec(tools)
            .map_err(|_| ProviderRequestCostError::ToolSchemaSerialization)?
            .len();
        cost.tools_tokens = estimate_tokens(bytes)?;
    }
    crate::attachment_protocol::validate_request(request, protocol)
        .map_err(|e| ProviderRequestCostError::InvalidAttachment(e.0))?;
    for (index, attachments) in &request.attachments {
        for attachment in attachments {
            let bytes = attachment.bytes().map_err(|_| {
                ProviderRequestCostError::UnsupportedAttachment {
                    message_index: *index,
                    mime: attachment.mime.clone(),
                }
            })?;
            let tokens = if attachment.mime == "text/plain" {
                estimate_tokens(bytes.len())?
            } else {
                let dimensions =
                    crate::attachment_protocol::image_dimensions(&attachment.mime, bytes).map_err(
                        |_| ProviderRequestCostError::UnsupportedAttachment {
                            message_index: *index,
                            mime: attachment.mime.clone(),
                        },
                    )?;
                images::tokens(&request.model_id, dimensions)
                    .ok_or(ProviderRequestCostError::UnknownImageBudget)?
            };
            cost.attachments_tokens = cost
                .attachments_tokens
                .checked_add(tokens)
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
        }
    }
    // ponytail: fixed framing allowance avoids serializing/base64-encoding the request twice.
    let mut framing = 32usize;
    for message in &request.messages {
        framing = framing
            .checked_add(20)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
        for call in message.assistant_tool_calls.iter().flatten() {
            framing = framing
                .checked_add(
                    16 + call.function_name.len().div_ceil(4) + call.tool_call_id.len().div_ceil(4),
                )
                .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
        }
    }
    cost.framing_tokens =
        u32::try_from(framing).map_err(|_| ProviderRequestCostError::ArithmeticOverflow)?;
    let output_cap_disposition = match (request.max_tokens, protocol) {
        (Some(n), _) => ProviderOutputCapDisposition::Emitted(n),
        (None, Protocol::Anthropic) => ProviderOutputCapDisposition::ProviderDefaulted(4096),
        (None, _) => ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
    };
    Ok(ProviderBudgetSemantics {
        request_cost: cost,
        output_cap_disposition,
    })
}

fn estimate_tokens(bytes: usize) -> Result<u32, ProviderRequestCostError> {
    // ponytail: byte estimates are approximate; use model tokenizers when exact admission is required.
    u32::try_from(bytes.div_ceil(4)).map_err(|_| ProviderRequestCostError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompletionMessage, MessageRole};

    #[test]
    fn budgets_charge_each_message_once_and_reject_overflow() -> Result<(), ProviderRequestCostError>
    {
        let request = CompletionRequest {
            model_id: "test".into(),
            max_tokens: Some(100),
            messages: vec![
                CompletionMessage::text(MessageRole::System, "sys"),
                CompletionMessage::text(MessageRole::User, "prompt"),
            ],
            ..Default::default()
        };
        let budget = generic_request_budget_semantics(&request, 1)?;
        assert!(
            budget.request_cost.system_tokens > 0 && budget.request_cost.pending_prompt_tokens > 0
        );
        assert_eq!(budget.request_cost.history_tokens, 0);
        assert_eq!(
            budget.output_cap_disposition,
            ProviderOutputCapDisposition::Emitted(100)
        );
        assert!(generic_request_budget_semantics(&request, 0).is_err());
        assert!(generic_request_budget_semantics(&request, 2).is_err());
        assert!(ProviderRequestCost {
            system_tokens: u32::MAX,
            history_tokens: 1,
            ..Default::default()
        }
        .total_input_tokens()
        .is_err());
        Ok(())
    }

    #[test]
    fn image_budgets_use_resolution_and_keep_unknown_models_unavailable(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::attachment_protocol::AttachmentMetadata;
        use base64::Engine;
        let mut png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")?;
        let mut request = CompletionRequest {
            model_id: "gpt-4o".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "Look")],
            ..Default::default()
        };
        request.attachments.insert(
            0,
            vec![AttachmentMetadata::from_bytes(
                "image",
                "image/png",
                None,
                &png,
                None,
            )],
        );
        for (protocol, model, expected) in [
            (crate::Protocol::Chat, "gpt-4o", 255),
            (crate::Protocol::Responses, "gpt-5.4-mini", 2),
            (crate::Protocol::Anthropic, "claude-sonnet-4-5-20250929", 1),
        ] {
            request.model_id = model.into();
            let cost = request_budget_semantics(&request, 0, protocol)?.request_cost;
            assert_eq!(cost.attachments_tokens, expected);
            assert!(cost.framing_tokens < 256);
        }
        let before = generic_request_budget_semantics(&request, 0)?.request_cost;
        png.resize(64 * 1024, 0);
        request.attachments.insert(
            0,
            vec![AttachmentMetadata::from_bytes(
                "image",
                "image/png",
                None,
                &png,
                None,
            )],
        );
        assert_eq!(
            generic_request_budget_semantics(&request, 0)?.request_cost,
            before
        );
        request.model_id = "future-unknown-model".into();
        assert!(generic_request_budget_semantics(&request, 0)
            .is_err_and(|e| e.to_string().contains("unknown")));
        request.attachments.get_mut(&0).ok_or("missing image")?[0].dimensions = Some(
            crate::attachment_protocol::AttachmentDimensions::new(500, 500),
        );
        assert!(crate::wire::encode(&request, crate::Protocol::Chat).is_err());
        for (model, width, height, expected) in [
            ("gpt-4o", 4096, 2048, 1105),
            ("gpt-4o", 2048, 2048, 765),
            ("gpt-5.4", 1024, 1024, 1229),
            ("claude-opus-4-7-20250929", 4000, 2000, 4784),
        ] {
            assert_eq!(
                images::tokens(
                    model,
                    crate::attachment_protocol::AttachmentDimensions::new(width, height)
                ),
                Some(expected)
            );
        }
        Ok(())
    }
}
