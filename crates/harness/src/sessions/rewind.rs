//! Product CLI for atomic prompt rewind (`harness sessions rewind`).
//!
//! Surfaces [`harness_core::prompt_rewind::atomic_prompt_rewind`]: project the
//! stored session conversation through a cutoff sequence and restore the supplied
//! file snapshot into the workspace, fail-closed with rollback on any half failure.
//! Events are read replay-derived and `events.jsonl` is never rewritten (the
//! atomic backend guarantees `events_append_only`).

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use harness_core::prompt_rewind::{atomic_prompt_rewind, plan_prompt_rewind, FileSnapshotEntry};
use serde::Serialize;

use crate::cli_io::load_events_from_run_dir;
use crate::recovery::resolve_session_run_dir;

use super::{session_dir, write_json_output};

#[derive(Debug, Args, Clone)]
pub struct RewindSessionCommand {
    /// Run id or session directory path whose stored events are rewound.
    #[arg(value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    /// Event sequence cutoff; events with seq > cutoff are excluded from the projection.
    #[arg(long, value_name = "SEQ")]
    pub cutoff: u64,

    /// Workspace root the file snapshot is restored into (required without --dry-run).
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// JSON file holding the file snapshot: an array of `{ "path", "content" }`.
    /// Required without --dry-run; ignored with --dry-run.
    #[arg(long, value_name = "PATH")]
    pub snapshot: Option<PathBuf>,

    /// Preview the projection only: report retained/discarded events and conversation
    /// message count without restoring files or requiring a snapshot/workspace.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RewindReport {
    harness_operation: &'static str,
    run_dir: PathBuf,
    workspace: PathBuf,
    cutoff_seq: u64,
    retained_event_count: usize,
    discarded_event_count: usize,
    conversation_message_count: usize,
    files_restored: usize,
    files_unchanged: usize,
    events_append_only: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DryRunRewindReport {
    harness_operation: &'static str,
    run_dir: PathBuf,
    cutoff_seq: u64,
    retained_event_count: usize,
    discarded_event_count: usize,
    conversation_message_count: usize,
    events_append_only: bool,
}

pub(super) fn rewind_session(
    command: RewindSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            let _ = writeln!(stderr, "session rewind failed: {err}");
            return 1;
        }
    };
    let events = match load_events_from_run_dir(&run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "session rewind failed: failed to read session events from {}: {err}",
                run_dir.display()
            );
            return 1;
        }
    };

    if command.dry_run {
        return match plan_prompt_rewind(&events, command.cutoff) {
            Ok(plan) => {
                let report = DryRunRewindReport {
                    harness_operation: "rewind-dry-run",
                    run_dir,
                    cutoff_seq: plan.cutoff_seq,
                    retained_event_count: plan.retained_event_count,
                    discarded_event_count: plan.discarded_event_count,
                    conversation_message_count: plan.conversation.messages.len(),
                    events_append_only: plan.events_append_only,
                };
                emit_dry_run_report(&report, command.json, stdout, stderr)
            }
            Err(err) => {
                let _ = writeln!(stderr, "session rewind failed: {err}");
                1
            }
        };
    }

    let workspace = match command.workspace {
        Some(path) => path,
        None => {
            let _ = writeln!(
                stderr,
                "session rewind failed: --workspace is required without --dry-run"
            );
            return 1;
        }
    };
    let snapshot_path = match command.snapshot {
        Some(path) => path,
        None => {
            let _ = writeln!(
                stderr,
                "session rewind failed: --snapshot is required without --dry-run"
            );
            return 1;
        }
    };
    let snapshot = match load_snapshot(&snapshot_path, stderr) {
        Ok(snapshot) => snapshot,
        Err(code) => return code,
    };

    match atomic_prompt_rewind(&events, command.cutoff, &workspace, &snapshot) {
        Ok(result) => {
            let report = RewindReport {
                harness_operation: "rewind",
                run_dir,
                workspace,
                cutoff_seq: result.conversation.cutoff_seq,
                retained_event_count: result.conversation.retained_event_count,
                discarded_event_count: result.conversation.discarded_event_count,
                conversation_message_count: result.conversation.conversation.messages.len(),
                files_restored: result.files_restored,
                files_unchanged: result.files_unchanged,
                events_append_only: result.events_append_only,
            };
            emit_report(&report, command.json, stdout, stderr)
        }
        Err(err) => {
            let _ = writeln!(stderr, "session rewind failed: {err}");
            1
        }
    }
}

