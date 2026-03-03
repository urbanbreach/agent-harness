use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::proj::RunStatus;

use crate::replay::summarize_session;

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
    let read_dir = match fs::read_dir(&session_dir) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            eprintln!(
                "failed to read session directory {}: {err}",
                session_dir.display()
            );
            return ExitCode::from(1);
        }
    };

    let mut run_dirs = read_dir
        .filter_map(|entry| entry.ok().map(|it| it.path()))
        .filter(|path| path.is_dir() && path.join("events.jsonl").exists())
        .collect::<Vec<_>>();
    run_dirs.sort();

    println!("{:<40} {:<12} run_name", "run_id", "status");
    for run_dir in run_dirs {
        match summarize_session(&run_dir) {
            Ok(summary) => {
                println!(
                    "{:<40} {:<12} {}",
                    summary.run_id,
                    status_label(summary.status),
                    summary.run_name.as_deref().unwrap_or("<unknown>")
                );
            }
            Err(err) => {
                eprintln!("failed to summarize {}: {err}", run_dir.display());
            }
        }
    }

    ExitCode::SUCCESS
}

fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Finished => "finished",
        RunStatus::Failed => "failed",
    }
}
