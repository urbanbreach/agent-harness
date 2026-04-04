use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{
    inspect_resume_plan, project_run_summary, project_session_catalog_entry,
    ChildSessionTerminalState, RunStatus, SessionCatalogEntry, SessionCatalogMetadata,
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
    pub session_dir: String,
    pub status: RunStatus,
    pub total_events: u64,
    pub counts_by_type: std::collections::BTreeMap<String, u64>,
    pub pending_permissions: Vec<String>,
    pub tasks_in_flight: Vec<String>,
    pub last_error: Option<String>,
    pub artifact_count: usize,
    pub artifacts: Vec<ArtifactInspection>,
    pub child_session_count: usize,
    pub child_sessions: Vec<ChildSessionInspection>,
}

#[derive(Debug, Clone)]
pub struct SessionInspectionEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
    pub sort_unix_ms: u128,
    pub artifact_count: usize,
    pub child_session_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactInspection {
    pub path: String,
    pub bytes: Option<u64>,
    pub digest: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_id: Option<String>,
    pub child_session_id: Option<String>,
    pub present_on_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChildSessionInspection {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub latest_child_request_id: Option<String>,
    pub profile: Option<String>,
    pub provider_model: Option<String>,
    pub terminal_state: Option<String>,
    pub terminal_reason: Option<String>,
    pub elapsed_ms: Option<u64>,
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
    let discovery = inspect_session_discovery(run_dir, &events);

    Ok(ReplaySummary {
        run_id,
        run_name,
        session_dir: run_dir.display().to_string(),
        status: projection.status,
        total_events: projection.counts.total_events,
        counts_by_type: projection.counts.by_type,
        pending_permissions: projection.pending_permissions.into_iter().collect(),
        tasks_in_flight: projection.tasks_in_flight.into_iter().collect(),
        last_error: projection.last_error,
        artifact_count: discovery.artifacts.len(),
        artifacts: discovery.artifacts,
        child_session_count: discovery.child_sessions.len(),
        child_sessions: discovery.child_sessions,
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
        },
    };

    let discovery = inspect_session_discovery(run_dir, &events);

    SessionInspectionEntry {
        run_dir: run_dir.to_path_buf(),
        catalog,
        sort_unix_ms,
        artifact_count: discovery.artifacts.len(),
        child_session_count: discovery.child_sessions.len(),
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

pub(crate) fn print_human_summary(summary: &ReplaySummary) {
    println!("run_id: {}", summary.run_id);
    println!(
        "run_name: {}",
        summary.run_name.as_deref().unwrap_or("<unknown>")
    );
    println!("session_dir: {}", summary.session_dir);
    println!("status: {:?}", summary.status);
    println!("total_events: {}", summary.total_events);
    if let Some(last_error) = &summary.last_error {
        println!("last_error: {last_error}");
    }
    println!("counts:");
    for (event_type, count) in &summary.counts_by_type {
        println!("  {event_type}: {count}");
    }
    println!("artifacts: {}", summary.artifact_count);
    for artifact in &summary.artifacts {
        println!(
            "  {} bytes={} digest={} tool_call={} tool={} child_session={} present={}",
            artifact.path,
            artifact
                .bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
            artifact.digest.as_deref().unwrap_or("<unknown>"),
            artifact.tool_call_id.as_deref().unwrap_or("<unknown>"),
            artifact.tool_id.as_deref().unwrap_or("<unknown>"),
            artifact.child_session_id.as_deref().unwrap_or("<unknown>"),
            if artifact.present_on_disk {
                "yes"
            } else {
                "no"
            }
        );
    }
    println!("child_sessions: {}", summary.child_session_count);
    for child in &summary.child_sessions {
        println!(
            "  {} profile={} provider/model={} parent_tool_call={} state={} elapsed_ms={}",
            child.session_id,
            child.profile.as_deref().unwrap_or("<unknown>"),
            child.provider_model.as_deref().unwrap_or("<unknown>"),
            child.parent_tool_call_id.as_deref().unwrap_or("<unknown>"),
            child.terminal_state.as_deref().unwrap_or("<unknown>"),
            child
                .elapsed_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        );
    }
}

#[derive(Debug, Clone, Default)]
struct SessionDiscovery {
    artifacts: Vec<ArtifactInspection>,
    child_sessions: Vec<ChildSessionInspection>,
}

#[derive(Debug, Clone, Default)]
struct ToolCallDiscovery {
    tool_id: Option<String>,
    child_session_id: Option<String>,
}

fn inspect_session_discovery(run_dir: &Path, events: &[EventEnvelopeV1]) -> SessionDiscovery {
    let resume_plan = inspect_resume_plan(run_dir);
    SessionDiscovery {
        artifacts: inspect_artifacts(run_dir, events, &resume_plan.tool_calls),
        child_sessions: inspect_child_sessions(&resume_plan),
    }
}

fn inspect_artifacts(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    tool_calls: &BTreeMap<String, harness_core::proj::ResumeToolCallSnapshot>,
) -> Vec<ArtifactInspection> {
    let tool_call_lookup = tool_calls
        .iter()
        .map(|(tool_call_id, snapshot)| {
            (
                tool_call_id.clone(),
                ToolCallDiscovery {
                    tool_id: snapshot.tool_id.clone().or_else(|| {
                        snapshot
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.canonical_tool_id.clone())
                    }),
                    child_session_id: snapshot
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.lineage.as_ref())
                        .and_then(|lineage| lineage.child_session_id.clone()),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut artifacts = BTreeMap::<String, ArtifactInspection>::new();
    collect_artifact_files(&run_dir.join("artifacts"), run_dir, &mut artifacts);

    for event in events {
        let EventV1::ArtifactWritten(payload) = &event.payload else {
            continue;
        };

        let entry = artifacts
            .entry(payload.path.clone())
            .or_insert_with(|| ArtifactInspection {
                path: payload.path.clone(),
                bytes: None,
                digest: None,
                tool_call_id: None,
                tool_id: None,
                child_session_id: None,
                present_on_disk: run_dir.join(&payload.path).exists(),
            });
        entry.bytes.get_or_insert(payload.bytes);
        entry.digest.get_or_insert_with(|| payload.digest.clone());
        if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
            entry
                .tool_call_id
                .get_or_insert_with(|| tool_call_id.clone());
            if let Some(tool_call) = tool_call_lookup.get(tool_call_id) {
                if entry.tool_id.is_none() {
                    entry.tool_id = tool_call.tool_id.clone();
                }
                if entry.child_session_id.is_none() {
                    entry.child_session_id = tool_call.child_session_id.clone();
                }
            }
        }
    }

    artifacts.into_values().collect()
}

fn collect_artifact_files(
    artifacts_dir: &Path,
    run_dir: &Path,
    artifacts: &mut BTreeMap<String, ArtifactInspection>,
) {
    let Ok(read_dir) = fs::read_dir(artifacts_dir) else {
        return;
    };

    for entry in read_dir.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_artifact_files(&path, run_dir, artifacts);
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let relative = path
            .strip_prefix(run_dir)
            .map(normalize_path)
            .unwrap_or_else(|_| normalize_path(&path));
        let bytes = path.metadata().ok().map(|metadata| metadata.len());
        artifacts
            .entry(relative.clone())
            .or_insert(ArtifactInspection {
                path: relative,
                bytes,
                digest: None,
                tool_call_id: None,
                tool_id: None,
                child_session_id: None,
                present_on_disk: true,
            });
    }
}

fn inspect_child_sessions(
    resume_plan: &harness_core::proj::ResumePlan,
) -> Vec<ChildSessionInspection> {
    resume_plan
        .child_sessions
        .iter()
        .filter(|(_, child)| {
            child.parent_session_id.is_some()
                || child.parent_tool_call_id.is_some()
                || child.parent_task_id.is_some()
                || child.parent_request_id.is_some()
        })
        .map(|(session_id, child)| ChildSessionInspection {
            session_id: session_id.clone(),
            parent_session_id: child.parent_session_id.clone(),
            parent_tool_call_id: child.parent_tool_call_id.clone(),
            parent_task_id: child.parent_task_id.clone(),
            parent_request_id: child.parent_request_id.clone(),
            latest_child_request_id: child.latest_child_request_id.clone(),
            profile: child.profile.clone(),
            provider_model: match (child.provider_id.as_deref(), child.model_id.as_deref()) {
                (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
                (Some(provider), None) => Some(format!("{provider}/<unknown>")),
                (None, Some(model)) => Some(format!("<unknown>/{model}")),
                (None, None) => None,
            },
            terminal_state: child.terminal_state.map(child_session_terminal_state_label),
            terminal_reason: child.terminal_reason.clone(),
            elapsed_ms: child.timing.as_ref().and_then(|timing| timing.elapsed_ms),
        })
        .collect()
}

fn child_session_terminal_state_label(state: ChildSessionTerminalState) -> String {
    match state {
        ChildSessionTerminalState::Completed => "completed",
        ChildSessionTerminalState::Cancelled => "cancelled",
        ChildSessionTerminalState::LateResult => "late_result",
    }
    .to_string()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
