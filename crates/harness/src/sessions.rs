use std::cmp::Reverse;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use serde::Serialize;

use crate::replay::{inspect_session_catalog, SessionInspectionEntry};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    List(SessionsListCommand),
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
        "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<5} reason",
        "run_id", "status", "run_name", "profile", "provider/model", "mode", "resume"
    )
    .expect("write session list header");

    for entry in entries {
        writeln!(
            output,
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
            if entry.catalog.is_resumable {
                "yes"
            } else {
                "no"
            },
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
            },
            sort_unix_ms,
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
            ),
            sample_entry(
                "run-finished",
                20,
                Some(RunStatus::Finished),
                Some("reviewer"),
                SessionModeSource::Prompt,
                false,
            ),
            sample_entry(
                "fixture-hidden",
                10,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::ScenarioFixture,
                true,
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
            ),
            sample_entry(
                "run-c",
                10,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
            ),
            sample_entry(
                "run-a",
                30,
                Some(RunStatus::Finished),
                Some("worker"),
                SessionModeSource::InteractiveLive,
                true,
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
                    "resume_disabled_reason": null
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
        )];

        let rendered = render_human_session_table(&entries);

        assert!(rendered.contains("run_id"));
        assert!(rendered.contains("run-human"));
        assert!(rendered.contains("<unavailable>"));
        assert!(rendered.contains("resume blocked"));
    }
}
