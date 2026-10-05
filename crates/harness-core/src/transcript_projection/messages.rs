use super::*;
use crate::session::AssistantPart;

fn assistant(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    event: &EventEnvelopeV1,
    provider_request: &str,
) -> usize {
    if let Some(&at) = index.providers.get(provider_request) {
        return at;
    }
    let turn = event.correlation_id.as_deref().unwrap_or(provider_request);
    let at = push_message(output, event, ProjectedMessageRole::Assistant, Some(turn));
    output.messages[at].state = ProjectedMessageState::Streaming;
    index.providers.insert(provider_request.into(), at);
    index.turns.insert(turn.into(), at);
    at
}

pub(super) fn apply(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    event: &EventEnvelopeV1,
) -> bool {
    match &event.payload {
        EventV1::UserMessageSubmitted(data) => {
            let at = push_message(
                output,
                event,
                ProjectedMessageRole::User,
                Some(data.request_id.as_str()),
            );
            output.messages[at]
                .parts
                .push(ProjectedPart::Text(ProjectedTextPart {
                    text: data.text.clone(),
                    provenance: ProvenanceRange::at(event),
                }));
            index.users.insert(data.request_id.to_string(), at);
        }
        EventV1::PromptAttachmentsSubmitted(data) => {
            if let Some(&at) = index.users.get(data.request_id.as_str()) {
                output.messages[at]
                    .attachments
                    .clone_from(&data.attachments);
                output.messages[at].provenance.extend(event);
            }
        }
        EventV1::ProviderRequestStarted(data) => {
            let at = assistant(output, index, event, data.request_id.as_str());
            let message = &mut output.messages[at];
            message.provider = Some(ProjectedProviderMessageMetadata {
                provider_request_id: Some(data.request_id.to_string()),
                provider_id: Some(data.provider_id.clone()),
                model_id: Some(data.model_id.clone()),
                prompt_summary: Some(data.prompt_summary.clone()),
                request_digest: Some(data.request_digest.clone()),
                ..Default::default()
            });
            message.provenance.extend(event);
        }
        EventV1::ProviderStreamDelta(data) | EventV1::ProviderReasoningDelta(data) => {
            let at = assistant(output, index, event, data.request_id.as_str());
            let message = &mut output.messages[at];
            let reasoning = matches!(event.payload, EventV1::ProviderReasoningDelta(_));
            match (reasoning, message.parts.last_mut()) {
                (true, Some(ProjectedPart::Reasoning(text)))
                | (false, Some(ProjectedPart::Text(text))) => {
                    text.text.push_str(&data.delta);
                    text.provenance.extend(event);
                }
                _ => {
                    let text = ProjectedTextPart {
                        text: data.delta.clone(),
                        provenance: ProvenanceRange::at(event),
                    };
                    message.parts.push(if reasoning {
                        ProjectedPart::Reasoning(text)
                    } else {
                        ProjectedPart::Text(text)
                    });
                }
            }
            message.provenance.extend(event);
        }
        EventV1::ProviderRequestFinished(data) => {
            let at = assistant(output, index, event, data.request_id.as_str());
            let message = &mut output.messages[at];
            message.state = match data.finish_reason.as_str() {
                "error" | "failed" => ProjectedMessageState::Failed,
                "cancelled" | "canceled" => ProjectedMessageState::Incomplete,
                _ => ProjectedMessageState::Complete,
            };
            let provider = message.provider.get_or_insert_with(Default::default);
            provider.finish_reason = Some(data.finish_reason.clone());
            provider.output_digest.clone_from(&data.output_digest);
            if let Some(metadata) = data
                .metadata
                .as_ref()
                .and_then(|m| m.assistant_message.as_ref())
            {
                merge_metadata(provider, metadata);
            }
            message.provenance.extend(event);
        }
        EventV1::AssistantMessageFinished(data) => {
            let at = assistant(output, index, event, data.request_id.as_str());
            if !data.parts.is_empty() {
                commit_parts(output, index, at, &data.parts, event);
            }
            let message = &mut output.messages[at];
            if message.state == ProjectedMessageState::Streaming {
                message.state = ProjectedMessageState::Complete;
            }
            if let Some(metadata) = &data.assistant_message {
                merge_metadata(
                    message.provider.get_or_insert_with(Default::default),
                    metadata,
                );
            }
            message.provenance.extend(event);
        }
        _ => return false,
    }
    true
}

fn merge_metadata(
    provider: &mut ProjectedProviderMessageMetadata,
    metadata: &ProviderAssistantMessageMetadata,
) {
    provider
        .assistant_message_id
        .clone_from(&metadata.message_id);
    provider
        .assistant_text_digest
        .clone_from(&metadata.text_digest);
    provider
        .assistant_reasoning_digest
        .clone_from(&metadata.reasoning_digest);
}

fn commit_parts(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    at: usize,
    parts: &[AssistantPart],
    event: &EventEnvelopeV1,
) {
    let mut tools = indexmap::IndexMap::new();
    let mut remaining = Vec::new();
    for part in std::mem::take(&mut output.messages[at].parts) {
        match part {
            ProjectedPart::ToolCall(tool) => {
                index.tools.remove(tool.tool_call_id.as_str());
                tools.insert(tool.tool_call_id.to_string(), tool);
            }
            ProjectedPart::Text(_) | ProjectedPart::Reasoning(_) => {}
            other => remaining.push(other),
        }
    }
    let message = &mut output.messages[at];
    for part in parts {
        let part = match part {
            AssistantPart::Text { text } => ProjectedPart::Text(ProjectedTextPart {
                text: text.clone(),
                provenance: ProvenanceRange::at(event),
            }),
            AssistantPart::Reasoning { text } => ProjectedPart::Reasoning(ProjectedTextPart {
                text: text.clone(),
                provenance: ProvenanceRange::at(event),
            }),
            AssistantPart::ToolCall(call) => {
                let mut tool = tools
                    .shift_remove(call.tool_call_id.as_str())
                    .unwrap_or_default();
                tool.tool_call_id = call.tool_call_id.clone();
                tool.tool_id.clone_from(&call.tool_id);
                tool.args_summary.clone_from(&call.args_summary);
                tool.args_digest.clone_from(&call.args_digest);
                if tool.provenance.first_seq == 0 {
                    tool.provenance = ProvenanceRange::at(event);
                } else {
                    tool.provenance.extend(event);
                }
                index
                    .tools
                    .insert(call.tool_call_id.to_string(), (at, message.parts.len()));
                ProjectedPart::ToolCall(tool)
            }
        };
        message.parts.push(part);
    }
    // Coordinator-owned eval children are absent from the provider's commit.
    message
        .parts
        .extend(tools.into_values().map(ProjectedPart::ToolCall));
    message.parts.extend(remaining);
    // A legacy result may precede its semantic commit; rebuild only this message's small indices.
    for (part_at, part) in message.parts.iter().enumerate() {
        match part {
            ProjectedPart::ToolCall(tool) => {
                index
                    .tools
                    .insert(tool.tool_call_id.to_string(), (at, part_at));
                for (permission_at, permission) in tool.permissions.iter().enumerate() {
                    index.permissions.insert(
                        permission.permission_id.clone(),
                        (at, part_at, Some(permission_at)),
                    );
                }
            }
            ProjectedPart::Permission(permission) => {
                index
                    .permissions
                    .insert(permission.permission_id.clone(), (at, part_at, None));
            }
            _ => {}
        }
    }
}
