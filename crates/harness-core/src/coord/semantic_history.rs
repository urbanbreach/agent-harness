use crate::agent::AssistantResponse;
use crate::event::{AssistantMessageFinishedEvent, EventBuilder};
use crate::ids::ToolCallId;
use crate::session::{AssistantPart, ProviderProvenance};

pub(super) fn assistant_message_finished_event<C, R>(
    builder: &EventBuilder<'_, C, R>,
    response: &AssistantResponse,
    tool_call_ids: &[ToolCallId],
) -> AssistantMessageFinishedEvent
where
    C: crate::clock::Clock + ?Sized,
    R: crate::redact::Redactor + ?Sized,
{
    let mut parts = Vec::with_capacity(response.tool_intents.len().saturating_add(2));
    if !response.reasoning.is_empty() {
        parts.push(AssistantPart::Reasoning {
            text: response.reasoning.clone(),
        });
    }
    if !response.text.is_empty() {
        parts.push(AssistantPart::Text {
            text: response.text.clone(),
        });
    }

    let provider_call_id = response
        .finished_metadata
        .provider_call_id
        .as_deref()
        .or(response.started_metadata.provider_call_id.as_deref());
    parts.extend(
        response
            .tool_intents
            .iter()
            .zip(tool_call_ids)
            .map(|(intent, tool_call_id)| {
                builder.assistant_tool_call_part(intent, tool_call_id.clone(), provider_call_id)
            }),
    );

    let assistant_message = response.finished_metadata.assistant_message.clone();
    let response_id = response
        .finished_metadata
        .provider_response_id
        .clone()
        .or_else(|| {
            assistant_message
                .as_ref()
                .and_then(|metadata| metadata.message_id.clone())
        });
    let stop_reason = response
        .finished_metadata
        .provider_stop_reason
        .clone()
        .or_else(|| Some(response.stop_reason.clone()));

    AssistantMessageFinishedEvent {
        request_id: response.request_id.to_string().into(),
        tool_call_count: response.tool_intents.len(),
        parts,
        provenance: Some(ProviderProvenance {
            provider_id: response.provider_id.clone(),
            model_id: response.model_id.clone(),
            request_id: response.request_id.to_string().into(),
            response_id,
            stop_reason,
            usage: response.usage.clone(),
        }),
        assistant_message,
    }
}
