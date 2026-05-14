use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::event::{EventEnvelopeV1, EventV1, RunStartedEvent};
use harness_core::proj::{
    project_resume_plan, project_run_summary, project_session_catalog_entry, project_team_state,
    ChildSessionTerminalState, RunStatus, SessionCatalogEntry, SessionCatalogMetadata,
};
use serde::Serialize;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME, META_FILE_NAME};
use crate::cli_labels::provider_model_label;
use crate::defaults::RESUME_UNAVAILABLE_FALLBACK_REASON;

#[derive(Debug, Args, Clone)]
pub struct ReplayCommand {
    #[arg(long)]
    pub session: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayArtifactSummary {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub present_on_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayChildSessionSummary {
    pub child_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_child_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<ChildSessionTerminalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_terminal_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_terminal_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivered_turn_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_tool_call_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
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
    pub counts_by_type: BTreeMap<String, u64>,
    pub pending_permissions: Vec<String>,
    pub tasks_in_flight: Vec<String>,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ReplayArtifactSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_sessions: Vec<ReplayChildSessionSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<ReplayTeamSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayTeamSummary {
    pub team_run_id: String,
    pub name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_status: Option<String>,
    pub member_count: usize,
    pub running_members: u32,
    pub pending_members: u32,
    pub message_count: usize,
    pub task_count: usize,
    pub shutdown_request_count: usize,
    pub member_turns: u32,
}

#[derive(Debug, Clone)]
pub struct SessionInspectionEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
    pub sort_unix_ms: u128,
    pub artifact_count: usize,
    pub child_session_count: usize,
}

impl SessionInspectionEntry {
    pub(crate) fn sort_by_updated_desc(entries: &mut [Self]) {
        entries.sort_by_key(|entry| (Reverse(entry.sort_unix_ms), entry.run_dir.clone()));
    }

    pub(crate) fn sort_by_updated_asc(entries: &mut [Self]) {
        entries.sort_by_key(|entry| (entry.sort_unix_ms, entry.run_dir.clone()));
    }
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
    let events = load_events_from_run_dir(run_dir)?;
    if events.is_empty() {
        return Err(format!("no events found in {}", run_dir.display()));
    }

    let projection = project_run_summary(events.iter()).map_err(|err| err.to_string())?;
    let run_id = events[0].run_id.clone();
    let catalog = project_session_catalog_entry(events.iter(), &run_id, None, None, None)
        .map_err(|err| err.to_string())?;
    let run_name = run_started_event(&events).map(|data| data.run_name.clone());
    let (artifacts, child_sessions) = summarize_recovery_story(run_dir, &events, &run_id);
    let teams = summarize_teams(&events);

    Ok(ReplaySummary {
        run_id,
        run_name,
        session_path: run_dir.to_path_buf(),
        status: projection.status,
        workspace_root: catalog.workspace_root,
        mode_source: catalog.mode_source,
        is_resumable: catalog.is_resumable,
        resume_disabled_reason: catalog.resume_disabled_reason,
        artifact_count: artifacts.len(),
        child_session_count: child_sessions.len(),
        parent_session_id: catalog.parent_session_id,
        total_events: projection.counts.total_events,
        counts_by_type: projection.counts.by_type,
        pending_permissions: projection.pending_permissions.into_iter().collect(),
        tasks_in_flight: projection.tasks_in_flight.into_iter().collect(),
        last_error: projection.last_error,
        artifacts,
        child_sessions,
        teams,
    })
}

fn summarize_teams(events: &[EventEnvelopeV1]) -> Vec<ReplayTeamSummary> {
    let Ok(projection) = project_team_state(events.iter()) else {
        return Vec::new();
    };
    projection
        .teams
        .into_iter()
        .map(|(team_run_id, team)| ReplayTeamSummary {
            team_run_id,
            name: team.name,
            status: format!("{:?}", team.status),
            lead_profile: team.lead.as_ref().and_then(|lead| lead.profile.clone()),
            lead_status: team.lead.as_ref().map(|lead| format!("{:?}", lead.status)),
            member_count: team.members.len(),
            running_members: team.bounds_consumption.running_members,
            pending_members: team.bounds_consumption.pending_members,
            message_count: team.messages.len(),
            task_count: team.tasks.len(),
            shutdown_request_count: team.shutdown_requests.len(),
            member_turns: team.bounds_consumption.member_turns,
        })
        .collect()
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
        .filter(|path| path.is_dir() && path.join(EVENTS_FILE_NAME).exists())
        .map(|run_dir| inspect_single_session(&run_dir))
        .map(normalize_lineage_entry)
        .collect::<Vec<_>>();

    SessionInspectionEntry::sort_by_updated_desc(&mut entries);

    Ok(entries)
}

pub(crate) fn normalize_lineage_entry(mut entry: SessionInspectionEntry) -> SessionInspectionEntry {
    if let Some(parent_run_id) = harness_lineage_parent_run_id(&entry.run_dir) {
        entry.catalog.parent_session_id = Some(parent_run_id);
    }
    entry
}

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    let body = fs::read_to_string(run_dir.join(META_FILE_NAME)).ok()?;
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

fn inspect_single_session(run_dir: &Path) -> SessionInspectionEntry {
    let run_id_fallback = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown-run>");

    let (events, events_error) = load_events_lossy(run_dir);
    let (meta, _meta_error) = load_meta_lossy(run_dir);

    let modified = run_dir
        .join(EVENTS_FILE_NAME)
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
            run_name: run_started_event(&events).map(|data| data.run_name.clone()),
            status: None,
            last_updated_at,
            workspace_root: run_started_event(&events).map(|data| data.workspace_root.clone()),
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

    let (artifact_count, child_session_count) =
        summarize_recovery_counts(run_dir, &events, run_id_fallback);

    SessionInspectionEntry {
        run_dir: run_dir.to_path_buf(),
        catalog,
        sort_unix_ms,
        artifact_count,
        child_session_count,
    }
}

fn run_started_event(events: &[EventEnvelopeV1]) -> Option<&RunStartedEvent> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data),
        _ => None,
    })
}

