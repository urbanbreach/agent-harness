use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{project_run_summary, RunStatus};
use serde::Serialize;

#[derive(Debug, Args, Clone)]
pub struct ReplayCommand {
    #[arg(long)]
    pub session: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplaySummary {
    pub run_id: String,
    pub run_name: Option<String>,
    pub status: RunStatus,
    pub total_events: u64,
    pub counts_by_type: std::collections::BTreeMap<String, u64>,
    pub pending_permissions: Vec<String>,
    pub tasks_in_flight: Vec<String>,
    pub last_error: Option<String>,
}

pub fn execute(command: ReplayCommand) -> ExitCode {
    match summarize_session(&command.session) {
        Ok(summary) => {
            if command.json {
                match serde_json::to_string_pretty(&summary) {
                    Ok(body) => println!("{body}"),
                    Err(err) => {
                        eprintln!("failed to serialize replay summary: {err}");
                        return ExitCode::from(1);
                    }
                }
            } else {
                print_human_summary(&summary);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("replay failed: {err}");
            ExitCode::from(1)
        }
    }
}

pub fn summarize_session(run_dir: &Path) -> Result<ReplaySummary, String> {
    let events = load_events(run_dir)?;
    if events.is_empty() {
        return Err(format!("no events found in {}", run_dir.display()));
    }

    let projection = project_run_summary(events.iter()).map_err(|err| err.to_string())?;
    let run_id = events[0].run_id.clone();
    let run_name = events.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data.run_name.clone()),
        _ => None,
    });

    Ok(ReplaySummary {
        run_id,
        run_name,
        status: projection.status,
        total_events: projection.counts.total_events,
        counts_by_type: projection.counts.by_type,
        pending_permissions: projection.pending_permissions.into_iter().collect(),
        tasks_in_flight: projection.tasks_in_flight.into_iter().collect(),
        last_error: projection.last_error,
    })
}

fn load_events(run_dir: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = run_dir.join("events.jsonl");
    let text = fs::read_to_string(&events_path).map_err(|err| {
        format!(
            "failed to read events file {}: {err}",
            events_path.display()
        )
    })?;
    text.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).map_err(|err| err.to_string()))
        .collect()
}

fn print_human_summary(summary: &ReplaySummary) {
    println!("run_id: {}", summary.run_id);
    println!(
        "run_name: {}",
        summary.run_name.as_deref().unwrap_or("<unknown>")
    );
    println!("status: {:?}", summary.status);
    println!("total_events: {}", summary.total_events);
    if let Some(last_error) = &summary.last_error {
        println!("last_error: {last_error}");
    }
    println!("counts:");
    for (event_type, count) in &summary.counts_by_type {
        println!("  {event_type}: {count}");
    }
}
