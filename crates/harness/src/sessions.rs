use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use harness_core::proj::{RunMetadata, RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::load_events_from_run_dir;
use serde::Serialize;

use crate::replay::{inspect_session_catalog, summarize_session, ReplayCommand, ReplaySummary};
use crate::tui::TuiCommand;

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    /// List known session runs from the configured session directory.
    List,
    /// Inspect one session by run id or directory name.
    Inspect(InspectSessionCommand),
    /// Replay one session by resolving its run id to a stored run directory.
    Replay(ReplaySessionCommand),
    /// Continue one resumable session in the live TUI.
    Continue(ContinueSessionCommand),
    /// Export one session as a JSON bundle for automation or archival.
    Export(ExportSessionCommand),
}

#[derive(Debug, Args, Clone)]
pub struct InspectSessionCommand {
    /// Run id or session directory name to inspect.
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

#[derive(Debug, Clone, Serialize)]
struct SessionInspectReport {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
    metadata: Option<RunMetadata>,
    replay: Option<ReplaySummary>,
    replay_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionExportBundle {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
    metadata: Option<RunMetadata>,
    replay: ReplaySummary,
    events: Vec<harness_core::event::EventEnvelopeV1>,
}

pub fn execute(
    command: SessionsCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    match command {
        SessionsCommand::List => list_sessions(global_session_dir),
        SessionsCommand::Inspect(command) => inspect_session(command, global_session_dir),
        SessionsCommand::Replay(command) => replay_session(command, global_session_dir),
        SessionsCommand::Continue(command) => {
            continue_session(command, config_path, global_session_dir)
        }
        SessionsCommand::Export(command) => export_session(command, global_session_dir),
    }
}

fn list_sessions(global_session_dir: Option<PathBuf>) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir) {
        return code;
    }

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    println!(
        "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<5} reason",
        "run_id", "status", "run_name", "profile", "provider/model", "mode", "resume"
    );

    for entry in entries {
        if matches!(
            entry.catalog.mode_source,
            SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
        ) {
            continue;
        }

        println!(
            "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<5} {}",
            entry.catalog.run_id,
            status_label(entry.catalog.status),
            entry.catalog.run_name.as_deref().unwrap_or("<unavailable>"),
            entry
                .catalog
                .profile_preset
                .as_deref()
                .unwrap_or("<unavailable>"),
            entry
                .catalog
                .provider_model
                .as_deref()
                .unwrap_or("<unavailable>"),
            mode_source_label(entry.catalog.mode_source),
            resumable_label(entry.catalog.is_resumable),
            entry
                .catalog
                .resume_disabled_reason
                .as_deref()
                .unwrap_or("-")
        );
    }

    ExitCode::SUCCESS
}

fn inspect_session(
    command: InspectSessionCommand,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    let report = inspect_report(&session);
    if command.json {
        return write_json_output(&report, None);
    }

    print_inspect_report(&report);
    ExitCode::SUCCESS
}

fn replay_session(command: ReplaySessionCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    crate::replay::execute(ReplayCommand {
        session: session.run_dir,
        json: command.json,
    })
}

fn continue_session(
    command: ContinueSessionCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    crate::tui::execute(
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
    )
}

fn export_session(command: ExportSessionCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    let events = match load_events_from_run_dir(&session.run_dir) {
        Ok(events) => events,
        Err(err) => {
            eprintln!("failed to export session {}: {err}", session.catalog.run_id);
            return ExitCode::from(1);
        }
    };
    let replay = match summarize_session(&session.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("failed to export session {}: {err}", session.catalog.run_id);
            return ExitCode::from(1);
        }
    };

    let run_dir = session.run_dir;
    let metadata = load_run_metadata(&run_dir);
    let export = SessionExportBundle {
        run_dir,
        catalog: session.catalog,
        metadata,
        replay,
        events,
    };

    write_json_output(&export, command.output)
}

