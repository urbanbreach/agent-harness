use std::collections::BTreeMap;

use crate::ids::{EntryId, ToolCallId};
use crate::session::{AssistantPart, SessionEntry, SessionEntryPayload};

use super::snapshot::{
    ActiveCompactionBranch, ActivePathCompactionSnapshot, ActivePathCompactionSnapshotInput,
    CompactionSnapshotEntry, CompactionSnapshotError, PriorActiveCompactionSummary,
    ToolPairIdentity,
};

/// Projects the canonical active branch into one protocol-safe compaction snapshot.
///
/// Branch summaries and all historical compaction entries are excluded from `entries`; only the
/// latest active compaction summary is retained in `prior_active_summary`. Tool calls and results
/// appear only when a unique call has one matching result on the active path.
///
/// # Errors
/// Returns [`CompactionSnapshotError`] when canonical ancestry is malformed or owner identity does
/// not match the canonical session.
pub fn build_active_path_compaction_snapshot(
    input: ActivePathCompactionSnapshotInput<'_>,
) -> Result<ActivePathCompactionSnapshot, CompactionSnapshotError> {
    if input.owner.session_id() != input.session.session_id() {
        return Err(CompactionSnapshotError::OwnerSessionMismatch {
            expected: input.session.session_id().clone(),
            actual: input.owner.session_id().clone(),
        });
    }
    let active_path = input.session.active_path()?;
    let mut calls = BTreeMap::<ToolCallId, Vec<EntryId>>::new();
    let mut results = BTreeMap::<ToolCallId, Vec<(EntryId, EntryId)>>::new();
    for entry in &active_path {
        match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => {
                for part in parts {
                    match part {
                        AssistantPart::ToolCall(call) => calls
                            .entry(call.tool_call_id.clone())
                            .or_default()
                            .push(entry.id.clone()),
                        AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => {}
                    }
                }
            }
            SessionEntryPayload::ToolResult {
                tool_call_id,
                requesting_assistant_entry_id,
                ..
            } => results
                .entry(tool_call_id.clone())
                .or_default()
                .push((entry.id.clone(), requesting_assistant_entry_id.clone())),
            SessionEntryPayload::UserMessage { .. }
            | SessionEntryPayload::ModelChange { .. }
            | SessionEntryPayload::ReasoningSettingChange { .. }
            | SessionEntryPayload::SystemContextUpdate { .. }
            | SessionEntryPayload::CompactionSummary { .. }
            | SessionEntryPayload::BranchSummary { .. }
            | SessionEntryPayload::CustomPersistedState { .. }
            | SessionEntryPayload::CustomModelVisibleContext { .. }
            | SessionEntryPayload::SessionMetadata { .. } => {}
        }
    }

    let tool_pairs = calls
        .into_iter()
        .filter_map(|(tool_call_id, assistant_entries)| {
            let [assistant_entry_id] = assistant_entries.as_slice() else {
                return None;
            };
            let [result] = results.get(&tool_call_id)?.as_slice() else {
                return None;
            };
            (result.1 == *assistant_entry_id).then(|| {
                (
                    tool_call_id.clone(),
                    ToolPairIdentity {
                        tool_call_id,
                        assistant_entry_id: assistant_entry_id.clone(),
                        result_entry_id: result.0.clone(),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();

    let prior_active_summary = active_path.iter().rev().find_map(|entry| {
        let SessionEntryPayload::CompactionSummary {
            summary,
            first_kept_entry_id,
            ..
        } = &entry.payload
        else {
            return None;
        };
        Some(PriorActiveCompactionSummary {
            entry_id: entry.id.clone(),
            summary: summary.clone(),
            first_kept_entry_id: first_kept_entry_id.clone(),
            legacy_source_sequence: input.legacy_source_sequences.sequence_for(&entry.id),
        })
    });

    let entries = active_path
        .iter()
        .filter_map(|entry| {
            protocol_safe_entry(entry, &tool_pairs).map(|entry| CompactionSnapshotEntry {
                legacy_source_sequence: input.legacy_source_sequences.sequence_for(&entry.id),
                tool_pairs: tool_pairs
                    .values()
                    .filter(|pair| {
                        pair.assistant_entry_id == entry.id || pair.result_entry_id == entry.id
                    })
                    .cloned()
                    .collect(),
                entry,
            })
        })
        .collect();

    Ok(ActivePathCompactionSnapshot {
        owner: input.owner,
        active_branch: ActiveCompactionBranch {
            leaf_entry_id: input.session.active_leaf().cloned(),
            entry_ids: active_path.iter().map(|entry| entry.id.clone()).collect(),
        },
        entries,
        pending_prompt: input.pending_prompt,
        prior_active_summary,
        current_model: input.current_model,
    })
}

fn protocol_safe_entry(
    entry: &SessionEntry,
    tool_pairs: &BTreeMap<ToolCallId, ToolPairIdentity>,
) -> Option<SessionEntry> {
    let payload = match &entry.payload {
        SessionEntryPayload::AssistantMessage { parts, provenance } => {
            SessionEntryPayload::AssistantMessage {
                parts: parts
                    .iter()
                    .filter(|part| match part {
                        AssistantPart::ToolCall(call) => tool_pairs
                            .get(&call.tool_call_id)
                            .is_some_and(|pair| pair.assistant_entry_id == entry.id),
                        AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => true,
                    })
                    .cloned()
                    .collect(),
                provenance: provenance.clone(),
            }
        }
        SessionEntryPayload::ToolResult { tool_call_id, .. } => {
            if tool_pairs
                .get(tool_call_id)
                .is_none_or(|pair| pair.result_entry_id != entry.id)
            {
                return None;
            }
            entry.payload.clone()
        }
        SessionEntryPayload::CompactionSummary { .. }
        | SessionEntryPayload::BranchSummary { .. } => return None,
        SessionEntryPayload::UserMessage { .. }
        | SessionEntryPayload::ModelChange { .. }
        | SessionEntryPayload::ReasoningSettingChange { .. }
        | SessionEntryPayload::SystemContextUpdate { .. }
        | SessionEntryPayload::CustomPersistedState { .. }
        | SessionEntryPayload::CustomModelVisibleContext { .. }
        | SessionEntryPayload::SessionMetadata { .. } => entry.payload.clone(),
    };
    Some(SessionEntry {
        id: entry.id.clone(),
        parent_id: entry.parent_id.clone(),
        turn_id: entry.turn_id.clone(),
        run_id: entry.run_id.clone(),
        payload,
    })
}