fn load_snapshot(
    path: &Path,
    stderr: &mut dyn std::io::Write,
) -> Result<Vec<FileSnapshotEntry>, i32> {
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "session rewind failed: failed to read snapshot {}: {err}",
                path.display()
            );
            return Err(1);
        }
    };
    match serde_json::from_str(&body) {
        Ok(snapshot) => Ok(snapshot),
        Err(err) => {
            let _ = writeln!(
                stderr,
                "session rewind failed: failed to parse snapshot {}: {err}",
                path.display()
            );
            Err(1)
        }
    }
}

fn emit_report(
    report: &RewindReport,
    json: bool,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    if json {
        return write_json_output(report, None, stdout, stderr);
    }
    let _ = writeln!(stdout, "session rewind applied");
    let _ = writeln!(stdout, "run_dir: {}", report.run_dir.display());
    let _ = writeln!(stdout, "workspace: {}", report.workspace.display());
    let _ = writeln!(stdout, "cutoff_seq: {}", report.cutoff_seq);
    let _ = writeln!(stdout, "retained_events: {}", report.retained_event_count);
    let _ = writeln!(stdout, "discarded_events: {}", report.discarded_event_count);
    let _ = writeln!(
        stdout,
        "conversation_messages: {}",
        report.conversation_message_count
    );
    let _ = writeln!(stdout, "files_restored: {}", report.files_restored);
    let _ = writeln!(stdout, "files_unchanged: {}", report.files_unchanged);
    let _ = writeln!(stdout, "events_append_only: {}", report.events_append_only);
    0
}

