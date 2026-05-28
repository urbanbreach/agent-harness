use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use harness_core::agent_catalog::resolve_agent_catalog;
use harness_core::config::{
    HarnessConfig, McpServerConfig, OpenAiCompatibleProviderConfig, ProviderConfig,
};
use harness_core::proj::{RunMetadata, RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_core::redact::{redact_value, DefaultRedactor, Redactor};
use harness_core::session_lineage::{
    latest_clone_stable_prefix, materialize_child_session, project_lineage_tree,
    validate_fork_stable_prefix, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind, SessionLineageNode, StableSessionPrefix,
};
use harness_tools::{
    coordinator_registry_with_mcp_and_editing, native_tool_catalog_entries,
    EditingToolSurfaceConfig,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::cli_io::{load_events_from_run_dir, load_run_metadata};
use crate::defaults::DEFAULT_SESSION_DIR;
use crate::readiness::ast_grep_adapter_readiness;
use crate::recovery::{inspect_session_recovery, resolve_session_run_dir, SessionRecoverySummary};
use crate::replay::{
    inspect_session_catalog, print_human_summary, summarize_session, ReplayArtifactSummary,
    ReplayCommand, ReplaySummary, SessionInspectionEntry,
};
use crate::tui::TuiCommand;
use crate::CliDeps;

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
                SessionInspectionEntry::sort_by_updated_desc(entries);
            }
            Self::UpdatedAsc => {
                SessionInspectionEntry::sort_by_updated_asc(entries);
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
    support: SessionExportSupport,
    events: Vec<harness_core::event::EventEnvelopeV1>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionExportSupport {
    doctor_json: Value,
    config_summary: Value,
    provider_summary: Value,
    agent_catalog_summary: Value,
    native_tool_catalog_summary: Value,
    session_tool_readiness: Value,
    route_metadata: Vec<SessionExportRouteMetadata>,
    artifact_index: Vec<ReplayArtifactSummary>,
}

#[derive(Debug, Clone)]
struct SessionExportReadiness {
    doctor_json: Value,
    config_summary: Value,
    provider_summary: Value,
    agent_catalog_summary: Value,
    native_tool_catalog_summary: Value,
    session_tool_readiness: Value,
    credential_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionExportRouteMetadata {
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_request_id: Option<String>,
    route: Value,
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
            list_sessions(command, global_session_dir, stdout, stderr)
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
        SessionsCommand::Export(command) => export_session(
            command,
            config_path,
            global_session_dir,
            stdout,
            stderr,
            deps,
        ),
        SessionsCommand::Tree(command) => {
            tree_sessions(command, global_session_dir, stdout, stderr)
        }
        SessionsCommand::Fork(command) => fork_session(command, global_session_dir, stdout, stderr),
        SessionsCommand::Clone(command) => {
            clone_session(command, global_session_dir, stdout, stderr)
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

fn list_sessions(
    command: SessionsListCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(stderr, "{err}");
            return 1;
        }
    };

    let entries = collect_list_entries(entries, &command);

    if command.json {
        match render_json_session_list(&entries) {
            Ok(body) => {
                let _ = writeln!(stdout, "{body}");
            }
            Err(err) => {
                let _ = writeln!(stderr, "{err}");
                return 1;
            }
        }
    } else {
        let _ = write!(stdout, "{}", render_human_session_table(&entries));
    }

    0
}

fn collect_list_entries(
    entries: Vec<SessionInspectionEntry>,
    command: &SessionsListCommand,
) -> Vec<SessionInspectionEntry> {
    let mut entries = entries
        .into_iter()
        .filter(SessionInspectionEntry::is_visible_in_operator_history)
        .filter(|entry| matches_list_filters(entry, command))
        .collect::<Vec<_>>();
    command.sort.sort_entries(&mut entries);
    entries
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

fn export_session(
    command: ExportSessionCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    deps: &CliDeps,
) -> i32 {
    let session_dir_override = global_session_dir.clone();
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

    let events = match load_events_from_run_dir(&session.run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "failed to export session {}: {err}",
                session.catalog.run_id
            );
            return 1;
        }
    };
    let replay = match summarize_session(&session.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "failed to export session {}: {err}",
                session.catalog.run_id
            );
            return 1;
        }
    };

    let run_dir = session.run_dir;
    let metadata = load_run_metadata(&run_dir);
    let session_workspace_root = replay.workspace_root.as_deref().map(PathBuf::from);
    let readiness = session_export_readiness(
        config_path.as_deref(),
        session_dir_override,
        session_workspace_root,
        deps,
    );
    let mut credential_values = deps.credential_env_values();
    credential_values.extend(readiness.credential_values.clone());
    let credential_values = dedupe_credential_values(credential_values);
    let export = SessionExportBundle {
        run_dir,
        support: session_export_support(&events, &replay, &session.catalog, readiness),
        catalog: session.catalog,
        metadata,
        replay,
        events,
    };

    write_redacted_export_output(&export, command.output, stdout, stderr, &credential_values)
}

