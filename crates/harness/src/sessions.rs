use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
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
            .expect("clap requires either --run or session selector")
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
    let summary = match inspect_session_recovery(&run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(stderr, "session reopen failed: {err}");
            return 1;
        }
    };

    if command.json {
        match serde_json::to_string_pretty(&summary) {
            Ok(body) => {
                let _ = writeln!(stdout, "{body}");
            }
            Err(err) => {
                let _ = writeln!(
                    stderr,
                    "session reopen failed: failed to serialize recovery summary: {err}"
                );
                return 1;
            }
        }
    } else {
        print_recovery_summary(&summary, stdout);
    }

    0
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

    if command.json {
        return write_json_output(
            &json!({
                "run_dir": session.run_dir.display().to_string(),
                "catalog": session.catalog,
                "replay": summary,
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
    print_human_summary(&summary, stdout);
    0
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
    use serde_json::json;

    #[allow(clippy::too_many_arguments)]
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
                run_id: "run-secret-export".to_string(),
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
        let output_dir = tempfile::tempdir().expect("tempdir");
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
        let output_dir = tempfile::tempdir().expect("tempdir");
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
    fn collect_list_entries_applies_filters_and_hides_non_operator_modes() {
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

        let rendered = render_json_session_list(&entries).expect("serialize json session list");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("parse rendered json session list");

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
}
