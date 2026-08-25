//! Prompt-level rewind planner (append-only events invariant).
//!
//! Rewind restores a *conversation projection* through a cutoff sequence
//! without rewriting `events.jsonl`. `atomic_prompt_rewind` also restores a
//! file snapshot and fails closed if either half fails (no partial success).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::conversation::{
    project_conversation, ConversationProjection, ConversationProjectionError,
};
use crate::digest::digest12;
use crate::event::EventEnvelopeV1;

/// Failures planning a prompt-level rewind.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PromptRewindError {
    #[error("event log is empty; nothing to rewind")]
    EmptyEventLog,
    #[error("cutoff seq {cutoff_seq} is outside event log range 1..={max_seq}")]
    CutoffOutOfRange { cutoff_seq: u64, max_seq: u64 },
    #[error("events are not seq-ordered: event seq {seq} followed {previous_seq}")]
    EventsOutOfOrder { previous_seq: u64, seq: u64 },
    #[error("conversation projection failed: {0}")]
    Projection(String),
}

impl From<ConversationProjectionError> for PromptRewindError {
    fn from(value: ConversationProjectionError) -> Self {
        match value {
            ConversationProjectionError::EventsOutOfOrder { previous_seq, seq } => {
                Self::EventsOutOfOrder { previous_seq, seq }
            }
            malformed @ ConversationProjectionError::ProviderDeltaBeforeStart { .. } => {
                Self::Projection(malformed.to_string())
            }
        }
    }
}

/// Result of a prompt-level rewind plan (read-only over the event log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptRewindPlan {
    pub cutoff_seq: u64,
    pub retained_event_count: usize,
    pub discarded_event_count: usize,
    pub conversation: ConversationProjection,
    /// Always true for this MVP: the planner never rewrites `events.jsonl`.
    pub events_append_only: bool,
}

/// Plan a prompt-level rewind: project conversation through `cutoff_seq`.
///
/// Events with `seq > cutoff_seq` are excluded from the projection. The source
/// event slice is never modified.
pub fn plan_prompt_rewind(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<PromptRewindPlan, PromptRewindError> {
    if events.is_empty() {
        return Err(PromptRewindError::EmptyEventLog);
    }
    ensure_contiguous_from_one(events)?;
    let max_seq = events.last().map(|event| event.seq).unwrap_or(0);
    if cutoff_seq == 0 || cutoff_seq > max_seq {
        return Err(PromptRewindError::CutoffOutOfRange {
            cutoff_seq,
            max_seq,
        });
    }

    let retained: Vec<&EventEnvelopeV1> = events
        .iter()
        .filter(|event| event.seq <= cutoff_seq)
        .collect();
    let retained_owned: Vec<EventEnvelopeV1> = retained.into_iter().cloned().collect();
    let conversation = project_conversation(&retained_owned, &[])?;
    let retained_event_count = retained_owned.len();
    let discarded_event_count = events.len().saturating_sub(retained_event_count);

    Ok(PromptRewindPlan {
        cutoff_seq,
        retained_event_count,
        discarded_event_count,
        conversation,
        events_append_only: true,
    })
}

/// Digest of an on-disk event log for append-only proofs.
pub fn event_log_digest(events_path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(events_path)?;
    Ok(digest12(&bytes))
}

/// Relative path + content for one file in a workspace snapshot restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshotEntry {
    pub path: String,
    pub content: String,
}

/// Result of atomic conversation + file rewind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicPromptRewindResult {
    pub conversation: PromptRewindPlan,
    pub files_restored: usize,
    pub files_unchanged: usize,
    /// Always true: this API never rewrites `events.jsonl`.
    pub events_append_only: bool,
}

