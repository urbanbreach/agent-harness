use std::collections::{BTreeMap, BTreeSet};

use super::{
    AssistantPart, CanonicalRecord, CanonicalRecordKind, CanonicalSession, SessionEntry,
    SessionEntryPayload, SessionError,
};
use crate::ids::{EntryId, SessionId, ToolCallId};

mod active_path;

pub(crate) use active_path::active_path;
use active_path::validate_selected_tool_pairs;

#[derive(Default)]
struct ToolPairingState {
    calls: BTreeMap<ToolCallId, EntryId>,
    results: BTreeMap<ToolCallId, EntryId>,
    settled: BTreeSet<ToolCallId>,
}

pub fn replay(
    session_id: SessionId,
    records: &[CanonicalRecord],
) -> Result<CanonicalSession, SessionError> {
    let mut session = CanonicalSession::empty(session_id);
    let mut record_sequences = BTreeSet::new();
    let mut tool_pairing = ToolPairingState::default();

    for record in records {
        if record.session_id != session.session_id {
            return Err(SessionError::MixedSession {
                expected: session.session_id.clone(),
                actual: record.session_id.clone(),
            });
        }

        let sequence = record.sequence.get();
        if !record_sequences.insert(sequence) {
            return Err(SessionError::DuplicateRecord { sequence });
        }
        let expected_previous = session.watermark.map_or(0, |watermark| watermark.get());
        if sequence != expected_previous.saturating_add(1) {
            return Err(SessionError::NonContiguousSequence {
                expected_previous,
                actual: sequence,
            });
        }
        if session.status.is_terminal() {
            return Err(SessionError::TerminalSessionMutation {
                session_id: session.session_id.clone(),
            });
        }

        match &record.kind {
            CanonicalRecordKind::RunStarted { attempt } => {
                if session.run_attempts.contains_key(&attempt.run_id) {
                    return Err(SessionError::DuplicateRun {
                        run_id: attempt.run_id.clone(),
                    });
                }
                session
                    .run_attempts
                    .insert(attempt.run_id.clone(), attempt.clone());
            }
            CanonicalRecordKind::RunStatusChanged { run_id, status } => {
                let Some(attempt) = session.run_attempts.get_mut(run_id) else {
                    return Err(SessionError::UnknownRun {
                        run_id: run_id.clone(),
                    });
                };
                if attempt.status.is_terminal() {
                    return Err(SessionError::TerminalRunMutation {
                        run_id: run_id.clone(),
                    });
                }
                attempt.status = *status;
            }
            CanonicalRecordKind::EntryCommitted { entry } => {
                validate_entry(&session, entry, &tool_pairing)?;
                let advances_active_path = match session.active_leaf.as_ref() {
                    None => entry.parent_id.is_none(),
                    Some(active_leaf) => entry.parent_id.as_ref() == Some(active_leaf),
                };
                match &entry.payload {
                    SessionEntryPayload::AssistantMessage { parts, .. } => {
                        for part in parts {
                            match part {
                                AssistantPart::ToolCall(tool_call) => {
                                    tool_pairing
                                        .calls
                                        .insert(tool_call.tool_call_id.clone(), entry.id.clone());
                                }
                                AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => {}
                            }
                        }
                    }
                    SessionEntryPayload::ToolResult { tool_call_id, .. } => {
                        tool_pairing
                            .results
                            .insert(tool_call_id.clone(), entry.id.clone());
                        tool_pairing.settled.insert(tool_call_id.clone());
                    }
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
                session.entries.insert(entry.id.clone(), entry.clone());
                if advances_active_path {
                    session.active_leaf = Some(entry.id.clone());
                }
            }
            CanonicalRecordKind::ActiveLeafSelected { entry_id } => {
                if !session.entries.contains_key(entry_id) {
                    return Err(SessionError::ActiveLeafMissing {
                        entry_id: entry_id.clone(),
                    });
                }
                validate_selected_tool_pairs(&session, entry_id, &tool_pairing)?;
                session.active_leaf = Some(entry_id.clone());
            }
            CanonicalRecordKind::SessionMetadataUpdated { metadata } => {
                session.metadata = metadata.clone();
            }
            CanonicalRecordKind::SessionStatusChanged { status } => {
                session.status = *status;
            }
        }
        session.watermark = Some(record.sequence);
    }

    Ok(session)
}

fn validate_entry(
    session: &CanonicalSession,
    entry: &SessionEntry,
    tool_pairing: &ToolPairingState,
) -> Result<(), SessionError> {
    if session.entries.contains_key(&entry.id) {
        return Err(SessionError::DuplicateEntry {
            entry_id: entry.id.clone(),
        });
    }
    if entry.parent_id.as_ref() == Some(&entry.id) {
        return Err(SessionError::ParentCycle {
            entry_id: entry.id.clone(),
        });
    }
    let Some(run) = session.run_attempts.get(&entry.run_id) else {
        return Err(SessionError::UnknownRun {
            run_id: entry.run_id.clone(),
        });
    };
    if run.status.is_terminal() {
        return Err(SessionError::TerminalRunMutation {
            run_id: entry.run_id.clone(),
        });
    }
    match &entry.payload {
        SessionEntryPayload::AssistantMessage { parts, .. } => {
            let mut entry_tool_calls = BTreeSet::new();
            for part in parts {
                match part {
                    AssistantPart::ToolCall(tool_call) => {
                        if tool_pairing.calls.contains_key(&tool_call.tool_call_id)
                            || !entry_tool_calls.insert(&tool_call.tool_call_id)
                        {
                            return Err(SessionError::DuplicateToolCall {
                                tool_call_id: tool_call.tool_call_id.clone(),
                            });
                        }
                    }
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => {}
                }
            }
        }
        SessionEntryPayload::ToolResult {
            tool_call_id,
            requesting_assistant_entry_id,
            ..
        } => {
            let Some(assistant_entry_id) = tool_pairing.calls.get(tool_call_id) else {
                return Err(SessionError::OrphanToolResult {
                    tool_call_id: tool_call_id.clone(),
                });
            };
            if assistant_entry_id != requesting_assistant_entry_id {
                return Err(SessionError::SplitToolPair {
                    tool_call_id: tool_call_id.clone(),
                    assistant_entry_id: requesting_assistant_entry_id.clone(),
                });
            }
            if !active_path(session)?
                .iter()
                .any(|path_entry| &path_entry.id == assistant_entry_id)
            {
                return Err(SessionError::ToolResultOffActivePath {
                    tool_call_id: tool_call_id.clone(),
                });
            }
            if tool_pairing.settled.contains(tool_call_id) {
                return Err(SessionError::ToolResultAlreadySettled {
                    tool_call_id: tool_call_id.clone(),
                });
            }
        }
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

    if let Some(parent_id) = &entry.parent_id {
        if !session.entries.contains_key(parent_id) {
            return Err(SessionError::MissingParent {
                entry_id: entry.id.clone(),
                parent_id: parent_id.clone(),
            });
        }
    }

    Ok(())
}
