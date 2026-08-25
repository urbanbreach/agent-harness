use std::collections::BTreeMap;

use crate::ids::EntryId;
use crate::session::{AssistantPart, CanonicalProviderView, SessionEntryPayload};

pub(super) fn tool_ids(view: &CanonicalProviderView) -> BTreeMap<String, String> {
    view.entries
        .iter()
        .flat_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => {
                        Some((call.tool_call_id.to_string(), call.tool_id.clone()))
                    }
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

pub(super) fn visible_historical_attachment_groups(
    view: &CanonicalProviderView,
) -> Vec<Vec<&crate::attachment_transport::AttachmentMetadata>> {
    let first_kept = view
        .latest_compaction_summary
        .as_ref()
        .map(|summary| &summary.first_kept_entry_id);
    let mut include_entry = first_kept.is_none();
    view.entries
        .iter()
        .filter_map(|entry| {
            include_entry |= first_kept == Some(&entry.id);
            include_entry.then(|| {
                view.attachments
                    .iter()
                    .filter(|attachment| attachment.entry_id == entry.id)
                    .map(|attachment| &attachment.attachment)
                    .collect::<Vec<_>>()
            })
        })
        .filter(|attachments| !attachments.is_empty())
        .collect()
}

pub(super) fn provider_tool_call_ids(view: &CanonicalProviderView) -> BTreeMap<String, String> {
    view.entries
        .iter()
        .flat_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => call
                        .provider_tool_call_id
                        .as_ref()
                        .map(|provider_id| (call.tool_call_id.to_string(), provider_id.clone())),
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => None,
                })
                .collect::<Vec<_>>(),
            SessionEntryPayload::UserMessage { .. }
            | SessionEntryPayload::ToolResult { .. }
            | SessionEntryPayload::ModelChange { .. }
            | SessionEntryPayload::ReasoningSettingChange { .. }
            | SessionEntryPayload::SystemContextUpdate { .. }
            | SessionEntryPayload::CompactionSummary { .. }
            | SessionEntryPayload::BranchSummary { .. }
            | SessionEntryPayload::CustomPersistedState { .. }
            | SessionEntryPayload::CustomModelVisibleContext { .. }
            | SessionEntryPayload::SessionMetadata { .. } => Vec::new(),
        })
        .collect()
}

pub(super) fn attachments_for_entry(
    view: &CanonicalProviderView,
    entry_id: &EntryId,
) -> Vec<crate::attachment_transport::AttachmentMetadata> {
    view.attachments
        .iter()
        .filter(|attachment| &attachment.entry_id == entry_id)
        .map(|attachment| attachment.attachment.clone())
        .collect()
}