fn session_export_support(
    events: &[harness_core::event::EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
    readiness: SessionExportReadiness,
) -> SessionExportSupport {
    SessionExportSupport {
        doctor_json: readiness.doctor_json,
        config_summary: readiness.config_summary,
        provider_summary: readiness.provider_summary,
        agent_catalog_summary: readiness.agent_catalog_summary,
        native_tool_catalog_summary: readiness.native_tool_catalog_summary,
        session_tool_readiness: readiness.session_tool_readiness,
        route_metadata: session_export_route_metadata(events, replay, catalog),
        artifact_index: replay.artifacts.clone(),
    }
}

fn session_export_readiness(
    config_path: Option<&Path>,
    session_dir_override: Option<PathBuf>,
    session_workspace_root: Option<PathBuf>,
    deps: &CliDeps,
) -> SessionExportReadiness {
    let mut context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            return unavailable_session_export_readiness(format!(
                "failed to resolve config context: {err}"
            ));
        }
    };
    if config_path.is_none() {
        if let Some(workspace_root) = session_workspace_root {
            context = context.with_current_dir(workspace_root);
        }
    }

    let loaded =
        match harness_core::config::load_resolved_config_with_context(config_path, &context) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                return unavailable_session_export_readiness(
                    "no config file found; support export includes replay-only evidence",
                );
            }
            Err(err) => {
                return unavailable_session_export_readiness(format!(
                    "failed to load config: {err}"
                ));
            }
        };

    let config_display = loaded.path_display();
    let paths = loaded.paths;
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir_override);
    let credential_values = session_export_config_credential_values(&config, deps);

    SessionExportReadiness {
        doctor_json: crate::doctor::support_report_json(config_display.clone(), &config, &|name| {
            deps.env_var_is_set(name)
        }),
        config_summary: session_export_config_summary(&config_display, &paths, &config),
        provider_summary: session_export_provider_summary(&config),
        agent_catalog_summary: session_export_agent_catalog_summary(&config),
        native_tool_catalog_summary: session_export_native_tool_catalog_summary(&config),
        session_tool_readiness: session_export_session_tool_readiness(&config),
        credential_values,
    }
}

fn unavailable_session_export_readiness(reason: impl Into<String>) -> SessionExportReadiness {
    let reason = reason.into();
    SessionExportReadiness {
        doctor_json: json!({
            "available": false,
            "no_network_probes": true,
            "reason": reason,
        }),
        config_summary: json!({
            "loaded": false,
            "no_network_probes": true,
            "reason": reason,
        }),
        provider_summary: json!({
            "loaded": false,
            "provider_count": 0,
            "providers": [],
            "no_network_probes": true,
            "reason": reason,
        }),
        agent_catalog_summary: json!({
            "loaded": false,
            "source": "harness_core::agent_catalog",
            "no_network_probes": true,
            "reason": reason,
        }),
        native_tool_catalog_summary: json!({
            "loaded": false,
            "source": "harness_tools::tool_catalog",
            "no_network_probes": true,
            "reason": reason,
        }),
        session_tool_readiness: json!({
            "available": false,
            "source": "event_replay",
            "redacted_by_default": true,
            "no_network_probes": true,
            "reason": reason,
        }),
        credential_values: Vec::new(),
    }
}

