use super::{
    tool_pairs, CanonicalActivePathSelection, CanonicalAttachment, CanonicalCompactionSummary,
    CanonicalProviderView, CanonicalUsageBoundary, ProviderViewError, ProviderViewInput,
    ProviderViewOwner, UsageBoundaryKind,
};
use crate::ids::EntryId;
use crate::session::{CanonicalSession, SessionEntryPayload};

pub(super) fn build(
    session: &CanonicalSession,
    input: ProviderViewInput,
) -> Result<CanonicalProviderView, ProviderViewError> {
    input.runtime_selection.validate()?;
    let selected = select_active_path(session, &input.owner, input.selected_leaf.as_ref())?;
    Ok(CanonicalProviderView {
        owner: input.owner,
        selected_leaf: selected.selected_leaf,
        active_entry_ids: selected.active_entry_ids,
        entries: selected.entries,
        pending_prompt: input.pending_prompt,
        latest_compaction_summary: selected.latest_compaction_summary,
        tool_pairs: selected.tool_pairs,
        attachments: selected.attachments,
        usage_boundaries: selected.usage_boundaries,
        watermark: session.watermark(),
        runtime_selection: input.runtime_selection,
    })
}

pub(super) fn select_active_path(
    session: &CanonicalSession,
    owner: &ProviderViewOwner,
    selected_leaf: Option<&EntryId>,
) -> Result<CanonicalActivePathSelection, ProviderViewError> {
    if owner.session_id() != session.session_id() {
        return Err(ProviderViewError::OwnerSessionMismatch {
            expected: session.session_id().clone(),
            actual: owner.session_id().clone(),
        });
    }
    let persisted_leaf = session
        .active_leaf()
        .cloned()
        .ok_or(ProviderViewError::MissingActiveLeaf)?;
    if let Some(selected) = selected_leaf {
        if selected != &persisted_leaf {
            return Err(ProviderViewError::SelectedLeafMismatch {
                selected: selected.clone(),
                persisted: persisted_leaf,
            });
        }
    }
    let active_path = session.active_path()?;
    let tool_pairs = tool_pairs::complete_pairs(&active_path);
    let latest_compaction_summary = active_path.iter().rev().find_map(|entry| {
        let SessionEntryPayload::CompactionSummary {
            summary,
            first_kept_entry_id,
            tokens_after,
            summary_usage,
            summary_provider_id,
            summary_model_id,
            ..
        } = &entry.payload
        else {
            return None;
        };
        Some(CanonicalCompactionSummary {
            entry_id: entry.id.clone(),
            summary: summary.clone(),
            first_kept_entry_id: first_kept_entry_id.clone(),
            tokens_after: *tokens_after,
            usage: summary_usage.clone(),
            provider_id: summary_provider_id.clone(),
            model_id: summary_model_id.clone(),
        })
    });
    let attachments = active_path
        .iter()
        .flat_map(|entry| match &entry.payload {
            SessionEntryPayload::UserMessage { attachments, .. } => attachments
                .iter()
                .cloned()
                .map(|attachment| CanonicalAttachment {
                    entry_id: entry.id.clone(),
                    attachment,
                })
                .collect::<Vec<_>>(),
            SessionEntryPayload::AssistantMessage { .. }
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
        .collect();
    let usage_boundaries = active_path
        .iter()
        .filter_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage {
                provenance: Some(provenance),
                ..
            } => provenance
                .usage
                .clone()
                .map(|usage| CanonicalUsageBoundary {
                    entry_id: entry.id.clone(),
                    kind: UsageBoundaryKind::Provider,
                    usage,
                }),
            SessionEntryPayload::CompactionSummary {
                summary_usage: Some(usage),
                ..
            } => Some(CanonicalUsageBoundary {
                entry_id: entry.id.clone(),
                kind: UsageBoundaryKind::Compaction,
                usage: usage.clone(),
            }),
            SessionEntryPayload::UserMessage { .. }
            | SessionEntryPayload::AssistantMessage {
                provenance: None, ..
            }
            | SessionEntryPayload::ToolResult { .. }
            | SessionEntryPayload::ModelChange { .. }
            | SessionEntryPayload::ReasoningSettingChange { .. }
            | SessionEntryPayload::SystemContextUpdate { .. }
            | SessionEntryPayload::CompactionSummary {
                summary_usage: None,
                ..
            }
            | SessionEntryPayload::BranchSummary { .. }
            | SessionEntryPayload::CustomPersistedState { .. }
            | SessionEntryPayload::CustomModelVisibleContext { .. }
            | SessionEntryPayload::SessionMetadata { .. } => None,
        })
        .collect();
    let entries = active_path
        .iter()
        .filter_map(|entry| tool_pairs::protocol_safe_entry(entry, &tool_pairs))
        .collect();
    Ok(CanonicalActivePathSelection {
        selected_leaf: persisted_leaf,
        active_entry_ids: active_path.iter().map(|entry| entry.id.clone()).collect(),
        entries,
        latest_compaction_summary,
        tool_pairs,
        attachments,
        usage_boundaries,
    })
}
