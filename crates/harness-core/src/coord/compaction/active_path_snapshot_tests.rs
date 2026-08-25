use crate::attachment_transport::AttachmentMetadata;
use crate::ids::{EntryId, SessionId, TurnId};
use crate::session::{AssistantPart, SessionEntryPayload};
use crate::UnwrapOrAbort;

use super::{
    build_active_path_compaction_snapshot, ActivePathCompactionSnapshot,
    ActivePathCompactionSnapshotInput, CompactionOwner, CompactionPlanBoundary,
    CompactionSnapshotError, CurrentCompactionModel, LegacySourceSequences,
    PendingCompactionPrompt,
};

#[path = "active_path_snapshot_tests/summary_and_error_tests.rs"]
mod summary_and_error_tests;
#[path = "active_path_snapshot_tests/support.rs"]
mod support;

use support::{
    branched_session, canonical_entry, canonical_session, compaction_snapshot, user_entry,
};

#[test]
fn compaction_snapshot_selects_only_the_active_branch_and_carries_request_identity() {
    let session = branched_session();
    let sequences = LegacySourceSequences::new([
        (EntryId::new("left"), 9),
        (EntryId::new("root"), 4),
        (EntryId::new("right"), 7),
    ])
    .unwrap_or_abort();
    let pending_prompt = PendingCompactionPrompt {
        turn_id: TurnId::new("turn-pending"),
        text: "pending".to_string(),
        attachments: Vec::new(),
    };

    let snapshot = build_active_path_compaction_snapshot(ActivePathCompactionSnapshotInput {
        session: &session,
        owner: CompactionOwner::root("root-agent", SessionId::new("session-branch")),
        legacy_source_sequences: &sequences,
        pending_prompt: Some(pending_prompt.clone()),
        current_model: CurrentCompactionModel::new("mock", "current-model"),
    })
    .unwrap_or_abort();

    assert_eq!(
        (
            snapshot.active_branch.entry_ids,
            snapshot
                .entries
                .iter()
                .map(|entry| entry.entry.id.clone())
                .collect::<Vec<_>>(),
            snapshot.owner.agent_id,
            snapshot.pending_prompt,
            snapshot.current_model,
        ),
        (
            vec![EntryId::new("root"), EntryId::new("left")],
            vec![EntryId::new("root"), EntryId::new("left")],
            "root-agent".to_string(),
            Some(pending_prompt),
            CurrentCompactionModel::new("mock", "current-model"),
        )
    );
}

#[test]
fn compaction_snapshot_keeps_root_and_child_owners_isolated() {
    let root = canonical_session(
        "root-session",
        vec![user_entry("root-entry", "root content")],
        None,
    );
    let child = canonical_session(
        "child-session",
        vec![user_entry("child-entry", "child content")],
        None,
    );

    let root_snapshot = compaction_snapshot(
        &root,
        CompactionOwner::root("root-agent", SessionId::new("root-session")),
        Vec::new(),
    )
    .unwrap_or_abort();
    let child_snapshot = compaction_snapshot(
        &child,
        CompactionOwner::child(
            "child-agent",
            SessionId::new("child-session"),
            SessionId::new("root-session"),
        ),
        Vec::new(),
    )
    .unwrap_or_abort();

    assert_eq!(
        (
            root_snapshot.owner,
            root_snapshot.entries[0].entry.id.clone(),
            child_snapshot.owner,
            child_snapshot.entries[0].entry.id.clone(),
        ),
        (
            CompactionOwner::root("root-agent", SessionId::new("root-session")),
            EntryId::new("root-entry"),
            CompactionOwner::child(
                "child-agent",
                SessionId::new("child-session"),
                SessionId::new("root-session"),
            ),
            EntryId::new("child-entry"),
        )
    );
}

#[test]
fn compaction_snapshot_rejects_cross_session_owner() {
    let session = canonical_session(
        "root-session",
        vec![user_entry("root-entry", "root content")],
        None,
    );

    let result = compaction_snapshot(
        &session,
        CompactionOwner::child(
            "child-agent",
            SessionId::new("child-session"),
            SessionId::new("root-session"),
        ),
        Vec::new(),
    );

    assert_eq!(
        result,
        Err(CompactionSnapshotError::OwnerSessionMismatch {
            expected: SessionId::new("root-session"),
            actual: SessionId::new("child-session"),
        })
    );
}

#[test]
fn compaction_plan_maps_entry_identity_to_legacy_sequence_deterministically() {
    let session = branched_session();
    let snapshot = compaction_snapshot(
        &session,
        CompactionOwner::root("root-agent", SessionId::new("session-branch")),
        vec![
            (EntryId::new("left"), 9),
            (EntryId::new("root"), 4),
            (EntryId::new("right"), 7),
        ],
    )
    .unwrap_or_abort();

    let plan = snapshot.into_plan(&EntryId::new("left")).unwrap_or_abort();

    assert_eq!(
        (
            plan.snapshot
                .entries
                .iter()
                .map(|entry| (entry.entry.id.clone(), entry.legacy_source_sequence))
                .collect::<Vec<_>>(),
            plan.first_kept,
        ),
        (
            vec![
                (EntryId::new("root"), Some(4)),
                (EntryId::new("left"), Some(9)),
            ],
            CompactionPlanBoundary {
                entry_id: EntryId::new("left"),
                turn_id: Some(TurnId::new("turn-left")),
                legacy_source_sequence: Some(9),
            },
        )
    );
}

#[test]
fn compaction_snapshot_preserves_message_parts_and_attachments() {
    let attachment = AttachmentMetadata::from_bytes(
        "diagram-工具-😀",
        "image/png",
        None,
        b"attachment-bytes",
        None,
    );
    let session = canonical_session(
        "session-content",
        vec![
            canonical_entry(
                "user",
                None,
                SessionEntryPayload::UserMessage {
                    text: "日本語 😀".to_string(),
                    attachments: vec![attachment.clone()],
                },
            ),
            canonical_entry(
                "assistant",
                Some("user"),
                SessionEntryPayload::AssistantMessage {
                    parts: vec![
                        AssistantPart::Reasoning {
                            text: "理由".to_string(),
                        },
                        AssistantPart::Text {
                            text: "done 😀".to_string(),
                        },
                    ],
                    provenance: None,
                },
            ),
        ],
        None,
    );

    let snapshot = compaction_snapshot(
        &session,
        CompactionOwner::root("root-agent", SessionId::new("session-content")),
        Vec::new(),
    )
    .unwrap_or_abort();

    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.entry.payload.clone())
            .collect::<Vec<_>>(),
        vec![
            SessionEntryPayload::UserMessage {
                text: "日本語 😀".to_string(),
                attachments: vec![attachment],
            },
            SessionEntryPayload::AssistantMessage {
                parts: vec![
                    AssistantPart::Reasoning {
                        text: "理由".to_string(),
                    },
                    AssistantPart::Text {
                        text: "done 😀".to_string(),
                    },
                ],
                provenance: None,
            },
        ]
    );
}