fn session_export_config_credential_values(config: &HarnessConfig, deps: &CliDeps) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(token) = config.integrations.remote_search.auth_token.as_deref() {
        push_credential_value(&mut values, token);
    }
    for instruction in &config.instruction_files {
        push_credential_value(&mut values, &instruction.content);
    }
    for agent in config.agents.values() {
        if let Some(prompt) = agent.system_prompt.as_deref() {
            push_credential_value(&mut values, prompt);
        }
    }
    for provider in config.providers.values() {
        let ProviderConfig::OpenAiCompatible(provider) = provider;
        push_credential_value(&mut values, &provider.api_key);
        for env_name in &provider.api_key_env {
            if let Some(value) = deps.env_var_value(env_name) {
                push_credential_value(&mut values, &value);
            }
        }
        for (name, value) in &provider.headers {
            if is_credential_name(name) {
                push_credential_value(&mut values, value);
            }
        }
    }
    for server in config.integrations.mcp.servers.values() {
        match server {
            McpServerConfig::Stdio { env, .. } => {
                for (name, value) in env {
                    if is_credential_name(name) {
                        push_credential_value(&mut values, value);
                    }
                }
            }
            McpServerConfig::Http { headers, .. } => {
                for (name, value) in headers {
                    if is_credential_name(name) {
                        push_credential_value(&mut values, value);
                    }
                }
            }
        }
    }
    dedupe_credential_values(values)
}

fn push_credential_value(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.len() >= 8 && !value.contains("[REDACTED") {
        values.push(value.to_string());
    }
}

fn dedupe_credential_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_credential_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "CREDENTIAL",
        "AUTHORIZATION",
        "COOKIE",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

fn session_export_config_summary(
    config_display: &str,
    paths: &[PathBuf],
    config: &HarnessConfig,
) -> Value {
    json!({
        "loaded": true,
        "config": config_display,
        "paths": paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "default_agent": config.default_agent.as_deref(),
        "agent_count": config.agents.len(),
        "provider_count": config.providers.len(),
        "model_profile_count": config.model_profiles.len(),
        "session_dir": config.paths.session_dir.display().to_string(),
        "no_network_probes": true,
    })
}

fn session_export_provider_summary(config: &HarnessConfig) -> Value {
    let providers = config
        .providers
        .iter()
        .map(|(id, provider)| match provider {
            ProviderConfig::OpenAiCompatible(provider) => {
                openai_compatible_provider_summary(id, provider)
            }
        })
        .collect::<Vec<_>>();

    json!({
        "loaded": true,
        "provider_count": providers.len(),
        "providers": providers,
        "no_network_probes": true,
    })
}

fn session_export_agent_catalog_summary(config: &HarnessConfig) -> Value {
    let catalog = resolve_agent_catalog(config);
    json!({
        "loaded": true,
        "source": "harness_core::agent_catalog",
        "entry_count": catalog.entries.len(),
        "category_fallback": catalog.category_fallback,
        "entries": catalog.entries,
        "no_network_probes": true,
    })
}

fn session_export_native_tool_catalog_summary(config: &HarnessConfig) -> Value {
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    let catalog = native_tool_catalog_entries(&registry);
    let required_v1_tools = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "background_cancel",
        "team_list",
        "ast_grep_search",
    ];
    json!({
        "loaded": true,
        "source": "harness_tools::tool_catalog",
        "tool_count": catalog.len(),
        "required_v1_tools": required_v1_tools,
        "missing_required_v1_tools": required_v1_tools
            .into_iter()
            .filter(|tool_id| !catalog.iter().any(|entry| entry.canonical_id == *tool_id))
            .collect::<Vec<_>>(),
        "tools": catalog,
        "ast_grep_adapter": ast_grep_adapter_readiness(),
        "no_network_probes": true,
    })
}

