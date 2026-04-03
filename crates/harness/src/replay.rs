use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{
    project_run_summary, project_session_catalog_entry, RunStatus, SessionCatalogEntry,
    SessionCatalogMetadata,
};
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
    pub session_path: PathBuf,
    pub status: RunStatus,
    pub workspace_root: Option<String>,
    pub mode_source: harness_core::proj::SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
    pub artifact_count: usize,
    pub child_session_count: usize,
    pub parent_session_id: Option<String>,
    pub total_events: u64,
    pub counts_by_type: std::collections::BTreeMap<String, u64>,
    pub pending_permissions: Vec<String>,
    pub tasks_in_flight: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionInspectionEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
    pub sort_unix_ms: u128,
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
    let catalog = project_session_catalog_entry(events.iter(), &run_id, None, None, None)
        .map_err(|err| err.to_string())?;
    let run_name = events.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data.run_name.clone()),
        _ => None,
    });

    Ok(ReplaySummary {
        run_id,
        run_name,
        session_path: run_dir.to_path_buf(),
        status: projection.status,
        workspace_root: catalog.workspace_root,
        mode_source: catalog.mode_source,
        is_resumable: catalog.is_resumable,
        resume_disabled_reason: catalog.resume_disabled_reason,
        artifact_count: catalog.artifact_count,
        child_session_count: catalog.child_session_count,
        parent_session_id: catalog.parent_session_id,
        total_events: projection.counts.total_events,
        counts_by_type: projection.counts.by_type,
        pending_permissions: projection.pending_permissions.into_iter().collect(),
        tasks_in_flight: projection.tasks_in_flight.into_iter().collect(),
        last_error: projection.last_error,
    })
}

pub fn inspect_session_catalog(session_dir: &Path) -> Result<Vec<SessionInspectionEntry>, String> {
    let read_dir = fs::read_dir(session_dir).map_err(|err| {
        format!(
            "failed to read session directory {}: {err}",
            session_dir.display()
        )
    })?;

    let mut entries = read_dir
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_dir() && path.join("events.jsonl").exists())
        .map(|run_dir| inspect_single_session(&run_dir))
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| (Reverse(entry.sort_unix_ms), entry.run_dir.clone()));

    Ok(entries)
}

fn inspect_single_session(run_dir: &Path) -> SessionInspectionEntry {
    let run_id_fallback = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown-run>");

    let (events, events_error) = load_events_lossy(run_dir);
    let (meta, _meta_error) = load_meta_lossy(run_dir);

    let modified = run_dir
        .join("events.jsonl")
        .metadata()
        .ok()
        .and_then(|it| it.modified().ok());
    let sort_unix_ms = modified
        .and_then(system_time_unix_ms)
        .or_else(|| {
            meta.as_ref().and_then(|it| {
                it.created_at
                    .as_deref()
                    .and_then(parse_unix_ms_from_string_timestamp)
            })
        })
        .unwrap_or(0);
    let last_updated_at = modified
        .and_then(system_time_unix_ms)
        .map(|it| it.to_string())
        .or_else(|| meta.as_ref().and_then(|it| it.created_at.clone()));

    let degraded_reason = events_error.map(|err| format!("events unavailable: {err}"));

    let catalog = match project_session_catalog_entry(
        events.iter(),
        run_id_fallback,
        meta.as_ref().map(|it| &it.catalog),
        last_updated_at.clone(),
        degraded_reason,
    ) {
        Ok(entry) => entry,
        Err(err) => SessionCatalogEntry {
            run_id: events
                .first()
                .map(|event| event.run_id.clone())
                .unwrap_or_else(|| run_id_fallback.to_string()),
            run_name: events.iter().find_map(|event| match &event.payload {
                EventV1::RunStarted(data) => Some(data.run_name.clone()),
                _ => None,
            }),
            status: None,
            last_updated_at,
            workspace_root: events.iter().find_map(|event| match &event.payload {
                EventV1::RunStarted(data) => Some(data.workspace_root.clone()),
                _ => None,
            }),
            profile_preset: None,
            provider_model: None,
            mode_source: harness_core::proj::SessionModeSource::Unknown,
            is_resumable: false,
            resume_disabled_reason: Some(format!("projection unavailable: {err}")),
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    };

    SessionInspectionEntry {
        run_dir: run_dir.to_path_buf(),
        catalog,
        sort_unix_ms,
    }
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

fn load_events_lossy(run_dir: &Path) -> (Vec<EventEnvelopeV1>, Option<String>) {
    let events_path = run_dir.join("events.jsonl");
    let text = match fs::read_to_string(&events_path) {
        Ok(text) => text,
        Err(err) => {
            return (
                Vec::new(),
                Some(format!(
                    "failed to read events file {}: {err}",
                    events_path.display()
                )),
            )
        }
    };

    let mut events = Vec::new();
    let mut first_error: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        match serde_json::from_str::<EventEnvelopeV1>(line) {
            Ok(event) => events.push(event),
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(format!("line {}: {err}", index + 1));
                }
            }
        }
    }

    (events, first_error)
}

