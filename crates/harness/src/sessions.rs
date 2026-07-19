// allow: SIZE_OK — sessions CLI command (list + inspect + export + tree + fork + clone + rename dispatchers)
use crate::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use harness_core::crash_recovery::{
    apply_crash_recovery, inspect_previous_crash, scan_previous_crashes, summarize_crash_reports,
    CrashRecoveryApplyResult, PreviousCrashReport,
};
use harness_core::foreign_session::{
    discover_foreign_sessions, import_foreign_session_as_replay, summarize_discover_candidates,
    ForeignSessionCandidate,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
#[cfg(test)]
use harness_core::redact::{DefaultRedactor, Redactor};
use serde::Serialize;
use serde_json::json;

use crate::defaults::DEFAULT_SESSION_DIR;
use crate::recovery::{inspect_session_recovery, resolve_session_run_dir, SessionRecoverySummary};
#[cfg(test)]
use crate::replay::ReplaySummary;
#[cfg(test)]
use crate::replay::SessionInspectionEntry;
use crate::replay::{
    inspect_session_catalog, print_human_summary, summarize_session, ReplayCommand,
};
use crate::tui::TuiCommand;
use crate::CliDeps;

mod export;
mod lineage;
mod list;
#[cfg(test)]
use export::{
    write_redacted_export_output_with_redactor, SessionExportBundle, SessionExportSupport,
};
pub use lineage::{CloneSessionCommand, ForkSessionCommand, TreeSessionCommand};
#[cfg(test)]
use list::{collect_list_entries, render_human_session_table, render_json_session_list};
pub use list::{SessionListSort, SessionStatusFilter};

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    /// List saved sessions with optional status/profile/resumable filters.
    List(SessionsListCommand),
    /// Inspect one saved session and summarize replay-derived state.
    Inspect(InspectSessionCommand),
    /// Reopen one session by resolving its run id to a stored run directory.
    Reopen(ReopenCommand),
    /// Replay one session by resolving its run id to a stored run directory.
    Replay(ReplaySessionCommand),
    /// Continue one resumable session in the live TUI.
    Continue(ContinueSessionCommand),
    /// Export one session as a JSON bundle for automation or archival.
    Export(ExportSessionCommand),
    /// Print the Harness session lineage tree.
    Tree(TreeSessionCommand),
    /// Create a Harness child session from an explicit stable cutoff.
    Fork(ForkSessionCommand),
    /// Create a Harness child session from the latest stable prefix.
    Clone(CloneSessionCommand),
    /// Import a foreign session as a new read-only replay session.
    Import(ImportSessionCommand),
    /// Discover foreign session candidates under a scan root (read-only).
    Discover(DiscoverSessionCommand),
    /// Scan session run directories for previous-crash markers (read-only).
    CrashScan(CrashScanSessionCommand),
}

