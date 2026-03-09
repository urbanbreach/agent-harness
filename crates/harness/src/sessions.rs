use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::proj::{RunStatus, SessionModeSource};

use crate::replay::inspect_session_catalog;

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";

#[derive(Debug, Subcommand, Clone)]
pub enum SessionsCommand {
    List,
}

pub fn execute(command: SessionsCommand, global_session_dir: Option<PathBuf>) -> ExitCode {
    match command {
        SessionsCommand::List => list_sessions(global_session_dir),
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
        );
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
