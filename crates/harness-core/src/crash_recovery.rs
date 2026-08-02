//! Previous-crash detection and recovery path for session run directories.
//!
//! Detection is read-only. Recovery reuses the JSONL event store open path,
//! which already repairs truncated crash tails and recovers dead-PID writer
//! locks without rewriting complete event lines.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session_paths::{
    ARTIFACTS_DIR_NAME, EVENTS_FILE_NAME, META_FILE_NAME, WRITER_LOCK_FILE_NAME,
};
use crate::store::{EventStoreError, JsonlFileEventStore};

const WRITER_LOCK_RECOVERY_FILE_NAME: &str = ".writer.lock.recovering";

/// Operator-facing recovery step after a previous crash was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashRecoveryAction {
    /// Resume with a new prompt once the run is resumable.
    ResumeWithPrompt,
    /// Reopen the session for inspection when resume is not available.
    ReopenSession,
    /// Exclusive open will recover locks/markers; no separate CLI step.
    OpenRecovers,
}

impl CrashRecoveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResumeWithPrompt => "resume_with_prompt",
            Self::ReopenSession => "reopen_session",
            Self::OpenRecovers => "open_recovers",
        }
    }

    pub fn operator_hint(self, run_id: &str) -> String {
        match self {
            Self::ResumeWithPrompt => {
                format!("harness prompt --resume {run_id} --text \"<next prompt>\"")
            }
            Self::ReopenSession => {
                format!("harness sessions reopen --session {run_id}")
            }
            Self::OpenRecovers => {
                "open this session exclusively; lock recovery runs automatically".to_string()
            }
        }
    }
}

/// Read-only previous-crash markers for a session run directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreviousCrashReport {
    pub run_dir: PathBuf,
    pub previous_crash_detected: bool,
    pub stale_writer_lock: bool,
    pub recovery_marker_present: bool,
    pub events_log_present: bool,
    /// Human-readable recovery guidance when a previous crash was detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_message: Option<String>,
    /// Structured recovery action for CLI/TUI operators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_action: Option<CrashRecoveryAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Inspect a run directory for previous-crash markers (does not mutate disk).
pub fn inspect_previous_crash(run_dir: &Path) -> PreviousCrashReport {
    let lock_path = run_dir.join(WRITER_LOCK_FILE_NAME);
    let recovery_path = run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME);
    let events_path = run_dir.join(EVENTS_FILE_NAME);

    let events_log_present = events_path.is_file();
    let recovery_marker_present = recovery_path.exists();
    let mut notes = Vec::new();
    let mut stale_writer_lock = false;

    if lock_path.is_file() {
        match fs::read_to_string(&lock_path) {
            Ok(contents) => {
                if let Some(pid) = parse_writer_lock_pid(&contents) {
                    if !process_exists(pid) {
                        stale_writer_lock = true;
                        notes.push(format!(
                            "stale writer lock held by dead pid {pid}; open_existing will recover"
                        ));
                    } else {
                        notes.push(format!(
                            "writer lock held by live pid {pid}; exclusive open may fail"
                        ));
                    }
                } else if contents.trim().is_empty() {
                    if unborn_run_dir(run_dir) {
                        stale_writer_lock = true;
                        notes.push(
                            "legacy empty writer lock on unborn run dir; open will recover"
                                .to_string(),
                        );
                    } else {
                        notes.push(
                            "empty writer lock present with session artifacts; exclusive open may fail"
                                .to_string(),
                        );
                    }
                } else if unborn_run_dir(run_dir) {
                    stale_writer_lock = true;
                    notes.push(
                        "legacy text writer lock on unborn run dir; open will recover".to_string(),
                    );
                } else {
                    notes.push(
                        "non-pid writer lock present with session artifacts; exclusive open may fail"
                            .to_string(),
                    );
                }
            }
            Err(err) => notes.push(format!("failed to read writer lock: {err}")),
        }
    }

    if recovery_marker_present {
        notes.push("recovery marker .writer.lock.recovering is present".to_string());
    }

    let previous_crash_detected = stale_writer_lock || recovery_marker_present;
    let recovery_message = previous_crash_detected.then(|| {
        build_recovery_message(
            stale_writer_lock,
            recovery_marker_present,
            events_log_present,
        )
    });
    let recovery_action = previous_crash_detected.then_some(CrashRecoveryAction::OpenRecovers);

    PreviousCrashReport {
        run_dir: run_dir.to_path_buf(),
        previous_crash_detected,
        stale_writer_lock,
        recovery_marker_present,
        events_log_present,
        recovery_message,
        recovery_action,
        notes,
    }
}