fn summarize_recovery_counts(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    fallback_run_id: &str,
) -> (usize, usize) {
    let (artifacts, child_sessions) = summarize_recovery_story(run_dir, events, fallback_run_id);
    (artifacts.len(), child_sessions.len())
}

fn summarize_recovery_story(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    fallback_run_id: &str,
) -> (Vec<ReplayArtifactSummary>, Vec<ReplayChildSessionSummary>) {
    let resume_plan = project_resume_plan(events.iter(), fallback_run_id).ok();

    let mut artifacts = artifact_inventory(run_dir, events, resume_plan.as_ref());
    artifacts.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.tool_call_id.cmp(&right.tool_call_id))
            .then_with(|| left.digest.cmp(&right.digest))
    });

    let mut child_artifact_paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut child_tool_call_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if let Some(resume_plan) = resume_plan.as_ref() {
        for (tool_call_id, tool_call) in &resume_plan.tool_calls {
            let Some(metadata) = tool_call.metadata.as_ref() else {
                continue;
            };
            let Some(lineage) = metadata.lineage.as_ref() else {
                continue;
            };
            let Some(child_session_id) = lineage.child_session_id.as_ref() else {
                continue;
            };

            child_tool_call_ids
                .entry(child_session_id.clone())
                .or_default()
                .insert(tool_call_id.clone());
            for artifact_ref in &metadata.artifact_refs {
                child_artifact_paths
                    .entry(child_session_id.clone())
                    .or_default()
                    .insert(artifact_ref.path.clone());
            }
        }
    }

    let mut child_sessions = resume_plan
        .as_ref()
        .map(|resume_plan| {
            resume_plan
                .child_sessions
                .iter()
                .filter(|(_, child)| {
                    child.parent_session_id.is_some()
                        || child.parent_tool_call_id.is_some()
                        || child.parent_task_id.is_some()
                        || child.parent_request_id.is_some()
                })
                .map(|(child_session_id, child)| ReplayChildSessionSummary {
                    child_session_id: child_session_id.clone(),
                    profile: child.profile.clone(),
                    provider_model: provider_model_label(
                        child.provider_id.as_deref(),
                        child.model_id.as_deref(),
                    ),
                    latest_child_request_id: child.latest_child_request_id.clone(),
                    parent_session_id: child.parent_session_id.clone(),
                    parent_tool_call_id: child.parent_tool_call_id.clone(),
                    parent_task_id: child.parent_task_id.clone(),
                    parent_request_id: child.parent_request_id.clone(),
                    terminal_state: child.terminal_state,
                    terminal_reason: child.terminal_reason.clone(),
                    notification_status: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.status.as_str().to_string()),
                    notification_summary: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.summary.clone()),
                    notification_terminal_event_id: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.terminal_event_id.clone()),
                    notification_terminal_task_id: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.terminal_task_id.clone()),
                    notification_delivered_turn_request_id: child
                        .background_notification
                        .as_ref()
                        .and_then(|notification| notification.delivered_turn_request_id.clone()),
                    elapsed_ms: child.timing.as_ref().and_then(|timing| timing.elapsed_ms),
                    related_tool_call_ids: child_tool_call_ids
                        .remove(child_session_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    artifact_paths: child_artifact_paths
                        .remove(child_session_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    next_actions: child_session_next_actions(
                        child_session_id,
                        child.latest_child_request_id.as_deref(),
                        child.terminal_state,
                    ),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    child_sessions.sort_by(|left, right| left.child_session_id.cmp(&right.child_session_id));

    (artifacts, child_sessions)
}

fn child_session_next_actions(
    child_session_id: &str,
    child_request_id: Option<&str>,
    terminal_state: Option<ChildSessionTerminalState>,
) -> Vec<String> {
    let mut actions = Vec::new();
    if let Some(request_id) = child_request_id {
        actions.push(format!(
            "background_output(request_id=\"{}\", block=false)",
            sanitize_human_text(request_id)
        ));
        if terminal_state.is_none() {
            actions.push(format!(
                "background_output(request_id=\"{}\", block=true)",
                sanitize_human_text(request_id)
            ));
        }
    }
    actions.push(format!(
        "task(session_id=\"{}\", run_in_background=false, load_skills=[], prompt=\"Continue this child task with additional instructions.\")",
        sanitize_human_text(child_session_id)
    ));
    actions
}

#[derive(Debug, Clone, Default)]
struct ToolCallDiscovery {
    tool_id: Option<String>,
    effective_tool_id: Option<String>,
    canonical_tool_id: Option<String>,
    alias_source_tool_id: Option<String>,
    child_session_id: Option<String>,
}

fn artifact_inventory(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    resume_plan: Option<&harness_core::proj::ResumePlan>,
) -> Vec<ReplayArtifactSummary> {
    let mut by_key: BTreeMap<(String, Option<String>, Option<String>), ReplayArtifactSummary> =
        BTreeMap::new();
    let tool_call_lookup = tool_call_discovery_lookup(resume_plan);

    for event in events {
        let EventV1::ArtifactWritten(payload) = &event.payload else {
            continue;
        };
        let discovery = payload
            .tool_call_id
            .as_ref()
            .and_then(|tool_call_id| tool_call_lookup.get(tool_call_id));
        let entry = by_key
            .entry((
                payload.path.clone(),
                payload.tool_call_id.clone(),
                Some(payload.digest.clone()),
            ))
            .or_insert_with(|| {
                artifact_summary(
                    payload.path.clone(),
                    Some(payload.digest.clone()),
                    Some(payload.bytes),
                    payload.tool_call_id.clone(),
                    run_dir.join(&payload.path).exists(),
                )
            });
        fill_missing_artifact_identity(
            entry,
            discovery,
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        );
        if entry.digest.is_none() {
            entry.digest = Some(payload.digest.clone());
        }
        if entry.bytes.is_none() {
            entry.bytes = Some(payload.bytes);
        }
        if entry.tool_call_id.is_none() {
            entry.tool_call_id = payload.tool_call_id.clone();
        }
        entry.present_on_disk |= run_dir.join(&payload.path).exists();
    }

    if let Some(resume_plan) = resume_plan {
        for (tool_call_id, tool_call) in &resume_plan.tool_calls {
            let Some(metadata) = tool_call.metadata.as_ref() else {
                continue;
            };
            let discovery = tool_call_lookup.get(tool_call_id);
            for artifact_ref in &metadata.artifact_refs {
                let entry = by_key
                    .entry((
                        artifact_ref.path.clone(),
                        Some(tool_call_id.clone()),
                        artifact_ref.digest.clone(),
                    ))
                    .or_insert_with(|| {
                        artifact_summary(
                            artifact_ref.path.clone(),
                            artifact_ref.digest.clone(),
                            None,
                            Some(tool_call_id.clone()),
                            run_dir.join(&artifact_ref.path).exists(),
                        )
                    });
                fill_missing_artifact_identity(
                    entry,
                    discovery,
                    metadata.canonical_tool_id.as_deref(),
                    metadata.alias_source_tool_id.as_deref(),
                );
                if entry.digest.is_none() {
                    entry.digest = artifact_ref.digest.clone();
                }
                if entry.tool_call_id.is_none() {
                    entry.tool_call_id = Some(tool_call_id.clone());
                }
                entry.present_on_disk |= run_dir.join(&artifact_ref.path).exists();
            }
        }
    }

    let mut on_disk = BTreeMap::new();
    collect_artifact_files(&run_dir.join("artifacts"), run_dir, &mut on_disk);
    for (path, bytes) in on_disk {
        let mut matched = false;
        for artifact in by_key.values_mut() {
            if artifact.path == path {
                matched = true;
                artifact.present_on_disk = true;
                if artifact.bytes.is_none() {
                    artifact.bytes = Some(bytes);
                }
            }
        }
        if !matched {
            by_key.insert(
                (path.clone(), None, None),
                artifact_summary(path, None, Some(bytes), None, true),
            );
        }
    }

    by_key.into_values().collect()
}

fn artifact_summary(
    path: String,
    digest: Option<String>,
    bytes: Option<u64>,
    tool_call_id: Option<String>,
    present_on_disk: bool,
) -> ReplayArtifactSummary {
    ReplayArtifactSummary {
        path,
        digest,
        bytes,
        tool_call_id,
        tool_id: None,
        effective_tool_id: None,
        canonical_tool_id: None,
        alias_source_tool_id: None,
        child_session_id: None,
        present_on_disk,
    }
}

fn fill_missing_artifact_identity(
    entry: &mut ReplayArtifactSummary,
    discovery: Option<&ToolCallDiscovery>,
    canonical_tool_id_fallback: Option<&str>,
    alias_source_tool_id_fallback: Option<&str>,
) {
    if entry.tool_id.is_none() {
        entry.tool_id = discovery.and_then(|it| it.tool_id.clone());
    }
    if entry.effective_tool_id.is_none() {
        entry.effective_tool_id = discovery.and_then(|it| it.effective_tool_id.clone());
    }
    if entry.canonical_tool_id.is_none() {
        entry.canonical_tool_id = discovery
            .and_then(|it| it.canonical_tool_id.clone())
            .or_else(|| canonical_tool_id_fallback.map(str::to_string));
    }
    if entry.alias_source_tool_id.is_none() {
        entry.alias_source_tool_id = discovery
            .and_then(|it| it.alias_source_tool_id.clone())
            .or_else(|| alias_source_tool_id_fallback.map(str::to_string));
    }
    if entry.child_session_id.is_none() {
        entry.child_session_id = discovery.and_then(|it| it.child_session_id.clone());
    }
}

fn tool_call_discovery_lookup(
    resume_plan: Option<&harness_core::proj::ResumePlan>,
) -> BTreeMap<String, ToolCallDiscovery> {
    resume_plan
        .map(|resume_plan| {
            resume_plan
                .tool_calls
                .iter()
                .map(|(tool_call_id, snapshot)| {
                    let metadata = snapshot.metadata.as_ref();
                    (
                        tool_call_id.clone(),
                        ToolCallDiscovery {
                            tool_id: snapshot
                                .tool_id
                                .clone()
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.invoked_tool_id.clone())
                                })
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.effective_tool_id.clone())
                                })
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone())),
                            effective_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.effective_tool_id.clone())
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.invoked_tool_id.clone())
                                })
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone()))
                                .or_else(|| snapshot.tool_id.clone()),
                            canonical_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.canonical_tool_id.clone())
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone())),
                            alias_source_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.alias_source_tool_id.clone())
                                .or_else(|| {
                                    metadata.and_then(|it| it.alias_source_tool_id.clone())
                                }),
                            child_session_id: metadata
                                .and_then(|it| it.lineage.as_ref())
                                .and_then(|lineage| lineage.child_session_id.clone()),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_artifact_files(
    artifacts_dir: &Path,
    run_dir: &Path,
    artifacts: &mut BTreeMap<String, u64>,
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

        let relative_path = path.strip_prefix(run_dir).unwrap_or(&path);
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let bytes = path
            .metadata()
            .ok()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        artifacts.entry(relative).or_insert(bytes);
    }
}

