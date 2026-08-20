//! CLI for session metadata management: rename, cleanup (delete), and restart.
//!
//! Rename delegates to the coordinator's offline title-mutation authority,
//! which acquires the event store, constructs the `SessionTitleUpdated`
//! envelope, and appends it. The CLI never opens stores or builds events
//! directly. It does not execute providers, tools, hooks, MCP, or network.
//!
//! Cleanup permanently removes a session run directory. It refuses to delete
//! sessions that hold an active writer lock (a running coordinator would
//! lose its event log mid-flight).
//!
//! Restart applies crash recovery (if a previous-crash marker is present)
//! and then continues the session in the live TUI, mirroring the
//! reopen-then-continue operator flow.

use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use harness_core::clock::RealClock;
use harness_core::crash_recovery::{apply_crash_recovery, inspect_previous_crash};
use harness_core::store::{acquire_session_writer_lock, EventStoreError};
use serde::Serialize;

use crate::recovery::resolve_session_run_dir;
use crate::tui::TuiCommand;

use super::{ensure_session_dir_exists, session_dir, write_json_output};

/// Rename (set the title of) a stored session by appending a `SessionTitleUpdated` event.
#[derive(Debug, Args, Clone)]
pub struct RenameSessionCommand {
    /// Run id or session directory path to rename.
    #[arg(value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    /// New session title.
    #[arg(value_name = "TITLE")]
    pub title: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Permanently delete a stored session directory.
#[derive(Debug, Args, Clone)]
pub struct CleanupSessionCommand {
    /// Run id or session directory path to delete.
    #[arg(value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    /// Skip the armed-confirmation prompt (required for non-interactive use).
    #[arg(long, default_value_t = false)]
    pub yes: bool,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Restart a session: apply crash recovery if needed, then continue in the live TUI.
#[derive(Debug, Args, Clone)]
pub struct RestartSessionCommand {
    /// Run id or session directory path to restart.
    #[arg(value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    #[arg(long, default_value_t = false)]
    pub exit_on_finish: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RenameReport {
    harness_operation: &'static str,
    run_dir: PathBuf,
    run_id: String,
    title: String,
    event_seq: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CleanupReport {
    harness_operation: &'static str,
    run_dir: PathBuf,
    run_id: String,
    deleted: bool,
}

pub(super) fn rename_session(
    command: RenameSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            let _ = writeln!(stderr, "session rename failed: {err}");
            return 1;
        }
    };

    let run_id = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&command.session)
        .to_string();

    let title = command.title.trim().to_string();
    if title.is_empty() {
        let _ = writeln!(stderr, "session rename failed: title must not be empty");
        return 1;
    }

    let store_session_dir = run_dir
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or(session_dir);
    let clock = RealClock::new();
    let appended = match harness_core::coord::update_session_title_offline(
        &clock,
        &store_session_dir,
        &run_id,
        &title,
    ) {
        Ok(envelope) => envelope,
        Err(err) => {
            let hint = if err.to_string().contains("writer lock") {
                " (writer lock held); stop the session before renaming"
            } else {
                ""
            };
            let _ = writeln!(stderr, "session rename failed: {err}{hint}");
            return 1;
        }
    };

    let report = RenameReport {
        harness_operation: "rename",
        run_dir,
        run_id,
        title,
        event_seq: appended.seq,
    };

    if command.json {
        write_json_output(&report, None, stdout, stderr)
    } else {
        let _ = writeln!(stdout, "session renamed");
        let _ = writeln!(stdout, "run_id: {}", report.run_id);
        let _ = writeln!(stdout, "title: {}", report.title);
        let _ = writeln!(stdout, "event_seq: {}", report.event_seq);
        0
    }
}

pub(super) fn cleanup_session(
    command: CleanupSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            let _ = writeln!(stderr, "session cleanup failed: {err}");
            return 1;
        }
    };

    let run_id = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&command.session)
        .to_string();

    if !command.yes {
        let _ = writeln!(
            stderr,
            "session cleanup: refusing to delete `{}` without --yes \
             (armed confirmation required for non-interactive use)",
            run_id
        );
        return 1;
    }

    // Concurrency invariant: the canonical writer lock is acquired atomically
    // and held through removal, so a coordinator can never acquire it between a
    // liveness check and the delete (TOCTOU). A live writer fails acquisition
    // and the cleanup refuses; a stale lock from a dead process is recovered.
    let _writer_lock = match acquire_session_writer_lock(&run_dir) {
        Ok(guard) => guard,
        Err(EventStoreError::AcquireWriterLock { .. }) => {
            let _ = writeln!(
                stderr,
                "session cleanup failed: session `{}` holds an active writer lock; \
                 stop the session before deleting",
                run_id
            );
            return 1;
        }
        Err(err) => {
            let _ = writeln!(
                stderr,
                "session cleanup failed: cannot acquire lock for {}: {err}",
                run_dir.display()
            );
            return 1;
        }
    };

    let deleted = match std::fs::remove_dir_all(&run_dir) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "session cleanup failed: cannot remove {}: {err}",
                run_dir.display()
            );
            return 1;
        }
    };

    let report = CleanupReport {
        harness_operation: "cleanup",
        run_dir,
        run_id,
        deleted,
    };

    if command.json {
        write_json_output(&report, None, stdout, stderr)
    } else if deleted {
        let _ = writeln!(stdout, "session deleted");
        let _ = writeln!(stdout, "run_id: {}", report.run_id);
        0
    } else {
        let _ = writeln!(
            stdout,
            "session not found (already removed): {}",
            report.run_id
        );
        0
    }
}

