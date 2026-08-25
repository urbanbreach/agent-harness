use crate::ids::{EntryId, SessionId};
use crate::session::{CanonicalSession, SessionEntryPayload, SessionError};
use crate::UnwrapOrAbort;

use super::support::{canonical_entry, canonical_session, compaction_snapshot, user_entry};
use super::{CompactionOwner, CompactionSnapshotError};

#[test]
fn compaction_snapshot_retains_only_latest_active_summary_and_excludes_older_or_off_path_summaries()
{
    let session = canonical_session(
        "session-summary",
        vec![
            user_entry("root", "root"),
            canonical_entry(
                "old-summary",
                Some("root"),
                SessionEntryPayload::CompactionSummary {
                    summary: "old active summary".to_string(),
                    first_kept_entry_id: EntryId::new("root"),
                    tokens_after: Some(80),
                    summary_usage: None,
                    summary_provider_id: Some("mock".to_string()),
                    summary_model_id: Some("old-model".to_string()),
                    preserved_state: None,
                },
            ),
            canonical_entry(
                "active-work",
                Some("old-summary"),
                SessionEntryPayload::UserMessage {
                    text: "active work".to_string(),
                    attachments: Vec::new(),
                },
            ),
            canonical_entry(
                "latest-summary",
                Some("active-work"),
                SessionEntryPayload::CompactionSummary {
                    summary: "latest active summary".to_string(),
                    first_kept_entry_id: EntryId::new("active-work"),
                    tokens_after: Some(40),
                    summary_usage: None,
                    summary_provider_id: Some("mock".to_string()),
                    summary_model_id: Some("current-model".to_string()),
                    preserved_state: None,
                },
            ),
            canonical_entry(
                "kept",
                Some("latest-summary"),
                SessionEntryPayload::UserMessage {
                    text: "kept".to_string(),
                    attachments: Vec::new(),
                },
            ),
            canonical_entry(
                "off-path-summary",
                Some("root"),
                SessionEntryPayload::CompactionSummary {
                    summary: "off-path summary".to_string(),
                    first_kept_entry_id: EntryId::new("root"),
                    tokens_after: Some(60),
                    summary_usage: None,
                    summary_provider_id: Some("mock".to_string()),
                    summary_model_id: Some("branch-model".to_string()),
                    preserved_state: None,
                },
            ),
        ],
        Some("kept"),
    );

    let snapshot = compaction_snapshot(
        &session,
        CompactionOwner::root("root-agent", SessionId::new("session-summary")),
        Vec::new(),
    )
    .unwrap_or_abort();

    assert_eq!(
        (
            snapshot.prior_active_summary.map(|summary| (
                summary.entry_id,
                summary.summary,
                summary.first_kept_entry_id,
            )),
            snapshot
                .entries
                .iter()
                .map(|entry| entry.entry.id.clone())
                .collect::<Vec<_>>(),
            snapshot.active_branch.entry_ids,
        ),
        (
            Some((
                EntryId::new("latest-summary"),
                "latest active summary".to_string(),
                EntryId::new("active-work"),
            )),
            vec![
                EntryId::new("root"),
                EntryId::new("active-work"),
                EntryId::new("kept"),
            ],
            vec![
                EntryId::new("root"),
                EntryId::new("old-summary"),
                EntryId::new("active-work"),
                EntryId::new("latest-summary"),
                EntryId::new("kept"),
            ],
        )
    );
}

#[test]
fn compaction_snapshot_malformed_input_fails_without_side_effects() {
    let valid = canonical_session("session-malformed", vec![user_entry("root", "root")], None);
    let mut serialized = serde_json::to_value(valid).unwrap_or_abort();
    serialized["active_leaf"] = serde_json::json!("missing-entry");
    let malformed: CanonicalSession = serde_json::from_value(serialized).unwrap_or_abort();
    let before = malformed.clone();

    let result = compaction_snapshot(
        &malformed,
        CompactionOwner::root("root-agent", SessionId::new("session-malformed")),
        Vec::new(),
    );

    assert_eq!(
        (result, malformed),
        (
            Err(CompactionSnapshotError::InvalidSession(
                SessionError::ActiveLeafMissing {
                    entry_id: EntryId::new("missing-entry"),
                },
            )),
            before,
        )
    );
}
