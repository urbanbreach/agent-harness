// allow: SIZE_OK — resume projection (plan reconstruction + state)
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::counter_id::parse_prefixed_counter;
use crate::event::{
    BackgroundTaskNotificationStatus, EventArtifactRef, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, HookExecutionMetadata, ProviderAssistantMessageMetadata,
    ProviderRequestFinishedMetadata, ProviderRequestStartedMetadata, ResolvedToolIdentity,
    TaskCompletionMetadata, TaskScheduleState, ToolCallLifecycleState, ToolCallMetadata,
    ToolCallStatus,
};
use crate::ids::RunId;
use crate::perm::PermissionGrantSet;
use crate::session::legacy::{recover_event_history, LegacyHistoryRecoveryError};
use crate::session_paths::EVENTS_FILE_NAME;
use crate::text::non_empty_trimmed;

use super::{load_run_metadata, ProjectionError, RunMetadata, RunStatus};

mod artifact_merge;
mod child_session;
mod metadata_merge;

use artifact_merge::{merge_session_artifact, merge_tool_metadata_artifacts};
use child_session::{
    apply_agent_turn_terminal_state, apply_child_session_metadata,
    child_terminal_state_from_background_status, derive_timing_from_start,
};
use metadata_merge::{merge_artifact_ref, merge_resolved_tool_identity, merge_tool_call_metadata};

