use std::cmp::Reverse;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use serde::Serialize;
use serde_json::json;

use crate::recovery::{inspect_session_recovery, resolve_session_run_dir, SessionRecoverySummary};
use crate::replay::{
    inspect_session_catalog, print_human_summary, summarize_session, SessionInspectionEntry,
};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    List(SessionsListCommand),
    Inspect {
        #[arg(long = "run")]
        run_id: String,

        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Reopen(ReopenCommand),
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

pub fn execute(command: SessionsCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    match command {
        SessionsCommand::List(command) => list_sessions(command, global_session_dir),
        SessionsCommand::Inspect { run_id, json } => {
            inspect_session(global_session_dir, &run_id, json)
        }
        SessionsCommand::Reopen(command) => reopen_session(command, global_session_dir),
    }
}

fn list_sessions(command: SessionsListCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    let session_dir = global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR));
    if !session_dir.exists() {
        eprintln!(
            "failed to read session directory {}: directory does not exist",
            session_dir.display()
        );
        return ExitCode::from(1);
    }
    if let Err(err) = fs::read_dir(&session_dir) {
        eprintln!(
            "failed to read session directory {}: {err}",
            session_dir.display()
        );
        return ExitCode::from(1);
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
            if entry.catalog.is_resumable {
                "yes"
            } else {
                "no"
            },
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

fn inspect_session(global_session_dir: Option<PathBuf>, run_id: &str, as_json: bool) -> ExitCode {
    let session_dir = global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR));
    if !session_dir.exists() {
        eprintln!(
            "failed to read session directory {}: directory does not exist",
            session_dir.display()
        );
        return ExitCode::from(1);
    }
    if let Err(err) = fs::read_dir(&session_dir) {
        eprintln!(
            "failed to read session directory {}: {err}",
            session_dir.display()
        );
        return ExitCode::from(1);
    }

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    let matches = entries
        .into_iter()
        .filter(|entry| entry.catalog.run_id == run_id)
        .collect::<Vec<_>>();

    let [entry] = matches.as_slice() else {
        if matches.is_empty() {
            eprintln!(
                "session inspect failed: no run with id `{run_id}` found in {}",
                session_dir.display()
            );
        } else {
            eprintln!(
                "session inspect failed: run id `{run_id}` is ambiguous in {}",
                session_dir.display()
            );
        }
        return ExitCode::from(1);
    };

    let summary = match summarize_session(&entry.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("session inspect failed: {err}");
            return ExitCode::from(1);
        }
    };

    if as_json {
        match serde_json::to_string_pretty(&json!({
            "run_dir": entry.run_dir.display().to_string(),
            "catalog": entry.catalog.clone(),
            "replay": summary,
        })) {
            Ok(body) => println!("{body}"),
            Err(err) => {
                eprintln!("failed to serialize session inspection: {err}");
                return ExitCode::from(1);
            }
        }
    } else {
        println!("mode: {}", mode_source_label(entry.catalog.mode_source));
        println!(
            "resume: {}",
            if entry.catalog.is_resumable {
                "yes"
            } else {
                "no"
            }
        );
        if let Some(reason) = entry.catalog.resume_disabled_reason.as_deref() {
            println!("resume_reason: {reason}");
        }
        print_human_summary(&summary);
    }

    ExitCode::SUCCESS
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
                "  - tool_call={} tool={} path={} digest={}",
                artifact.tool_call_id,
                artifact.tool_id.as_deref().unwrap_or("<unavailable>"),
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