pub(super) fn restart_session(
    command: RestartSessionCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            let _ = writeln!(stderr, "session restart failed: {err}");
            return 1;
        }
    };

    // Apply crash recovery if a previous-crash marker is present.
    let before = inspect_previous_crash(&run_dir);
    if before.previous_crash_detected {
        let parent = run_dir.parent().unwrap_or(&session_dir);
        let run_id = run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&command.session);
        match apply_crash_recovery(parent, run_id, false) {
            Ok(result) => {
                let _ = writeln!(
                    stderr,
                    "session restart: crash recovery applied: {}",
                    result.one_line()
                );
            }
            Err(err) => {
                let _ = writeln!(stderr, "session restart failed: crash recovery: {err}");
                return 1;
            }
        }
    }

    // Continue the session in the live TUI (same as `sessions continue`).
    crate::sessions::exit_code_to_i32(crate::tui::execute(
        TuiCommand {
            replay: None,
            continue_session: Some(run_dir),
            scenario: None,
            mock: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: command.exit_on_finish,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        config_path,
        Some(session_dir),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliDeps;
    use crate::CliIo;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
        SCHEMA_VERSION,
    };
    use std::io::Cursor;
    use tempfile::tempdir;

    fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
        let mut body = String::new();
        for event in events {
            body.push_str(&serde_json::to_string(event).unwrap());
            body.push('\n');
        }
        std::fs::write(run_dir.join("events.jsonl"), body).unwrap();
    }

    fn finished_session(run_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempdir().unwrap();
        let run_dir = temp.path().join("sessions").join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let events = vec![
            EventEnvelopeV1 {
                schema_version: SCHEMA_VERSION,
                event_id: "evt-1".into(),
                seq: 1,
                run_id: run_id.into(),
                mono_ms: 1,
                ts: None,
                actor: EventActor::new(ActorKind::System, None),
                correlation_id: None,
                causation_id: None,
                stream_key: None,
                payload: EventV1::RunStarted(RunStartedEvent {
                    run_name: "test".into(),
                    workspace_root: "/tmp".into(),
                }),
            },
            EventEnvelopeV1 {
                schema_version: SCHEMA_VERSION,
                event_id: "evt-2".into(),
                seq: 2,
                run_id: run_id.into(),
                mono_ms: 2,
                ts: None,
                actor: EventActor::new(ActorKind::System, None),
                correlation_id: None,
                causation_id: None,
                stream_key: None,
                payload: EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".into(),
                }),
            },
        ];
        write_events_jsonl(&run_dir, &events);
        (temp, run_dir)
    }

    fn run_cli(args: &[String]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real();
        let mut argv: Vec<String> = vec!["harness".to_string(), "sessions".to_string()];
        argv.extend_from_slice(args);
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn rename_appends_title_event_and_keeps_existing_events() {
        // arrange: a finished session with two events
        let (_temp, run_dir) = finished_session("run_rename");
        let events_before = std::fs::read(run_dir.join("events.jsonl")).unwrap();

        // act: rename via CLI
        let (code, stdout, stderr) = run_cli(&[
            "rename".to_string(),
            run_dir.display().to_string(),
            "My Custom Title".to_string(),
            "--json".to_string(),
        ]);

        // assert: success, event appended, existing events preserved
        assert_eq!(code, 0, "stderr: {stderr}");
        let body: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(body["harness_operation"], "rename");
        assert_eq!(body["title"], "My Custom Title");
        assert_eq!(body["event_seq"], 3);

        let events_after = std::fs::read(run_dir.join("events.jsonl")).unwrap();
        assert!(
            events_after.len() > events_before.len(),
            "events.jsonl must have grown"
        );
        assert!(
            events_after.starts_with(&events_before),
            "existing events must be preserved (append-only)"
        );
        let events_after_str = String::from_utf8_lossy(&events_after).into_owned();
        let last_line = events_after_str.lines().last().unwrap();
        let envelope: serde_json::Value = serde_json::from_str(last_line).unwrap();
        assert_eq!(envelope["payload"]["event_type"], "session_title_updated");
        assert_eq!(envelope["payload"]["data"]["title"], "My Custom Title");
    }

    #[test]
    fn rename_fails_for_empty_title() {
        // arrange: a finished session
        let (_temp, run_dir) = finished_session("run_rename_empty");

        // act: rename with empty title
        let (code, _stdout, stderr) = run_cli(&[
            "rename".to_string(),
            run_dir.display().to_string(),
            "  ".to_string(),
        ]);

        // assert: fail-closed
        assert_eq!(code, 1);
        assert!(stderr.contains("title must not be empty"));
    }

    #[test]
    fn rename_fails_when_writer_lock_held() {
        // arrange: a session with an active writer lock
        let (_temp, run_dir) = finished_session("run_rename_locked");
        std::fs::write(run_dir.join(".writer.lock"), "pid=1\ntoken=1\n").unwrap();

        // act: rename via CLI
        let (code, _stdout, stderr) = run_cli(&[
            "rename".to_string(),
            run_dir.display().to_string(),
            "New Title".to_string(),
        ]);

        // assert: fail-closed — cannot rename an active session
        assert_eq!(code, 1);
        assert!(stderr.contains("writer lock held"));
    }

    #[test]
    fn cleanup_deletes_session_with_yes_flag() {
        // arrange: a finished session
        let (_temp, run_dir) = finished_session("run_cleanup");
        assert!(run_dir.exists());

        // act: cleanup with --yes
        let (code, stdout, stderr) = run_cli(&[
            "cleanup".to_string(),
            run_dir.display().to_string(),
            "--yes".to_string(),
            "--json".to_string(),
        ]);

        // assert: session directory removed
        assert_eq!(code, 0, "stderr: {stderr}");
        let body: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(body["harness_operation"], "cleanup");
        assert_eq!(body["deleted"], true);
        assert!(!run_dir.exists());
    }

    #[test]
    fn cleanup_refuses_without_yes_flag() {
        // arrange: a finished session
        let (_temp, run_dir) = finished_session("run_cleanup_noyes");
        assert!(run_dir.exists());

        // act: cleanup without --yes
        let (code, _stdout, stderr) =
            run_cli(&["cleanup".to_string(), run_dir.display().to_string()]);

        // assert: fail-closed — armed confirmation required
        assert_eq!(code, 1);
        assert!(stderr.contains("--yes"));
        assert!(
            run_dir.exists(),
            "session must not be deleted without --yes"
        );
    }

    #[test]
    fn cleanup_refuses_when_writer_lock_held() {
        // arrange: a session with an active writer lock
        let (_temp, run_dir) = finished_session("run_cleanup_locked");
        std::fs::write(run_dir.join(".writer.lock"), "pid=1\ntoken=1\n").unwrap();

        // act: cleanup with --yes
        let (code, _stdout, stderr) = run_cli(&[
            "cleanup".to_string(),
            run_dir.display().to_string(),
            "--yes".to_string(),
        ]);

        // assert: fail-closed — cannot delete an active session
        assert_eq!(code, 1);
        assert!(stderr.contains("writer lock"));
        assert!(run_dir.exists(), "session must not be deleted when locked");
    }

    #[test]
    fn cleanup_refuses_while_live_store_holds_lock_then_succeeds_after_release() {
        // arrange — a session whose writer lock is held by a live event store
        let (_temp, run_dir) = finished_session("run_cleanup_live");
        let session_parent = run_dir.parent().unwrap().to_path_buf();
        let run_id = run_dir.file_name().unwrap().to_str().unwrap().to_string();
        let live_store =
            harness_core::store::JsonlFileEventStore::open_existing(&session_parent, &run_id, true)
                .expect("live store acquires the writer lock");

        // act — attempt cleanup with --yes while the lock is held
        let (code, _stdout, stderr) = run_cli(&[
            "cleanup".to_string(),
            run_dir.display().to_string(),
            "--yes".to_string(),
        ]);

        // assert — cleanup is refused and the directory survives the held lock
        assert_eq!(code, 1);
        assert!(stderr.contains("writer lock"));
        assert!(
            run_dir.exists(),
            "live session must survive cleanup attempt"
        );

        // act — the live writer releases the lock, then cleanup retries
        drop(live_store);
        let (code, _stdout, stderr) = run_cli(&[
            "cleanup".to_string(),
            run_dir.display().to_string(),
            "--yes".to_string(),
        ]);

        // assert — deletion proceeds once no live writer remains
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(!run_dir.exists());
    }

    #[test]
    fn cleanup_recovers_stale_writer_lock_from_dead_process() {
        // arrange — a session whose writer lock belongs to a dead process
        let (_temp, run_dir) = finished_session("run_cleanup_stale");
        std::fs::write(run_dir.join(".writer.lock"), "pid=4294967295\ntoken=1\n").unwrap();

        // act — attempt cleanup with --yes and --json
        let (code, stdout, stderr) = run_cli(&[
            "cleanup".to_string(),
            run_dir.display().to_string(),
            "--yes".to_string(),
            "--json".to_string(),
        ]);

        // assert — the stale lock is recovered and the session is deleted
        assert_eq!(code, 0, "stderr: {stderr}");
        let body: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(body["deleted"], true);
        assert!(!run_dir.exists());
    }
}