const REQUEST_ID_PREFIX: &str = "req_";
const TASK_ID_PREFIX: &str = "task_";
const TOOL_CALL_ID_PREFIX: &str = "toolcall_";
const PERMISSION_ID_PREFIX: &str = "perm_";

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
    pub resolved_tool_identity: Option<ResolvedToolIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_state: Option<ToolCallLifecycleState>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResumeProviderLifecycleMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_started: Option<ProviderRequestStartedMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_finished: Option<ProviderRequestFinishedMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_assistant_message_finished: Option<ProviderAssistantMessageMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeArtifactSnapshot {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_contract_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_file_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_file_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildSessionTerminalState {
    Completed,
    Cancelled,
    Failed,
    TimedOut,
    LateResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeBackgroundTaskNotificationSnapshot {
    pub status: BackgroundTaskNotificationStatus,
    pub summary: String,
    pub terminal_event_id: String,
    pub terminal_task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_turn_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResumeChildSessionSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_child_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_lifecycle: Option<ResumeProviderLifecycleMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<ChildSessionTerminalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_notification: Option<ResumeBackgroundTaskNotificationSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
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
    #[serde(default, skip_serializing_if = "PermissionGrantSet::is_empty")]
    pub active_permission_grants: PermissionGrantSet,
    pub tasks_in_flight: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_calls: BTreeMap<String, ResumeToolCallSnapshot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub completed_tasks: BTreeMap<String, ResumeTaskSnapshot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session_artifacts: BTreeMap<String, ResumeArtifactSnapshot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub child_sessions: BTreeMap<String, ResumeChildSessionSnapshot>,
    pub workspace_root: Option<String>,
    pub provider_model: Option<String>,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
}

impl ResumePlan {
    pub fn run_status(&self) -> Option<RunStatus> {
        self.latest_lifecycle_status.run_status()
    }

    pub(crate) fn blocked(run_id: String, reason: String) -> Self {
        Self {
            run_id,
            latest_lifecycle_status: LifecycleSegmentStatus::Missing,
            max_seq: 0,
            id_watermarks: ResumeIdWatermarks::default(),
            known_agents: BTreeMap::new(),
            known_profiles: BTreeSet::new(),
            pending_permissions: BTreeSet::new(),
            active_permission_grants: PermissionGrantSet::default(),
            tasks_in_flight: BTreeSet::new(),
            tool_calls: BTreeMap::new(),
            completed_tasks: BTreeMap::new(),
            session_artifacts: BTreeMap::new(),
            child_sessions: BTreeMap::new(),
            workspace_root: None,
            provider_model: None,
            is_resumable: false,
            resume_disabled_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone)]
struct AgentTurnProjectionState {
    agent_id: String,
    request_id: Option<String>,
    started_mono_ms: u64,
    provider_id: Option<String>,
    model_id: Option<String>,
}

pub fn inspect_resume_plan(run_dir: &Path) -> ResumePlan {
    let fallback_run_id = fallback_run_id_from_path(run_dir);
    let events = match read_events_for_resume_inspection(run_dir, &fallback_run_id) {
        Ok(events) => events,
        Err(reason) => return ResumePlan::blocked(fallback_run_id, reason),
    };
    inspect_resume_plan_from_events(run_dir, &fallback_run_id, &events)
}

pub(crate) fn inspect_resume_plan_from_events(
    run_dir: &Path,
    fallback_run_id: &str,
    events: &[EventEnvelopeV1],
) -> ResumePlan {
    let metadata = load_run_metadata(run_dir);
    match project_resume_plan(events.iter(), fallback_run_id) {
        Ok(mut plan) => {
            apply_resume_metadata_fallback(&mut plan, metadata.as_ref());
            plan
        }
        Err(err) => ResumePlan::blocked(
            fallback_run_id.to_string(),
            format!("event log is corrupt or non-monotonic: {err}"),
        ),
    }
}

pub(crate) fn project_resume_plan_from_run_history(
    run_dir: &Path,
    fallback_run_id: &str,
    events: &[EventEnvelopeV1],
) -> Result<ResumePlan, ProjectionError> {
    let metadata = load_run_metadata(run_dir);
    let mut plan = project_resume_plan(events, fallback_run_id)?;
    apply_resume_metadata_fallback(&mut plan, metadata.as_ref());
    Ok(plan)
}

fn apply_resume_metadata_fallback(plan: &mut ResumePlan, metadata: Option<&RunMetadata>) {
    if plan.provider_model.is_none() {
        if let Some(context) =
            metadata.and_then(|metadata| metadata.recorded_runtime_context.as_ref())
        {
            plan.provider_model = Some(format!("{}/{}", context.provider, context.model));
        }
    }

    plan.resume_disabled_reason = resume_plan_disabled_reason(
        plan.max_seq,
        plan.latest_lifecycle_status,
        &plan.pending_permissions,
        &plan.tasks_in_flight,
        plan.workspace_root.as_deref(),
        &plan.known_profiles,
        plan.provider_model.as_deref(),
    );
    plan.is_resumable = plan.resume_disabled_reason.is_none();
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
    let mut active_permission_grants = PermissionGrantSet::default();
    let mut tasks_in_flight = BTreeSet::new();
    let mut tool_calls = BTreeMap::new();
    let mut completed_tasks = BTreeMap::new();
    let mut session_artifacts = BTreeMap::new();
    let mut child_sessions = BTreeMap::new();
    let mut agent_turns_in_flight = BTreeMap::new();
    let mut agent_turns_terminal_pending_late = BTreeMap::new();
    let mut workspace_root = None;
    let mut provider_model = None;
    let mut run_id: Option<String> = None;
    let mut max_seq = 0_u64;
    for (expected_seq, event) in (1_u64..).zip(events) {
        if event.seq != expected_seq {
            return Err(ProjectionError::NonContiguousSeq {
                expected: expected_seq,
                current: event.seq,
            });
        }
        max_seq = event.seq;

        match run_id.as_deref() {
            None => run_id = Some(event.run_id.to_string()),
            Some(existing) if existing == event.run_id.as_str() => {}
            Some(existing) => {
                return Err(ProjectionError::RunIdMismatch {
                    expected: existing.to_string(),
                    actual: event.run_id.to_string(),
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
                agent_turns_in_flight.clear();
                agent_turns_terminal_pending_late.clear();
            }
            EventV1::SessionTitleUpdated(_) => {}
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
                let child = child_sessions
                    .entry(payload.agent_id.clone())
                    .or_insert_with(ResumeChildSessionSnapshot::default);
                if child.profile.is_none() {
                    child.profile = Some(payload.profile.clone());
                }
                if child.parent_session_id.is_none() {
                    child.parent_session_id = payload.parent_agent_id.clone();
                }
            }
            EventV1::TaskScheduled(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    payload.task_id.as_str(),
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.insert(payload.task_id.to_string());

                if payload.state == TaskScheduleState::Started {
                    let Some(queue_key) = payload.queue_key.as_deref() else {
                        continue;
                    };
                    let Some((provider_id, model_id)) = parse_provider_model_queue_key(queue_key)
                    else {
                        continue;
                    };
                    let Some(agent_id) = event.actor.agent_id.as_ref() else {
                        continue;
                    };

                    let request_id = event.correlation_id.clone();
                    let turn = AgentTurnProjectionState {
                        agent_id: agent_id.clone(),
                        request_id: request_id.clone(),
                        started_mono_ms: event.mono_ms,
                        provider_id: Some(provider_id.clone()),
                        model_id: Some(model_id.clone()),
                    };
                    agent_turns_in_flight.insert(payload.task_id.to_string(), turn);

                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child.latest_child_request_id = request_id;
                    child.provider_id = Some(provider_id);
                    child.model_id = Some(model_id);
                    child.terminal_state = None;
                    child.terminal_reason = None;
                    child.timing = Some(ExecutionTimingMetadata {
                        started_mono_ms: Some(event.mono_ms),
                        finished_mono_ms: None,
                        elapsed_ms: None,
                    });
                }
            }
            EventV1::TaskCancelled(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    payload.task_id.as_str(),
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(payload.task_id.as_str());

                if let Some(turn) = agent_turns_in_flight.remove(payload.task_id.as_str()) {
                    apply_agent_turn_terminal_state(
                        &mut child_sessions,
                        &turn,
                        ChildSessionTerminalState::Cancelled,
                        Some(payload.reason.clone()),
                        event.mono_ms,
                        None,
                        &[],
                    );
                    agent_turns_terminal_pending_late.insert(payload.task_id.to_string(), turn);
                }
            }
            EventV1::TaskCompleted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    payload.task_id.as_str(),
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(payload.task_id.as_str());
                completed_tasks.insert(
                    payload.task_id.to_string(),
                    ResumeTaskSnapshot {
                        result_digest: Some(payload.result_digest.clone()),
                        metadata: payload.metadata.clone(),
                    },
                );

                if let Some(metadata) = payload.metadata.as_ref() {
                    apply_child_session_metadata(
                        &mut child_sessions,
                        metadata.lineage.as_ref(),
                        event.actor.agent_id.as_deref(),
                        metadata.timing.as_ref(),
                        &metadata.hook_executions,
                    );
                }

                if let Some(turn) = agent_turns_in_flight.remove(payload.task_id.as_str()) {
                    let timing = payload
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.timing.clone())
                        .unwrap_or_else(|| {
                            derive_timing_from_start(turn.started_mono_ms, event.mono_ms)
                        });
                    let hook_executions = payload
                        .metadata
                        .as_ref()
                        .map(|metadata| metadata.hook_executions.clone())
                        .unwrap_or_default();

                    apply_agent_turn_terminal_state(
                        &mut child_sessions,
                        &turn,
                        ChildSessionTerminalState::Completed,
                        None,
                        event.mono_ms,
                        Some(timing),
                        &hook_executions,
                    );
                }
            }
            EventV1::TaskResultLate(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_task_id,
                    payload.task_id.as_str(),
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(payload.task_id.as_str());

                if let Some(turn) = agent_turns_terminal_pending_late
                    .remove(payload.task_id.as_str())
                    .or_else(|| agent_turns_in_flight.remove(payload.task_id.as_str()))
                {
                    apply_agent_turn_terminal_state(
                        &mut child_sessions,
                        &turn,
                        ChildSessionTerminalState::LateResult,
                        None,
                        event.mono_ms,
                        Some(derive_timing_from_start(
                            turn.started_mono_ms,
                            event.mono_ms,
                        )),
                        &[],
                    );
                } else if let Some(agent_id) = event.actor.agent_id.as_ref() {
                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child.terminal_state = Some(ChildSessionTerminalState::LateResult);
                    child.terminal_reason = None;
                }
            }
            EventV1::BackgroundTaskNotification(payload) => {
                tasks_in_flight.remove(payload.task_id.as_str());
                tasks_in_flight.remove(&payload.terminal_task_id);
                agent_turns_in_flight.remove(payload.task_id.as_str());
                agent_turns_in_flight.remove(&payload.terminal_task_id);
                agent_turns_terminal_pending_late.remove(payload.task_id.as_str());
                agent_turns_terminal_pending_late.remove(&payload.terminal_task_id);

                let child = child_sessions
                    .entry(payload.child_session_id.to_string())
                    .or_insert_with(ResumeChildSessionSnapshot::default);
                if child.parent_session_id.is_none() {
                    child.parent_session_id = Some(payload.parent_session_id.to_string());
                }
                child.latest_child_request_id = Some(payload.child_request_id.clone());
                child.terminal_state =
                    Some(child_terminal_state_from_background_status(payload.status));
                child.terminal_reason = match payload.status {
                    BackgroundTaskNotificationStatus::Completed => None,
                    BackgroundTaskNotificationStatus::Cancelled
                    | BackgroundTaskNotificationStatus::Failed
                    | BackgroundTaskNotificationStatus::TimedOut => Some(payload.summary.clone()),
                };
                child.background_notification = Some(ResumeBackgroundTaskNotificationSnapshot {
                    status: payload.status,
                    summary: payload.summary.clone(),
                    terminal_event_id: payload.terminal_event_id.clone(),
                    terminal_task_id: payload.terminal_task_id.clone(),
                    delivered_turn_request_id: payload.delivered_turn_request_id.clone(),
                });
            }
            EventV1::ProviderRequestStarted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
                provider_model = Some(format!("{}/{}", payload.provider_id, payload.model_id));

                if let Some(agent_id) = event.actor.agent_id.as_ref() {
                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child.latest_child_request_id = Some(payload.request_id.to_string());
                    child.provider_id = Some(payload.provider_id.clone());
                    child.model_id = Some(payload.model_id.clone());
                    if let Some(metadata) = payload.metadata.clone() {
                        child
                            .provider_lifecycle
                            .get_or_insert_with(ResumeProviderLifecycleMetadata::default)
                            .latest_started = Some(metadata);
                    }
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ProviderReasoningDelta(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ProviderRequestFinished(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
                if let (Some(agent_id), Some(metadata)) =
                    (event.actor.agent_id.as_ref(), payload.metadata.clone())
                {
                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child
                        .provider_lifecycle
                        .get_or_insert_with(ResumeProviderLifecycleMetadata::default)
                        .latest_finished = Some(metadata);
                }
            }
            EventV1::AssistantMessageFinished(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
                if let (Some(agent_id), Some(metadata)) = (
                    event.actor.agent_id.as_ref(),
                    payload.assistant_message.clone(),
                ) {
                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child
                        .provider_lifecycle
                        .get_or_insert_with(ResumeProviderLifecycleMetadata::default)
                        .latest_assistant_message_finished = Some(metadata);
                }
            }
            EventV1::ToolCallRequested(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    payload.tool_call_id.as_str(),
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.to_string())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.tool_id = Some(payload.tool_id.clone());
                tool_call.lifecycle_state = Some(ToolCallLifecycleState::Pending);
                merge_resolved_tool_identity(
                    tool_call,
                    ResolvedToolIdentity::from_tool_call(
                        Some(payload.tool_id.as_str()),
                        payload.metadata.as_ref(),
                    ),
                );
                if let Some(metadata) = payload.metadata.as_ref() {
                    apply_child_session_metadata(
                        &mut child_sessions,
                        metadata.lineage.as_ref(),
                        event.actor.agent_id.as_deref(),
                        metadata.timing.as_ref(),
                        &metadata.hook_executions,
                    );
                    merge_tool_call_metadata(tool_call, metadata.clone());
                    merge_tool_metadata_artifacts(
                        &mut session_artifacts,
                        payload.tool_call_id.as_str(),
                        tool_call,
                        metadata,
                    );
                }
            }
            EventV1::ToolCallStarted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    payload.tool_call_id.as_str(),
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.to_string())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.lifecycle_state = Some(ToolCallLifecycleState::Running);
            }
            EventV1::ToolCallFinished(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    payload.tool_call_id.as_str(),
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.to_string())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.lifecycle_state =
                    Some(ToolCallLifecycleState::from_finish_status(payload.status));
                tool_call.status = Some(payload.status);
                tool_call.output_digest = payload.output_digest.clone();
                tool_call.output_json = payload.output_json.clone();
                merge_resolved_tool_identity(
                    tool_call,
                    ResolvedToolIdentity::from_tool_call(
                        tool_call.tool_id.as_deref(),
                        payload.metadata.as_ref(),
                    ),
                );
                if let Some(metadata) = payload.metadata.as_ref() {
                    apply_child_session_metadata(
                        &mut child_sessions,
                        metadata.lineage.as_ref(),
                        event.actor.agent_id.as_deref(),
                        metadata.timing.as_ref(),
                        &metadata.hook_executions,
                    );
                    merge_tool_call_metadata(tool_call, metadata.clone());
                    merge_tool_metadata_artifacts(
                        &mut session_artifacts,
                        payload.tool_call_id.as_str(),
                        tool_call,
                        metadata,
                    );
                }
            }
            EventV1::ArtifactWritten(payload) => {
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    let tool_call = tool_calls
                        .entry(tool_call_id.to_string())
                        .or_insert_with(ResumeToolCallSnapshot::default);
                    if let Some(tool_metadata) = payload.tool_metadata.as_ref() {
                        let invoked_tool_id = tool_call.tool_id.clone();
                        merge_resolved_tool_identity(
                            tool_call,
                            ResolvedToolIdentity::from_tool_artifact(
                                invoked_tool_id.as_deref(),
                                Some(tool_metadata),
                            ),
                        );
                    }
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

                merge_session_artifact(&mut session_artifacts, &tool_calls, payload);
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
            EventV1::PermissionGrantRecorded(payload) => {
                active_permission_grants.record(payload.grant.clone());
            }
            EventV1::UserMessageSubmitted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_request_id,
                    payload.request_id.as_str(),
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
        active_permission_grants,
        tasks_in_flight,
        tool_calls,
        completed_tasks,
        session_artifacts,
        child_sessions,
        workspace_root,
        provider_model,
        is_resumable: resume_disabled_reason.is_none(),
        resume_disabled_reason,
    })
}

fn read_events_for_resume_inspection(
    run_dir: &Path,
    expected_run_id: &str,
) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let expected_run_id = RunId::new(expected_run_id);
    let recovery = match recover_event_history(&events_path, &expected_run_id) {
        Ok(recovery) => Ok(recovery),
        Err(LegacyHistoryRecoveryError::RunMismatch {
            line_number: 1,
            actual,
            ..
        }) => recover_event_history(&events_path, &actual),
        Err(error) => Err(error),
    };
    recovery
        .map(|recovery| recovery.into_parts().0)
        .map_err(|error| error.to_string())
}

