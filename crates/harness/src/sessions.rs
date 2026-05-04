use std::cmp::Reverse;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use harness_core::proj::{RunMetadata, RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_core::session_lineage::{
    latest_clone_stable_prefix, materialize_child_session, project_lineage_tree,
    validate_fork_stable_prefix, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind, SessionLineageNode, StableSessionPrefix,
};
use harness_tui::load_events_from_run_dir;
use serde::Serialize;
use serde_json::json;

use crate::recovery::{inspect_session_recovery, resolve_session_run_dir, SessionRecoverySummary};
use crate::replay::{
    inspect_session_catalog, print_human_summary, summarize_session, ReplayCommand, ReplaySummary,
    SessionInspectionEntry,
};
use crate::tui::TuiCommand;

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    List(SessionsListCommand),
    Inspect(InspectSessionCommand),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SessionStatusFilter {
    Running,
    Finished,
    Failed,
    Unavailable,
}

impl SessionStatusFilter {
    fn matches(self, status: Option<RunStatus>) -> bool {
        matches!(
            (self, status),
            (Self::Running, Some(RunStatus::Running))
                | (Self::Finished, Some(RunStatus::Finished))
                | (Self::Failed, Some(RunStatus::Failed))
                | (Self::Unavailable, None)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SessionListSort {
    #[default]
    #[value(name = "updated_desc")]
    UpdatedDesc,
    #[value(name = "updated_asc")]
    UpdatedAsc,
    #[value(name = "run_id_asc")]
    RunIdAsc,
    #[value(name = "run_id_desc")]
    RunIdDesc,
}

impl SessionListSort {
    fn sort_entries(self, entries: &mut [SessionInspectionEntry]) {
        match self {
            Self::UpdatedDesc => {
                entries.sort_by_key(|entry| (Reverse(entry.sort_unix_ms), entry.run_dir.clone()));
            }
            Self::UpdatedAsc => {
                entries.sort_by_key(|entry| (entry.sort_unix_ms, entry.run_dir.clone()));
            }
            Self::RunIdAsc => {
                entries.sort_by(|left, right| {
                    left.catalog
                        .run_id
                        .cmp(&right.catalog.run_id)
                        .then_with(|| left.run_dir.cmp(&right.run_dir))
                });
            }
            Self::RunIdDesc => {
                entries.sort_by(|left, right| {
                    right
                        .catalog
                        .run_id
                        .cmp(&left.catalog.run_id)
                        .then_with(|| right.run_dir.cmp(&left.run_dir))
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SessionListJsonEntry {
    run_dir: PathBuf,
    #[serde(flatten)]
    catalog: SessionCatalogEntry,
}

impl From<&SessionInspectionEntry> for SessionListJsonEntry {
    fn from(entry: &SessionInspectionEntry) -> Self {
        Self {
            run_dir: entry.run_dir.clone(),
            catalog: entry.catalog.clone(),
        }
    }
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

#[derive(Debug, Args, Clone)]
pub struct TreeSessionCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub root: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub filter: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct ForkSessionCommand {
    /// Run id or session directory path to fork from.
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub source: String,

    /// Stable event sequence cutoff to copy into the child session.
    #[arg(long, value_name = "SEQ")]
    pub cutoff: u64,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct CloneSessionCommand {
    /// Run id or session directory path to clone from.
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub source: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone)]
struct ResolvedSession {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
}

#[derive(Debug, Clone, Serialize)]
struct SessionExportBundle {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
    metadata: Option<RunMetadata>,
    replay: ReplaySummary,
    events: Vec<harness_core::event::EventEnvelopeV1>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionTreeJson {
    harness_lineage: Vec<SessionTreeJsonRow>,
    session_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionTreeJsonRow {
    depth: usize,
    run_dir: PathBuf,
    #[serde(flatten)]
    catalog: SessionCatalogEntry,
}

#[derive(Debug, Clone)]
struct SessionTreeRow {
    depth: usize,
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
}

#[derive(Debug, Clone, Serialize)]
struct ChildSessionWriteReport {
    harness_operation: &'static str,
    child_run_id: String,
    child_run_dir: PathBuf,
    source_run_id: Option<String>,
    source_run_dir: PathBuf,
    source_cutoff_seq: u64,
    event_count: usize,
    artifact_count: usize,
    warnings: Vec<String>,
    errors: Vec<String>,
}

pub fn execute(
    command: SessionsCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    match command {
        SessionsCommand::List(command) => list_sessions(command, global_session_dir),
        SessionsCommand::Inspect(command) => inspect_session(command, global_session_dir),
        SessionsCommand::Reopen(command) => reopen_session(command, global_session_dir),
        SessionsCommand::Replay(command) => replay_session(command, global_session_dir),
        SessionsCommand::Continue(command) => {
            continue_session(command, config_path, global_session_dir)
        }
        SessionsCommand::Export(command) => export_session(command, global_session_dir),
        SessionsCommand::Tree(command) => tree_sessions(command, global_session_dir),
        SessionsCommand::Fork(command) => fork_session(command, global_session_dir),
        SessionsCommand::Clone(command) => clone_session(command, global_session_dir),
    }
}

fn list_sessions(command: SessionsListCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
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

    let entries = collect_list_entries(entries, &command);

    if command.json {
        match render_json_session_list(&entries) {
            Ok(body) => println!("{body}"),
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::from(1);
            }
        }
    } else {
        print!("{}", render_human_session_table(&entries));
    }

    ExitCode::SUCCESS
}

fn collect_list_entries(
    entries: Vec<SessionInspectionEntry>,
    command: &SessionsListCommand,
) -> Vec<SessionInspectionEntry> {
    let mut entries = entries
        .into_iter()
        .filter(is_listed_session)
        .filter(|entry| matches_list_filters(entry, command))
        .collect::<Vec<_>>();
    command.sort.sort_entries(&mut entries);
    entries
}

fn is_listed_session(entry: &SessionInspectionEntry) -> bool {
    !matches!(
        entry.catalog.mode_source,
        SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
    )
}

fn matches_list_filters(entry: &SessionInspectionEntry, command: &SessionsListCommand) -> bool {
    command
        .status
        .is_none_or(|status| status.matches(entry.catalog.status))
        && command
            .profile
            .as_deref()
            .is_none_or(|profile| entry.catalog.profile_preset.as_deref() == Some(profile))
        && command
            .resumable
            .is_none_or(|resumable| entry.catalog.is_resumable == resumable)
}

fn render_json_session_list(entries: &[SessionInspectionEntry]) -> Result<String, String> {
    let json_entries = entries
        .iter()
        .map(SessionListJsonEntry::from)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json_entries)
        .map_err(|err| format!("failed to serialize session list: {err}"))
}

fn render_human_session_table(entries: &[SessionInspectionEntry]) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<7} {:<9} {:<8} {:<30} {:<20} reason",
        "run_id",
        "status",
        "run_name",
        "profile",
        "provider/model",
        "mode",
        "resume",
        "artifacts",
        "children",
        "session_path",
        "parent"
    )
    .expect("write session list header");

    for entry in entries {
        writeln!(
            output,
            "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<7} {:<9} {:<8} {:<30} {:<20} {}",
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
            entry.artifact_count,
            entry.child_session_count,
            entry.run_dir.display(),
            entry.catalog.parent_session_id.as_deref().unwrap_or("-"),
            entry
                .catalog
                .resume_disabled_reason
                .as_deref()
                .unwrap_or("-")
        )
        .expect("write session list row");
    }

    output
}

fn reopen_session(command: ReopenCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    let session_dir = global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR));
    let run_dir = match resolve_session_run_dir(&command.session, &session_dir) {
        Ok(run_dir) => run_dir,
        Err(err) => {
            eprintln!("session reopen failed: {err}");
            return ExitCode::from(1);
        }
    };
    let summary = match inspect_session_recovery(&run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("session reopen failed: {err}");
            return ExitCode::from(1);
        }
    };

    if command.json {
        match serde_json::to_string_pretty(&summary) {
            Ok(body) => println!("{body}"),
            Err(err) => {
                eprintln!("session reopen failed: failed to serialize recovery summary: {err}");
                return ExitCode::from(1);
            }
        }
    } else {
        print_recovery_summary(&summary);
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

    let session = match command.run_id.as_deref() {
        Some(run_id) => resolve_session_by_run_id(&session_dir, run_id),
        None => resolve_session(&session_dir, command.selector()),
    };
    let session = match session {
        Ok(session) => session,
        Err(err) => {
            eprintln!("session inspect failed: {err}");
            return ExitCode::from(1);
        }
    };

    let summary = match summarize_session(&session.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("session inspect failed: {err}");
            return ExitCode::from(1);
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
        );
    }

    println!("mode: {}", mode_source_label(session.catalog.mode_source));
    println!("resume: {}", resumable_label(session.catalog.is_resumable));
    if let Some(reason) = session.catalog.resume_disabled_reason.as_deref() {
        println!("resume_reason: {reason}");
    }
    print_human_summary(&summary);
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

fn tree_sessions(command: TreeSessionCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    let default_session_dir = session_dir(global_session_dir);
    let (session_dir, root_run_id) = match command.root.as_deref() {
        Some(root) => match resolve_session_for_write_or_tree(&default_session_dir, root) {
            Ok(session) => {
                let session_dir = session
                    .run_dir
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| default_session_dir.clone());
                (session_dir, Some(session.catalog.run_id))
            }
            Err(err) => {
                eprintln!("Harness session tree failed: {err}");
                return ExitCode::from(1);
            }
        },
        None => {
            if let Err(code) = ensure_session_dir_exists(&default_session_dir) {
                return code;
            }
            (default_session_dir, None)
        }
    };

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("Harness session tree failed: {err}");
            return ExitCode::from(1);
        }
    };
    let rows = collect_tree_rows(entries, root_run_id.as_deref(), command.filter.as_deref());

    if command.json {
        let report = SessionTreeJson {
            session_count: rows.len(),
            harness_lineage: rows
                .iter()
                .map(|row| SessionTreeJsonRow {
                    depth: row.depth,
                    run_dir: row.run_dir.clone(),
                    catalog: row.catalog.clone(),
                })
                .collect(),
            root: root_run_id,
            filter: command.filter,
        };
        write_json_output(&report, None)
    } else {
        print!(
            "{}",
            render_human_session_tree(&rows, root_run_id.as_deref(), command.filter.as_deref())
        );
        ExitCode::SUCCESS
    }
}

fn fork_session(command: ForkSessionCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    write_child_session(
        "fork",
        &command.source,
        global_session_dir,
        command.json,
        |events| validate_fork_stable_prefix(events, command.cutoff),
    )
}

fn clone_session(command: CloneSessionCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    write_child_session(
        "clone",
        &command.source,
        global_session_dir,
        command.json,
        latest_clone_stable_prefix,
    )
}

fn write_child_session(
    operation: &'static str,
    source: &str,
    global_session_dir: Option<PathBuf>,
    json: bool,
    stable_prefix: impl FnOnce(
        &[harness_core::event::EventEnvelopeV1],
    ) -> Result<
        StableSessionPrefix,
        harness_core::session_lineage::SessionLineageError,
    >,
) -> ExitCode {
    let session_dir = session_dir(global_session_dir);
    let source = match resolve_session_for_write_or_tree(&session_dir, source) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("Harness session {operation} failed: {err}");
            return ExitCode::from(1);
        }
    };
    let events = match load_events_from_run_dir(&source.run_dir) {
        Ok(events) => events,
        Err(err) => {
            eprintln!(
                "Harness session {operation} failed: failed to read source session {}: {err}",
                source.catalog.run_id
            );
            return ExitCode::from(1);
        }
    };
    let stable_prefix = match stable_prefix(&events) {
        Ok(prefix) => prefix,
        Err(err) => {
            eprintln!("Harness session {operation} failed: {err}");
            return ExitCode::from(1);
        }
    };
    let result = match materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source.run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    }) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Harness session {operation} failed: {err}");
            return ExitCode::from(1);
        }
    };

    let report = ChildSessionWriteReport {
        harness_operation: operation,
        child_run_id: result.child_run_id,
        child_run_dir: result.child_run_dir,
        source_run_id: result.source_run_id,
        source_run_dir: source.run_dir,
        source_cutoff_seq: result.source_cutoff_seq,
        event_count: result.event_count,
        artifact_count: result.artifact_count,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    if json {
        write_json_output(&report, None)
    } else {
        println!("Harness session {operation} created");
        println!("child_run_id: {}", report.child_run_id);
        println!("child_run_dir: {}", report.child_run_dir.display());
        if let Some(source_run_id) = report.source_run_id.as_deref() {
            println!("source_run_id: {source_run_id}");
        }
        println!("source_run_dir: {}", report.source_run_dir.display());
        println!("source_cutoff_seq: {}", report.source_cutoff_seq);
        println!("events: {}", report.event_count);
        println!("artifacts: {}", report.artifact_count);
        ExitCode::SUCCESS
    }
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

fn collect_tree_rows(
    entries: Vec<SessionInspectionEntry>,
    root_run_id: Option<&str>,
    filter: Option<&str>,
) -> Vec<SessionTreeRow> {
    let entries = entries
        .into_iter()
        .map(normalize_lineage_entry)
        .collect::<Vec<_>>();
    let run_dirs = entries
        .iter()
        .map(|entry| (entry.catalog.run_id.clone(), entry.run_dir.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let tree = project_lineage_tree(entries.into_iter().map(|entry| entry.catalog));
    let rows = match root_run_id {
        Some(root_run_id) => tree
            .roots
            .iter()
            .find_map(|node| flatten_from_node(node, root_run_id)),
        None => Some(
            tree.flatten()
                .into_iter()
                .map(|row| (row.depth, row.entry.clone()))
                .collect(),
        ),
    }
    .unwrap_or_default();

    let filter = filter.map(|text| text.to_ascii_lowercase());
    rows.into_iter()
        .filter_map(|(depth, catalog)| {
            let run_dir = run_dirs.get(&catalog.run_id).cloned().unwrap_or_default();
            let row = SessionTreeRow {
                depth,
                run_dir,
                catalog,
            };
            filter
                .as_deref()
                .is_none_or(|filter| tree_row_matches_filter(&row, filter))
                .then_some(row)
        })
        .collect()
}

fn normalize_lineage_entry(mut entry: SessionInspectionEntry) -> SessionInspectionEntry {
    if let Some(parent_run_id) = harness_lineage_parent_run_id(&entry.run_dir) {
        entry.catalog.parent_session_id = Some(parent_run_id);
    }
    entry
}

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    let body = fs::read_to_string(run_dir.join("meta.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&body).ok()?;
    let lineage = value.get("harness_lineage")?;
    lineage
        .get("harness_source_run_id")
        .or_else(|| lineage.get("parent_run_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn flatten_from_node(
    node: &SessionLineageNode,
    root_run_id: &str,
) -> Option<Vec<(usize, SessionCatalogEntry)>> {
    if node.entry.run_id == root_run_id {
        let mut rows = Vec::new();
        flatten_node_into(node, 0, &mut rows);
        return Some(rows);
    }
    node.children
        .iter()
        .find_map(|child| flatten_from_node(child, root_run_id))
}

fn flatten_node_into(
    node: &SessionLineageNode,
    depth: usize,
    rows: &mut Vec<(usize, SessionCatalogEntry)>,
) {
    rows.push((depth, node.entry.clone()));
    for child in &node.children {
        flatten_node_into(child, depth + 1, rows);
    }
}

fn tree_row_matches_filter(row: &SessionTreeRow, filter: &str) -> bool {
    row.catalog.run_id.to_ascii_lowercase().contains(filter)
        || row
            .catalog
            .run_name
            .as_deref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(filter))
        || row
            .catalog
            .parent_session_id
            .as_deref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(filter))
        || row
            .run_dir
            .display()
            .to_string()
            .to_ascii_lowercase()
            .contains(filter)
}

fn render_human_session_tree(
    rows: &[SessionTreeRow],
    root_run_id: Option<&str>,
    filter: Option<&str>,
) -> String {
    let mut output = String::new();
    writeln!(output, "Harness session lineage").expect("write tree title");
    if let Some(root_run_id) = root_run_id {
        writeln!(output, "root: {root_run_id}").expect("write tree root");
    }
    if let Some(filter) = filter {
        writeln!(output, "filter: {filter}").expect("write tree filter");
    }
    if rows.is_empty() {
        writeln!(output, "  <no sessions>").expect("write empty tree");
        return output;
    }

    for row in rows {
        let indent = "  ".repeat(row.depth);
        writeln!(
            output,
            "{indent}- {} status={} resume={} parent={} path={}",
            row.catalog.run_id,
            status_label(row.catalog.status),
            resumable_label(row.catalog.is_resumable),
            row.catalog.parent_session_id.as_deref().unwrap_or("-"),
            row.run_dir.display()
        )
        .expect("write tree row");
    }
    output
}

fn run_dir_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn load_run_metadata(run_dir: &Path) -> Option<RunMetadata> {
    let meta_path = run_dir.join("meta.json");
    let body = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&body).ok()
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

fn print_recovery_summary(summary: &SessionRecoverySummary) {
    println!("run_id: {}", summary.run_id);
    println!(
        "run_name: {}",
        summary.run_name.as_deref().unwrap_or("<unknown>")
    );
    println!("run_dir: {}", summary.run_dir.display());
    println!(
        "workspace_root: {}",
        summary.workspace_root.as_deref().unwrap_or("<unknown>")
    );
    println!("status: {}", status_label(summary.status));
    println!(
        "profile: {}",
        summary.profile.as_deref().unwrap_or("<unavailable>")
    );
    println!(
        "provider/model: {}",
        summary.provider_model.as_deref().unwrap_or("<unavailable>")
    );
    println!("mode: {}", mode_source_label(summary.mode));
    println!(
        "resumable: {}",
        if summary.resumable { "yes" } else { "no" }
    );
    if let Some(reason) = &summary.resume_disabled_reason {
        println!("resume_disabled_reason: {reason}");
    }
    if let Some(agent_id) = &summary.resume_agent_id {
        println!("resume_agent_id: {agent_id}");
    }
    if let Some(last_error) = &summary.last_error {
        println!("last_error: {last_error}");
    }
    if let Some(continue_hint) = &summary.continue_hint {
        println!("continue_hint: {continue_hint}");
    }
    println!("total_events: {}", summary.total_events);

    if !summary.pending_permissions.is_empty() {
        println!("pending_permissions:");
        for permission_id in &summary.pending_permissions {
            println!("  - {permission_id}");
        }
    }
    if !summary.tasks_in_flight.is_empty() {
        println!("tasks_in_flight:");
        for task_id in &summary.tasks_in_flight {
            println!("  - {task_id}");
        }
    }
    if !summary.prompt_context.is_empty() {
        println!("prompt_context:");
        for entry in &summary.prompt_context {
            println!(
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
        println!("child_sessions:");
        for child in &summary.child_sessions {
            println!(
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
        println!("artifacts:");
        for artifact in &summary.artifacts {
            println!(
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
}