/// Failures for atomic prompt rewind (fail-closed; no partial success).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AtomicPromptRewindError {
    #[error("conversation rewind failed: {0}")]
    Conversation(#[from] PromptRewindError),
    #[error("file snapshot restore failed: {0}")]
    FileRestore(String),
    #[error(
        "file snapshot restore failed after conversation plan ({file_error}); \
         workspace rolled back"
    )]
    FileRestoreRolledBack { file_error: String },
    #[error(
        "file snapshot restore failed ({file_error}) and rollback also failed \
         ({rollback_error}); workspace may be inconsistent"
    )]
    FileRestoreRollbackFailed {
        file_error: String,
        rollback_error: String,
    },
}

/// Combine conversation projection rewind with file snapshot restore.
///
/// Fail-closed contract:
/// 1. Plan conversation first; on failure return without touching files.
/// 2. Apply file restores with pre-backup; on any failure roll back all
///    file changes and return error (conversation plan is discarded).
/// 3. Events stay append-only; this function never mutates the event log.
pub fn atomic_prompt_rewind(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
    workspace_root: &Path,
    file_snapshot: &[FileSnapshotEntry],
) -> Result<AtomicPromptRewindResult, AtomicPromptRewindError> {
    let conversation = plan_prompt_rewind(events, cutoff_seq)?;
    if file_snapshot.is_empty() {
        return Ok(AtomicPromptRewindResult {
            conversation,
            files_restored: 0,
            files_unchanged: 0,
            events_append_only: true,
        });
    }

    let mut backups: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    let mut files_restored = 0usize;
    let mut files_unchanged = 0usize;

    for entry in file_snapshot {
        let relative = normalize_relative_path(&entry.path);
        if relative.is_empty() || relative.contains("..") {
            let err = format!("invalid snapshot path `{relative}`");
            return Err(rollback_or_escalate(workspace_root, &backups, err));
        }
        let target = workspace_root.join(&relative);
        let previous = if target.is_file() {
            match fs::read_to_string(&target) {
                Ok(content) => Some(content),
                Err(err) => {
                    return Err(rollback_or_escalate(
                        workspace_root,
                        &backups,
                        format!("read {}: {err}", target.display()),
                    ));
                }
            }
        } else {
            None
        };

        if previous.as_deref() == Some(entry.content.as_str()) {
            files_unchanged = files_unchanged.saturating_add(1);
            continue;
        }

        if let Some(parent) = target.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                return Err(rollback_or_escalate(
                    workspace_root,
                    &backups,
                    format!("create parent for {}: {err}", target.display()),
                ));
            }
        }
        if let Err(err) = fs::write(&target, &entry.content) {
            return Err(rollback_or_escalate(
                workspace_root,
                &backups,
                format!("write {}: {err}", target.display()),
            ));
        }
        backups.insert(target, previous);
        files_restored = files_restored.saturating_add(1);
    }

    Ok(AtomicPromptRewindResult {
        conversation,
        files_restored,
        files_unchanged,
        events_append_only: true,
    })
}

fn rollback_or_escalate(
    workspace_root: &Path,
    backups: &BTreeMap<PathBuf, Option<String>>,
    file_error: String,
) -> AtomicPromptRewindError {
    match rollback_file_changes(backups) {
        Ok(()) => {
            let _ = workspace_root;
            AtomicPromptRewindError::FileRestoreRolledBack { file_error }
        }
        Err(rollback_error) => AtomicPromptRewindError::FileRestoreRollbackFailed {
            file_error,
            rollback_error,
        },
    }
}