fn load_events_lossy(run_dir: &Path) -> (Vec<EventEnvelopeV1>, Option<String>) {
    let events_path = run_dir.join(EVENTS_FILE_NAME);
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
    let meta_path = run_dir.join(META_FILE_NAME);
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
    let session_path = sanitize_human_text(&summary.session_path.display().to_string());

    println!("run_id: {}", sanitize_human_text(&summary.run_id));
    println!(
        "run_name: {}",
        summary
            .run_name
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    println!("session_path: {session_path}");
    println!("status: {:?}", summary.status);
    println!(
        "workspace_root: {}",
        summary
            .workspace_root
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "<unknown>".to_string())
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
                .map(|reason| format!("blocked · {}", sanitize_human_text(reason)))
                .unwrap_or_else(|| "blocked".to_string())
        }
    );
    println!(
        "parent_session: {}",
        summary
            .parent_session_id
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "-".to_string())
    );
    println!("child_sessions: {}", summary.child_session_count);
    println!("artifacts: {}", summary.artifact_count);
    println!("teams: {}", summary.teams.len());
    println!("total_events: {}", summary.total_events);
    if let Some(last_error) = &summary.last_error {
        println!("last_error: {}", sanitize_human_text(last_error));
    }
    println!("next_steps:");
    println!("  replay_tui: harness tui --replay {session_path}");
    if summary.is_resumable {
        println!("  continue_tui: harness tui --continue {session_path}");
    } else {
        println!(
            "  continue_tui: unavailable ({})",
            summary
                .resume_disabled_reason
                .as_deref()
                .map(sanitize_human_text)
                .unwrap_or_else(|| RESUME_UNAVAILABLE_FALLBACK_REASON.to_string())
        );
    }
    println!("counts:");
    for (event_type, count) in &summary.counts_by_type {
        println!("  {event_type}: {count}");
    }

    println!("artifacts: {}", summary.artifact_count);
    if summary.artifacts.is_empty() {
        println!("  <none>");
    } else {
        for artifact in &summary.artifacts {
            println!("  - {}", sanitize_human_text(&artifact.path));
            let mut details = Vec::new();
            if let Some(digest) = artifact.digest.as_deref() {
                details.push(format!("digest={}", short_digest(digest)));
            }
            if let Some(bytes) = artifact.bytes {
                details.push(format!("bytes={bytes}"));
            }
            push_sanitized_detail(&mut details, "tool_call", artifact.tool_call_id.as_deref());
            push_sanitized_detail(&mut details, "tool", artifact.tool_id.as_deref());
            push_sanitized_detail(
                &mut details,
                "effective",
                artifact.effective_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "canonical",
                artifact.canonical_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "alias",
                artifact.alias_source_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "child_session",
                artifact.child_session_id.as_deref(),
            );
            details.push(format!(
                "present={}",
                if artifact.present_on_disk {
                    "yes"
                } else {
                    "no"
                }
            ));
            if !details.is_empty() {
                println!("    {}", details.join(", "));
            }
        }
    }

    println!("child_sessions: {}", summary.child_session_count);
    if summary.child_sessions.is_empty() {
        println!("  <none>");
    } else {
        for child in &summary.child_sessions {
            println!("  - {}", sanitize_human_text(&child.child_session_id));
            let mut details = Vec::new();
            push_sanitized_detail(&mut details, "profile", child.profile.as_deref());
            push_sanitized_detail(
                &mut details,
                "provider_model",
                child.provider_model.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "request",
                child.latest_child_request_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "parent_tool",
                child.parent_tool_call_id.as_deref(),
            );
            push_sanitized_detail(&mut details, "parent_task", child.parent_task_id.as_deref());
            push_sanitized_detail(
                &mut details,
                "parent_request",
                child.parent_request_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "parent_session",
                child.parent_session_id.as_deref(),
            );
            if let Some(terminal_state) = child.terminal_state {
                details.push(format!("state={terminal_state:?}"));
            }
            push_sanitized_detail(
                &mut details,
                "notification",
                child.notification_status.as_deref(),
            );
            if let Some(elapsed_ms) = child.elapsed_ms {
                details.push(format!("elapsed_ms={elapsed_ms}"));
            }
            push_sanitized_detail(&mut details, "reason", child.terminal_reason.as_deref());
            push_sanitized_detail(
                &mut details,
                "notification_summary",
                child.notification_summary.as_deref(),
            );
            if !details.is_empty() {
                println!("    {}", details.join(", "));
            }
            if !child.related_tool_call_ids.is_empty() {
                println!(
                    "    tool_calls={}",
                    child
                        .related_tool_call_ids
                        .iter()
                        .map(|value| sanitize_human_text(value))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !child.artifact_paths.is_empty() {
                println!(
                    "    artifacts={}",
                    child
                        .artifact_paths
                        .iter()
                        .map(|value| sanitize_human_text(value))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !child.next_actions.is_empty() {
                println!("    next_actions:");
                for action in &child.next_actions {
                    println!("      - {}", sanitize_human_text(action));
                }
            }
        }
    }

    println!("teams: {}", summary.teams.len());
    if summary.teams.is_empty() {
        println!("  <none>");
    } else {
        for team in &summary.teams {
            println!(
                "  - {} ({})",
                sanitize_human_text(&team.name),
                sanitize_human_text(&team.team_run_id)
            );
            println!(
                "    status={}, members={} (running={}, pending={}), messages={}, tasks={}, shutdown_requests={}, member_turns={}",
                sanitize_human_text(&team.status),
                team.member_count,
                team.running_members,
                team.pending_members,
                team.message_count,
                team.task_count,
                team.shutdown_request_count,
                team.member_turns
            );
            if let Some(lead_profile) = team.lead_profile.as_deref() {
                println!(
                    "    lead={} ({})",
                    sanitize_human_text(lead_profile),
                    sanitize_human_text(team.lead_status.as_deref().unwrap_or("unknown"))
                );
            }
        }
    }
}

fn sanitize_human_text(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(&mut sanitized, "\\u{{{:x}}}", c as u32);
            }
            _ => sanitized.push(ch),
        }
    }
    sanitized
}

