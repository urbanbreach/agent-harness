use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::event::{
    EventArtifactRef, EventEnvelopeV1, EventV1, TaskCompletionMetadata, ToolCallMetadata,
    ToolCallStatus,
};

const EVENTS_FILE_NAME: &str = "events.jsonl";
const REQUEST_ID_PREFIX: &str = "req_";
const TASK_ID_PREFIX: &str = "task_";
const TOOL_CALL_ID_PREFIX: &str = "toolcall_";
const PERMISSION_ID_PREFIX: &str = "perm_";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionModeSource {
    InteractiveLive,
    InteractiveMock,
    Prompt,
    ScenarioFixture,
    ReplayOnly,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionCatalogMetadata {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_name: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub profile_preset: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode_source: Option<SessionModeSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCatalogEntry {
    pub run_id: String,
    pub run_name: Option<String>,
    pub status: Option<RunStatus>,
    pub last_updated_at: Option<String>,
    pub workspace_root: Option<String>,
    pub profile_preset: Option<String>,
    pub provider_model: Option<String>,
    pub mode_source: SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleSegmentStatus {
    #[default]
    Missing,
    Active,
    Finished,
    Failed,
}

impl LifecycleSegmentStatus {
    fn run_status(self) -> Option<RunStatus> {
        match self {
            Self::Missing => None,
            Self::Active => Some(RunStatus::Running),
            Self::Finished => Some(RunStatus::Finished),
            Self::Failed => Some(RunStatus::Failed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResumeIdWatermarks {
    pub max_request_id: u64,
    pub max_task_id: u64,
    pub max_tool_call_id: u64,
    pub max_permission_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResumeToolCallSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ToolCallStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResumeTaskSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskCompletionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePlan {
    pub run_id: String,
    pub latest_lifecycle_status: LifecycleSegmentStatus,
    pub max_seq: u64,
    pub id_watermarks: ResumeIdWatermarks,
    pub known_agents: BTreeMap<String, String>,
    pub known_profiles: BTreeSet<String>,
    pub pending_permissions: BTreeSet<String>,
    pub tasks_in_flight: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_calls: BTreeMap<String, ResumeToolCallSnapshot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub completed_tasks: BTreeMap<String, ResumeTaskSnapshot>,
    pub workspace_root: Option<String>,
    pub provider_model: Option<String>,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
}

impl ResumePlan {
    pub fn run_status(&self) -> Option<RunStatus> {
        self.latest_lifecycle_status.run_status()
    }

    fn blocked(run_id: String, reason: String) -> Self {
        Self {
            run_id,
            latest_lifecycle_status: LifecycleSegmentStatus::Missing,
            max_seq: 0,
            id_watermarks: ResumeIdWatermarks::default(),
            known_agents: BTreeMap::new(),
            known_profiles: BTreeSet::new(),
            pending_permissions: BTreeSet::new(),
            tasks_in_flight: BTreeSet::new(),
            tool_calls: BTreeMap::new(),
            completed_tasks: BTreeMap::new(),
            workspace_root: None,
            provider_model: None,
            is_resumable: false,
            resume_disabled_reason: Some(reason),
        }
    }
}

impl SessionCatalogEntry {
    pub fn is_default_picker_candidate(&self) -> bool {
        matches!(
            self.mode_source,
            SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RunCounts {
    pub total_events: u64,
    pub by_type: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub status: RunStatus,
    pub counts: RunCounts,
    pub last_error: Option<String>,
    pub tasks_in_flight: BTreeSet<String>,
    pub pending_permissions: BTreeSet<String>,
}

impl Default for RunSummary {
    fn default() -> Self {
        Self {
            status: RunStatus::Running,
            counts: RunCounts::default(),
            last_error: None,
            tasks_in_flight: BTreeSet::new(),
            pending_permissions: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEventRef {
    pub seq: u64,
    pub event_id: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
    pub event_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimelineIndex {
    pub events: Vec<TimelineEventRef>,
    pub correlation_groups: BTreeMap<String, Vec<u64>>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    #[error("events must be strictly increasing by seq: previous={previous}, current={current}")]
    NonMonotonicSeq { previous: u64, current: u64 },
    #[error("events must be contiguous by seq: expected={expected}, current={current}")]
    NonContiguousSeq { expected: u64, current: u64 },
    #[error("events contain multiple run ids: expected={expected}, actual={actual}")]
    RunIdMismatch { expected: String, actual: String },
    #[error(
        "invalid {counter_kind} id `{id}`; expected prefix `{expected_prefix}` followed by digits"
    )]
    InvalidCounterId {
        counter_kind: &'static str,
        id: String,
        expected_prefix: &'static str,
    },
}

pub fn project_run_summary<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<RunSummary, ProjectionError> {
    let mut summary = RunSummary::default();
    let mut last_seq: Option<u64> = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_run_summary_event(&mut summary, event);
    }

    Ok(summary)
}

pub fn project_timeline_index<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<TimelineIndex, ProjectionError> {
    let mut index = TimelineIndex::default();
    let mut last_seq: Option<u64> = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_timeline_event(&mut index, event);
    }

    Ok(index)
}

pub fn inspect_resume_plan(run_dir: &Path) -> ResumePlan {
    let fallback_run_id = fallback_run_id_from_path(run_dir);
    let events = match read_events_for_resume_inspection(run_dir) {
        Ok(events) => events,
        Err(reason) => return ResumePlan::blocked(fallback_run_id, reason),
    };

    match project_resume_plan(events.iter(), &fallback_run_id) {
        Ok(plan) => plan,
        Err(err) => ResumePlan::blocked(
            fallback_run_id,
            format!("event log is corrupt or non-monotonic: {err}"),
        ),
    }
}

pub fn project_resume_plan<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    fallback_run_id: &str,
) -> Result<ResumePlan, ProjectionError> {
    let mut latest_lifecycle_status = LifecycleSegmentStatus::Missing;
    let mut id_watermarks = ResumeIdWatermarks::default();
    let mut known_agents = BTreeMap::new();
    let mut known_profiles = BTreeSet::new();
    let mut pending_permissions = BTreeSet::new();
    let mut tasks_in_flight = BTreeSet::new();
    let mut tool_calls = BTreeMap::new();
    let mut completed_tasks = BTreeMap::new();
    let mut workspace_root = None;
    let mut provider_model = None;
    let mut run_id: Option<String> = None;
    let mut max_seq = 0_u64;
    let mut expected_seq = 1_u64;

    for event in events {
        if event.seq != expected_seq {
            return Err(ProjectionError::NonContiguousSeq {
                expected: expected_seq,
                current: event.seq,
            });
        }
        expected_seq += 1;
        max_seq = event.seq;

        match run_id.as_deref() {
            None => run_id = Some(event.run_id.clone()),
            Some(existing) if existing == event.run_id.as_str() => {}
            Some(existing) => {
                return Err(ProjectionError::RunIdMismatch {
                    expected: existing.to_string(),
                    actual: event.run_id.clone(),
                })
            }
        }

        match &event.payload {
            EventV1::RunStarted(payload) => {
                latest_lifecycle_status = LifecycleSegmentStatus::Active;
                workspace_root = Some(payload.workspace_root.clone());
                known_agents.clear();
                known_profiles.clear();
                pending_permissions.clear();
                tasks_in_flight.clear();
                tool_calls.clear();
                completed_tasks.clear();
            }
            EventV1::RunFinished(_) => {
                if latest_lifecycle_status != LifecycleSegmentStatus::Missing {
                    latest_lifecycle_status = LifecycleSegmentStatus::Finished;
                }
            }
            EventV1::RunFailed(_) => {
                if latest_lifecycle_status != LifecycleSegmentStatus::Missing {
                    latest_lifecycle_status = LifecycleSegmentStatus::Failed;
                }
            }
            EventV1::AgentSpawned(payload) => {
                known_agents.insert(payload.agent_id.clone(), payload.profile.clone());
                known_profiles.insert(payload.profile.clone());
            }
            EventV1::TaskScheduled(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.insert(payload.task_id.clone());
            }
            EventV1::TaskCancelled(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::TaskCompleted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(&payload.task_id);
                completed_tasks.insert(
                    payload.task_id.clone(),
                    ResumeTaskSnapshot {
                        result_digest: Some(payload.result_digest.clone()),
                        metadata: payload.metadata.clone(),
                    },
                );
            }
            EventV1::TaskResultLate(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::ProviderRequestStarted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
                provider_model = Some(format!("{}/{}", payload.provider_id, payload.model_id));
            }
            EventV1::ProviderStreamDelta(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ProviderRequestFinished(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ToolCallRequested(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    &payload.tool_call_id,
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.clone())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.tool_id = Some(payload.tool_id.clone());
                if let Some(metadata) = payload.metadata.clone() {
                    merge_tool_call_metadata(tool_call, metadata);
                }
            }
            EventV1::ToolCallStarted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    &payload.tool_call_id,
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
            }
            EventV1::ToolCallFinished(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    &payload.tool_call_id,
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.clone())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.status = Some(payload.status);
                tool_call.output_digest = payload.output_digest.clone();
                tool_call.output_json = payload.output_json.clone();
                if let Some(metadata) = payload.metadata.clone() {
                    merge_tool_call_metadata(tool_call, metadata);
                }
            }
            EventV1::ArtifactWritten(payload) => {
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    let tool_call = tool_calls
                        .entry(tool_call_id.clone())
                        .or_insert_with(ResumeToolCallSnapshot::default);
                    let metadata = tool_call
                        .metadata
                        .get_or_insert_with(ToolCallMetadata::default);
                    if let Some(tool_metadata) = payload.tool_metadata.as_ref() {
                        if metadata.canonical_tool_id.is_none() {
                            metadata.canonical_tool_id = tool_metadata.canonical_tool_id.clone();
                        }
                        if metadata.alias_source_tool_id.is_none() {
                            metadata.alias_source_tool_id =
                                tool_metadata.alias_source_tool_id.clone();
                        }
                    }
                    merge_artifact_ref(
                        &mut metadata.artifact_refs,
                        EventArtifactRef {
                            path: payload.path.clone(),
                            digest: Some(payload.digest.clone()),
                        },
                    );
                }
            }
            EventV1::PermissionRequested(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_permission_id,
                    &payload.permission_id,
                    PERMISSION_ID_PREFIX,
                    "permission",
                )?;
                pending_permissions.insert(payload.permission_id.clone());
            }
            EventV1::PermissionResolved(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_permission_id,
                    &payload.permission_id,
                    PERMISSION_ID_PREFIX,
                    "permission",
                )?;
                pending_permissions.remove(&payload.permission_id);
            }
            EventV1::UserMessageSubmitted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            _ => {}
        }
    }

    let run_id = run_id.unwrap_or_else(|| fallback_run_id.to_string());
    let resume_disabled_reason = resume_plan_disabled_reason(
        max_seq,
        latest_lifecycle_status,
        &pending_permissions,
        &tasks_in_flight,
        workspace_root.as_deref(),
        &known_profiles,
        provider_model.as_deref(),
    );

    Ok(ResumePlan {
        run_id,
        latest_lifecycle_status,
        max_seq,
        id_watermarks,
        known_agents,
        known_profiles,
        pending_permissions,
        tasks_in_flight,
        tool_calls,
        completed_tasks,
        workspace_root,
        provider_model,
        is_resumable: resume_disabled_reason.is_none(),
        resume_disabled_reason,
    })
}

fn merge_tool_call_metadata(snapshot: &mut ResumeToolCallSnapshot, incoming: ToolCallMetadata) {
    let metadata = snapshot
        .metadata
        .get_or_insert_with(ToolCallMetadata::default);
    if metadata.canonical_tool_id.is_none() {
        metadata.canonical_tool_id = incoming.canonical_tool_id;
    }
    if metadata.alias_source_tool_id.is_none() {
        metadata.alias_source_tool_id = incoming.alias_source_tool_id;
    }
    if metadata.lineage.is_none() {
        metadata.lineage = incoming.lineage;
    }
    if metadata.timing.is_none() {
        metadata.timing = incoming.timing;
    }
    for artifact_ref in incoming.artifact_refs {
        merge_artifact_ref(&mut metadata.artifact_refs, artifact_ref);
    }
}

fn merge_artifact_ref(existing: &mut Vec<EventArtifactRef>, candidate: EventArtifactRef) {
    if existing
        .iter()
        .any(|current| current.path == candidate.path && current.digest == candidate.digest)
    {
        return;
    }

    existing.push(candidate);
    existing.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.digest.cmp(&right.digest))
    });
}

fn read_events_for_resume_inspection(run_dir: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let body = fs::read_to_string(&events_path)
        .map_err(|source| format!("failed to read {}: {source}", events_path.display()))?;

    let mut events = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let event = serde_json::from_str::<EventEnvelopeV1>(line).map_err(|source| {
            format!(
                "invalid JSON event at line {} in {}: {source}",
                index + 1,
                events_path.display()
            )
        })?;
        events.push(event);
    }

    Ok(events)
}

fn fallback_run_id_from_path(run_dir: &Path) -> String {
    run_dir
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown-run>")
        .to_string()
}

fn update_id_watermark(
    max_value: &mut u64,
    id: &str,
    expected_prefix: &'static str,
    counter_kind: &'static str,
) -> Result<(), ProjectionError> {
    let Some(parsed) = parse_prefixed_counter(id, expected_prefix) else {
        return Err(ProjectionError::InvalidCounterId {
            counter_kind,
            id: id.to_string(),
            expected_prefix,
        });
    };
    *max_value = (*max_value).max(parsed);
    Ok(())
}

fn parse_prefixed_counter(id: &str, expected_prefix: &str) -> Option<u64> {
    let tail = id.strip_prefix(expected_prefix)?;
    if tail.is_empty() {
        return None;
    }
    tail.parse::<u64>().ok()
}

fn resume_plan_disabled_reason(
    max_seq: u64,
    latest_lifecycle_status: LifecycleSegmentStatus,
    pending_permissions: &BTreeSet<String>,
    tasks_in_flight: &BTreeSet<String>,
    workspace_root: Option<&str>,
    known_profiles: &BTreeSet<String>,
    provider_model: Option<&str>,
) -> Option<String> {
    if max_seq == 0 {
        return Some("session events are unavailable".to_string());
    }
    if latest_lifecycle_status == LifecycleSegmentStatus::Missing {
        return Some("latest lifecycle segment is unavailable".to_string());
    }
    if latest_lifecycle_status == LifecycleSegmentStatus::Active {
        return Some("run is still active".to_string());
    }
    if !pending_permissions.is_empty() {
        return Some("pending permissions must be resolved".to_string());
    }
    if !tasks_in_flight.is_empty() {
        return Some("tasks are still in flight".to_string());
    }
    if workspace_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Some("workspace root is unavailable".to_string());
    }
    if known_profiles.is_empty() {
        return Some("agent/profile bindings are unavailable".to_string());
    }
    if provider_model.is_none() {
        return Some("provider/model binding is unavailable".to_string());
    }

    None
}

pub fn project_session_catalog_entry<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    fallback_run_id: &str,
    metadata: Option<&SessionCatalogMetadata>,
    last_updated_at: Option<String>,
    degraded_reason: Option<String>,
) -> Result<SessionCatalogEntry, ProjectionError> {
    let collected = events.into_iter().collect::<Vec<_>>();

    let run_started = collected.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data),
        _ => None,
    });
    let spawned = collected.iter().find_map(|event| match &event.payload {
        EventV1::AgentSpawned(data) => Some(data),
        _ => None,
    });
    let provider_started = collected
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data) => Some(data),
            _ => None,
        });

    let run_name = run_started
        .map(|data| data.run_name.clone())
        .or_else(|| metadata.and_then(|meta| meta.run_name.clone()));
    let workspace_root = run_started
        .map(|data| data.workspace_root.clone())
        .or_else(|| metadata.and_then(|meta| meta.workspace_root.clone()));
    let profile_preset = spawned
        .map(|data| data.profile.clone())
        .or_else(|| metadata.and_then(|meta| meta.profile_preset.clone()));
    let provider = provider_started
        .map(|data| data.provider_id.clone())
        .or_else(|| metadata.and_then(|meta| meta.provider.clone()));
    let model = provider_started
        .map(|data| data.model_id.clone())
        .or_else(|| metadata.and_then(|meta| meta.model.clone()));

    let provider_model = match (provider.as_deref(), model.as_deref()) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(format!("{provider}/<unavailable>")),
        (None, Some(model)) => Some(format!("<unavailable>/{model}")),
        (None, None) => None,
    };

    let mode_source = metadata
        .and_then(|meta| meta.mode_source)
        .unwrap_or_else(|| infer_mode_source(run_name.as_deref(), provider.as_deref()));

    let run_id = collected
        .first()
        .map(|event| event.run_id.clone())
        .or_else(|| metadata.and_then(|meta| meta.run_id.clone()))
        .unwrap_or_else(|| fallback_run_id.to_string());

    let resume_plan = project_resume_plan(collected.iter().copied(), fallback_run_id)?;
    let status = resume_plan.run_status();

    let resume_disabled_reason = resume_disabled_reason(
        mode_source,
        &resume_plan,
        profile_preset.as_deref(),
        provider_model.as_deref(),
        degraded_reason,
    );

    Ok(SessionCatalogEntry {
        run_id,
        run_name,
        status,
        last_updated_at,
        workspace_root,
        profile_preset,
        provider_model,
        mode_source,
        is_resumable: resume_disabled_reason.is_none(),
        resume_disabled_reason,
    })
}

