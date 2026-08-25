use std::collections::BTreeMap;

use crate::ids::{EntryId, ToolCallId};
use crate::session::{AssistantPart, SessionEntry, SessionEntryPayload};

use super::CanonicalToolPair;

pub(super) fn complete_pairs(active_path: &[&SessionEntry]) -> Vec<CanonicalToolPair> {
    let mut call_order = Vec::new();
    let mut calls = BTreeMap::<ToolCallId, Vec<EntryId>>::new();
    let mut results = BTreeMap::<ToolCallId, Vec<(EntryId, EntryId)>>::new();
    for entry in active_path {
        match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => {
                for part in parts {
                    match part {
                        AssistantPart::ToolCall(call) => {
                            call_order.push(call.tool_call_id.clone());
                            calls
                                .entry(call.tool_call_id.clone())
                                .or_default()
                                .push(entry.id.clone());
                        }
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
    call_order
        .into_iter()
        .filter_map(|tool_call_id| {
            let [assistant_entry_id] = calls.get(&tool_call_id)?.as_slice() else {
                return None;
            };
            let [result] = results.get(&tool_call_id)?.as_slice() else {
                return None;
            };
            (result.1 == *assistant_entry_id).then(|| CanonicalToolPair {
                tool_call_id,
                assistant_entry_id: assistant_entry_id.clone(),
                result_entry_id: result.0.clone(),
            })
        })
        .collect()
}

pub(super) fn protocol_safe_entry(
    entry: &SessionEntry,
    tool_pairs: &[CanonicalToolPair],
) -> Option<SessionEntry> {
    let payload = match &entry.payload {
        SessionEntryPayload::AssistantMessage { parts, provenance } => {
            SessionEntryPayload::AssistantMessage {
                parts: parts
                    .iter()
                    .filter(|part| match part {
                        AssistantPart::ToolCall(call) => tool_pairs.iter().any(|pair| {
                            let same_call = pair.tool_call_id == call.tool_call_id;
                            let same_entry = pair.assistant_entry_id == entry.id;
                            same_call && same_entry
                        }),
                        AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => true,
                    })
                    .cloned()
                    .collect(),
                provenance: provenance.clone(),
            }
        }
        SessionEntryPayload::ToolResult { tool_call_id, .. } => {
            if !tool_pairs
                .iter()
                .any(|pair| pair.tool_call_id == *tool_call_id && pair.result_entry_id == entry.id)
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