/// Prefer resume when the catalog marks the run resumable; otherwise reopen.
pub fn resolve_crash_recovery_action(is_resumable: bool) -> CrashRecoveryAction {
    if is_resumable {
        CrashRecoveryAction::ResumeWithPrompt
    } else {
        CrashRecoveryAction::ReopenSession
    }
}

/// Operator-facing counts for a multi-run crash scan (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CrashRecoveryScanSummary {
    pub scanned: usize,
    pub previous_crash: usize,
    pub clean: usize,
    pub stale_writer_lock: usize,
    pub recovery_marker: usize,
}

impl CrashRecoveryScanSummary {
    pub fn one_line(&self) -> String {
        format!(
            "crash scan: {} previous-crash, {} clean ({} scanned; {} stale-lock, {} recovery-marker)",
            self.previous_crash,
            self.clean,
            self.scanned,
            self.stale_writer_lock,
            self.recovery_marker
        )
    }

    pub const fn has_previous_crash(&self) -> bool {
        self.previous_crash > 0
    }
}

/// Scan immediate children of a sessions root for previous-crash markers (read-only).
///
/// Non-directories and unreadable roots yield an empty list (fail soft for scan UX).
pub fn scan_previous_crashes(sessions_root: &Path) -> Vec<PreviousCrashReport> {
    let Ok(entries) = fs::read_dir(sessions_root) else {
        return Vec::new();
    };
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            reports.push(inspect_previous_crash(&path));
        }
    }
    reports.sort_by(|left, right| left.run_dir.cmp(&right.run_dir));
    reports
}

/// Summarize multi-run crash inspect results for CLI/operator surfaces.
pub fn summarize_crash_reports(reports: &[PreviousCrashReport]) -> CrashRecoveryScanSummary {
    let mut summary = CrashRecoveryScanSummary {
        scanned: reports.len(),
        ..CrashRecoveryScanSummary::default()
    };
    for report in reports {
        if report.previous_crash_detected {
            summary.previous_crash = summary.previous_crash.saturating_add(1);
        } else {
            summary.clean = summary.clean.saturating_add(1);
        }
        if report.stale_writer_lock {
            summary.stale_writer_lock = summary.stale_writer_lock.saturating_add(1);
        }
        if report.recovery_marker_present {
            summary.recovery_marker = summary.recovery_marker.saturating_add(1);
        }
    }
    summary
}

impl PreviousCrashReport {
    pub fn one_line(&self) -> String {
        if !self.previous_crash_detected {
            return format!("clean: {}", self.run_dir.display());
        }
        let action = self
            .recovery_action
            .map(|action| action.as_str())
            .unwrap_or("open_recovers");
        format!(
            "previous-crash: {} (stale_lock={}, recovery_marker={}, events={}, action={})",
            self.run_dir.display(),
            self.stale_writer_lock,
            self.recovery_marker_present,
            self.events_log_present,
            action
        )
    }
}

fn build_recovery_message(
    stale_writer_lock: bool,
    recovery_marker_present: bool,
    events_log_present: bool,
) -> String {
    let mut parts = Vec::new();
    parts.push("Previous crash detected.".to_string());
    if stale_writer_lock {
        parts.push(
            "Stale writer lock from a dead process will be recovered on next exclusive open."
                .to_string(),
        );
    }
    if recovery_marker_present {
        parts.push("Recovery marker present; open_existing will finish lock recovery.".to_string());
    }
    if events_log_present {
        parts.push(
            "Event log is present; truncated crash tails are repaired without rewriting complete lines."
                .to_string(),
        );
    } else {
        parts.push("No events.jsonl found yet; session may be unborn.".to_string());
    }
    parts.push(
        "Use `harness sessions inspect <run>` then resume with `harness prompt --resume` when resumable."
            .to_string(),
    );
    parts.join(" ")
}