fn apply_run_summary_event(summary: &mut RunSummary, event: &EventEnvelopeV1) {
    summary.counts.total_events += 1;
    let event_type = event_type_name(&event.payload);
    *summary.counts.by_type.entry(event_type).or_insert(0) += 1;

    match &event.payload {
        EventV1::RunStarted(_) => {
            summary.status = RunStatus::Running;
        }
        EventV1::RunFinished(_) => {
            summary.status = RunStatus::Finished;
        }
        EventV1::RunFailed(payload) => {
            summary.status = RunStatus::Failed;
            summary.last_error = Some(payload.error.clone());
        }
        EventV1::TaskScheduled(payload) => {
            summary.tasks_in_flight.insert(payload.task_id.clone());
        }
        EventV1::TaskCancelled(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
        }
        EventV1::TaskCompleted(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
        }
        EventV1::PermissionRequested(payload) => {
            summary
                .pending_permissions
                .insert(payload.permission_id.clone());
        }
        EventV1::PermissionResolved(payload) => {
            summary.pending_permissions.remove(&payload.permission_id);
        }
        EventV1::UserMessageSubmitted(_) => {}
        _ => {}
    }
}

fn apply_timeline_event(index: &mut TimelineIndex, event: &EventEnvelopeV1) {
    if let Some(correlation_id) = &event.correlation_id {
        index
            .correlation_groups
            .entry(correlation_id.clone())
            .or_default()
            .push(event.seq);
    }

    index.events.push(TimelineEventRef {
        seq: event.seq,
        event_id: event.event_id.clone(),
        correlation_id: event.correlation_id.clone(),
        causation_id: event.causation_id.clone(),
        stream_key: event.stream_key.clone(),
        event_type: event_type_name(&event.payload),
    });
}