fn session_dir(global_session_dir: Option<PathBuf>) -> PathBuf {
    global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR))
}

fn ensure_session_dir_exists(session_dir: &Path) -> Result<(), ExitCode> {
    if !session_dir.exists() {
        eprintln!(
            "failed to read session directory {}: directory does not exist",
            session_dir.display()
        );
        return Err(ExitCode::from(1));
    }
    if let Err(err) = fs::read_dir(session_dir) {
        eprintln!(
            "failed to read session directory {}: {err}",
            session_dir.display()
        );
        return Err(ExitCode::from(1));
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

fn run_dir_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn inspect_report(session: &ResolvedSession) -> SessionInspectReport {
    match summarize_session(&session.run_dir) {
        Ok(replay) => SessionInspectReport {
            run_dir: session.run_dir.clone(),
            catalog: session.catalog.clone(),
            metadata: load_run_metadata(&session.run_dir),
            replay: Some(replay),
            replay_error: None,
        },
        Err(err) => SessionInspectReport {
            run_dir: session.run_dir.clone(),
            catalog: session.catalog.clone(),
            metadata: load_run_metadata(&session.run_dir),
            replay: None,
            replay_error: Some(err),
        },
    }
}

fn load_run_metadata(run_dir: &Path) -> Option<RunMetadata> {
    let meta_path = run_dir.join("meta.json");
    let body = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&body).ok()
}

fn print_inspect_report(report: &SessionInspectReport) {
    println!("run_id: {}", report.catalog.run_id);
    println!("run_dir: {}", report.run_dir.display());
    println!(
        "run_name: {}",
        report
            .catalog
            .run_name
            .as_deref()
            .unwrap_or("<unavailable>")
    );
    println!("status: {}", status_label(report.catalog.status));
    println!("mode: {}", mode_source_label(report.catalog.mode_source));
    println!(
        "resumable: {}",
        resumable_label(report.catalog.is_resumable)
    );
    println!(
        "resume_reason: {}",
        report
            .catalog
            .resume_disabled_reason
            .as_deref()
            .unwrap_or("-")
    );
    println!(
        "workspace_root: {}",
        report
            .catalog
            .workspace_root
            .as_deref()
            .unwrap_or("<unavailable>")
    );
    println!(
        "profile: {}",
        report
            .catalog
            .profile_preset
            .as_deref()
            .unwrap_or("<unavailable>")
    );
    println!(
        "provider_model: {}",
        report
            .catalog
            .provider_model
            .as_deref()
            .unwrap_or("<unavailable>")
    );
    println!(
        "last_updated_at: {}",
        report
            .catalog
            .last_updated_at
            .as_deref()
            .unwrap_or("<unavailable>")
    );
    if let Some(metadata) = &report.metadata {
        println!("recorded_run_name: {}", metadata.run_name);
        println!("harness_version: {}", metadata.harness_version);
        println!("config_digest: {}", metadata.config_digest);
    }
    if let Some(replay) = &report.replay {
        println!("total_events: {}", replay.total_events);
        println!("pending_permissions: {}", replay.pending_permissions.len());
        println!("tasks_in_flight: {}", replay.tasks_in_flight.len());
        if let Some(last_error) = &replay.last_error {
            println!("last_error: {last_error}");
        }
    }
    if let Some(replay_error) = &report.replay_error {
        println!("replay_error: {replay_error}");
    }
}

fn write_json_output<T: Serialize>(value: &T, output: Option<PathBuf>) -> ExitCode {
    let body = match serde_json::to_string_pretty(value) {
        Ok(body) => body,
        Err(err) => {
            eprintln!("failed to serialize session report: {err}");
            return ExitCode::from(1);
        }
    };

    if let Some(path) = output {
        if let Err(err) = fs::write(&path, format!("{body}\n")) {
            eprintln!("failed to write {}: {err}", path.display());
            return ExitCode::from(1);
        }
    } else {
        println!("{body}");
    }

    ExitCode::SUCCESS
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