/// Open an existing session event store, applying crash-tail + lock recovery.
pub fn recover_session_event_store(
    session_dir: impl AsRef<Path>,
    run_id: impl AsRef<str>,
    deterministic: bool,
) -> Result<JsonlFileEventStore, EventStoreError> {
    JsonlFileEventStore::open_existing(session_dir, run_id, deterministic)
}

/// Structured outcome of applying crash recovery to one session run.
///
/// Exclusive open recovers dead-PID writer locks, finishes recovery-marker
/// cleanup, and repairs truncated crash tails without rewriting complete lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecoveryApplyResult {
    pub run_id: String,
    pub run_dir: PathBuf,
    /// True when exclusive open was performed.
    pub applied: bool,
    pub before: PreviousCrashReport,
    pub after: PreviousCrashReport,
    /// True when previous-crash markers were present and cleared after open.
    pub recovered: bool,
    pub recovery_marker_cleared: bool,
    pub stale_lock_cleared: bool,
    pub events_log_present: bool,
}

impl CrashRecoveryApplyResult {
    pub fn one_line(&self) -> String {
        if !self.applied {
            return format!("crash recovery skipped: {}", self.run_dir.display());
        }
        if self.recovered {
            format!(
                "crash recovery applied: {} (marker_cleared={}, stale_lock_cleared={}, events={})",
                self.run_dir.display(),
                self.recovery_marker_cleared,
                self.stale_lock_cleared,
                self.events_log_present
            )
        } else if self.before.previous_crash_detected {
            format!(
                "crash recovery open completed: {} (markers may remain; events={})",
                self.run_dir.display(),
                self.events_log_present
            )
        } else {
            format!(
                "crash recovery open completed: {} (no previous-crash markers; events={})",
                self.run_dir.display(),
                self.events_log_present
            )
        }
    }
}

/// Apply exclusive-open crash recovery for one run under a sessions root.
///
/// This is the product reopen path: inspect markers → open_existing (lock + tail
/// repair) → clear orphan recovery markers → re-inspect. Dropping the store
/// releases the writer lock.
pub fn apply_crash_recovery(
    session_dir: impl AsRef<Path>,
    run_id: impl AsRef<str>,
    deterministic: bool,
) -> Result<CrashRecoveryApplyResult, EventStoreError> {
    let session_dir = session_dir.as_ref();
    let run_id = run_id.as_ref();
    let run_dir = session_dir.join(run_id);
    let before = inspect_previous_crash(&run_dir);

    let store = recover_session_event_store(session_dir, run_id, deterministic)?;
    drop(store);

    // Exclusive open can leave a pre-existing `.writer.lock.recovering` marker
    // when recovery finished without holding a live writer lock. Clear orphans.
    clear_orphan_recovery_marker(&run_dir);

    let after = inspect_previous_crash(&run_dir);
    let recovery_marker_cleared = before.recovery_marker_present && !after.recovery_marker_present;
    let stale_lock_cleared = before.stale_writer_lock && !after.stale_writer_lock;
    let recovered = before.previous_crash_detected
        && (recovery_marker_cleared || stale_lock_cleared || !after.previous_crash_detected);
    let events_log_present = after.events_log_present;

    Ok(CrashRecoveryApplyResult {
        run_id: run_id.to_string(),
        run_dir,
        applied: true,
        before,
        after,
        recovered,
        recovery_marker_cleared,
        stale_lock_cleared,
        events_log_present,
    })
}

fn clear_orphan_recovery_marker(run_dir: &Path) {
    let recovery_path = run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME);
    if !recovery_path.exists() {
        return;
    }
    let lock_path = run_dir.join(WRITER_LOCK_FILE_NAME);
    if lock_path.is_file() {
        if let Ok(contents) = fs::read_to_string(&lock_path) {
            if let Some(pid) = parse_writer_lock_pid(&contents) {
                if process_exists(pid) {
                    return;
                }
            }
        }
    }
    let _ = fs::remove_file(recovery_path);
}

fn parse_writer_lock_pid(contents: &str) -> Option<u32> {
    contents.lines().find_map(|line| {
        line.strip_prefix("pid=")
            .and_then(|pid| pid.parse::<u32>().ok())
    })
}

