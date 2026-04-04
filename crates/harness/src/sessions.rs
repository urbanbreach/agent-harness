use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use harness_core::proj::{RunStatus, SessionModeSource};
use serde_json::json;

use crate::recovery::{inspect_session_recovery, resolve_session_run_dir, SessionRecoverySummary};
use crate::replay::{inspect_session_catalog, print_human_summary, summarize_session};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    List,
    Inspect {
        #[arg(long = "run")]
        run_id: String,

        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Reopen(ReopenCommand),
}

#[derive(Debug, Args, Clone)]
pub struct ReopenCommand {
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub session: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

pub fn execute(command: SessionsCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    match command {
        SessionsCommand::List => list_sessions(global_session_dir),
        SessionsCommand::Inspect { run_id, json } => {
            inspect_session(global_session_dir, &run_id, json)
        }
        SessionsCommand::Reopen(command) => reopen_session(command, global_session_dir),
    }
}

fn list_sessions(global_session_dir: Option<PathBuf>) -> ExitCode {
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

    println!(
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
    );

    for entry in entries {
        if matches!(
            entry.catalog.mode_source,
            SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
        ) {
            continue;
        }

        println!(
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
        );
    }

    ExitCode::SUCCESS
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