fn fallback_run_id_from_path(run_dir: &Path) -> String {
    run_dir
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(non_empty_trimmed)
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

fn parse_provider_model_queue_key(queue_key: &str) -> Option<(String, String)> {
    let tail = queue_key.strip_prefix("provider_model:")?;
    let mut parts = tail.splitn(2, ':');
    let provider_id = non_empty_trimmed(parts.next()?)?.to_string();
    let model_id = non_empty_trimmed(parts.next()?)?.to_string();
    Some((provider_id, model_id))
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
    if workspace_root.and_then(non_empty_trimmed).is_none() {
        return Some("workspace root is unavailable".to_string());
    }
    if known_profiles.is_empty() {
        return Some("agent/profile bindings are unavailable".to_string());
    }
    if known_profiles.iter().any(|profile| {
        !matches!(
            profile.as_str(),
            "default" | "explore" | "general" | "librarian"
        )
    }) {
        return Some("legacy unsupported profile binding cannot be resumed".to_string());
    }
    if provider_model.is_none() {
        return Some("provider/model binding is unavailable".to_string());
    }

    None
}

#[cfg(test)]
mod generic_profile_tests {
    use std::collections::BTreeSet;

    use super::{resume_plan_disabled_reason, LifecycleSegmentStatus};

    #[test]
    fn legacy_role_profile_binding_disables_resume() {
        // arrange
        let known_profiles = BTreeSet::from(["build".to_string()]);

        // act
        let reason = resume_plan_disabled_reason(
            1,
            LifecycleSegmentStatus::Finished,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some("/workspace/project"),
            &known_profiles,
            Some("mock/model-1"),
        );

        // assert
        assert_eq!(
            reason.as_deref(),
            Some("legacy unsupported profile binding cannot be resumed")
        );
    }

    #[test]
    fn supported_named_subagent_profile_bindings_remain_resumable() {
        // arrange
        let known_profiles = BTreeSet::from([
            "default".to_string(),
            "explore".to_string(),
            "general".to_string(),
            "librarian".to_string(),
        ]);

        // act
        let reason = resume_plan_disabled_reason(
            1,
            LifecycleSegmentStatus::Finished,
            &BTreeSet::new(),
            &BTreeSet::new(),
            Some("/workspace/project"),
            &known_profiles,
            Some("mock/model-1"),
        );

        // assert
        assert_eq!(reason, None);
    }
}
