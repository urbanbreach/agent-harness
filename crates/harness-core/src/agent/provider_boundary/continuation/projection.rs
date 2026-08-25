use crate::agent::ProviderConversationTurn;
use crate::conversation::{
    ConversationAssistantMessage, ConversationCheckpointMessage, ConversationMessage,
    ConversationToolCall, ConversationToolResultMessage, ConversationUserMessage,
};

mod overlay;
mod view_metadata;

use view_metadata::{attachments_for_entry, tool_ids};

use crate::event::ToolCallStatus;
use crate::ids::{EntryId, RequestId};
use crate::session::{
    AssistantPart, CanonicalProviderView, SessionEntry, SessionEntryPayload, ToolResultStatus,
};
pub(super) fn conversation_messages(view: &CanonicalProviderView) -> Vec<ConversationMessage> {
    conversation_messages_excluding(view, &std::collections::BTreeSet::new(), true, true)
}

pub(super) fn provider_tool_call_ids(view: &CanonicalProviderView) -> BTreeMap<String, String> {
    view_metadata::provider_tool_call_ids(view)
}

pub(super) fn visible_historical_attachment_groups(
    view: &CanonicalProviderView,
) -> Vec<Vec<&crate::attachment_transport::AttachmentMetadata>> {
    view_metadata::visible_historical_attachment_groups(view)
}

pub(super) fn recovery_conversation_messages(
    view: &CanonicalProviderView,
) -> Vec<ConversationMessage> {
    let kept_ids = recovery_kept_entry_ids(view);
    let tool_ids = tool_ids(view);
    view.entries
        .iter()
        .filter(|entry| kept_ids.contains(&entry.id))
        .filter_map(|entry| {
            entry_message(
                entry,
                &view.owner.agent_id,
                &attachments_for_entry(view, &entry.id),
                &tool_ids,
                false,
            )
        })
        .collect()
}

fn recovery_kept_entry_ids(view: &CanonicalProviderView) -> std::collections::BTreeSet<EntryId> {
    let Some(summary) = view.latest_compaction_summary.as_ref() else {
        return view.active_entry_ids.iter().cloned().collect();
    };
    let boundary = view
        .active_entry_ids
        .iter()
        .position(|entry_id| entry_id == &summary.first_kept_entry_id)
        .unwrap_or(0);
    view.active_entry_ids[boundary..]
        .iter()
        .filter(|entry_id| entry_id != &&summary.entry_id)
        .cloned()
        .collect()
}

pub(super) fn conversation_messages_with_transient_overlay(
    view: &CanonicalProviderView,
    transient_turns: &[ProviderConversationTurn],
) -> Vec<ConversationMessage> {
    overlay::conversation_messages_with_transient_overlay(view, transient_turns)
}

fn conversation_messages_excluding(
    view: &CanonicalProviderView,
    excluded_turn_ids: &std::collections::BTreeSet<String>,
    include_pending_prompt: bool,
    lower_attachments: bool,
) -> Vec<ConversationMessage> {
    let mut messages = Vec::with_capacity(view.entries.len().saturating_add(2));
    let first_kept = view
        .latest_compaction_summary
        .as_ref()
        .map(|summary| summary.first_kept_entry_id.as_str());
    if let Some(summary) = &view.latest_compaction_summary {
        messages.push(ConversationMessage::Checkpoint(
            ConversationCheckpointMessage {
                checkpoint_id: summary.entry_id.to_string(),
                agent_id: view.owner.agent_id.clone(),
                through_seq: view.watermark.map_or(0, |watermark| watermark.get()),
                summary: summary.summary.clone(),
            },
        ));
    }
    let mut include_entry = first_kept.is_none();
    let tool_ids = tool_ids(view);
    for entry in &view.entries {
        include_entry |= first_kept == Some(entry.id.as_str());
        let excluded = entry
            .turn_id
            .as_ref()
            .is_some_and(|turn_id| excluded_turn_ids.contains(turn_id.as_str()));
        if include_entry && !excluded {
            messages.extend(entry_message(
                entry,
                &view.owner.agent_id,
                &attachments_for_entry(view, &entry.id),
                &tool_ids,
                lower_attachments,
            ));
        }
    }
    if include_pending_prompt {
        messages.extend(pending_prompt_message(view));
    }
    messages
}

