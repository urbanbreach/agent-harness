use crate::CompletionRequest;
use harness_tokenizer::Counter;
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
    request_budget_semantics_at(request, pending_prompt_index, protocol, None)
}

/// Budget semantics for the deployment at `host` (`None` = the built-in endpoint).
pub(crate) fn request_budget_semantics_at(
    request: &CompletionRequest,
    pending_prompt_index: usize,
    protocol: crate::Protocol,
    host: Option<&str>,
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
    let counter = request_counter(request, protocol);
    let mut cost = ProviderRequestCost::default();
    for (index, message) in request.messages.iter().enumerate() {
        let calls = message.assistant_tool_calls.iter().flatten();
        let text = counter.count_fragments(std::iter::once(message.content.as_str()).chain(
            calls.flat_map(|call| [call.function_name.as_str(), call.arguments_json.as_str()]),
        ));
        // Provider continuations ride the wire as opaque bytes and cannot be tokenized.
        let opaque = message
            .continuation
            .as_ref()
            .filter(|s| s.matches(request, protocol))
            .map_or(0, |s| s.byte_len().div_ceil(4));
        let tokens = u32::try_from(opaque)
            .ok()
            .and_then(|opaque| text.checked_add(opaque))
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
        let component = if message.role == MessageRole::System {
            &mut cost.system_tokens
        } else if index == pending_prompt_index {
            &mut cost.pending_prompt_tokens
        } else {
            &mut cost.history_tokens
        };
        *component = component
            .checked_add(tokens)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
    }
    if let Some(tools) = &request.tools {
        let schema = serde_json::to_string(tools)
            .map_err(|_| ProviderRequestCostError::ToolSchemaSerialization)?;
        cost.tools_tokens = counter.count_text(&schema);
    }
    crate::attachment_protocol::validate_request_at(request, protocol, host)
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
                counter.count_text(&String::from_utf8_lossy(bytes))
            } else {
                images::tokens(&counter, attachment, bytes)
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

/// The pricing domain of the model that will read `request`.
fn request_counter(request: &CompletionRequest, protocol: crate::Protocol) -> Counter {
    Counter::new(Some(crate::token_reader::reader_for(
        request,
        &request.model_id,
        protocol,
        None,
    )))
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
    fn image_budgets_follow_the_reader_rule_and_fall_back_for_unknown_models(
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
            (crate::Protocol::Chat, "gpt-4o", 2),
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
        assert_eq!(
            generic_request_budget_semantics(&request, 0)?
                .request_cost
                .attachments_tokens,
            2
        );
        request.attachments.get_mut(&0).ok_or("missing image")?[0].dimensions = Some(
            crate::attachment_protocol::AttachmentDimensions::new(500, 500),
        );
        assert!(crate::wire::encode(&request, crate::Protocol::Chat).is_err());
        Ok(())
    }
}