fn emit_dry_run_report(
    report: &DryRunRewindReport,
    json: bool,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    if json {
        return write_json_output(report, None, stdout, stderr);
    }
    let _ = writeln!(
        stdout,
        "session rewind dry-run (projection only, no file restore)"
    );
    let _ = writeln!(stdout, "run_dir: {}", report.run_dir.display());
    let _ = writeln!(stdout, "cutoff_seq: {}", report.cutoff_seq);
    let _ = writeln!(stdout, "retained_events: {}", report.retained_event_count);
    let _ = writeln!(stdout, "discarded_events: {}", report.discarded_event_count);
    let _ = writeln!(
        stdout,
        "conversation_messages: {}",
        report.conversation_message_count
    );
    let _ = writeln!(stdout, "events_append_only: {}", report.events_append_only);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use crate::{CliDeps, CliIo};
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION,
    };
    use std::fs;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn worker() -> EventActor {
        EventActor::new(ActorKind::Worker, Some("agent_1".to_string()))
    }

    fn user_message(seq: u64, text: &str) -> EventEnvelopeV1 {
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
            payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: format!("req_{seq}").into(),
                text: text.to_string(),
            }),
        }
    }

    fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
        let mut body = String::new();
        for event in events {
            body.push_str(&serde_json::to_string(event).unwrap_or_abort());
            body.push('\n');
        }
        fs::write(run_dir.join("events.jsonl"), body).unwrap_or_abort();
    }

    fn seed_session_with_three_messages() -> (tempfile::TempDir, PathBuf) {
        let temp = tempdir().unwrap_or_abort();
        let run_dir = temp.path().join("sessions").join("run_rewind");
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        let events = vec![
            user_message(1, "first"),
            user_message(2, "second"),
            user_message(3, "third"),
        ];
        write_events_jsonl(&run_dir, &events);
        (temp, run_dir)
    }

    fn write_snapshot(path: &Path, entries: &[FileSnapshotEntry]) {
        let body = serde_json::to_string(entries).unwrap_or_abort();
        fs::write(path, body).unwrap_or_abort();
    }

    fn run_rewind(args: &[String]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real();
        let mut argv: Vec<String> = vec![
            "harness".to_string(),
            "sessions".to_string(),
            "rewind".to_string(),
        ];
        argv.extend_from_slice(args);
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn rewind_restores_files_and_keeps_events_append_only() {
        // arrange — a stored three-message session plus a mutated workspace file
        let (_temp, run_dir) = seed_session_with_three_messages();
        let workspace = _temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "after").unwrap_or_abort();
        let events_path = run_dir.join("events.jsonl");
        let events_before = fs::read(&events_path).unwrap_or_abort();
        let snapshot_path = _temp.path().join("snapshot.json");
        write_snapshot(
            &snapshot_path,
            &[FileSnapshotEntry {
                path: "notes.txt".into(),
                content: "before".into(),
            }],
        );

        // act — rewind the conversation through seq 2 and restore the snapshot
        let (code, stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "2".to_string(),
            "--workspace".to_string(),
            workspace.display().to_string(),
            "--snapshot".to_string(),
            snapshot_path.display().to_string(),
            "--json".to_string(),
        ]);

        // assert — files restored, events unchanged, honest append-only flag
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"files_restored\": 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("\"retained_event_count\": 2"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"discarded_event_count\": 1"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"events_append_only\": true"),
            "stdout: {stdout}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "before");
        assert_eq!(
            fs::read(&events_path).unwrap_or_abort(),
            events_before,
            "events.jsonl must stay append-only and byte-identical"
        );
    }

    #[test]
    fn rewind_fails_closed_on_bad_cutoff_and_leaves_files_untouched() {
        // arrange
        let (_temp, run_dir) = seed_session_with_three_messages();
        let workspace = _temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "keep").unwrap_or_abort();
        let snapshot_path = _temp.path().join("snapshot.json");
        write_snapshot(
            &snapshot_path,
            &[FileSnapshotEntry {
                path: "notes.txt".into(),
                content: "changed".into(),
            }],
        );

        // act — cutoff beyond the event log
        let (code, _stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "9".to_string(),
            "--workspace".to_string(),
            workspace.display().to_string(),
            "--snapshot".to_string(),
            snapshot_path.display().to_string(),
        ]);

        // assert — fail-closed; the workspace file is never modified
        assert_eq!(code, 1);
        assert!(stderr.contains("session rewind failed"), "stderr: {stderr}");
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "keep");
    }

    #[test]
    fn rewind_rolls_back_workspace_on_invalid_snapshot_path() {
        // arrange — a valid restore followed by an escaping path in one snapshot
        let (_temp, run_dir) = seed_session_with_three_messages();
        let workspace = _temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("ok.txt");
        fs::write(&target, "original").unwrap_or_abort();
        let snapshot_path = _temp.path().join("snapshot.json");
        write_snapshot(
            &snapshot_path,
            &[
                FileSnapshotEntry {
                    path: "ok.txt".into(),
                    content: "mutated".into(),
                },
                FileSnapshotEntry {
                    path: "../escape.txt".into(),
                    content: "nope".into(),
                },
            ],
        );

        // act
        let (code, _stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "1".to_string(),
            "--workspace".to_string(),
            workspace.display().to_string(),
            "--snapshot".to_string(),
            snapshot_path.display().to_string(),
        ]);

        // assert — fail-closed rollback restores the already-applied change
        assert_eq!(code, 1);
        assert!(stderr.contains("session rewind failed"), "stderr: {stderr}");
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "original");
        assert!(!_temp.path().join("escape.txt").exists());
    }

    #[test]
    fn dry_run_projects_conversation_without_restoring_files_or_requiring_snapshot() {
        // arrange — a stored three-message session plus a mutated workspace file
        let (_temp, run_dir) = seed_session_with_three_messages();
        let workspace = _temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let target = workspace.join("notes.txt");
        fs::write(&target, "after").unwrap_or_abort();
        let events_path = run_dir.join("events.jsonl");
        let events_before = fs::read(&events_path).unwrap_or_abort();

        // act — dry-run projects through cutoff 2 without --workspace or --snapshot
        let (code, stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "2".to_string(),
            "--dry-run".to_string(),
            "--json".to_string(),
        ]);

        // assert — projection reported, files untouched, events unchanged
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"harness_operation\": \"rewind-dry-run\""),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"retained_event_count\": 2"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"discarded_event_count\": 1"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"events_append_only\": true"),
            "stdout: {stdout}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap_or_abort(), "after");
        assert_eq!(
            fs::read(&events_path).unwrap_or_abort(),
            events_before,
            "events.jsonl must stay byte-identical"
        );
    }

    #[test]
    fn dry_run_fails_closed_on_bad_cutoff() {
        // arrange
        let (_temp, run_dir) = seed_session_with_three_messages();

        // act — cutoff beyond the event log
        let (code, _stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "9".to_string(),
            "--dry-run".to_string(),
        ]);

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("session rewind failed"), "stderr: {stderr}");
    }

    #[test]
    fn rewind_without_workspace_or_snapshot_fails_closed() {
        // arrange
        let (_temp, run_dir) = seed_session_with_three_messages();

        // act — no --dry-run, no --workspace
        let (code, _stdout, stderr) = run_rewind(&[
            run_dir.display().to_string(),
            "--cutoff".to_string(),
            "2".to_string(),
        ]);

        // assert
        assert_eq!(code, 1);
        assert!(
            stderr.contains("--workspace is required without --dry-run"),
            "stderr: {stderr}"
        );
    }
}