fn session_export_session_tool_readiness(config: &HarnessConfig) -> Value {
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    let ids = registry.tool_ids().into_iter().collect::<BTreeSet<_>>();
    let session_tools = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
    ];
    json!({
        "available": session_tools.iter().all(|tool_id| ids.contains(*tool_id)),
        "source": "event_replay",
        "redacted_by_default": true,
        "side_effect_free": true,
        "tools": session_tools,
        "missing": session_tools
            .into_iter()
            .filter(|tool_id| !ids.contains(*tool_id))
            .collect::<Vec<_>>(),
        "no_network_probes": true,
    })
}

fn openai_compatible_provider_summary(
    id: &str,
    provider: &OpenAiCompatibleProviderConfig,
) -> Value {
    json!({
        "id": id,
        "type": "openai_compatible",
        "name": provider.name.as_deref(),
        "base_url": provider.base_url.as_str(),
        "model_count": provider.models.len(),
        "models": provider.models.keys().cloned().collect::<Vec<_>>(),
        "credentials": provider_credentials_summary(provider),
        "api_key_env": provider.api_key_env.clone(),
        "timeout_ms": provider.timeout_ms,
        "header_count": provider.headers.len(),
    })
}

fn provider_credentials_summary(provider: &OpenAiCompatibleProviderConfig) -> &'static str {
    if !provider.api_key.trim().is_empty() {
        "inline_redacted"
    } else if !provider.api_key_env.is_empty() {
        "env_reference"
    } else {
        "missing"
    }
}

