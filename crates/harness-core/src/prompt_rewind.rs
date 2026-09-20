//! Prompt-level rewind planner (append-only events invariant).
//!
//! Rewind restores a *conversation projection* through a cutoff sequence
//! without rewriting `events.jsonl`. `atomic_prompt_rewind` also restores a
//! file snapshot and fails closed if either half fails (no partial success).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::conversation::ConversationProjection;
use crate::digest::digest12;
use crate::event::EventEnvelopeV1;
use crate::path_selector::{recheck_restore_target, resolve_restore_target};
use crate::session::CanonicalSessionProjection;

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
    let conversation = CanonicalSessionProjection::from_event_history(&retained_owned)
        .map_err(|error| PromptRewindError::Projection(error.to_string()))?
        .conversation;
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

struct FileBackup {
    target: PathBuf,
    previous: Option<(Vec<u8>, fs::Permissions)>,
}

/// Combine conversation projection rewind with file snapshot restore.
///
/// Fail-closed contract:
/// 1. Plan conversation first; on failure return without touching files.
/// 2. Validate every target, then apply restores with pre-backup; on failure roll
///    back all file changes and return error (conversation plan is discarded).
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

    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|err| AtomicPromptRewindError::FileRestore(format!("resolve workspace: {err}")))?;
    let workspace_root = workspace_root.as_path();
    let mut targets = BTreeSet::new();
    let backups = file_snapshot
        .iter()
        .map(|entry| {
            let target = resolve_restore_target(workspace_root, Path::new(&entry.path))
                .map_err(|err| AtomicPromptRewindError::FileRestore(err.to_string()))?;
            if !targets.insert(target.clone()) {
                return Err(AtomicPromptRewindError::FileRestore(format!(
                    "duplicate snapshot target: {}",
                    target.display()
                )));
            }
            recheck_restore_target(workspace_root, &target)
                .map_err(|err| AtomicPromptRewindError::FileRestore(err.to_string()))?;
            let previous = match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.is_file() => {
                    let content = fs::read(&target).map_err(|err| {
                        AtomicPromptRewindError::FileRestore(format!(
                            "read {}: {err}",
                            target.display()
                        ))
                    })?;
                    Some((content, metadata.permissions()))
                }
                Ok(_) => {
                    return Err(AtomicPromptRewindError::FileRestore(format!(
                        "unsupported snapshot target: {}",
                        target.display()
                    )));
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(AtomicPromptRewindError::FileRestore(format!(
                        "inspect {}: {err}",
                        target.display()
                    )));
                }
            };
            Ok(FileBackup { target, previous })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = Vec::new();
    let mut files_unchanged = 0usize;

    for (entry, backup) in file_snapshot.iter().zip(&backups) {
        let target = &backup.target;
        if backup
            .previous
            .as_ref()
            .map(|(content, _)| content.as_slice())
            == Some(entry.content.as_bytes())
        {
            files_unchanged = files_unchanged.saturating_add(1);
            continue;
        }

        replace_file_atomically(
            workspace_root,
            target,
            entry.content.as_bytes(),
            backup.previous.as_ref().map(|(_, permissions)| permissions),
        )
        .map_err(|err| rollback_or_escalate(workspace_root, &changed, err))?;
        // Originals were captured in preflight; only completed renames need undoing.
        changed.push(backup);
    }

    Ok(AtomicPromptRewindResult {
        conversation,
        files_restored: changed.len(),
        files_unchanged,
        events_append_only: true,
    })
}

fn replace_file_atomically(
    workspace_root: &Path,
    target: &Path,
    content: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<(), String> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create parent for {}: {err}", target.display()))?;
    }
    recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
    let (temp_path, mut file) = loop {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = target.with_file_name(format!(
            ".harness-rewind-{}-{counter}.tmp",
            std::process::id()
        ));
        if temp_path == target {
            continue;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if permissions.is_some() {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp_path) {
            Ok(file) => break (temp_path, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("create temporary {}: {err}", temp_path.display())),
        }
    };
    let result = (|| {
        #[cfg(test)]
        tests::fail_temporary_write(&mut file, content)
            .map_err(|err| format!("write {}: {err}", target.display()))?;
        file.write_all(content)
            .map_err(|err| format!("write {}: {err}", target.display()))?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions.clone())
                .map_err(|err| format!("set permissions for {}: {err}", target.display()))?;
        }
        file.sync_all()
            .map_err(|err| format!("sync {}: {err}", target.display()))?;
        drop(file);
        recheck_restore_target(workspace_root, target).map_err(|err| err.to_string())?;
        recheck_restore_target(workspace_root, &temp_path).map_err(|err| err.to_string())?;
        fs::rename(&temp_path, target).map_err(|err| format!("replace {}: {err}", target.display()))
    })();
    if let Err(error) = result {
        let cleanup = recheck_restore_target(workspace_root, &temp_path)
            .map_err(|err| err.to_string())
            .and_then(|()| match fs::remove_file(&temp_path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.to_string()),
            });
        return Err(match cleanup {
            Ok(()) => error,
            Err(cleanup_error) => format!(
                "{error}; remove temporary {}: {cleanup_error}",
                temp_path.display()
            ),
        });
    }
    Ok(())
}