fn unborn_run_dir(run_dir: &Path) -> bool {
    if run_dir.join(EVENTS_FILE_NAME).exists()
        || run_dir.join(META_FILE_NAME).exists()
        || run_dir.join(ARTIFACTS_DIR_NAME).exists()
    {
        return false;
    }

    let Ok(entries) = fs::read_dir(run_dir) else {
        return false;
    };
    entries.into_iter().all(|entry| {
        entry
            .ok()
            .and_then(|entry| entry.file_name().into_string().ok())
            .is_some_and(|name| {
                name == WRITER_LOCK_FILE_NAME || name == WRITER_LOCK_RECOVERY_FILE_NAME
            })
    })
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_exists(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
    use crate::store::{EventEnvelopeWithoutSeqV1, EventStore};
    use crate::UnwrapOrAbort;
    use std::io::Write;
    use tokio_stream::StreamExt;

    fn run_started_draft(run_id: &str, mono_ms: u64) -> EventEnvelopeWithoutSeqV1 {
        EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{mono_ms:020}"),
            run_id: run_id.into(),
            mono_ms,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: "crash-recovery".into(),
                workspace_root: "/tmp".into(),
            }),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inspect_detects_stale_writer_lock_as_previous_crash() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let run_dir = temp.path().join("run_crash");
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        fs::write(run_dir.join(EVENTS_FILE_NAME), "").unwrap_or_abort();
        fs::write(
            run_dir.join(WRITER_LOCK_FILE_NAME),
            "pid=999999999\ntoken=1\n",
        )
        .unwrap_or_abort();

        let report = inspect_previous_crash(&run_dir);
        assert!(report.previous_crash_detected);
        assert!(report.stale_writer_lock);
        assert!(report.events_log_present);
        let message = report.recovery_message.expect("recovery message");
        assert!(message.contains("Previous crash detected"));
        assert!(message.contains("Stale writer lock"));
        assert!(message.contains("sessions inspect"));
        assert_eq!(
            report.recovery_action,
            Some(CrashRecoveryAction::OpenRecovers)
        );
        assert_eq!(
            resolve_crash_recovery_action(true),
            CrashRecoveryAction::ResumeWithPrompt
        );
        assert_eq!(
            resolve_crash_recovery_action(false),
            CrashRecoveryAction::ReopenSession
        );
    }

    #[test]
    fn inspect_detects_recovery_marker() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let run_dir = temp.path().join("run_recovering");
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        fs::write(run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME), "pid=1\n").unwrap_or_abort();

        let report = inspect_previous_crash(&run_dir);
        assert!(report.previous_crash_detected);
        assert!(report.recovery_marker_present);
        let message = report.recovery_message.expect("recovery message");
        assert!(message.contains("Recovery marker present"));
    }

    #[test]
    fn inspect_without_crash_has_no_recovery_message() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let run_dir = temp.path().join("run_clean");
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        fs::write(run_dir.join(EVENTS_FILE_NAME), "").unwrap_or_abort();

        let report = inspect_previous_crash(&run_dir);
        assert!(!report.previous_crash_detected);
        assert!(report.recovery_message.is_none());
        assert!(report.one_line().starts_with("clean:"));
    }

    #[test]
    fn scan_previous_crashes_summarizes_mixed_session_root() {
        // arrange
        // act
        // assert
        // Given: sessions root with one clean run and one recovery-marker crash
        let temp = tempfile::tempdir().unwrap_or_abort();
        let sessions = temp.path().join("sessions");
        let clean = sessions.join("run_clean");
        let crashed = sessions.join("run_crashed");
        fs::create_dir_all(&clean).unwrap_or_abort();
        fs::create_dir_all(&crashed).unwrap_or_abort();
        fs::write(clean.join(EVENTS_FILE_NAME), "").unwrap_or_abort();
        fs::write(crashed.join(EVENTS_FILE_NAME), "").unwrap_or_abort();
        fs::write(crashed.join(WRITER_LOCK_RECOVERY_FILE_NAME), "pid=1\n").unwrap_or_abort();

        // When
        let reports = scan_previous_crashes(&sessions);
        let summary = summarize_crash_reports(&reports);

        // Then
        assert_eq!(reports.len(), 2);
        assert_eq!(summary.scanned, 2);
        assert_eq!(summary.previous_crash, 1);
        assert_eq!(summary.clean, 1);
        assert_eq!(summary.recovery_marker, 1);
        assert!(summary.has_previous_crash());
        assert!(summary.one_line().contains("previous-crash"));
        assert!(reports.iter().any(|report| {
            report.previous_crash_detected && report.one_line().contains("previous-crash")
        }));
        assert!(scan_previous_crashes(temp.path().join("missing").as_path()).is_empty());
    }

    #[tokio::test]
    async fn recover_session_repairs_truncated_tail_via_store_path() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let run_id = "run_recover_tail";
        let file_path = {
            let store = JsonlFileEventStore::open(temp.path(), run_id, false).unwrap_or_abort();
            store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
            store.append(run_started_draft(run_id, 2)).unwrap_or_abort();
            store.file_path().to_path_buf()
        };

        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .unwrap_or_abort();
            file.write_all(b"{").unwrap_or_abort();
        }

        let recovered = recover_session_event_store(temp.path(), run_id, false).unwrap_or_abort();
        let mut stream = recovered.replay(1).unwrap_or_abort();
        let mut seqs = Vec::new();
        while let Some(item) = stream.next().await {
            seqs.push(item.unwrap_or_abort().seq);
        }
        assert_eq!(seqs, vec![1, 2]);

        let contents = fs::read_to_string(recovered.file_path()).unwrap_or_abort();
        assert!(!contents.ends_with('{'));
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn apply_crash_recovery_clears_recovery_marker_and_reports_outcome() {
        // arrange
        // act
        // assert
        // Given: run with events + recovery marker (previous crash)
        let temp = tempfile::tempdir().unwrap_or_abort();
        let sessions = temp.path().join("sessions");
        let run_id = "run_apply_marker";
        let run_dir = sessions.join(run_id);
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        {
            let store = JsonlFileEventStore::open(&sessions, run_id, false).unwrap_or_abort();
            store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
        }
        fs::write(run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME), "pid=1\n").unwrap_or_abort();
        assert!(inspect_previous_crash(&run_dir).previous_crash_detected);

        // When: operator apply path runs exclusive open recovery
        let result = apply_crash_recovery(&sessions, run_id, false).unwrap_or_abort();

        // Then: recovery applied, marker cleared, after-state clean
        assert!(result.applied);
        assert!(result.recovered);
        assert!(result.recovery_marker_cleared);
        assert!(result.before.previous_crash_detected);
        assert!(!result.after.previous_crash_detected);
        assert!(!result.after.recovery_marker_present);
        assert!(result.events_log_present);
        assert!(result.one_line().contains("crash recovery applied"));
        assert!(!run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME).exists());
        assert!(!run_dir.join(WRITER_LOCK_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn apply_crash_recovery_repairs_truncated_tail_and_clears_marker() {
        // arrange
        // act
        // assert
        // Given: truncated events tail + recovery marker
        let temp = tempfile::tempdir().unwrap_or_abort();
        let sessions = temp.path().join("sessions");
        let run_id = "run_apply_tail";
        let run_dir = sessions.join(run_id);
        let file_path = {
            let store = JsonlFileEventStore::open(&sessions, run_id, false).unwrap_or_abort();
            store.append(run_started_draft(run_id, 1)).unwrap_or_abort();
            store.append(run_started_draft(run_id, 2)).unwrap_or_abort();
            store.file_path().to_path_buf()
        };
        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .unwrap_or_abort();
            file.write_all(b"{").unwrap_or_abort();
        }
        fs::write(run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME), "pid=1\n").unwrap_or_abort();

        // When
        let result = apply_crash_recovery(&sessions, run_id, false).unwrap_or_abort();

        // Then: recovered + complete lines only
        assert!(result.recovered);
        assert!(result.recovery_marker_cleared);
        let contents = fs::read_to_string(&file_path).unwrap_or_abort();
        assert!(!contents.ends_with('{'));
        assert_eq!(contents.lines().count(), 2);

        let store = recover_session_event_store(&sessions, run_id, false).unwrap_or_abort();
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut seqs = Vec::new();
        while let Some(item) = stream.next().await {
            seqs.push(item.unwrap_or_abort().seq);
        }
        assert_eq!(seqs, vec![1, 2]);
    }
}