fn session_export_route_metadata(
    events: &[harness_core::event::EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> Vec<SessionExportRouteMetadata> {
    let mut metadata = vec![session_replay_route_metadata(events, replay, catalog)];
    metadata.extend(events.iter().filter_map(|event| {
        let harness_core::event::EventV1::ToolCallFinished(payload) = &event.payload else {
            return None;
        };
        let output_json = payload.output_json.as_ref()?;
        let route = output_json.get("route")?.clone();
        Some(SessionExportRouteMetadata {
            source: "task_output".to_string(),
            tool_call_id: Some(payload.tool_call_id.clone()),
            child_session_id: output_string_field(output_json, "child_session_id")
                .or_else(|| output_string_field(output_json, "session_id"))
                .or_else(|| output_string_field(output_json, "task_id")),
            child_request_id: output_string_field(output_json, "child_request_id")
                .or_else(|| output_string_field(output_json, "request_id")),
            route,
        })
    }));
    metadata
}

fn session_replay_route_metadata(
    events: &[harness_core::event::EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> SessionExportRouteMetadata {
    let mut profiles = BTreeSet::new();
    let mut provider_models = BTreeSet::new();
    let mut tools = Vec::new();
    let mut permissions = Vec::new();

    for event in events {
        match &event.payload {
            harness_core::event::EventV1::AgentSpawned(payload) => {
                profiles.insert(payload.profile.clone());
            }
            harness_core::event::EventV1::ProviderRequestStarted(payload) => {
                if let Some(label) = crate::cli_labels::provider_model_label(
                    Some(&payload.provider_id),
                    Some(&payload.model_id),
                ) {
                    provider_models.insert(label);
                }
            }
            harness_core::event::EventV1::ToolCallRequested(payload) => {
                tools.push(json!({
                    "tool_call_id": payload.tool_call_id,
                    "tool_id": payload.tool_id,
                    "seq": event.seq,
                }));
            }
            harness_core::event::EventV1::PermissionRequested(payload) => {
                permissions.push(json!({
                    "permission_id": payload.permission_id,
                    "kind": payload.kind,
                    "tool_call_id": payload.tool_call_id,
                    "default_decision": payload.default_decision,
                    "seq": event.seq,
                }));
            }
            harness_core::event::EventV1::PermissionResolved(payload) => {
                permissions.push(json!({
                    "permission_id": payload.permission_id,
                    "decision": payload.decision,
                    "reason": payload.reason,
                    "seq": event.seq,
                }));
            }
            _ => {}
        }
    }

    SessionExportRouteMetadata {
        source: "session_replay".to_string(),
        tool_call_id: None,
        child_session_id: None,
        child_request_id: None,
        route: json!({
            "run_id": replay.run_id,
            "run_name": replay.run_name,
            "status": replay.status,
            "mode_source": catalog.mode_source,
            "profile_preset": catalog.profile_preset,
            "provider_model": catalog.provider_model,
            "profiles": profiles.into_iter().collect::<Vec<_>>(),
            "provider_models": provider_models.into_iter().collect::<Vec<_>>(),
            "tools": tools,
            "permissions": permissions,
            "artifact_count": replay.artifact_count,
            "child_session_count": replay.child_session_count,
            "is_resumable": replay.is_resumable,
            "resume_disabled_reason": replay.resume_disabled_reason,
        }),
    }
}

fn output_string_field(output_json: &Value, key: &str) -> Option<String> {
    output_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn write_redacted_export_output(
    export: &SessionExportBundle,
    output: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    credential_values: &[String],
) -> i32 {
    let redactor = DefaultRedactor::default();
    write_redacted_export_output_with_redactor(
        export,
        output,
        stdout,
        stderr,
        &redactor,
        &redactor,
        credential_values,
    )
}

fn write_redacted_export_output_with_redactor<R: Redactor + ?Sized>(
    export: &SessionExportBundle,
    output: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    redactor: &R,
    scanner: &DefaultRedactor,
    credential_values: &[String],
) -> i32 {
    let value = match serde_json::to_value(export) {
        Ok(value) => value,
        Err(err) => {
            let _ = writeln!(stderr, "failed to serialize session export: {err}");
            return 1;
        }
    };
    let mut redacted = redact_value(redactor, &value);
    let redacted_body = serde_json::to_string(&redacted).unwrap_or_default();
    let secret_finding_count = scanner.secret_finding_count(&redacted_body)
        + credential_value_secret_finding_count(&redacted, credential_values);
    if secret_finding_count > 0 {
        let _ = writeln!(
            stderr,
            "failed to export session: redaction scanner found {secret_finding_count} unredacted secret marker(s)"
        );
        return 1;
    }
    let redacted_marker_count = redacted_body.matches("[REDACTED").count();
    if let Some(support) = redacted.get_mut("support").and_then(Value::as_object_mut) {
        support.insert(
            "redaction_manifest".to_string(),
            json!({
                "status": if secret_finding_count == 0 { "clean" } else { "failed" },
                "redactor": "harness-default-redactor",
                "redacted_marker_count": redacted_marker_count,
            }),
        );
        support.insert(
            "secret_scan_status".to_string(),
            json!({
                "status": if secret_finding_count == 0 { "clean" } else { "failed" },
                "scanner": "harness-session-export-secret-scan",
                "secret_finding_count": secret_finding_count,
            }),
        );
    }

    write_json_output(&redacted, output, stdout, stderr)
}

fn credential_value_secret_finding_count(value: &Value, values: &[String]) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => 0,
        Value::String(text) => text_credential_finding_count(text, values),
        Value::Array(items) => items
            .iter()
            .map(|item| credential_value_secret_finding_count(item, values))
            .sum(),
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                text_credential_finding_count(key, values)
                    + credential_value_secret_finding_count(value, values)
            })
            .sum(),
    }
}

fn text_credential_finding_count(text: &str, values: &[String]) -> usize {
    values
        .iter()
        .filter(|value| value.len() >= 8 && !value.contains("[REDACTED") && text.contains(*value))
        .count()
}

fn tree_sessions(
    command: TreeSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
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
                let _ = writeln!(stderr, "Harness session tree failed: {err}");
                return 1;
            }
        },
        None => {
            if let Err(code) = ensure_session_dir_exists(&default_session_dir, stderr) {
                return code;
            }
            (default_session_dir, None)
        }
    };

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session tree failed: {err}");
            return 1;
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
        write_json_output(&report, None, stdout, stderr)
    } else {
        let _ = write!(
            stdout,
            "{}",
            render_human_session_tree(&rows, root_run_id.as_deref(), command.filter.as_deref())
        );
        0
    }
}