fn rollback_or_escalate(
    workspace_root: &Path,
    backups: &[&FileBackup],
    file_error: String,
) -> AtomicPromptRewindError {
    match rollback_file_changes(workspace_root, backups) {
        Ok(()) => AtomicPromptRewindError::FileRestoreRolledBack { file_error },
        Err(rollback_error) => AtomicPromptRewindError::FileRestoreRollbackFailed {
            file_error,
            rollback_error,
        },
    }
}

fn rollback_file_changes(workspace_root: &Path, backups: &[&FileBackup]) -> Result<(), String> {
    let mut errors = Vec::new();
    for backup in backups.iter().rev() {
        let path = &backup.target;
        let result = match &backup.previous {
            Some((content, permissions)) => {
                replace_file_atomically(workspace_root, path, content, Some(permissions))
            }
            None => recheck_restore_target(workspace_root, path)
                .map_err(|err| err.to_string())
                .and_then(|()| match fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(err) => Err(format!("rollback remove {}: {err}", path.display())),
                }),
        };
        if let Err(err) = result {
            errors.push(err);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
    use std::cell::RefCell;
    use std::collections::VecDeque;

    thread_local! {
        static FAIL_WRITES: RefCell<VecDeque<bool>> = const { RefCell::new(VecDeque::new()) };
    }

    pub(super) fn fail_temporary_write(file: &mut fs::File, content: &[u8]) -> io::Result<()> {
        if FAIL_WRITES.with(|failures| failures.borrow_mut().pop_front()) == Some(true) {
            file.write_all(&content[..content.len() / 2])?;
            return Err(io::Error::other("injected partial temporary write"));
        }
        Ok(())
    }

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
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        // Given: event log + workspace file
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "after").unwrap_or_abort();
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap_or_abort();
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
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&target).unwrap_or_abort().permissions().mode() & 0o7777,
            0o751
        );
        let result = atomic_prompt_rewind(&events, 2, &workspace, &snapshot).unwrap_or_abort();
        assert_eq!((result.files_restored, result.files_unchanged), (0, 1));
    }

    #[test]
    fn atomic_prompt_rewind_rolls_back_failed_writes() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        // Fail both the first temporary write and later writes after completed renames.
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        let events = vec![user_message(1, "first"), user_message(2, "second")];
        let original = b"original\0\xff bytes";
        fs::write(&target, original).unwrap_or_abort();
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap_or_abort();
        let other = workspace.join("other.txt");
        fs::write(&other, "other-original").unwrap_or_abort();
        #[cfg(unix)]
        fs::set_permissions(&other, fs::Permissions::from_mode(0o440)).unwrap_or_abort();
        let events_path = workspace.join("events.jsonl");
        let journal = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_abort())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&events_path, &journal).unwrap_or_abort();
        let unowned = workspace.join(".harness-rewind-unowned.tmp");
        fs::write(&unowned, "unowned").unwrap_or_abort();
        let entries = || {
            fs::read_dir(&workspace)
                .unwrap_or_abort()
                .map(|entry| entry.unwrap_or_abort().file_name())
                .collect::<BTreeSet<_>>()
        };
        let entries_before = entries();
        let snapshot = [
            FileSnapshotEntry {
                path: "notes.txt".into(),
                content: "changed-again".into(),
            },
            FileSnapshotEntry {
                path: "new.txt".into(),
                content: "created".into(),
            },
            FileSnapshotEntry {
                path: "other.txt".into(),
                content: "other-restored".into(),
            },
            FileSnapshotEntry {
                path: "last.txt".into(),
                content: "last-created".into(),
            },
        ];
        for fail_at in 0..snapshot.len() {
            FAIL_WRITES.with(|failures| {
                *failures.borrow_mut() = (0..=fail_at).map(|index| index == fail_at).collect();
            });
            let result = atomic_prompt_rewind(&events, 2, &workspace, &snapshot);
            assert!(
                matches!(result, Err(AtomicPromptRewindError::FileRestoreRolledBack { ref file_error })
                    if file_error.contains("injected partial temporary write")),
                "failure at {fail_at}: {result:?}"
            );
            assert_eq!(fs::read(&target).unwrap_or_abort(), original);
            assert_eq!(fs::read(&other).unwrap_or_abort(), b"other-original");
            #[cfg(unix)]
            for (path, mode) in [(&target, 0o751), (&other, 0o440)] {
                assert_eq!(
                    fs::metadata(path).unwrap_or_abort().permissions().mode() & 0o7777,
                    mode
                );
            }
            assert_eq!(entries(), entries_before, "failure at {fail_at}");
            assert_eq!(fs::read(&events_path).unwrap_or_abort(), journal.as_bytes());
            assert_eq!(fs::read(&unowned).unwrap_or_abort(), b"unowned");
        }

        // A failed rollback leaves that file intact and still undoes the other changes.
        FAIL_WRITES.with(|failures| {
            *failures.borrow_mut() = [false, false, false, true, true].into();
        });
        let error =
            atomic_prompt_rewind(&events, 2, &workspace, &snapshot).expect_err("write failure");
        assert!(matches!(
            error,
            AtomicPromptRewindError::FileRestoreRollbackFailed { file_error, rollback_error }
                if file_error.contains("last.txt")
                    && file_error.contains("injected partial temporary write")
                    && rollback_error.contains("other.txt")
                    && rollback_error.contains("injected partial temporary write")
        ));
        assert_eq!(fs::read(&target).unwrap_or_abort(), original);
        assert_eq!(fs::read(&other).unwrap_or_abort(), b"other-restored");
        assert_eq!(entries(), entries_before);
        assert_eq!(fs::read(&events_path).unwrap_or_abort(), journal.as_bytes());
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
    fn atomic_prompt_rewind_preflights_all_paths_before_mutation() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("ok.txt");
        fs::write(&target, "original").unwrap_or_abort();
        fs::create_dir(workspace.join("directory")).unwrap_or_abort();
        let events = vec![user_message(1, "a")];
        let events_path = workspace.join("events.jsonl");
        let journal = format!("{}\n", serde_json::to_string(&events[0]).unwrap_or_abort());
        fs::write(&events_path, &journal).unwrap_or_abort();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap_or_abort();
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, "external-original").unwrap_or_abort();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&outside, workspace.join("link")).unwrap_or_abort();
            symlink(outside.join("absent"), workspace.join("dangling")).unwrap_or_abort();
            symlink(&target, workspace.join("alias.txt")).unwrap_or_abort();
            symlink(&workspace, workspace.join("inside")).unwrap_or_abort();
        }
        #[cfg(unix)]
        let _socket =
            std::os::unix::net::UnixListener::bind(workspace.join("socket")).unwrap_or_abort();
        let absolute_inside = target.to_string_lossy();
        let absolute_outside = sentinel.to_string_lossy();
        let cases = [
            ("ok.txt", false),
            ("./ok.txt", false),
            ("new/leaf.txt", false),
            ("directory", false),
            #[cfg(unix)]
            ("alias.txt", false),
            #[cfg(unix)]
            ("inside/new/leaf.txt", false),
            #[cfg(unix)]
            ("socket", false),
            ("", false),
            (".", false),
            ("../outside/sentinel.txt", false),
            ("./../outside/sentinel.txt", false),
            ("nested/../ok.txt", false),
            (absolute_inside.as_ref(), false),
            (absolute_outside.as_ref(), false),
            ("ok.txt/child", false),
            #[cfg(unix)]
            ("link/missing.txt", false),
            #[cfg(unix)]
            ("link/sentinel.txt", false),
            #[cfg(unix)]
            ("dangling", false),
            #[cfg(unix)]
            ("dangling/child", false),
            ("notes..txt", true),
        ];

        for (path, valid) in cases {
            let snapshot = [
                FileSnapshotEntry {
                    path: "ok.txt".into(),
                    content: "mutated".into(),
                },
                FileSnapshotEntry {
                    path: "new/leaf.txt".into(),
                    content: "new".into(),
                },
                FileSnapshotEntry {
                    path: path.into(),
                    content: "restored".into(),
                },
            ];
            let result = atomic_prompt_rewind(&events, 1, &workspace, &snapshot);
            if valid {
                assert_eq!(result.unwrap_or_abort().files_restored, 3);
                assert_eq!(
                    fs::read_to_string(workspace.join(path)).unwrap_or_abort(),
                    "restored"
                );
            } else {
                assert!(
                    matches!(result, Err(AtomicPromptRewindError::FileRestore(_))),
                    "{path}: {result:?}"
                );
                assert_eq!(
                    fs::read_to_string(&target).unwrap_or_abort(),
                    "original",
                    "{path}"
                );
                assert!(
                    !workspace.join("new").exists(),
                    "preflight created a directory: {path}"
                );
            }
            assert_eq!(fs::read(&sentinel).unwrap_or_abort(), b"external-original");
            assert!(!outside.join("missing.txt").exists());
            assert!(!outside.join("absent").exists());
            assert_eq!(fs::read(&events_path).unwrap_or_abort(), journal.as_bytes());
        }
    }
}