fn push_sanitized_detail(details: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        details.push(format!("{key}={}", sanitize_human_text(value)));
    }
}

fn short_digest(digest: &str) -> String {
    digest.chars().take(12).collect()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
    };
    use tempfile::tempdir;

    fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: run_id.to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("test".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        }
    }

    fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize event"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(run_dir.join(EVENTS_FILE_NAME), format!("{body}\n")).expect("write events");
    }

    #[test]
    fn test_inspect_session_catalog_returns_valid_entries() {
        let session_dir = tempdir().expect("tempdir");

        // Valid session 1
        let run1_dir = session_dir.path().join("run1");
        std::fs::create_dir_all(&run1_dir).expect("create run dir");
        write_events_jsonl(
            &run1_dir,
            &[envelope(
                "run1_id",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "run1-name".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            )],
        );

        // Valid session 2
        let run2_dir = session_dir.path().join("run2");
        std::fs::create_dir_all(&run2_dir).expect("create run dir");
        write_events_jsonl(
            &run2_dir,
            &[envelope(
                "run2_id",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "run2-name".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            )],
        );

        // Invalid session (no events.jsonl)
        let run3_dir = session_dir.path().join("run3");
        std::fs::create_dir_all(&run3_dir).expect("create run dir");

        // File that is not a directory
        let file_path = session_dir.path().join("not_a_dir.txt");
        std::fs::write(&file_path, "test").expect("write file");

        let entries = inspect_session_catalog(session_dir.path()).expect("inspect catalog");

        assert_eq!(entries.len(), 2);

        let mut run_ids: Vec<_> = entries.iter().map(|e| e.catalog.run_id.clone()).collect();
        run_ids.sort();

        assert_eq!(run_ids, vec!["run1_id", "run2_id"]);
    }

    #[test]
    fn test_inspect_session_catalog_invalid_dir() {
        let temp = tempdir().expect("tempdir");
        let missing_dir = temp.path().join("missing");

        let err = inspect_session_catalog(&missing_dir).unwrap_err();

        assert!(err.contains("failed to read session directory"));
    }
}