fn rollback_file_changes(backups: &BTreeMap<PathBuf, Option<String>>) -> Result<(), String> {
    for (path, previous) in backups.iter().rev() {
        match previous {
            Some(content) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|err| format!("rollback create {}: {err}", path.display()))?;
                }
                fs::write(path, content)
                    .map_err(|err| format!("rollback write {}: {err}", path.display()))?;
            }
            None => {
                if path.is_file() {
                    fs::remove_file(path)
                        .map_err(|err| format!("rollback remove {}: {err}", path.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn normalize_relative_path(path: &str) -> String {
    path.trim().trim_start_matches("./").replace('\\', "/")
}

fn ensure_contiguous_from_one(events: &[EventEnvelopeV1]) -> Result<(), PromptRewindError> {
    let mut previous = 0_u64;
    for (expected, event) in (1_u64..).zip(events.iter()) {
        if event.seq != expected {
            return Err(PromptRewindError::EventsOutOfOrder {
                previous_seq: previous,
                seq: event.seq,
            });
        }
        previous = event.seq;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::ConversationMessage;
    use crate::event::{ActorKind, EventActor, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION};
    use crate::UnwrapOrAbort;
    use std::io::Write;

    fn worker() -> EventActor {
        EventActor::new(ActorKind::Worker, Some("agent_1".to_string()))
    }

    fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:020}"),
            seq,
            run_id: "run_rewind".into(),
            mono_ms: seq,
            ts: None,
            actor: worker(),
            correlation_id: Some(format!("req_{seq}")),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn user_message(seq: u64, text: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: format!("req_{seq}").into(),
                text: text.to_string(),
            }),
        )
    }

    #[test]
    fn plan_prompt_rewind_restores_conversation_through_cutoff() {
        // arrange
        // act
        // assert
        let events = vec![
            user_message(1, "first"),
            user_message(2, "second"),
            user_message(3, "third"),
        ];

        let plan = plan_prompt_rewind(&events, 2).unwrap_or_abort();
        assert_eq!(plan.cutoff_seq, 2);
        assert_eq!(plan.retained_event_count, 2);
        assert_eq!(plan.discarded_event_count, 1);
        assert!(plan.events_append_only);

        let texts: Vec<&str> = plan
            .conversation
            .messages
            .iter()
            .filter_map(|message| match message {
                ConversationMessage::User(user) => Some(user.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn plan_prompt_rewind_fails_recoverably_for_bad_cutoff() {
        // arrange
        // act
        // assert
        let events = vec![user_message(1, "only")];
        let err = plan_prompt_rewind(&events, 9).expect_err("out of range");
        assert!(matches!(
            err,
            PromptRewindError::CutoffOutOfRange {
                cutoff_seq: 9,
                max_seq: 1
            }
        ));
        let empty = plan_prompt_rewind(&[], 1).expect_err("empty");
        assert!(matches!(empty, PromptRewindError::EmptyEventLog));
    }

    #[test]
    fn plan_prompt_rewind_does_not_rewrite_events_jsonl() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let events_path = temp.path().join("events.jsonl");
        let events = vec![user_message(1, "a"), user_message(2, "b")];
        {
            let mut file = fs::File::create(&events_path).unwrap_or_abort();
            for event in &events {
                let line = serde_json::to_string(event).unwrap_or_abort();
                writeln!(file, "{line}").unwrap_or_abort();
            }
            file.sync_all().unwrap_or_abort();
        }

        let before = event_log_digest(&events_path).unwrap_or_abort();
        let before_bytes = fs::read(&events_path).unwrap_or_abort();

        let plan = plan_prompt_rewind(&events, 1).unwrap_or_abort();
        assert!(plan.events_append_only);

        let after = event_log_digest(&events_path).unwrap_or_abort();
        let after_bytes = fs::read(&events_path).unwrap_or_abort();
        assert_eq!(before, after);
        assert_eq!(before_bytes, after_bytes);
        assert_eq!(String::from_utf8_lossy(&after_bytes).lines().count(), 2);
    }

    #[test]
    fn event_log_digest_is_content_addressed() {
        // arrange — one body under two paths, then an append on one
        let dir = tempfile::tempdir().expect("tempdir");
        let events = vec![user_message(1, "first"), user_message(2, "second")];
        let mut body = String::new();
        for event in &events {
            body.push_str(&serde_json::to_string(event).expect("serialize"));
            body.push('\n');
        }
        let path_a = dir.path().join("a.jsonl");
        let path_b = dir.path().join("b.jsonl");
        fs::write(&path_a, &body).expect("write a");
        fs::write(&path_b, &body).expect("write b");

        // act
        let digest_a = event_log_digest(&path_a).expect("digest a");
        let digest_b = event_log_digest(&path_b).expect("digest b");
        fs::write(&path_a, format!("{body}{{\"seq\":3}}\n")).expect("append");
        let digest_after_append = event_log_digest(&path_a).expect("digest after");

        // assert — identical bytes share a digest; any append changes it
        assert_eq!(digest_a, digest_b);
        assert_ne!(digest_a, digest_after_append);
    }

    #[test]
    fn atomic_prompt_rewind_restores_conversation_and_files() {
        // arrange
        // act
        // assert
        // Given: event log + workspace file
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "after").unwrap_or_abort();
        let events = vec![
            user_message(1, "first"),
            user_message(2, "second"),
            user_message(3, "third"),
        ];
        let snapshot = [FileSnapshotEntry {
            path: "notes.txt".into(),
            content: "before".into(),
        }];

        // When
        let result = atomic_prompt_rewind(&events, 2, &workspace, &snapshot).unwrap_or_abort();

        // Then
        assert!(result.events_append_only);
        assert_eq!(result.conversation.retained_event_count, 2);
        assert_eq!(result.files_restored, 1);
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "before");
    }

    #[test]
    fn atomic_prompt_rewind_never_rewrites_events_jsonl() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let events_path = workspace.join("events.jsonl");
        let events = vec![user_message(1, "first"), user_message(2, "second")];
        {
            let mut file = fs::File::create(&events_path).unwrap_or_abort();
            for event in &events {
                let line = serde_json::to_string(event).unwrap_or_abort();
                writeln!(file, "{line}").unwrap_or_abort();
            }
            file.sync_all().unwrap_or_abort();
        }
        let before_digest = event_log_digest(&events_path).unwrap_or_abort();
        let snapshot = [FileSnapshotEntry {
            path: "notes.txt".into(),
            content: "rewound".into(),
        }];

        // act
        let result = atomic_prompt_rewind(&events, 1, &workspace, &snapshot).unwrap_or_abort();

        // assert — file restore ran while the on-disk event log stayed append-only
        assert!(result.events_append_only);
        assert_eq!(result.files_restored, 1);
        assert_eq!(
            event_log_digest(&events_path).unwrap_or_abort(),
            before_digest
        );
    }

    #[test]
    fn atomic_prompt_rewind_fails_closed_on_conversation_error() {
        // arrange
        // act
        // assert
        // Given: bad cutoff + file that must stay untouched
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "keep").unwrap_or_abort();
        let events = vec![user_message(1, "only")];
        let snapshot = [FileSnapshotEntry {
            path: "notes.txt".into(),
            content: "changed".into(),
        }];

        // When
        let err = atomic_prompt_rewind(&events, 9, &workspace, &snapshot).expect_err("bad cutoff");

        // Then: conversation error, file untouched
        assert!(matches!(
            err,
            AtomicPromptRewindError::Conversation(PromptRewindError::CutoffOutOfRange { .. })
        ));
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "keep");
    }

    #[test]
    fn atomic_prompt_rewind_rolls_back_files_on_invalid_path() {
        // arrange
        // act
        // assert
        // Given: valid first file restore then invalid path
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("ok.txt");
        fs::write(&target, "original").unwrap_or_abort();
        let events = vec![user_message(1, "a")];
        let snapshot = [
            FileSnapshotEntry {
                path: "ok.txt".into(),
                content: "mutated".into(),
            },
            FileSnapshotEntry {
                path: "../escape.txt".into(),
                content: "nope".into(),
            },
        ];

        // When
        let err =
            atomic_prompt_rewind(&events, 1, &workspace, &snapshot).expect_err("invalid path");

        // Then: fail-closed rollback restores original content
        assert!(matches!(
            err,
            AtomicPromptRewindError::FileRestoreRolledBack { .. }
        ));
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "original");
    }
}