fn pending_prompt_message(view: &CanonicalProviderView) -> Option<ConversationMessage> {
    let prompt = view.pending_prompt.as_ref()?;
    Some(ConversationMessage::User(ConversationUserMessage {
        request_id: RequestId::new(prompt.turn_id.to_string()),
        text: crate::attachment_transport::lower_provider_attachments(
            &prompt.text,
            &prompt.attachments,
        ),
        seq: None,
        agent_id: Some(view.owner.agent_id.clone()),
    }))
}

fn entry_message(
    entry: &SessionEntry,
    agent_id: &str,
    attachments: &[crate::attachment_transport::AttachmentMetadata],
    tool_ids: &BTreeMap<String, String>,
    lower_attachments: bool,
) -> Option<ConversationMessage> {
    let request_id = RequestId::new(
        entry
            .turn_id
            .as_ref()
            .map_or_else(|| entry.id.to_string(), ToString::to_string),
    );
    match &entry.payload {
        SessionEntryPayload::UserMessage { text, .. } => {
            Some(ConversationMessage::User(ConversationUserMessage {
                request_id,
                text: if lower_attachments {
                    crate::attachment_transport::lower_provider_attachments(text, attachments)
                } else {
                    text.clone()
                },
                seq: None,
                agent_id: Some(agent_id.to_string()),
            }))
        }
        SessionEntryPayload::AssistantMessage { parts, provenance } => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::Text { text } => Some(text.as_str()),
                    AssistantPart::Reasoning { .. } | AssistantPart::ToolCall(_) => None,
                })
                .collect::<String>();
            let tool_calls = parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => Some(ConversationToolCall {
                        tool_call_id: call.tool_call_id.clone(),
                        tool_id: call.tool_id.clone(),
                        args_summary: call.args_summary.clone(),
                        args_digest: call.args_digest.clone(),
                        seq: None,
                        metadata: None,
                    }),
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => None,
                })
                .collect();
            Some(ConversationMessage::Assistant(
                ConversationAssistantMessage {
                    request_id,
                    agent_id: Some(agent_id.to_string()),
                    text,
                    tool_calls,
                    stop_reason: provenance
                        .as_deref()
                        .and_then(|value| value.stop_reason.clone()),
                    first_seq: None,
                    last_seq: None,
                    provider_id: provenance.as_deref().map(|value| value.provider_id.clone()),
                    model_id: provenance.as_deref().map(|value| value.model_id.clone()),
                    output_digest: None,
                },
            ))
        }
        SessionEntryPayload::ToolResult {
            tool_call_id,
            status,
            output_summary,
            output_digest,
            output_json,
            ..
        } => Some(ConversationMessage::ToolResult(Box::new(
            ConversationToolResultMessage {
                request_id,
                tool_call_id: tool_call_id.clone(),
                tool_id: tool_ids.get(tool_call_id.as_str()).cloned(),
                status: match status {
                    ToolResultStatus::Succeeded => ToolCallStatus::Succeeded,
                    ToolResultStatus::Failed => ToolCallStatus::Failed,
                },
                output_summary: output_summary.clone(),
                output_digest: output_digest.clone(),
                output_json: output_json.clone(),
                seq: None,
                metadata: None,
            },
        ))),
        SessionEntryPayload::ModelChange { .. }
        | SessionEntryPayload::ReasoningSettingChange { .. }
        | SessionEntryPayload::SystemContextUpdate { .. }
        | SessionEntryPayload::CompactionSummary { .. }
        | SessionEntryPayload::BranchSummary { .. }
        | SessionEntryPayload::CustomPersistedState { .. }
        | SessionEntryPayload::CustomModelVisibleContext { .. }
        | SessionEntryPayload::SessionMetadata { .. } => None,
    }
}

use std::collections::BTreeMap;