fn event_type_name(event: &EventV1) -> String {
    match event {
        EventV1::RunStarted(_) => "run_started",
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::UiIntentReceived(_) => "ui_intent_received",
        EventV1::UserMessageSubmitted(_) => "user_message_submitted",
    }
    .to_string()
}

fn infer_mode_source(run_name: Option<&str>, provider: Option<&str>) -> SessionModeSource {
    match run_name.unwrap_or_default() {
        "interactive" => {
            if provider == Some("mock") {
                SessionModeSource::InteractiveMock
            } else {
                SessionModeSource::InteractiveLive
            }
        }
        "prompt" => SessionModeSource::Prompt,
        "replay" => SessionModeSource::ReplayOnly,
        "golden_path" | "golden_path_interactive" => SessionModeSource::ScenarioFixture,
        _ => SessionModeSource::Unknown,
    }
}

fn resume_disabled_reason(
    mode_source: SessionModeSource,
    resume_plan: &ResumePlan,
    profile_preset: Option<&str>,
    provider_model: Option<&str>,
    degraded_reason: Option<String>,
) -> Option<String> {
    if let Some(reason) = degraded_reason {
        return Some(reason);
    }

    match mode_source {
        SessionModeSource::ScenarioFixture => {
            return Some("scenario fixture runs are excluded from resume".to_string());
        }
        SessionModeSource::ReplayOnly => {
            return Some("replay-only launches are not resumable".to_string());
        }
        SessionModeSource::Prompt => {
            return Some("prompt runs are not resumable".to_string());
        }
        SessionModeSource::Unknown => {
            return Some("session mode source is unavailable".to_string());
        }
        SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock => {}
    }

    if let Some(reason) = &resume_plan.resume_disabled_reason {
        return Some(reason.clone());
    }
    if profile_preset.is_none() {
        return Some("profile preset is unavailable".to_string());
    }
    if provider_model.is_none() {
        return Some("provider/model is unavailable".to_string());
    }

    None
}

