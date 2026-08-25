use crate::ids::{EntryId, RunId, SessionId, TurnId};
use crate::session::reducer::replay as replay_session;
use crate::session::{
    CanonicalRecord, CanonicalRecordKind, CanonicalSession, RecordSequence, RunAttempt, RunStatus,
    SessionEntry, SessionEntryPayload,
};
use crate::UnwrapOrAbort;

use super::{
    build_active_path_compaction_snapshot, ActivePathCompactionSnapshot,
    ActivePathCompactionSnapshotInput, CompactionOwner, CompactionSnapshotError,
    CurrentCompactionModel, LegacySourceSequences,
};

pub(super) fn canonical_entry(
    id: &str,
    parent_id: Option<&str>,
    payload: SessionEntryPayload,
) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent_id.map(EntryId::new),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-compaction-snapshot"),
        payload,
    }
}

pub(super) fn canonical_session(
    session_id: &str,
    entries: Vec<SessionEntry>,
    selected_leaf: Option<&str>,
) -> CanonicalSession {
    let session_id = SessionId::new(session_id);
    let mut records = vec![CanonicalRecord {
        session_id: session_id.clone(),
        sequence: RecordSequence::new(1),
        kind: CanonicalRecordKind::RunStarted {
            attempt: RunAttempt {
                run_id: RunId::new("run-compaction-snapshot"),
                status: RunStatus::Active,
                legacy_run_id: None,
            },
        },
    }];
    records.extend(
        entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| CanonicalRecord {
                session_id: session_id.clone(),
                sequence: RecordSequence::new(index as u64 + 2),
                kind: CanonicalRecordKind::EntryCommitted { entry },
            }),
    );
    if let Some(entry_id) = selected_leaf {
        records.push(CanonicalRecord {
            session_id: session_id.clone(),
            sequence: RecordSequence::new(records.len() as u64 + 1),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new(entry_id),
            },
        });
    }
    replay_session(session_id, &records).unwrap_or_abort()
}

pub(super) fn compaction_snapshot(
    session: &CanonicalSession,
    owner: CompactionOwner,
    sequences: Vec<(EntryId, u64)>,
) -> Result<ActivePathCompactionSnapshot, CompactionSnapshotError> {
    let legacy_source_sequences = LegacySourceSequences::new(sequences).unwrap_or_abort();
    build_active_path_compaction_snapshot(ActivePathCompactionSnapshotInput {
        session,
        owner,
        legacy_source_sequences: &legacy_source_sequences,
        pending_prompt: None,
        current_model: CurrentCompactionModel::new("mock", "model-1"),
    })
}

pub(super) fn branched_session() -> CanonicalSession {
    canonical_session(
        "session-branch",
        vec![
            user_entry("root", "root"),
            canonical_entry(
                "left",
                Some("root"),
                SessionEntryPayload::UserMessage {
                    text: "left".to_string(),
                    attachments: Vec::new(),
                },
            ),
            canonical_entry(
                "right",
                Some("root"),
                SessionEntryPayload::UserMessage {
                    text: "right".to_string(),
                    attachments: Vec::new(),
                },
            ),
        ],
        Some("left"),
    )
}

pub(super) fn user_entry(id: &str, text: &str) -> SessionEntry {
    canonical_entry(
        id,
        None,
        SessionEntryPayload::UserMessage {
            text: text.to_string(),
            attachments: Vec::new(),
        },
    )
}