#[derive(Debug, Args, Clone)]
pub struct ImportSessionCommand {
    /// Foreign session directory (must contain supported events.jsonl marker).
    #[arg(long = "from", value_name = "PATH")]
    pub from: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct DiscoverSessionCommand {
    /// Directory whose immediate children are scanned for foreign session markers.
    #[arg(long = "from", value_name = "PATH")]
    pub from: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct CrashScanSessionCommand {
    /// Sessions root whose immediate children are scanned for previous-crash markers.
    /// Defaults to the global/default session directory when omitted.
    #[arg(long = "from", value_name = "PATH")]
    pub from: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct SessionsListCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[arg(long, value_enum)]
    pub status: Option<SessionStatusFilter>,

    #[arg(long)]
    pub profile: Option<String>,

    #[arg(long)]
    pub resumable: Option<bool>,

    #[arg(long, value_enum, default_value_t = SessionListSort::UpdatedDesc)]
    pub sort: SessionListSort,
}

#[derive(Debug, Args, Clone)]
pub struct InspectSessionCommand {
    #[arg(long = "run", value_name = "RUN_ID", conflicts_with = "session")]
    pub run_id: Option<String>,

    #[arg(value_name = "RUN_ID_OR_PATH", required_unless_present = "run_id")]
    pub session: Option<String>,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

impl InspectSessionCommand {
    fn selector(&self) -> &str {
        self.session
            .as_deref()
            .or(self.run_id.as_deref())
            .unwrap_or_abort()
    }
}

#[derive(Debug, Args, Clone)]
pub struct ReopenCommand {
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ReplaySessionCommand {
    /// Run id or session directory name to replay.
    pub session: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ContinueSessionCommand {
    /// Run id or session directory name to continue in the live TUI.
    pub session: String,

    #[arg(long, default_value_t = false)]
    pub exit_on_finish: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ExportSessionCommand {
    /// Run id or session directory name to export.
    pub session: String,

    /// Optional file path for the exported JSON bundle. Writes to stdout when omitted.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ResolvedSession {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
}

pub fn execute_with_io(
    command: SessionsCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    deps: &CliDeps,
) -> i32 {
    match command {
        SessionsCommand::List(command) => {
            list::list_sessions(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Inspect(command) => {
            inspect_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Reopen(command) => {
            reopen_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Replay(command) => {
            replay_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Continue(command) => {
            continue_session(command, config_path, global_session_dir, stderr)
        }
        SessionsCommand::Export(command) => export::export_session(
            command,
            config_path,
            global_session_dir,
            stdout,
            stderr,
            deps,
        ),
        SessionsCommand::Tree(command) => {
            lineage::tree_sessions(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Fork(command) => {
            lineage::fork_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Clone(command) => {
            lineage::clone_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Import(command) => {
            import_session(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Discover(command) => discover_sessions(command, stdout, stderr),
        SessionsCommand::CrashScan(command) => {
            crash_scan_sessions(command, global_session_dir, stdout, stderr)
        }
    }
}

fn exit_code_to_i32(code: ExitCode) -> i32 {
    if code == ExitCode::SUCCESS {
        0
    } else {
        1
    }
}

fn reopen_session(
    command: ReopenCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            let _ = writeln!(stderr, "session reopen failed: {err}");
            return 1;
        }
    };

    let crash_recovery = apply_reopen_crash_recovery(&run_dir, stderr);
    let crash_recovery = match crash_recovery {
        Ok(result) => result,
        Err(code) => return code,
    };

    let summary = match inspect_session_recovery(&run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(stderr, "session reopen failed: {err}");
            return 1;
        }
    };

    if command.json {
        return write_json_output(
            &json!({
                "summary": summary,
                "crash_recovery": crash_recovery,
            }),
            None,
            stdout,
            stderr,
        );
    }

    if let Some(recovery) = &crash_recovery {
        let _ = writeln!(stdout, "crash_recovery: {}", recovery.one_line());
    }
    print_recovery_summary(&summary, stdout);

    0
}

fn apply_reopen_crash_recovery(
    run_dir: &std::path::Path,
    stderr: &mut dyn std::io::Write,
) -> Result<Option<CrashRecoveryApplyResult>, i32> {
    let before = inspect_previous_crash(run_dir);
    if !before.previous_crash_detected {
        return Ok(None);
    }
    let Some(parent) = run_dir.parent() else {
        let _ = writeln!(
            stderr,
            "session reopen failed: cannot resolve sessions parent for {}",
            run_dir.display()
        );
        return Err(1);
    };
    let Some(run_id) = run_dir.file_name().and_then(|name| name.to_str()) else {
        let _ = writeln!(
            stderr,
            "session reopen failed: cannot resolve run id for {}",
            run_dir.display()
        );
        return Err(1);
    };
    match apply_crash_recovery(parent, run_id, false) {
        Ok(result) => Ok(Some(result)),
        Err(err) => {
            let _ = writeln!(stderr, "session reopen failed: crash recovery: {err}");
            Err(1)
        }
    }
}

fn inspect_session(
    command: InspectSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let session = match command.run_id.as_deref() {
        Some(run_id) => resolve_session_by_run_id(&session_dir, run_id),
        None => resolve_session(&session_dir, command.selector()),
    };
    let session = match session {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "session inspect failed: {err}");
            return 1;
        }
    };

    let summary = match summarize_session(&session.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(stderr, "session inspect failed: {err}");
            return 1;
        }
    };
    let previous_crash = inspect_previous_crash(&session.run_dir);
    let recovery_action = previous_crash.previous_crash_detected.then(|| {
        harness_core::crash_recovery::resolve_crash_recovery_action(session.catalog.is_resumable)
    });
    let recovery_action_hint =
        recovery_action.map(|action| action.operator_hint(session.catalog.run_id.as_str()));

    if command.json {
        return write_json_output(
            &json!({
                "run_dir": session.run_dir.display().to_string(),
                "catalog": session.catalog,
                "replay": summary,
                "previous_crash_detected": previous_crash.previous_crash_detected,
                "recovery_message": previous_crash.recovery_message,
                "recovery_action": recovery_action.map(|action| action.as_str()),
                "recovery_action_hint": recovery_action_hint,
                "previous_crash": previous_crash.previous_crash_detected
                    .then_some(&previous_crash),
            }),
            None,
            stdout,
            stderr,
        );
    }

    let _ = writeln!(
        stdout,
        "mode: {}",
        mode_source_label(session.catalog.mode_source)
    );
    let _ = writeln!(
        stdout,
        "resume: {}",
        resumable_label(session.catalog.is_resumable)
    );
    if let Some(reason) = session.catalog.resume_disabled_reason.as_deref() {
        let _ = writeln!(stdout, "resume_reason: {reason}");
    }
    let _ = writeln!(
        stdout,
        "previous_crash_detected: {}",
        previous_crash.previous_crash_detected
    );
    if let Some(message) = previous_crash.recovery_message.as_deref() {
        let _ = writeln!(stdout, "recovery_message: {message}");
    }
    if let Some(action) = recovery_action {
        let _ = writeln!(stdout, "recovery_action: {}", action.as_str());
        if let Some(hint) = recovery_action_hint.as_deref() {
            let _ = writeln!(stdout, "recovery_action_hint: {hint}");
        }
    }
    print_human_summary(&summary, stdout);
    0
}

fn import_session(
    command: ImportSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    match import_foreign_session_as_replay(&command.from, &session_dir) {
        Ok(result) => {
            if command.json {
                return write_json_output(&result, None, stdout, stderr);
            }
            let _ = writeln!(
                stdout,
                "foreign session imported as replay-only harness session"
            );
            let _ = writeln!(stdout, "run_id: {}", result.run_id);
            let _ = writeln!(stdout, "run_dir: {}", result.run_dir.display());
            let _ = writeln!(stdout, "events: {}", result.event_count);
            let _ = writeln!(stdout, "format: {}", result.format);
            let _ = writeln!(stdout, "source_path: {}", result.source_path.display());
            let _ = writeln!(
                stdout,
                "next: harness sessions inspect {}  # then sessions replay {}",
                result.run_id, result.run_id
            );
            0
        }
        Err(err) => {
            let _ = writeln!(stderr, "session import failed: {err}");
            1
        }
    }
}

fn discover_sessions(
    command: DiscoverSessionCommand,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    match discover_foreign_sessions(&command.from) {
        Ok(candidates) => {
            if command.json {
                return write_json_output(
                    &json!({
                        "scan_root": command.from.display().to_string(),
                        "count": candidates.len(),
                        "candidates": candidates,
                    }),
                    None,
                    stdout,
                    stderr,
                );
            }
            if candidates.is_empty() {
                let _ = writeln!(
                    stdout,
                    "no foreign session candidates under {}",
                    command.from.display()
                );
                return 0;
            }
            let _ = writeln!(
                stdout,
                "foreign session candidates under {}:",
                command.from.display()
            );
            for candidate in &candidates {
                let _ = writeln!(stdout, "{}", format_discover_candidate(candidate));
            }
            let summary = summarize_discover_candidates(&candidates);
            let _ = writeln!(stdout, "summary: {}", summary.one_line());
            let _ = writeln!(
                stdout,
                "next: harness sessions import --from <path>  # events.jsonl marker only"
            );
            0
        }
        Err(err) => {
            let _ = writeln!(stderr, "session discover failed: {err}");
            1
        }
    }
}

fn crash_scan_sessions(
    command: CrashScanSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let scan_root = command
        .from
        .unwrap_or_else(|| session_dir(global_session_dir));
    if !scan_root.is_dir() {
        let _ = writeln!(
            stderr,
            "session crash-scan failed: scan root is not a directory: {}",
            scan_root.display()
        );
        return 1;
    }

    let reports = scan_previous_crashes(&scan_root);
    let summary = summarize_crash_reports(&reports);

    if command.json {
        return write_json_output(
            &json!({
                "scan_root": scan_root.display().to_string(),
                "summary": summary,
                "reports": reports,
            }),
            None,
            stdout,
            stderr,
        );
    }

    if reports.is_empty() {
        let _ = writeln!(
            stdout,
            "no session run directories under {}",
            scan_root.display()
        );
        return 0;
    }

    let _ = writeln!(stdout, "previous-crash scan under {}:", scan_root.display());
    for report in &reports {
        let _ = writeln!(stdout, "{}", format_crash_scan_report(report));
    }
    let _ = writeln!(stdout, "summary: {}", summary.one_line());
    if summary.has_previous_crash() {
        let _ = writeln!(
            stdout,
            "next: harness sessions inspect <run>  # then sessions reopen <run> when needed"
        );
    }
    0
}

fn format_crash_scan_report(report: &PreviousCrashReport) -> String {
    report.one_line()
}

fn format_discover_candidate(candidate: &ForeignSessionCandidate) -> String {
    match candidate {
        ForeignSessionCandidate::Discoverable { kind, path, marker } => {
            let importable = if candidate.is_importable() {
                "importable"
            } else {
                "not-importable-yet"
            };
            format!(
                "discoverable  kind={}  marker={}  import={}  path={}",
                kind.as_str(),
                marker,
                importable,
                path.display()
            )
        }
        ForeignSessionCandidate::Corrupt { kind, path, reason } => {
            format!(
                "corrupt       kind={}  path={}  reason={}",
                kind.as_str(),
                path.display(),
                reason
            )
        }
        ForeignSessionCandidate::Rejected { path, reason } => {
            format!("rejected      path={}  reason={}", path.display(), reason)
        }
    }
}

fn replay_session(
    command: ReplaySessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "{err}");
            return 1;
        }
    };

    crate::replay::execute_with_io(
        ReplayCommand {
            session: session.run_dir,
            json: command.json,
        },
        stdout,
        stderr,
    )
}

fn continue_session(
    command: ContinueSessionCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "{err}");
            return 1;
        }
    };

    exit_code_to_i32(crate::tui::execute(
        TuiCommand {
            replay: None,
            continue_session: Some(session.run_dir),
            scenario: None,
            mock: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: command.exit_on_finish,
            profile: None,
        },
        config_path,
        Some(session_dir),
    ))
}

fn session_dir(global_session_dir: Option<PathBuf>) -> PathBuf {
    global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR))
}

fn ensure_session_dir_exists(
    session_dir: &Path,
    stderr: &mut dyn std::io::Write,
) -> Result<(), i32> {
    if !session_dir.exists() {
        let _ = writeln!(
            stderr,
            "failed to read session directory {}: directory does not exist",
            session_dir.display()
        );
        return Err(1);
    }
    if let Err(err) = fs::read_dir(session_dir) {
        let _ = writeln!(
            stderr,
            "failed to read session directory {}: {err}",
            session_dir.display()
        );
        return Err(1);
    }
    Ok(())
}

fn resolve_session(session_dir: &Path, selector: &str) -> Result<ResolvedSession, String> {
    let matches = inspect_session_catalog(session_dir)?
        .into_iter()
        .filter(|entry| {
            entry.catalog.run_id == selector || run_dir_name(&entry.run_dir) == Some(selector)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(format!(
            "no session matched `{selector}` in {}",
            session_dir.display()
        )),
        [entry] => Ok(ResolvedSession {
            run_dir: entry.run_dir.clone(),
            catalog: entry.catalog.clone(),
        }),
        _ => Err(format!(
            "multiple sessions matched `{selector}` in {}; use a unique run id",
            session_dir.display()
        )),
    }
}

fn resolve_session_by_run_id(session_dir: &Path, run_id: &str) -> Result<ResolvedSession, String> {
    let matches = inspect_session_catalog(session_dir)?
        .into_iter()
        .filter(|entry| entry.catalog.run_id == run_id)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(format!(
            "no run with id `{run_id}` found in {}",
            session_dir.display()
        )),
        [entry] => Ok(ResolvedSession {
            run_dir: entry.run_dir.clone(),
            catalog: entry.catalog.clone(),
        }),
        _ => Err(format!(
            "run id `{run_id}` is ambiguous in {}",
            session_dir.display()
        )),
    }
}

fn resolve_session_for_write_or_tree(
    session_dir: &Path,
    selector: &str,
) -> Result<ResolvedSession, String> {
    let run_dir = resolve_session_run_dir(selector, session_dir)?;
    let catalog_dir = run_dir.parent().unwrap_or(session_dir);
    inspect_session_catalog(catalog_dir)?
        .into_iter()
        .find(|entry| entry.run_dir == run_dir)
        .map(|entry| ResolvedSession {
            run_dir: entry.run_dir,
            catalog: entry.catalog,
        })
        .ok_or_else(|| {
            format!(
                "failed to inspect session catalog entry for {}",
                run_dir.display()
            )
        })
}

fn run_dir_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn write_json_output<T: Serialize>(
    value: &T,
    output: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let body = match serde_json::to_string_pretty(value) {
        Ok(body) => body,
        Err(err) => {
            let _ = writeln!(stderr, "failed to serialize session report: {err}");
            return 1;
        }
    };

    if let Some(path) = output {
        if let Err(err) = fs::write(&path, format!("{body}\n")) {
            let _ = writeln!(stderr, "failed to write {}: {err}", path.display());
            return 1;
        }
    } else {
        let _ = writeln!(stdout, "{body}");
    }

    0
}

fn resumable_label(is_resumable: bool) -> &'static str {
    if is_resumable {
        "yes"
    } else {
        "no"
    }
}

fn status_label(status: Option<RunStatus>) -> &'static str {
    match status {
        Some(RunStatus::Running) => "running",
        Some(RunStatus::Finished) => "finished",
        Some(RunStatus::Failed) => "failed",
        None => "<unavailable>",
    }
}

fn mode_source_label(mode_source: SessionModeSource) -> &'static str {
    match mode_source {
        SessionModeSource::InteractiveLive => "interactive_live",
        SessionModeSource::InteractiveMock => "interactive_mock",
        SessionModeSource::Prompt => "prompt",
        SessionModeSource::ScenarioFixture => "scenario_fixture",
        SessionModeSource::ReplayOnly => "replay_only",
        SessionModeSource::Unknown => "<unavailable>",
    }
}