fn enforce_seq(last_seq: Option<u64>, current_seq: u64) -> Result<(), ProjectionError> {
    if let Some(previous) = last_seq {
        if current_seq <= previous {
            return Err(ProjectionError::NonMonotonicSeq {
                previous,
                current: current_seq,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{project_run_summary, project_timeline_index, ProjectionError, RunStatus};
    use crate::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, RunFailedEvent, RunFinishedEvent,
        RunStartedEvent, TaskCompletedEvent, TaskScheduleState, TaskScheduledEvent,
        ToolCallRequestedEvent, SCHEMA_VERSION,
    };

    #[test]
    fn applying_same_jsonl_twice_yields_identical_run_summary() {
        let jsonl = fixture_jsonl();
        let first: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).expect("valid fixture line"))
            .collect();
        let second: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).expect("valid fixture line"))
            .collect();

        let summary_a = project_run_summary(first.iter()).expect("project first replay");
        let summary_b = project_run_summary(second.iter()).expect("project second replay");

        assert_eq!(summary_a, summary_b);
        assert_eq!(summary_a.status, RunStatus::Finished);
        assert!(summary_a.tasks_in_flight.is_empty());
        assert!(summary_a.pending_permissions.is_empty());
    }

    #[test]
    fn projections_ignore_side_effects_during_replay() {
        let events = [
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                Some("corr-1"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"touch /tmp/should_not_run\"}".to_string(),
                    args_digest: "digest123456".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                3,
                Some("corr-1"),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_1".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_1".to_string()),
                    summary: "allow command".to_string(),
                    request_digest: "reqdigest1234".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                4,
                Some("corr-1"),
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_1".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                5,
                Some("corr-1"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_1".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: None,
                }),
            ),
            envelope(
                6,
                Some("corr-1"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_1".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "resultdigest".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                7,
                Some("corr-1"),
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "ok".to_string(),
                }),
            ),
        ];

        let summary = project_run_summary(events.iter()).expect("project summary");
        let timeline = project_timeline_index(events.iter()).expect("project timeline");

        assert_eq!(summary.status, RunStatus::Finished);
        assert!(summary.tasks_in_flight.is_empty());
        assert!(summary.pending_permissions.is_empty());
        assert_eq!(timeline.events.len(), 7);
        assert_eq!(
            timeline.correlation_groups.get("corr-1"),
            Some(&vec![2, 3, 4, 5, 6, 7])
        );
    }

    #[test]
    fn projections_require_strict_seq_order() {
        let events = [
            envelope(
                2,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                1,
                None,
                EventV1::RunFailed(RunFailedEvent {
                    error: "out of order".to_string(),
                }),
            ),
        ];

        let err = project_run_summary(events.iter()).expect_err("must reject non-monotonic seq");
        assert!(matches!(
            err,
            ProjectionError::NonMonotonicSeq {
                previous: 2,
                current: 1
            }
        ));
    }

    fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_projection".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:run_projection".to_string()),
            payload,
        }
    }

    fn fixture_jsonl() -> &'static str {
        r#"{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"run_fixture","mono_ms":1,"actor":{"kind":"system","agent_id":"coordinator"},"stream_key":"run:run_fixture","payload":{"event_type":"run_started","data":{"run_name":"fixture","workspace_root":"/workspace/project"}}}
{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"run_fixture","mono_ms":2,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"task:task_1","payload":{"event_type":"task_scheduled","data":{"task_id":"task_1","state":"started"}}}
{"schema_version":1,"event_id":"evt-0003","seq":3,"run_id":"run_fixture","mono_ms":3,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"permission:perm_1","payload":{"event_type":"permission_requested","data":{"permission_id":"perm_1","kind":"shell","tool_call_id":"toolcall_1","summary":"allow command","request_digest":"reqdigest1234","timeout_ms":1000,"default_decision":"deny"}}}
{"schema_version":1,"event_id":"evt-0004","seq":4,"run_id":"run_fixture","mono_ms":4,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"permission:perm_1","payload":{"event_type":"permission_resolved","data":{"permission_id":"perm_1","decision":"allow","reason":"approved"}}}
{"schema_version":1,"event_id":"evt-0005","seq":5,"run_id":"run_fixture","mono_ms":5,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"task:task_1","payload":{"event_type":"task_completed","data":{"task_id":"task_1","result_summary":"done","result_digest":"resultdigest"}}}
{"schema_version":1,"event_id":"evt-0006","seq":6,"run_id":"run_fixture","mono_ms":6,"actor":{"kind":"system","agent_id":"coordinator"},"stream_key":"run:run_fixture","payload":{"event_type":"run_finished","data":{"summary":"ok"}}}"#
    }
}
