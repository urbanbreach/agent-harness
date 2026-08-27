use harness_core::attachment_transport::{AttachmentDimensions, AttachmentMetadata};
use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalRecord, CanonicalRecordKind, CanonicalSession,
    ProviderProvenance, RecordSequence, RunAttempt, RunStatus, SessionEntry, SessionEntryPayload,
    ToolResultStatus,
};
use harness_core::UnwrapOrAbort;
use harness_providers::CompletionUsage;
use serde_json::{json, Value};

pub(super) fn provider_context_digest(session: &CanonicalSession, owner: &str) -> String {
    let path = session.active_path().unwrap_or_abort();
    let value = json!({
        "owner": owner,
        "session_id": session.session_id(),
        "entry_ids": path.iter().map(|entry| &entry.id).collect::<Vec<_>>(),
        "entries": path.iter().map(|entry| serde_json::to_value(entry).unwrap_or_abort()).collect::<Vec<_>>(),
        "tool_pairs": complete_tool_pairs(&path).iter().map(tool_pair_json).collect::<Vec<_>>(),
    });
    blake3::hash(&serde_json::to_vec(&value).unwrap_or_abort())
        .to_hex()
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolPairIdentity {
    pub(super) tool_call_id: ToolCallId,
    pub(super) assistant_entry_id: EntryId,
    pub(super) result_entry_id: EntryId,
}

pub(super) fn complete_tool_pairs(path: &[&SessionEntry]) -> Vec<ToolPairIdentity> {
    path.iter()
        .filter_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => parts.iter().find_map(|part| {
                let AssistantPart::ToolCall(call) = part else {
                    return None;
                };
                path.iter().find_map(|candidate| match &candidate.payload {
                    SessionEntryPayload::ToolResult {
                        tool_call_id,
                        requesting_assistant_entry_id,
                        ..
                    } if tool_call_id == &call.tool_call_id
                        && requesting_assistant_entry_id == &entry.id =>
                    {
                        Some(ToolPairIdentity {
                            tool_call_id: call.tool_call_id.clone(),
                            assistant_entry_id: entry.id.clone(),
                            result_entry_id: candidate.id.clone(),
                        })
                    }
                    _ => None,
                })
            }),
            _ => None,
        })
        .collect()
}

pub(super) fn tool_pair_json(pair: &ToolPairIdentity) -> Value {
    json!({
        "tool_call_id": &pair.tool_call_id,
        "assistant_entry_id": &pair.assistant_entry_id,
        "result_entry_id": &pair.result_entry_id,
    })
}

mod fixture {
    include!("03_provider_view_selected_branch_fixture_test.rs");
}
pub(super) use fixture::fixture_records;
