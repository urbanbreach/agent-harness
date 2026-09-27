//! Inspection does not acquire a writer or repair files. Explicit recovery preserves torn bytes.
use crate::{
    event::*,
    proj::InFlight,
    store::{EventStore, EventStoreError, Journal},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
const MARKER: &str = ".writer.lock.recovering";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashRecoveryAction {
    ResumeWithPrompt,
    ReopenSession,
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
            Self::ReopenSession => format!("harness sessions reopen --session {run_id}"),
            Self::OpenRecovers => {
                "open this session exclusively to recover interrupted work".into()
            }
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousCrashReport {
    pub run_dir: PathBuf,
    pub previous_crash_detected: bool,
    pub stale_writer_lock: bool,
    pub recovery_marker_present: bool,
    pub events_log_present: bool,
    pub recovery_message: Option<String>,
    pub recovery_action: Option<CrashRecoveryAction>,
    pub notes: Vec<String>,
}
impl PreviousCrashReport {
    pub fn one_line(&self) -> String {
        if !self.previous_crash_detected {
            let status = if self.notes.is_empty() {
                "clean"
            } else {
                "inspection note"
            };
            return format!("{status}: {}", self.run_dir.display());
        }
        format!(
            "previous-crash: {} (stale_lock={}, recovery_marker={}, events={}, action={})",
            self.run_dir.display(),
            self.stale_writer_lock,
            self.recovery_marker_present,
            self.events_log_present,
            self.recovery_action
                .map_or("reopen_session", CrashRecoveryAction::as_str)
        )
    }
}
pub fn resolve_crash_recovery_action(is_resumable: bool) -> CrashRecoveryAction {
    if is_resumable {
        CrashRecoveryAction::ResumeWithPrompt
    } else {
        CrashRecoveryAction::ReopenSession
    }
}
pub fn inspect_previous_crash(run_dir: &Path) -> PreviousCrashReport {
    let mut report = PreviousCrashReport {
        run_dir: run_dir.into(),
        ..Default::default()
    };
    if !fs::symlink_metadata(run_dir).is_ok_and(|m| m.is_dir()) {
        return report;
    }
    report.events_log_present =
        fs::symlink_metadata(run_dir.join("events.jsonl")).is_ok_and(|m| m.is_file());
    report.recovery_marker_present = fs::symlink_metadata(run_dir.join(MARKER)).is_ok();
    let lock = run_dir.join(".writer.lock");
    let lock_present = lock.exists();
    let _lock = match crate::store::existing_writer_lock(run_dir) {
        Ok(lock) => lock,
        Err(error) => {
            report
                .notes
                .push(format!("writer inspection unavailable: {error}"));
            return report;
        }
    };
    if report.events_log_present {
        match crate::store::read_events(&run_dir.join("events.jsonl")).and_then(|events| {
            crate::proj::checked_history(&events)
                .map_err(|_| EventStoreError::Invalid("invalid complete history"))?;
            let mut state = InFlight::default();
            for event in &events {
                state.apply(event);
            }
            Ok((state.stable(), events.is_empty()))
        }) {
            Ok((stable, empty)) => {
                report.previous_crash_detected = !stable;
                report.stale_writer_lock = lock_present && (!stable || empty);
            }
            Err(error) => {
                report.previous_crash_detected = true;
                report.stale_writer_lock = lock_present;
                report
                    .notes
                    .push(format!("journal inspection failed: {error}"));
            }
        }
    } else {
        report.stale_writer_lock = lock_present;
    }
    report.previous_crash_detected |= report.stale_writer_lock || report.recovery_marker_present;
    if report.previous_crash_detected {
        report.recovery_action = Some(CrashRecoveryAction::OpenRecovers);
        report.recovery_message=Some("Previous crash detected. Exclusive recovery preserves complete records and marks interrupted work as unknown.".into());
    }
    report
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecoveryScanSummary {
    pub scanned: usize,
    pub previous_crash: usize,
    pub clean: usize,
    pub stale_writer_lock: usize,
    pub recovery_marker: usize,
}
impl CrashRecoveryScanSummary {
    pub fn one_line(&self) -> String {
        format!("crash scan: {} previous-crash, {} clean ({} scanned; {} stale-lock, {} recovery-marker)",self.previous_crash,self.clean,self.scanned,self.stale_writer_lock,self.recovery_marker)
    }
    pub const fn has_previous_crash(&self) -> bool {
        self.previous_crash > 0
    }
}
pub fn scan_previous_crashes(root: &Path) -> Vec<PreviousCrashReport> {
    let mut reports: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_dir())
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| inspect_previous_crash(&e.path()))
        .collect();
    reports.sort_by(|a, b| a.run_dir.cmp(&b.run_dir));
    reports
}
pub fn summarize_crash_reports(reports: &[PreviousCrashReport]) -> CrashRecoveryScanSummary {
    CrashRecoveryScanSummary {
        scanned: reports.len(),
        previous_crash: reports.iter().filter(|r| r.previous_crash_detected).count(),
        clean: reports
            .iter()
            .filter(|r| !r.previous_crash_detected && r.notes.is_empty())
            .count(),
        stale_writer_lock: reports.iter().filter(|r| r.stale_writer_lock).count(),
        recovery_marker: reports.iter().filter(|r| r.recovery_marker_present).count(),
    }
}

pub fn recover_session_event_store(
    session_dir: impl AsRef<Path>,
    run_id: impl AsRef<str>,
    _deterministic: bool,
) -> Result<Journal, EventStoreError> {
    crate::store::validate_session_id(run_id.as_ref())?;
    let marker = session_dir.as_ref().join(run_id.as_ref()).join(MARKER);
    let clear_marker = match fs::symlink_metadata(&marker) {
        Ok(meta) if meta.is_file() => true,
        Ok(_) => {
            return Err(EventStoreError::Invalid(
                "recovery marker must be a regular file",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    let journal = Journal::open_for_recovery(session_dir.as_ref(), run_id.as_ref())?;
    let events = crate::store::read_events(journal.file_path())?;
    crate::proj::checked_history(&events)
        .map_err(|_| EventStoreError::Invalid("invalid complete history; recovery was refused"))?;
    let mut state = InFlight::default();
    for event in &events {
        state.apply(event);
    }
    if !state.stable() {
        let template = events
            .last()
            .ok_or(EventStoreError::Invalid("missing recovery history"))?;
        let reason="interrupted by session restart; outcome is unknown, inspect current state before retrying";
        let mut identities: std::collections::HashSet<_> =
            events.iter().map(|e| e.event_id.clone()).collect();
        for event in state
            .terminals(reason)
            .into_iter()
            .chain(std::iter::once(EventEnvelopeV1 {
                payload: EventV1::RunFailed(RunFailedEvent {
                    error: reason.into(),
                }),
                actor: EventActor::new(ActorKind::System, None),
                ..template.clone()
            }))
        {
            let seq = journal.next_seq()?;
            let mut suffix = seq;
            let id = loop {
                let candidate = format!("{}-recovery-{suffix}", run_id.as_ref());
                if identities.insert(candidate.clone()) {
                    break candidate;
                }
                suffix = suffix
                    .checked_add(1)
                    .ok_or(EventStoreError::Invalid("recovery identifiers exhausted"))?;
            };
            let mut envelope: crate::store::EventEnvelopeWithoutSeqV1 = event.into();
            envelope.event_id = id;
            envelope.mono_ms = template.mono_ms;
            envelope.ts = None;
            envelope.payload = crate::redact::redact_event_payload(
                &crate::redact::DefaultRedactor::default(),
                envelope.payload,
            )?;
            journal.append(envelope)?;
        }
    }
    if clear_marker {
        fs::remove_file(&marker)?;
        #[cfg(unix)]
        if let Some(parent) = marker.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(journal)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecoveryApplyResult {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub applied: bool,
    pub before: PreviousCrashReport,
    pub after: PreviousCrashReport,
    pub recovered: bool,
    pub recovery_marker_cleared: bool,
    pub stale_lock_cleared: bool,
    pub events_log_present: bool,
}
pub fn apply_crash_recovery(
    session_dir: impl AsRef<Path>,
    run_id: impl AsRef<str>,
    deterministic: bool,
) -> Result<CrashRecoveryApplyResult, EventStoreError> {
    crate::store::validate_session_id(run_id.as_ref())?;
    let dir = session_dir.as_ref().join(run_id.as_ref());
    let before = inspect_previous_crash(&dir);
    let journal =
        recover_session_event_store(session_dir.as_ref(), run_id.as_ref(), deterministic)?;
    journal.close_writer()?;
    let after = inspect_previous_crash(&dir);
    Ok(CrashRecoveryApplyResult {
        run_id: run_id.as_ref().into(),
        run_dir: dir,
        applied: before.previous_crash_detected,
        recovered: before.previous_crash_detected && !after.previous_crash_detected,
        recovery_marker_cleared: before.recovery_marker_present && !after.recovery_marker_present,
        stale_lock_cleared: before.stale_writer_lock && !after.stale_writer_lock,
        events_log_present: after.events_log_present,
        before,
        after,
    })
}