fn fork_session(
    command: ForkSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    write_child_session(
        "fork",
        &command.source,
        global_session_dir,
        command.json,
        stdout,
        stderr,
        |events| validate_fork_stable_prefix(events, command.cutoff),
    )
}

fn clone_session(
    command: CloneSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    write_child_session(
        "clone",
        &command.source,
        global_session_dir,
        command.json,
        stdout,
        stderr,
        latest_clone_stable_prefix,
    )
}

fn write_child_session(
    operation: &'static str,
    source: &str,
    global_session_dir: Option<PathBuf>,
    json: bool,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    stable_prefix: impl FnOnce(
        &[harness_core::event::EventEnvelopeV1],
    ) -> Result<
        StableSessionPrefix,
        harness_core::session_lineage::SessionLineageError,
    >,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    let source = match resolve_session_for_write_or_tree(&session_dir, source) {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
        }
    };
    let events = match load_events_from_run_dir(&source.run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "Harness session {operation} failed: failed to read source session {}: {err}",
                source.catalog.run_id
            );
            return 1;
        }
    };
    let stable_prefix = match stable_prefix(&events) {
        Ok(prefix) => prefix,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
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
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
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
        write_json_output(&report, None, stdout, stderr)
    } else {
        let _ = writeln!(stdout, "Harness session {operation} created");
        let _ = writeln!(stdout, "child_run_id: {}", report.child_run_id);
        let _ = writeln!(stdout, "child_run_dir: {}", report.child_run_dir.display());
        if let Some(source_run_id) = report.source_run_id.as_deref() {
            let _ = writeln!(stdout, "source_run_id: {source_run_id}");
        }
        let _ = writeln!(
            stdout,
            "source_run_dir: {}",
            report.source_run_dir.display()
        );
        let _ = writeln!(stdout, "source_cutoff_seq: {}", report.source_cutoff_seq);
        let _ = writeln!(stdout, "events: {}", report.event_count);
        let _ = writeln!(stdout, "artifacts: {}", report.artifact_count);
        0
    }
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

fn collect_tree_rows(
    entries: Vec<SessionInspectionEntry>,
    root_run_id: Option<&str>,
    filter: Option<&str>,
) -> Vec<SessionTreeRow> {
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
    contains_normalized_filter(&row.catalog.run_id, filter)
        || optional_text_contains_normalized_filter(row.catalog.run_name.as_deref(), filter)
        || optional_text_contains_normalized_filter(
            row.catalog.parent_session_id.as_deref(),
            filter,
        )
        || contains_normalized_filter(&row.run_dir.display().to_string(), filter)
}

fn optional_text_contains_normalized_filter(value: Option<&str>, filter: &str) -> bool {
    value.is_some_and(|value| contains_normalized_filter(value, filter))
}

fn contains_normalized_filter(value: &str, filter: &str) -> bool {
    value.to_ascii_lowercase().contains(filter)
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
                teams: Vec::new(),
            },
            support: SessionExportSupport {
                doctor_json: json!({}),
                config_summary: json!({}),
                provider_summary: json!({}),
                agent_catalog_summary: json!({}),
                native_tool_catalog_summary: json!({}),
                session_tool_readiness: json!({}),
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
    fn tree_row_filter_matches_all_searchable_fields_case_insensitively() {
        let mut row = SessionTreeRow {
            depth: 0,
            run_dir: PathBuf::from("/tmp/Session-Path"),
            catalog: sample_entry(
                "RUN-ABC",
                42,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
                0,
                0,
                Some("PARENT-ABC"),
            )
            .catalog,
        };
        row.catalog.run_name = Some("Named Session".to_string());

        for filter in ["run-abc", "named", "parent-abc", "session-path"] {
            assert!(
                tree_row_matches_filter(&row, filter),
                "filter `{filter}` should match the tree row"
            );
        }

        assert!(!tree_row_matches_filter(&row, "missing"));
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