fn load_meta_lossy(
    run_dir: &Path,
) -> (Option<SessionCatalogMetadataWithCreatedAt>, Option<String>) {
    let meta_path = run_dir.join("meta.json");
    if !meta_path.exists() {
        return (None, None);
    }

    let body = match fs::read_to_string(&meta_path) {
        Ok(body) => body,
        Err(err) => {
            return (
                None,
                Some(format!(
                    "failed to read metadata file {}: {err}",
                    meta_path.display()
                )),
            )
        }
    };

    match serde_json::from_str::<SessionCatalogMetadataWithCreatedAt>(&body) {
        Ok(meta) => (Some(meta), None),
        Err(err) => (
            None,
            Some(format!(
                "failed to parse metadata file {}: {err}",
                meta_path.display()
            )),
        ),
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SessionCatalogMetadataWithCreatedAt {
    #[serde(flatten)]
    catalog: SessionCatalogMetadata,
    #[serde(default)]
    created_at: Option<String>,
}

impl AsRef<SessionCatalogMetadata> for SessionCatalogMetadataWithCreatedAt {
    fn as_ref(&self) -> &SessionCatalogMetadata {
        &self.catalog
    }
}

fn system_time_unix_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn parse_unix_ms_from_string_timestamp(value: &str) -> Option<u128> {
    value.parse::<u128>().ok()
}

fn print_human_summary(summary: &ReplaySummary) {
    println!("run_id: {}", summary.run_id);
    println!(
        "run_name: {}",
        summary.run_name.as_deref().unwrap_or("<unknown>")
    );
    println!("session_path: {}", summary.session_path.display());
    println!("status: {:?}", summary.status);
    println!(
        "workspace_root: {}",
        summary.workspace_root.as_deref().unwrap_or("<unknown>")
    );
    println!("mode: {:?}", summary.mode_source);
    println!(
        "continue: {}",
        if summary.is_resumable {
            "ready".to_string()
        } else {
            summary
                .resume_disabled_reason
                .as_deref()
                .map(|reason| format!("blocked · {reason}"))
                .unwrap_or_else(|| "blocked".to_string())
        }
    );
    println!(
        "parent_session: {}",
        summary.parent_session_id.as_deref().unwrap_or("-")
    );
    println!("child_sessions: {}", summary.child_session_count);
    println!("artifacts: {}", summary.artifact_count);
    println!("total_events: {}", summary.total_events);
    if let Some(last_error) = &summary.last_error {
        println!("last_error: {last_error}");
    }
    println!("next_steps:");
    println!(
        "  replay_tui: harness tui --replay {}",
        summary.session_path.display()
    );
    if summary.is_resumable {
        println!(
            "  continue_tui: harness tui --continue {}",
            summary.session_path.display()
        );
    } else {
        println!(
            "  continue_tui: unavailable ({})",
            summary
                .resume_disabled_reason
                .as_deref()
                .unwrap_or("resume unavailable without reason")
        );
    }
    println!("counts:");
    for (event_type, count) in &summary.counts_by_type {
        println!("  {event_type}: {count}");
    }
}