fn print_recovery_summary(summary: &SessionRecoverySummary, out: &mut dyn std::io::Write) {
    macro_rules! line {
        ($($arg:tt)*) => {
            let _ = writeln!(out, $($arg)*);
        };
    }
    line!("run_id: {}", summary.run_id);
    line!(
        "run_name: {}",
        summary.run_name.as_deref().unwrap_or("<unknown>")
    );
    line!("run_dir: {}", summary.run_dir.display());
    line!(
        "workspace_root: {}",
        summary.workspace_root.as_deref().unwrap_or("<unknown>")
    );
    line!("status: {}", status_label(summary.status));
    line!(
        "profile: {}",
        summary.profile.as_deref().unwrap_or("<unavailable>")
    );
    line!(
        "provider/model: {}",
        summary.provider_model.as_deref().unwrap_or("<unavailable>")
    );
    line!("mode: {}", mode_source_label(summary.mode));
    line!("resumable: {}", resumable_label(summary.resumable));
    if let Some(reason) = &summary.resume_disabled_reason {
        line!("resume_disabled_reason: {reason}");
    }
    if let Some(agent_id) = &summary.resume_agent_id {
        line!("resume_agent_id: {agent_id}");
    }
    if let Some(last_error) = &summary.last_error {
        line!("last_error: {last_error}");
    }
    if let Some(continue_hint) = &summary.continue_hint {
        line!("continue_hint: {continue_hint}");
    }
    line!(
        "previous_crash_detected: {}",
        summary.previous_crash_detected
    );
    if let Some(message) = &summary.recovery_message {
        line!("recovery_message: {message}");
    }
    if let Some(action) = summary.recovery_action {
        line!("recovery_action: {}", action.as_str());
    }
    if let Some(hint) = &summary.recovery_action_hint {
        line!("recovery_action_hint: {hint}");
    }
    line!("total_events: {}", summary.total_events);

    if !summary.pending_permissions.is_empty() {
        line!("pending_permissions:");
        for permission_id in &summary.pending_permissions {
            line!("  - {permission_id}");
        }
    }
    if !summary.tasks_in_flight.is_empty() {
        line!("tasks_in_flight:");
        for task_id in &summary.tasks_in_flight {
            line!("  - {task_id}");
        }
    }
    if !summary.prompt_context.is_empty() {
        line!("prompt_context:");
        for entry in &summary.prompt_context {
            line!(
                "  - seq={} kind={} request={} agent={} text={}",
                entry.seq,
                entry.kind,
                entry.request_id.as_deref().unwrap_or("<unknown>"),
                entry.agent_id.as_deref().unwrap_or("<unknown>"),
                entry.text
            );
        }
    }
    if !summary.child_sessions.is_empty() {
        line!("child_sessions:");
        for child in &summary.child_sessions {
            line!(
                "  - {} profile={} provider/model={} child_request={} state={} elapsed_ms={}",
                child.agent_id,
                child.profile.as_deref().unwrap_or("<unavailable>"),
                child.provider_model.as_deref().unwrap_or("<unavailable>"),
                child
                    .latest_child_request_id
                    .as_deref()
                    .unwrap_or("<unavailable>"),
                child.terminal_state.as_deref().unwrap_or("<unavailable>"),
                child
                    .elapsed_ms
                    .map(|value| value.to_string())
                    .as_deref()
                    .unwrap_or("<unavailable>")
            );
        }
    }
    if !summary.artifacts.is_empty() {
        line!("artifacts:");
        for artifact in &summary.artifacts {
            line!(
                "  - tool_call={} tool={} kind={} path={} digest={}",
                artifact.tool_call_id.as_deref().unwrap_or("<none>"),
                artifact.tool_id.as_deref().unwrap_or("<unavailable>"),
                artifact.kind.as_deref().unwrap_or("<unavailable>"),
                artifact.path,
                artifact.digest.as_deref().unwrap_or("<unavailable>")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use serde_json::json;

    fn sample_entry(
        run_id: &str,
        sort_unix_ms: u128,
        status: Option<RunStatus>,
        profile_preset: Option<&str>,
        mode_source: SessionModeSource,
        is_resumable: bool,
        artifact_count: usize,
        child_session_count: usize,
        parent_session_id: Option<&str>,
    ) -> SessionInspectionEntry {
        SessionInspectionEntry {
            run_dir: PathBuf::from(format!("/tmp/{run_id}")),
            catalog: SessionCatalogEntry {
                run_id: run_id.to_string(),
                run_name: Some(format!("{run_id}-name")),
                status,
                last_updated_at: Some(sort_unix_ms.to_string()),
                workspace_root: Some(format!("/workspaces/{run_id}")),
                profile_preset: profile_preset.map(str::to_string),
                provider_model: Some("openai/gpt-5.4".to_string()),
                mode_source,
                is_resumable,
                resume_disabled_reason: (!is_resumable).then(|| "resume blocked".to_string()),
                artifact_count,
                child_session_count,
                parent_session_id: parent_session_id.map(str::to_string),
            },
            sort_unix_ms,
            artifact_count,
            child_session_count,
        }
    }

    fn minimal_export_bundle_with_secret(secret: &str) -> SessionExportBundle {
        let catalog = sample_entry(
            "run-secret-export",
            1,
            Some(RunStatus::Finished),
            Some("build"),
            SessionModeSource::Prompt,
            false,
            0,
            0,
            None,
        )
        .catalog;
        let mut bundle = SessionExportBundle {
            run_dir: PathBuf::from("/tmp/run-secret-export"),
            catalog,
            metadata: None,
            replay: ReplaySummary {
                run_id: "run-secret-export".into(),
                run_name: Some("secret-export".to_string()),
                session_path: PathBuf::from("/tmp/run-secret-export"),
                status: RunStatus::Finished,
                workspace_root: Some("/tmp/workspace".to_string()),
                mode_source: SessionModeSource::Prompt,
                is_resumable: false,
                resume_disabled_reason: None,
                artifact_count: 0,
                child_session_count: 0,
                parent_session_id: None,
                total_events: 0,
                counts_by_type: std::collections::BTreeMap::new(),
                pending_permissions: Vec::new(),
                tasks_in_flight: Vec::new(),
                last_error: None,
                artifacts: Vec::new(),
                child_sessions: Vec::new(),
            },
            support: SessionExportSupport {
                doctor_json: json!({}),
                config_summary: json!({}),
                provider_summary: json!({}),
                agent_catalog_summary: json!({}),
                skill_catalog_summary: json!({}),
                native_tool_catalog_summary: json!({}),
                session_tool_readiness: json!({}),
                credential_store_manifest: json!({}),
                route_metadata: Vec::new(),
                artifact_index: Vec::new(),
            },
            events: Vec::new(),
        };
        bundle.support.doctor_json = json!({ "leaked": secret });
        bundle
    }

    struct NoopRedactor;

    impl Redactor for NoopRedactor {
        fn redact_text(&self, s: &str) -> String {
            s.to_string()
        }
    }

    #[test]
    fn support_export_fails_closed_when_redaction_scan_finds_secret() {
        // arrange
        let export = minimal_export_bundle_with_secret("sk-proj-leaked_0123456789abcdef");
        let output_dir = tempfile::tempdir().unwrap_or_abort();
        let output_path = output_dir.path().join("support.json");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // act
        let code = write_redacted_export_output_with_redactor(
            &export,
            Some(output_path.clone()),
            &mut stdout,
            &mut stderr,
            &NoopRedactor,
            &DefaultRedactor::default(),
            &[],
        );

        // assert
        assert_eq!(code, 1);
        assert!(!output_path.exists());
        assert!(stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&stderr).contains("redaction scanner found"),
            "stderr should explain fail-closed redaction: {}",
            String::from_utf8_lossy(&stderr)
        );
    }

    #[test]
    fn support_export_fails_closed_when_env_credential_value_survives_redaction() {
        // arrange
        let export = minimal_export_bundle_with_secret("plain-env-secret-value");
        let output_dir = tempfile::tempdir().unwrap_or_abort();
        let output_path = output_dir.path().join("support.json");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let env_values = vec!["plain-env-secret-value".to_string()];

        // act
        let code = write_redacted_export_output_with_redactor(
            &export,
            Some(output_path.clone()),
            &mut stdout,
            &mut stderr,
            &NoopRedactor,
            &DefaultRedactor::default(),
            &env_values,
        );

        // assert
        assert_eq!(code, 1);
        assert!(!output_path.exists());
        assert!(stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&stderr).contains("redaction scanner found"),
            "stderr should explain env credential fail-closed scan: {}",
            String::from_utf8_lossy(&stderr)
        );
    }

    #[test]
    fn support_export_omits_provider_reasoning_delta_events() {
        // arrange
        let raw_reasoning = "raw hidden reasoning text must not leave local events";
        let mut export = minimal_export_bundle_with_secret("non-secret-placeholder");
        export.support.doctor_json = json!({});
        export.replay.total_events = 1;
        export
            .replay
            .counts_by_type
            .insert("provider_reasoning_delta".to_string(), 1);
        export.events.push(harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: "event-reasoning".to_string(),
            seq: 1,
            run_id: "run-secret-export".into(),
            mono_ms: 1,
            ts: None,
            actor: harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                None,
            ),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: harness_core::event::EventV1::ProviderReasoningDelta(
                harness_core::event::ProviderReasoningDeltaEvent {
                    request_id: "provider-request-1".into(),
                    delta: raw_reasoning.to_string(),
                },
            ),
        });
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // act
        let code = write_redacted_export_output_with_redactor(
            &export,
            None,
            &mut stdout,
            &mut stderr,
            &NoopRedactor,
            &DefaultRedactor::default(),
            &[],
        );

        // assert
        assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
        assert!(stderr.is_empty());
        let rendered = String::from_utf8(stdout).unwrap_or_abort();
        assert!(!rendered.contains(raw_reasoning));
        assert!(!rendered.contains("provider_reasoning_delta"));
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_abort();
        assert_eq!(parsed["events"].as_array().map(Vec::len), Some(0));
        assert_eq!(parsed["replay"]["total_events"], 0);
        assert_eq!(parsed["replay"]["counts_by_type"], json!({}));
    }

    #[test]
    fn collect_list_entries_applies_filters_and_hides_non_operator_modes() {
        // arrange
        // act
        // assert
        let entries = vec![
            sample_entry(
                "run-running",
                30,
                Some(RunStatus::Running),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
                0,
                0,
                None,
            ),
            sample_entry(
                "run-finished",
                20,
                Some(RunStatus::Finished),
                Some("reviewer"),
                SessionModeSource::Prompt,
                false,
                0,
                0,
                None,
            ),
            sample_entry(
                "fixture-hidden",
                10,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::ScenarioFixture,
                true,
                0,
                0,
                None,
            ),
        ];

        let command = SessionsListCommand {
            status: Some(SessionStatusFilter::Running),
            profile: Some("worker".to_string()),
            resumable: Some(true),
            ..SessionsListCommand::default()
        };

        let filtered = collect_list_entries(entries, &command);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].catalog.run_id, "run-running");
    }

    #[test]
    fn collect_list_entries_supports_machine_sorting() {
        // arrange
        // act
        // assert
        let entries = vec![
            sample_entry(
                "run-b",
                20,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
                0,
                0,
                None,
            ),
            sample_entry(
                "run-c",
                10,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
                0,
                0,
                None,
            ),
            sample_entry(
                "run-a",
                30,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
                0,
                0,
                None,
            ),
        ];

        let command = SessionsListCommand {
            sort: SessionListSort::RunIdAsc,
            ..SessionsListCommand::default()
        };

        let filtered = collect_list_entries(entries, &command);
        let run_ids = filtered
            .iter()
            .map(|entry| entry.catalog.run_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(run_ids, vec!["run-a", "run-b", "run-c"]);
    }

    #[test]
    fn render_json_session_list_emits_machine_readable_fields() {
        // arrange
        // act
        // assert
        let entries = vec![sample_entry(
            "run-json",
            42,
            Some(RunStatus::Running),
            Some("worker"),
            SessionModeSource::InteractiveMock,
            true,
            2,
            1,
            Some("run-parent"),
        )];

        let rendered = render_json_session_list(&entries).unwrap_or_abort();
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_abort();

        assert_eq!(
            parsed,
            json!([
                {
                    "run_dir": "/tmp/run-json",
                    "run_id": "run-json",
                    "run_name": "run-json-name",
                    "status": "running",
                    "last_updated_at": "42",
                    "workspace_root": "/workspaces/run-json",
                    "profile_preset": "worker",
                    "provider_model": "openai/gpt-5.4",
                    "mode_source": "interactive_mock",
                    "is_resumable": true,
                    "resume_disabled_reason": null,
                    "artifact_count": 2,
                    "child_session_count": 1,
                    "parent_session_id": "run-parent"
                }
            ])
        );
    }

    #[test]
    fn render_human_session_table_keeps_operator_facing_default() {
        // arrange
        // act
        // assert
        let entries = vec![sample_entry(
            "run-human",
            42,
            None,
            None,
            SessionModeSource::Unknown,
            false,
            3,
            2,
            Some("run-parent"),
        )];

        let rendered = render_human_session_table(&entries);

        assert!(rendered.contains("run_id"));
        assert!(rendered.contains("artifacts"));
        assert!(rendered.contains("children"));
        assert!(rendered.contains("session_path"));
        assert!(rendered.contains("parent"));
        assert!(rendered.contains("run-human"));
        assert!(rendered.contains("<unavailable>"));
        assert!(rendered.contains("resume blocked"));
        assert!(rendered.contains("/tmp/run-human"));
        assert!(rendered.contains("run-parent"));
    }

    #[test]
    fn render_human_session_table_surfaces_meaningful_session_title() {
        // arrange
        let mut entry = sample_entry(
            "run-title",
            42,
            Some(RunStatus::Finished),
            Some("build"),
            SessionModeSource::InteractiveLive,
            true,
            0,
            0,
            None,
        );
        entry.catalog.run_name = Some("map chat renderers".to_string());

        // act
        let rendered = render_human_session_table(&[entry]);

        // assert
        assert!(rendered.contains("run_name"));
        assert!(rendered.contains("map chat renderers"));
        assert!(
            !rendered.contains("<unavailable>"),
            "a titled session should not degrade to only run id/path: {rendered}"
        );
    }

    #[test]
    fn discover_sessions_json_lists_discoverable_foreign_candidate() {
        // arrange: immediate child with a valid Codex session.json marker
        let root = tempfile::tempdir().unwrap();
        let foreign = root.path().join("codex-session");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(
            foreign.join("session.json"),
            r#"{"id":"abc","title":"demo"}"#,
        )
        .unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // act
        let code = discover_sessions(
            DiscoverSessionCommand {
                from: root.path().to_path_buf(),
                json: true,
            },
            &mut stdout,
            &mut stderr,
        );

        // assert
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        assert!(
            stderr.is_empty(),
            "stderr={}",
            String::from_utf8_lossy(&stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&stdout).expect("discover json stdout");
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["candidates"][0]["status"], "discoverable");
        assert_eq!(parsed["candidates"][0]["kind"], "codex");
        assert_eq!(parsed["candidates"][0]["marker"], "session.json");
        assert_eq!(
            parsed["candidates"][0]["path"].as_str().unwrap(),
            foreign.display().to_string()
        );
    }

    #[test]
    fn discover_sessions_human_summarizes_empty_scan_root() {
        // arrange: empty directory under scan root
        let root = tempfile::tempdir().unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // act
        let code = discover_sessions(
            DiscoverSessionCommand {
                from: root.path().to_path_buf(),
                json: false,
            },
            &mut stdout,
            &mut stderr,
        );

        // assert
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        let rendered = String::from_utf8_lossy(&stdout);
        assert!(
            rendered.contains("no foreign session candidates under"),
            "unexpected human output: {rendered}"
        );
    }

    #[test]
    fn execute_with_io_dispatches_discover_command() {
        // arrange
        let root = tempfile::tempdir().unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // act
        let code = execute_with_io(
            SessionsCommand::Discover(DiscoverSessionCommand {
                from: root.path().to_path_buf(),
                json: false,
            }),
            None,
            None,
            &mut stdout,
            &mut stderr,
            &CliDeps::default(),
        );

        // assert
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        let rendered = String::from_utf8_lossy(&stdout);
        assert!(
            rendered.contains("no foreign session candidates under"),
            "unexpected dispatch output: {rendered}"
        );
    }

    #[test]
    fn crash_scan_sessions_json_summarizes_mixed_session_root() {
        // arrange
        // act
        // assert
        // Given: sessions root with one clean run and one recovery-marker crash
        let root = tempfile::tempdir().unwrap();
        let clean = root.path().join("run_clean");
        let crashed = root.path().join("run_crashed");
        std::fs::create_dir_all(&clean).unwrap();
        std::fs::create_dir_all(&crashed).unwrap();
        std::fs::write(clean.join("events.jsonl"), "").unwrap();
        std::fs::write(crashed.join("events.jsonl"), "").unwrap();
        std::fs::write(crashed.join(".writer.lock.recovering"), "pid=1\n").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // When
        let code = crash_scan_sessions(
            CrashScanSessionCommand {
                from: Some(root.path().to_path_buf()),
                json: true,
            },
            None,
            &mut stdout,
            &mut stderr,
        );

        // Then
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        assert!(
            stderr.is_empty(),
            "stderr={}",
            String::from_utf8_lossy(&stderr)
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&stdout).expect("crash-scan json stdout");
        assert_eq!(parsed["summary"]["scanned"], 2);
        assert_eq!(parsed["summary"]["previous_crash"], 1);
        assert_eq!(parsed["summary"]["clean"], 1);
        assert_eq!(parsed["summary"]["recovery_marker"], 1);
        assert_eq!(parsed["reports"].as_array().map(|rows| rows.len()), Some(2));
    }

    #[test]
    fn reopen_session_applies_crash_recovery_for_marker_then_summarizes() {
        // arrange
        // act
        // assert
        // Given: finished prompt session with recovery marker under sessions root
        use harness_core::event::{
            ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
            SCHEMA_VERSION,
        };

        let sessions = tempfile::tempdir().unwrap();
        let run_id = "run_reopen_recover";
        let run_dir = sessions.path().join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let events = [
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
                    run_name: "prompt".into(),
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
        let mut body = String::new();
        for event in &events {
            body.push_str(&serde_json::to_string(event).unwrap());
            body.push('\n');
        }
        std::fs::write(run_dir.join("events.jsonl"), body).unwrap();
        std::fs::write(run_dir.join(".writer.lock.recovering"), "pid=1\n").unwrap();
        assert!(run_dir.join(".writer.lock.recovering").exists());

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // When: reopen applies exclusive-open recovery then prints summary
        let code = reopen_session(
            ReopenCommand {
                session: run_id.to_string(),
                json: true,
            },
            Some(sessions.path().to_path_buf()),
            &mut stdout,
            &mut stderr,
        );

        // Then: recovery applied + marker cleared + summary present
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        let parsed: serde_json::Value =
            serde_json::from_slice(&stdout).expect("reopen json stdout");
        assert_eq!(parsed["crash_recovery"]["applied"], true);
        assert_eq!(parsed["crash_recovery"]["recovered"], true);
        assert_eq!(parsed["crash_recovery"]["recovery_marker_cleared"], true);
        assert_eq!(parsed["summary"]["run_id"], run_id);
        assert!(!run_dir.join(".writer.lock.recovering").exists());
        assert!(!parsed["summary"]["previous_crash_detected"]
            .as_bool()
            .unwrap_or(true));
    }

    #[test]
    fn crash_scan_sessions_human_lists_previous_crash_and_summary() {
        // arrange
        // act
        // assert
        // Given: sessions root with recovery marker
        let root = tempfile::tempdir().unwrap();
        let crashed = root.path().join("run_crashed");
        std::fs::create_dir_all(&crashed).unwrap();
        std::fs::write(crashed.join("events.jsonl"), "").unwrap();
        std::fs::write(crashed.join(".writer.lock.recovering"), "pid=1\n").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // When
        let code = crash_scan_sessions(
            CrashScanSessionCommand {
                from: Some(root.path().to_path_buf()),
                json: false,
            },
            None,
            &mut stdout,
            &mut stderr,
        );

        // Then
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        let rendered = String::from_utf8_lossy(&stdout);
        assert!(
            rendered.contains("previous-crash scan under"),
            "unexpected human output: {rendered}"
        );
        assert!(
            rendered.contains("previous-crash:"),
            "expected previous-crash line: {rendered}"
        );
        assert!(
            rendered.contains("summary: crash scan:"),
            "expected summary line: {rendered}"
        );
        assert!(
            rendered.contains("next: harness sessions inspect"),
            "expected next-step guidance: {rendered}"
        );
    }

    #[test]
    fn execute_with_io_dispatches_crash_scan_command() {
        // arrange
        // act
        // assert
        // Given: empty sessions root
        let root = tempfile::tempdir().unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // When
        let code = execute_with_io(
            SessionsCommand::CrashScan(CrashScanSessionCommand {
                from: Some(root.path().to_path_buf()),
                json: false,
            }),
            None,
            None,
            &mut stdout,
            &mut stderr,
            &CliDeps::default(),
        );

        // Then
        assert_eq!(code, 0, "stderr={}", String::from_utf8_lossy(&stderr));
        let rendered = String::from_utf8_lossy(&stdout);
        assert!(
            rendered.contains("no session run directories under"),
            "unexpected dispatch output: {rendered}"
        );
    }
}
