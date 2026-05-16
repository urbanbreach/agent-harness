use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::agent::AgentModelRef;
use crate::config::{registered_profile_model_metadata, ResolvedProfileModelMetadata};
use crate::counter_id::parse_prefixed_counter;
use crate::event::{
    first_lineage_parent_session_id, ActorKind, BackgroundTaskNotificationStatus, EventActor,
    EventArtifactRef, EventEnvelopeV1, EventV1, ExecutionTimingMetadata, HookExecutionMetadata,
    ProviderAssistantMessageMetadata, ProviderRequestFinishedMetadata,
    ProviderRequestStartedMetadata, ResolvedToolIdentity, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState, TaskTerminalScope, TeamBounds,
    TeamMemberRole, TeamMemberSelector, TeamMemberSpec, TeamMessage, TeamSpec, TeamTask,
    ToolCallLifecycleState, ToolCallMetadata, ToolCallStatus,
};
use crate::perm::PermissionGrantSet;
use crate::session_paths::{EVENTS_FILE_NAME, META_FILE_NAME};
use crate::text::non_empty_trimmed;

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
pub struct RecordedRuntimeContext {
    pub profile: String,
    #[serde(default)]
    pub profile_description: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub provider_display_label: Option<String>,
    #[serde(default)]
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub variant: Option<String>,
    pub display_label: String,
    #[serde(default)]
    pub model_display_label: Option<String>,
    #[serde(default)]
    pub variant_display_label: Option<String>,
    pub token_window_label: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub recommended_for: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
}

impl RecordedRuntimeContext {
    pub fn from_profile_model(profile: &str, model_ref: &str) -> Self {
        if let Some(metadata) = registered_profile_model_metadata(profile) {
            return Self::from(metadata);
        }

        let model_ref = AgentModelRef::parse(model_ref);
        let display_label = model_ref.model_id.clone();

        Self {
            profile: profile.to_string(),
            profile_description: None,
            provider: model_ref.provider_id,
            provider_display_label: None,
            provider_backend_label: None,
            model: model_ref.model_id,
            variant: None,
            display_label,
            model_display_label: None,
            variant_display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            recommended_for: None,
            reasoning_effort: None,
            text_verbosity: None,
        }
    }
}

impl From<ResolvedProfileModelMetadata> for RecordedRuntimeContext {
    fn from(metadata: ResolvedProfileModelMetadata) -> Self {
        Self {
            profile: metadata.profile,
            profile_description: metadata.profile_description,
            provider: metadata.provider,
            provider_display_label: Some(metadata.provider_display_label),
            provider_backend_label: metadata.provider_backend_label,
            model: metadata.model,
            variant: metadata.variant,
            display_label: metadata.display_label,
            model_display_label: Some(metadata.model_display_label),
            variant_display_label: metadata.variant_display_label,
            token_window_label: metadata.token_window_label,
            context_window_tokens: metadata.context_window_tokens,
            max_input_tokens: metadata.max_input_tokens,
            max_output_tokens: metadata.max_output_tokens,
            description: metadata.description,
            recommended_for: metadata.recommended_for,
            reasoning_effort: metadata.reasoning_effort,
            text_verbosity: metadata.text_verbosity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    pub run_name: String,
    pub workspace_root: String,
    #[serde(default)]
    pub created_at: Option<String>,
    pub config_digest: String,
    pub harness_version: String,
    #[serde(default)]
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
    #[serde(default)]
    pub mode_source: Option<SessionModeSource>,
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
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
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
    pub artifact_count: usize,
    pub child_session_count: usize,
    pub parent_session_id: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestRef {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BackgroundToolCallCounts {
    pub requested: u64,
    pub succeeded: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunStatus {
    Active,
    ShutdownRequested,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberStatus {
    Pending,
    Running,
    ShutdownRequested,
    ShutdownApproved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLeadProjection {
    pub selector: TeamMemberSelector,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberProjection {
    pub name: String,
    pub role: TeamMemberRole,
    pub spec: TeamMemberSpec,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_requester: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_rejected_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamShutdownRequestProjection {
    pub member_name: String,
    pub requester: String,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRunProjection {
    pub team_run_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TeamRunStatus,
    pub bounds: TeamBounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<TeamLeadProjection>,
    pub members: BTreeMap<String, TeamMemberProjection>,
    pub messages: Vec<TeamMessage>,
    pub tasks: BTreeMap<String, TeamTask>,
    pub shutdown_requests: BTreeMap<String, TeamShutdownRequestProjection>,
    pub bounds_consumption: TeamBoundsConsumption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mono_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TeamBoundsConsumption {
    pub running_members: u32,
    pub pending_members: u32,
    pub shutdown_approved_members: u32,
    pub messages: u32,
    pub tasks: u32,
    pub member_turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_wall_clock_minutes: Option<u32>,
}

impl TeamRunProjection {
    fn from_spec(team_run_id: String, spec: TeamSpec, created_mono_ms: u64) -> Self {
        let lead = spec.lead.clone().map(|selector| TeamLeadProjection {
            selector,
            status: TeamMemberStatus::Pending,
            agent_id: None,
            profile: None,
        });
        let members = spec
            .members
            .iter()
            .cloned()
            .map(|member| {
                let name = member.name.clone();
                let role = member.role;
                (
                    name.clone(),
                    TeamMemberProjection {
                        name,
                        role,
                        spec: member,
                        status: TeamMemberStatus::Pending,
                        agent_id: None,
                        profile: None,
                        shutdown_requester: None,
                        shutdown_rejected_reason: None,
                    },
                )
            })
            .collect();

        Self {
            team_run_id,
            name: spec.name,
            description: spec.description,
            status: TeamRunStatus::Active,
            bounds: spec.bounds,
            lead,
            members,
            messages: Vec::new(),
            tasks: BTreeMap::new(),
            shutdown_requests: BTreeMap::new(),
            bounds_consumption: TeamBoundsConsumption::default(),
            created_mono_ms: Some(created_mono_ms),
            last_mono_ms: Some(created_mono_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TeamProjection {
    pub teams: BTreeMap<String, TeamRunProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestProjection {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_task_id: Option<String>,
    pub status: String,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    pub tool_calls: BackgroundToolCallCounts,
    pub late_result: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackgroundRequestProjectionError {
    #[error("provide request_id, task_id, or session_id returned by a background task call")]
    MissingSelector,
    #[error("background request is not in the caller's task lineage")]
    Unauthorized,
    #[error("could not resolve background request `{0}`")]
    UnknownRequest(String),
    #[error("could not resolve background request for task_id/session_id `{0}`; pass the request_id returned by task(run_in_background=true)")]
    UnknownSelector(String),
    #[error("background request `{0}` has no projected events")]
    MissingProjection(String),
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
    pub tool_call_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_continuation_id: Option<String>,
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
            active_permission_grants: PermissionGrantSet::default(),
            tasks_in_flight: BTreeSet::new(),
            tool_calls: BTreeMap::new(),
            completed_tasks: BTreeMap::new(),
            session_artifacts: BTreeMap::new(),
            child_sessions: BTreeMap::new(),
            workspace_root: None,
            provider_model: None,
            active_continuation_id: None,
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

fn resume_plan_artifact_count(plan: &ResumePlan) -> usize {
    plan.session_artifacts.len()
}

fn resume_plan_child_session_count(plan: &ResumePlan) -> usize {
    plan.child_sessions
        .values()
        .filter(|child| {
            child.parent_session_id.is_some()
                || child.parent_tool_call_id.is_some()
                || child.parent_task_id.is_some()
                || child.parent_request_id.is_some()
        })
        .count()
}

pub fn resolve_background_request_ref<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    actor: &EventActor,
    request_id: Option<&str>,
    selector_hint: Option<&str>,
) -> Result<BackgroundRequestRef, BackgroundRequestProjectionError> {
    let explicit_request_id = request_id.and_then(non_empty_trimmed);
    let selector_hint = selector_hint.and_then(non_empty_trimmed);

    if explicit_request_id.is_none() && selector_hint.is_none() {
        return Err(BackgroundRequestProjectionError::MissingSelector);
    }

    let mut latest_request_id = None;
    let mut parent_by_agent = BTreeMap::new();
    let mut saw_matching_unauthorized = false;
    let mut saw_explicit_request = false;

    for event in events {
        match &event.payload {
            EventV1::AgentSpawned(data) => {
                if let Some(parent_agent_id) = data.parent_agent_id.as_deref() {
                    parent_by_agent.insert(data.agent_id.clone(), parent_agent_id.to_string());
                }
            }
            EventV1::TaskScheduled(data) => {
                let event_request_id = event.correlation_id.as_deref();
                let matches_explicit_request = explicit_request_id == event_request_id;
                let matches_session = explicit_request_id.is_none()
                    && selector_hint.is_some_and(|selector| {
                        event.actor.agent_id.as_deref() == Some(selector)
                            || data.task_id == selector
                    });
                if !matches_explicit_request && !matches_session {
                    continue;
                }
                if matches_explicit_request {
                    saw_explicit_request = true;
                }
                if background_request_authorized(
                    actor,
                    &parent_by_agent,
                    event.actor.agent_id.as_deref(),
                ) {
                    latest_request_id = event.correlation_id.clone();
                } else {
                    saw_matching_unauthorized = true;
                }
            }
            _ => {}
        }
    }

    let request_id = match latest_request_id {
        Some(request_id) => request_id,
        None if saw_matching_unauthorized => {
            return Err(BackgroundRequestProjectionError::Unauthorized);
        }
        None if explicit_request_id.is_some() && !saw_explicit_request => {
            return Err(BackgroundRequestProjectionError::UnknownRequest(
                explicit_request_id
                    .expect("explicit request id checked")
                    .to_string(),
            ));
        }
        None => {
            return Err(BackgroundRequestProjectionError::UnknownSelector(
                selector_hint.expect("selector checked").to_string(),
            ));
        }
    };

    Ok(BackgroundRequestRef {
        request_id,
        session_id_hint: selector_hint.map(str::to_string),
    })
}

pub fn project_background_request<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    request_ref: &BackgroundRequestRef,
) -> Result<BackgroundRequestProjection, BackgroundRequestProjectionError> {
    let mut tool_calls = BackgroundToolCallCounts::default();
    let mut started_mono_ms = None;
    let mut session_id = request_ref.session_id_hint.clone();
    let mut scheduler_task_id = None;
    let mut latest_scheduled_state = None;
    let mut result_summary = None;
    let mut failure_summary = None;
    let mut duration_ms = None;
    let mut terminal_status = None;
    let mut late_result = false;
    let mut saw_event = false;

    for event in events {
        let matches_notification = matches!(
            &event.payload,
            EventV1::BackgroundTaskNotification(data)
                if data.child_request_id == request_ref.request_id
        );
        if event.correlation_id.as_deref() != Some(request_ref.request_id.as_str())
            && !matches_notification
        {
            continue;
        }
        saw_event = true;

        match &event.payload {
            EventV1::TaskScheduled(data) => {
                if data
                    .queue_key
                    .as_deref()
                    .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
                {
                    latest_scheduled_state = Some(data.state);
                    scheduler_task_id = Some(data.task_id.clone());
                    if let Some(agent_id) = event.actor.agent_id.as_ref() {
                        session_id = Some(agent_id.clone());
                    }
                    if data.state == TaskScheduleState::Started {
                        started_mono_ms = Some(event.mono_ms);
                    }
                }
            }
            EventV1::ToolCallRequested(_) => {
                tool_calls.requested += 1;
            }
            EventV1::ToolCallFinished(data) => match data.status {
                ToolCallStatus::Succeeded => {
                    tool_calls.succeeded += 1;
                }
                ToolCallStatus::Failed => {
                    tool_calls.failed += 1;
                }
            },
            EventV1::TaskCompleted(data) => {
                if is_background_agent_turn_completion(data, scheduler_task_id.as_deref()) {
                    terminal_status = Some("completed".to_string());
                    result_summary = Some(data.result_summary.clone());
                    duration_ms = data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.timing.as_ref())
                        .and_then(|timing| timing.elapsed_ms)
                        .or_else(|| elapsed_ms_from_events(started_mono_ms, event.mono_ms));
                }
            }
            EventV1::TaskCancelled(data) => {
                if is_background_agent_turn_cancellation(data, scheduler_task_id.as_deref()) {
                    terminal_status = Some("cancelled".to_string());
                    failure_summary = Some(data.reason.clone());
                }
            }
            EventV1::TaskResultLate(_) => {
                late_result = true;
            }
            EventV1::BackgroundTaskNotification(data) => {
                terminal_status = Some(data.status.as_str().to_string());
                session_id = Some(data.child_session_id.clone());
                scheduler_task_id = Some(data.task_id.clone());
                match data.status {
                    BackgroundTaskNotificationStatus::Completed => {
                        result_summary = Some(data.summary.clone());
                    }
                    BackgroundTaskNotificationStatus::Cancelled
                    | BackgroundTaskNotificationStatus::Failed
                    | BackgroundTaskNotificationStatus::TimedOut => {
                        failure_summary = Some(data.summary.clone());
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_event {
        return Err(BackgroundRequestProjectionError::MissingProjection(
            request_ref.request_id.clone(),
        ));
    }

    let status = terminal_status.unwrap_or_else(|| match latest_scheduled_state {
        Some(TaskScheduleState::Started) => "running".to_string(),
        Some(TaskScheduleState::Queued) => "queued".to_string(),
        None => "scheduled".to_string(),
    });
    let cancel_reason = (status == "cancelled")
        .then(|| failure_summary.clone())
        .flatten();

    Ok(BackgroundRequestProjection {
        request_id: request_ref.request_id.clone(),
        session_id,
        scheduler_task_id,
        terminal: matches!(
            status.as_str(),
            "completed" | "cancelled" | "failed" | "timed_out"
        ),
        duration_ms,
        result_summary,
        failure_summary,
        tool_calls,
        late_result,
        cancel_reason,
        status,
    })
}

pub fn project_team_state<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<TeamProjection, ProjectionError> {
    let mut projection = TeamProjection::default();
    let mut last_seq = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_team_event(&mut projection, event);
    }

    Ok(projection)
}

fn apply_team_event(projection: &mut TeamProjection, event: &EventEnvelopeV1) {
    match &event.payload {
        EventV1::TeamCreated(payload) => {
            let mut team = TeamRunProjection::from_spec(
                payload.team_run_id.clone(),
                payload.spec.clone(),
                event.mono_ms,
            );
            refresh_team_derived_state(&mut team, event.mono_ms);
            projection.teams.insert(payload.team_run_id.clone(), team);
        }
        EventV1::TeamMemberSpawned(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if payload.member_name == "lead" {
                    if let Some(lead) = team.lead.as_mut() {
                        if lead.agent_id.is_none() {
                            lead.status = TeamMemberStatus::Running;
                            lead.agent_id = Some(payload.agent_id.clone());
                            lead.profile = Some(payload.profile.clone());
                        }
                    }
                } else if let Some(member) = team.members.get_mut(&payload.member_name) {
                    if member.agent_id.is_none() {
                        member.status = TeamMemberStatus::Running;
                        member.agent_id = Some(payload.agent_id.clone());
                        member.profile = Some(payload.profile.clone());
                    }
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamMessageSent(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if team
                    .messages
                    .iter()
                    .all(|message| message.message_id != payload.message.message_id)
                {
                    if member_write_participant_for_event(team, event, Some(&payload.message.from))
                        .is_some()
                    {
                        team.bounds_consumption.member_turns =
                            team.bounds_consumption.member_turns.saturating_add(1);
                    }
                    team.messages.push(payload.message.clone());
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamTaskCreated(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if !team.tasks.contains_key(&payload.task.task_id) {
                    let mut task = payload.task.clone();
                    task.blocks.clear();
                    if member_write_participant_for_event(team, event, task.owner.as_deref())
                        .is_some()
                    {
                        team.bounds_consumption.member_turns =
                            team.bounds_consumption.member_turns.saturating_add(1);
                    }
                    team.tasks.insert(task.task_id.clone(), task);
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamTaskUpdated(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if member_write_participant_for_event(team, event, payload.owner.as_deref())
                    .is_some()
                {
                    team.bounds_consumption.member_turns =
                        team.bounds_consumption.member_turns.saturating_add(1);
                }
                if let Some(task) = team.tasks.get_mut(&payload.task_id) {
                    task.status = payload.status;
                    if payload.owner.is_some() {
                        task.owner = payload.owner.clone();
                    }
                    if !payload.metadata.is_empty() {
                        task.metadata.extend(payload.metadata.clone());
                    }
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownRequested(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::ShutdownRequested;
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::ShutdownRequested;
                    member.shutdown_requester = Some(payload.requester.clone());
                    member.shutdown_rejected_reason = None;
                }
                team.shutdown_requests.insert(
                    payload.member_name.clone(),
                    TeamShutdownRequestProjection {
                        member_name: payload.member_name.clone(),
                        requester: payload.requester.clone(),
                        status: TeamMemberStatus::ShutdownRequested,
                        rejected_reason: None,
                    },
                );
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownApproved(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::ShutdownRequested;
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::ShutdownApproved;
                    member.shutdown_rejected_reason = None;
                }
                if let Some(request) = team.shutdown_requests.get_mut(&payload.member_name) {
                    request.status = TeamMemberStatus::ShutdownApproved;
                    request.rejected_reason = None;
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownRejected(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::Running;
                    member.shutdown_rejected_reason = Some(payload.reason.clone());
                }
                if let Some(request) = team.shutdown_requests.get_mut(&payload.member_name) {
                    request.status = TeamMemberStatus::Running;
                    request.rejected_reason = Some(payload.reason.clone());
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamDeleted(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::Deleted;
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        _ => {}
    }
}

fn refresh_team_derived_state(team: &mut TeamRunProjection, mono_ms: u64) {
    team.last_mono_ms = Some(mono_ms);
    refresh_team_shutdown_status(team);
    refresh_team_task_blocks(team);
    team.bounds_consumption.running_members = team
        .members
        .values()
        .filter(|member| {
            matches!(
                member.status,
                TeamMemberStatus::Running | TeamMemberStatus::ShutdownRequested
            )
        })
        .count() as u32;
    team.bounds_consumption.pending_members = team
        .members
        .values()
        .filter(|member| member.status == TeamMemberStatus::Pending)
        .count() as u32;
    team.bounds_consumption.shutdown_approved_members = team
        .members
        .values()
        .filter(|member| member.status == TeamMemberStatus::ShutdownApproved)
        .count() as u32;
    team.bounds_consumption.messages = team.messages.len() as u32;
    team.bounds_consumption.tasks = team.tasks.len() as u32;
    team.bounds_consumption.elapsed_wall_clock_minutes = team
        .created_mono_ms
        .map(|created| mono_ms.saturating_sub(created) / 60_000)
        .map(|minutes| minutes.min(u64::from(u32::MAX)) as u32);
}

fn refresh_team_task_blocks(team: &mut TeamRunProjection) {
    for task in team.tasks.values_mut() {
        task.blocks.clear();
    }
    let edges = team
        .tasks
        .iter()
        .flat_map(|(task_id, task)| {
            task.blocked_by
                .iter()
                .cloned()
                .map(|blocked_by| (blocked_by, task_id.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (blocked_by, task_id) in edges {
        if let Some(blocker) = team.tasks.get_mut(&blocked_by) {
            if !blocker.blocks.contains(&task_id) {
                blocker.blocks.push(task_id);
            }
        }
    }
}

fn member_write_participant<'a>(
    team: &'a TeamRunProjection,
    participant: &str,
) -> Option<&'a TeamMemberProjection> {
    team.members
        .get(participant)
        .filter(|member| member.role == TeamMemberRole::Member)
}

fn member_write_participant_for_event<'a>(
    team: &'a TeamRunProjection,
    event: &EventEnvelopeV1,
    explicit_participant: Option<&str>,
) -> Option<&'a TeamMemberProjection> {
    if event.actor.kind == ActorKind::Worker {
        if let Some(agent_id) = event.actor.agent_id.as_deref() {
            return team
                .members
                .values()
                .find(|member| member.agent_id.as_deref() == Some(agent_id))
                .filter(|member| member.role == TeamMemberRole::Member);
        }
    }
    explicit_participant.and_then(|participant| member_write_participant(team, participant))
}

fn refresh_team_shutdown_status(team: &mut TeamRunProjection) {
    if team.status == TeamRunStatus::Deleted {
        return;
    }
    team.status = if team.members.values().any(|member| {
        matches!(
            member.status,
            TeamMemberStatus::ShutdownRequested | TeamMemberStatus::ShutdownApproved
        )
    }) {
        TeamRunStatus::ShutdownRequested
    } else {
        TeamRunStatus::Active
    };
}

fn background_request_authorized(
    actor: &EventActor,
    parent_by_agent: &BTreeMap<String, String>,
    request_agent_id: Option<&str>,
) -> bool {
    if actor.kind != ActorKind::Worker {
        return true;
    }
    let Some(caller_agent_id) = actor.agent_id.as_deref() else {
        return false;
    };
    let Some(mut candidate_agent_id) = request_agent_id else {
        return false;
    };

    if candidate_agent_id == caller_agent_id {
        return true;
    }

    let mut seen = BTreeSet::new();
    while seen.insert(candidate_agent_id.to_string()) {
        let Some(parent_agent_id) = parent_by_agent.get(candidate_agent_id) else {
            return false;
        };
        if parent_agent_id == caller_agent_id {
            return true;
        }
        candidate_agent_id = parent_agent_id;
    }

    false
}

fn is_background_agent_turn_completion(
    event: &TaskCompletedEvent,
    scheduler_task_id: Option<&str>,
) -> bool {
    event
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        == Some(TaskTerminalScope::AgentTurn)
        || scheduler_task_id == Some(event.task_id.as_str())
}

fn is_background_agent_turn_cancellation(
    event: &TaskCancelledEvent,
    scheduler_task_id: Option<&str>,
) -> bool {
    event.task_scope == Some(TaskTerminalScope::AgentTurn)
        || scheduler_task_id == Some(event.task_id.as_str())
}

fn elapsed_ms_from_events(started_mono_ms: Option<u64>, finished_mono_ms: u64) -> Option<u64> {
    started_mono_ms.map(|started| finished_mono_ms.saturating_sub(started))
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

#[derive(Debug, Clone)]
struct AgentTurnProjectionState {
    agent_id: String,
    request_id: Option<String>,
    started_mono_ms: u64,
    provider_id: Option<String>,
    model_id: Option<String>,
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
    let metadata = load_run_metadata(run_dir);

    match project_resume_plan(events.iter(), &fallback_run_id) {
        Ok(mut plan) => {
            apply_resume_metadata_fallback(&mut plan, metadata.as_ref());
            plan
        }
        Err(err) => ResumePlan::blocked(
            fallback_run_id,
            format!("event log is corrupt or non-monotonic: {err}"),
        ),
    }
}

pub fn load_run_metadata(run_dir: &Path) -> Option<RunMetadata> {
    let body = fs::read_to_string(run_dir.join(META_FILE_NAME)).ok()?;
    serde_json::from_str(&body).ok()
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
    let mut active_continuation_id = None;
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
                session_artifacts.clear();
                agent_turns_in_flight.clear();
                agent_turns_terminal_pending_late.clear();
                active_continuation_id = None;
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
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.insert(payload.task_id.clone());

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
                    agent_turns_in_flight.insert(payload.task_id.clone(), turn);

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
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(&payload.task_id);

                if let Some(turn) = agent_turns_in_flight.remove(&payload.task_id) {
                    apply_agent_turn_terminal_state(
                        &mut child_sessions,
                        &turn,
                        ChildSessionTerminalState::Cancelled,
                        Some(payload.reason.clone()),
                        event.mono_ms,
                        None,
                        &[],
                    );
                    agent_turns_terminal_pending_late.insert(payload.task_id.clone(), turn);
                }
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

                if let Some(metadata) = payload.metadata.as_ref() {
                    apply_child_session_metadata(
                        &mut child_sessions,
                        metadata.lineage.as_ref(),
                        event.actor.agent_id.as_deref(),
                        metadata.timing.as_ref(),
                        &metadata.hook_executions,
                    );
                }

                if let Some(turn) = agent_turns_in_flight.remove(&payload.task_id) {
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
                    &payload.task_id,
                    TASK_ID_PREFIX,
                    "task",
                )?;
                tasks_in_flight.remove(&payload.task_id);

                if let Some(turn) = agent_turns_terminal_pending_late
                    .remove(&payload.task_id)
                    .or_else(|| agent_turns_in_flight.remove(&payload.task_id))
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
                tasks_in_flight.remove(&payload.task_id);
                tasks_in_flight.remove(&payload.terminal_task_id);
                agent_turns_in_flight.remove(&payload.task_id);
                agent_turns_in_flight.remove(&payload.terminal_task_id);
                agent_turns_terminal_pending_late.remove(&payload.task_id);
                agent_turns_terminal_pending_late.remove(&payload.terminal_task_id);

                let child = child_sessions
                    .entry(payload.child_session_id.clone())
                    .or_insert_with(ResumeChildSessionSnapshot::default);
                if child.parent_session_id.is_none() {
                    child.parent_session_id = Some(payload.parent_session_id.clone());
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
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
                provider_model = Some(format!("{}/{}", payload.provider_id, payload.model_id));

                if let Some(agent_id) = event.actor.agent_id.as_ref() {
                    let child = child_sessions
                        .entry(agent_id.clone())
                        .or_insert_with(ResumeChildSessionSnapshot::default);
                    child.latest_child_request_id = Some(payload.request_id.clone());
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
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ProviderReasoningDelta(payload) => {
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
                    &payload.request_id,
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
                    &payload.tool_call_id,
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.clone())
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
                        &payload.tool_call_id,
                        tool_call,
                        metadata,
                    );
                }
            }
            EventV1::ToolCallStarted(payload) => {
                update_id_watermark(
                    &mut id_watermarks.max_tool_call_id,
                    &payload.tool_call_id,
                    TOOL_CALL_ID_PREFIX,
                    "tool call",
                )?;
                let tool_call = tool_calls
                    .entry(payload.tool_call_id.clone())
                    .or_insert_with(ResumeToolCallSnapshot::default);
                tool_call.lifecycle_state = Some(ToolCallLifecycleState::Running);
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
                        &payload.tool_call_id,
                        tool_call,
                        metadata,
                    );
                }
            }
            EventV1::ArtifactWritten(payload) => {
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    let tool_call = tool_calls
                        .entry(tool_call_id.clone())
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
                    &payload.request_id,
                    REQUEST_ID_PREFIX,
                    "request",
                )?;
            }
            EventV1::ContinuationStarted(payload) => {
                active_continuation_id = Some(payload.continuation_id.clone());
            }
            EventV1::ContinuationReminderQueued(_) => {}
            EventV1::ContinuationStopped(payload) => {
                if active_continuation_id.as_deref() == Some(payload.continuation_id.as_str()) {
                    active_continuation_id = None;
                }
            }
            EventV1::ContinuationLimitReached(payload) => {
                if active_continuation_id.as_deref() == Some(payload.continuation_id.as_str()) {
                    active_continuation_id = None;
                }
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
        active_continuation_id,
        is_resumable: resume_disabled_reason.is_none(),
        resume_disabled_reason,
    })
}

fn merge_resolved_tool_identity(
    snapshot: &mut ResumeToolCallSnapshot,
    incoming: ResolvedToolIdentity,
) {
    if incoming.is_empty() {
        return;
    }

    let identity = snapshot
        .resolved_tool_identity
        .get_or_insert_with(ResolvedToolIdentity::default);
    if identity.invoked_tool_id.is_none() {
        identity.invoked_tool_id = incoming.invoked_tool_id;
    }
    if identity.effective_tool_id.is_none() {
        identity.effective_tool_id = incoming.effective_tool_id;
    }
    if identity.canonical_tool_id.is_none() {
        identity.canonical_tool_id = incoming.canonical_tool_id;
    }
    if identity.alias_source_tool_id.is_none() {
        identity.alias_source_tool_id = incoming.alias_source_tool_id;
    }
}

fn merge_tool_call_metadata(snapshot: &mut ResumeToolCallSnapshot, incoming: ToolCallMetadata) {
    let ToolCallMetadata {
        canonical_tool_id,
        alias_source_tool_id,
        lineage,
        artifact_refs,
        timing,
        hook_executions,
    } = incoming;

    let metadata = snapshot
        .metadata
        .get_or_insert_with(ToolCallMetadata::default);
    if metadata.canonical_tool_id.is_none() {
        metadata.canonical_tool_id = canonical_tool_id;
    }
    if metadata.alias_source_tool_id.is_none() {
        metadata.alias_source_tool_id = alias_source_tool_id;
    }
    if metadata.lineage.is_none() {
        metadata.lineage = lineage;
    }
    if metadata.timing.is_none() {
        metadata.timing = timing;
    }
    for artifact_ref in artifact_refs {
        merge_artifact_ref(&mut metadata.artifact_refs, artifact_ref);
    }
    for hook_execution in hook_executions {
        merge_hook_execution(&mut metadata.hook_executions, hook_execution);
    }
}

fn merge_session_artifact(
    session_artifacts: &mut BTreeMap<String, ResumeArtifactSnapshot>,
    tool_calls: &BTreeMap<String, ResumeToolCallSnapshot>,
    payload: &crate::event::ArtifactWrittenEvent,
) {
    let tool_snapshot = payload
        .tool_call_id
        .as_ref()
        .and_then(|tool_call_id| tool_calls.get(tool_call_id));
    let lineage = tool_snapshot
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .and_then(|metadata| metadata.lineage.as_ref());
    let tool_id = tool_snapshot
        .and_then(|snapshot| snapshot.tool_id.clone())
        .or_else(|| {
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.canonical_tool_id.clone())
        });
    let key = artifact_snapshot_key(&payload.path, Some(payload.digest.as_str()));

    session_artifacts
        .entry(key)
        .and_modify(|artifact| {
            if artifact.bytes.is_none() {
                artifact.bytes = Some(payload.bytes);
            }
            if artifact.tool_call_id.is_none() {
                artifact.tool_call_id = payload.tool_call_id.clone();
            }
            if artifact.tool_id.is_none() {
                artifact.tool_id = tool_id.clone();
            }
            if artifact.artifact_kind.is_none() {
                artifact.artifact_kind = payload.metadata.get("artifact_kind").cloned();
            }
            if artifact.summary_contract_version.is_none() {
                artifact.summary_contract_version =
                    metadata_u32(&payload.metadata, "summary_contract_version");
            }
            if artifact.read_file_count.is_none() {
                artifact.read_file_count = metadata_u32(&payload.metadata, "read_file_count");
            }
            if artifact.modified_file_count.is_none() {
                artifact.modified_file_count =
                    metadata_u32(&payload.metadata, "modified_file_count");
            }
            if artifact.parent_tool_call_id.is_none() {
                artifact.parent_tool_call_id =
                    lineage.and_then(|lineage| lineage.parent_tool_call_id.clone());
            }
            if artifact.parent_task_id.is_none() {
                artifact.parent_task_id =
                    lineage.and_then(|lineage| lineage.parent_task_id.clone());
            }
            if artifact.parent_request_id.is_none() {
                artifact.parent_request_id =
                    lineage.and_then(|lineage| lineage.parent_request_id.clone());
            }
            if artifact.child_session_id.is_none() {
                artifact.child_session_id =
                    lineage.and_then(|lineage| lineage.child_session_id.clone());
            }
        })
        .or_insert_with(|| ResumeArtifactSnapshot {
            path: payload.path.clone(),
            digest: Some(payload.digest.clone()),
            bytes: Some(payload.bytes),
            tool_call_id: payload.tool_call_id.clone(),
            tool_id,
            artifact_kind: payload.metadata.get("artifact_kind").cloned(),
            summary_contract_version: metadata_u32(&payload.metadata, "summary_contract_version"),
            read_file_count: metadata_u32(&payload.metadata, "read_file_count"),
            modified_file_count: metadata_u32(&payload.metadata, "modified_file_count"),
            parent_tool_call_id: lineage.and_then(|lineage| lineage.parent_tool_call_id.clone()),
            parent_task_id: lineage.and_then(|lineage| lineage.parent_task_id.clone()),
            parent_request_id: lineage.and_then(|lineage| lineage.parent_request_id.clone()),
            child_session_id: lineage.and_then(|lineage| lineage.child_session_id.clone()),
        });
}

fn merge_tool_metadata_artifacts(
    session_artifacts: &mut BTreeMap<String, ResumeArtifactSnapshot>,
    tool_call_id: &str,
    snapshot: &ResumeToolCallSnapshot,
    metadata: &ToolCallMetadata,
) {
    for artifact in &metadata.artifact_refs {
        let key = artifact_snapshot_key(&artifact.path, artifact.digest.as_deref());
        session_artifacts
            .entry(key)
            .and_modify(|entry| {
                if entry.tool_call_id.is_none() {
                    entry.tool_call_id = Some(tool_call_id.to_string());
                }
                if entry.tool_id.is_none() {
                    entry.tool_id = snapshot.tool_id.clone();
                }
                if entry.parent_tool_call_id.is_none() {
                    entry.parent_tool_call_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_tool_call_id.clone());
                }
                if entry.parent_task_id.is_none() {
                    entry.parent_task_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_task_id.clone());
                }
                if entry.parent_request_id.is_none() {
                    entry.parent_request_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_request_id.clone());
                }
                if entry.child_session_id.is_none() {
                    entry.child_session_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.child_session_id.clone());
                }
            })
            .or_insert_with(|| ResumeArtifactSnapshot {
                path: artifact.path.clone(),
                digest: artifact.digest.clone(),
                bytes: None,
                tool_call_id: Some(tool_call_id.to_string()),
                tool_id: snapshot.tool_id.clone(),
                artifact_kind: None,
                summary_contract_version: None,
                read_file_count: None,
                modified_file_count: None,
                parent_tool_call_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_tool_call_id.clone()),
                parent_task_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_task_id.clone()),
                parent_request_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_request_id.clone()),
                child_session_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.clone()),
            });
    }
}

fn metadata_u32(metadata: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    metadata.get(key).and_then(|value| value.parse().ok())
}

fn artifact_snapshot_key(path: &str, digest: Option<&str>) -> String {
    let digest = digest.unwrap_or_default();
    format!("{path}\u{001f}{digest}")
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

fn merge_hook_execution(
    existing: &mut Vec<HookExecutionMetadata>,
    candidate: HookExecutionMetadata,
) {
    if existing.iter().any(|current| current == &candidate) {
        return;
    }
    existing.push(candidate);
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

fn apply_child_session_metadata(
    child_sessions: &mut BTreeMap<String, ResumeChildSessionSnapshot>,
    lineage: Option<&TaskLineageMetadata>,
    parent_session_id: Option<&str>,
    timing: Option<&ExecutionTimingMetadata>,
    hook_executions: &[HookExecutionMetadata],
) {
    let Some(lineage) = lineage else {
        return;
    };
    let Some(child_session_id) = lineage.child_session_id.as_ref() else {
        return;
    };

    let child = child_sessions.entry(child_session_id.clone()).or_default();
    if child.parent_tool_call_id.is_none() {
        child.parent_tool_call_id = lineage.parent_tool_call_id.clone();
    }
    if child.parent_task_id.is_none() {
        child.parent_task_id = lineage.parent_task_id.clone();
    }
    if child.parent_request_id.is_none() {
        child.parent_request_id = lineage.parent_request_id.clone();
    }
    if child.parent_session_id.is_none() {
        child.parent_session_id = lineage
            .parent_session_id
            .clone()
            .or_else(|| parent_session_id.map(str::to_string));
    }
    if let Some(request_id) = lineage.child_request_id.as_ref() {
        child.latest_child_request_id = Some(request_id.clone());
    }
    if child.provider_id.is_none() {
        child.provider_id = lineage.child_provider_id.clone();
    }
    if child.model_id.is_none() {
        child.model_id = lineage.child_model_id.clone();
    }
    if let Some(timing) = timing {
        child.timing = Some(timing.clone());
    }
    for hook_execution in hook_executions {
        merge_hook_execution(&mut child.hook_executions, hook_execution.clone());
    }
}

fn apply_agent_turn_terminal_state(
    child_sessions: &mut BTreeMap<String, ResumeChildSessionSnapshot>,
    turn: &AgentTurnProjectionState,
    terminal_state: ChildSessionTerminalState,
    terminal_reason: Option<String>,
    finished_mono_ms: u64,
    timing_override: Option<ExecutionTimingMetadata>,
    hook_executions: &[HookExecutionMetadata],
) {
    let child = child_sessions.entry(turn.agent_id.clone()).or_default();
    child.latest_child_request_id = turn.request_id.clone();
    if let Some(provider_id) = turn.provider_id.as_ref() {
        child.provider_id = Some(provider_id.clone());
    }
    if let Some(model_id) = turn.model_id.as_ref() {
        child.model_id = Some(model_id.clone());
    }
    child.terminal_state = Some(terminal_state);
    child.terminal_reason = terminal_reason;
    child.timing = Some(
        timing_override
            .unwrap_or_else(|| derive_timing_from_start(turn.started_mono_ms, finished_mono_ms)),
    );
    for hook_execution in hook_executions {
        merge_hook_execution(&mut child.hook_executions, hook_execution.clone());
    }
}

fn child_terminal_state_from_background_status(
    status: BackgroundTaskNotificationStatus,
) -> ChildSessionTerminalState {
    match status {
        BackgroundTaskNotificationStatus::Completed => ChildSessionTerminalState::Completed,
        BackgroundTaskNotificationStatus::Cancelled => ChildSessionTerminalState::Cancelled,
        BackgroundTaskNotificationStatus::Failed => ChildSessionTerminalState::Failed,
        BackgroundTaskNotificationStatus::TimedOut => ChildSessionTerminalState::TimedOut,
    }
}

fn derive_timing_from_start(
    started_mono_ms: u64,
    finished_mono_ms: u64,
) -> ExecutionTimingMetadata {
    ExecutionTimingMetadata {
        started_mono_ms: Some(started_mono_ms),
        finished_mono_ms: Some(finished_mono_ms),
        elapsed_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
    }
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
    let recorded_runtime_context = metadata.and_then(|meta| meta.recorded_runtime_context.as_ref());

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

    let latest_title = collected
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::SessionTitleUpdated(data) => Some(data.title.clone()),
            _ => None,
        });

    let run_name = latest_title
        .or_else(|| run_started.map(|data| data.run_name.clone()))
        .or_else(|| metadata.and_then(|meta| meta.run_name.clone()));
    let workspace_root = run_started
        .map(|data| data.workspace_root.clone())
        .or_else(|| metadata.and_then(|meta| meta.workspace_root.clone()));
    let profile_preset = spawned
        .map(|data| data.profile.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.profile.clone()))
        .or_else(|| metadata.and_then(|meta| meta.profile_preset.clone()));
    let provider = provider_started
        .map(|data| data.provider_id.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.provider.clone()))
        .or_else(|| metadata.and_then(|meta| meta.provider.clone()));
    let model = provider_started
        .map(|data| data.model_id.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.model.clone()))
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
    let artifact_count = resume_plan_artifact_count(&resume_plan);
    let child_session_count = resume_plan_child_session_count(&resume_plan);
    let parent_session_id =
        first_lineage_parent_session_id(collected.iter().copied()).map(str::to_string);

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
        artifact_count,
        child_session_count,
        parent_session_id,
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
        EventV1::SessionTitleUpdated(_) => {}
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
        EventV1::BackgroundTaskNotification(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
            summary.tasks_in_flight.remove(&payload.terminal_task_id);
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
        EventV1::SessionTitleUpdated(_) => "session_title_updated",
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::BackgroundTaskNotification(_) => "background_task_notification",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderReasoningDelta(_) => "provider_reasoning_delta",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::AssistantMessageFinished(_) => "assistant_message_finished",
        EventV1::CompactionRequested(_) => "compaction_requested",
        EventV1::CompactionWritten(_) => "compaction_written",
        EventV1::CompactionApplied(_) => "compaction_applied",
        EventV1::CompactionFailed(_) => "compaction_failed",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionGrantRecorded(_) => "permission_grant_recorded",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::TeamCreated(_) => "team_created",
        EventV1::TeamMemberSpawned(_) => "team_member_spawned",
        EventV1::TeamMessageSent(_) => "team_message_sent",
        EventV1::TeamTaskCreated(_) => "team_task_created",
        EventV1::TeamTaskUpdated(_) => "team_task_updated",
        EventV1::PersistentTaskCreated(_) => "persistent_task_created",
        EventV1::PersistentTaskUpdated(_) => "persistent_task_updated",
        EventV1::TeamShutdownRequested(_) => "team_shutdown_requested",
        EventV1::TeamShutdownApproved(_) => "team_shutdown_approved",
        EventV1::TeamShutdownRejected(_) => "team_shutdown_rejected",
        EventV1::TeamDeleted(_) => "team_deleted",
        EventV1::ContinuationStarted(_) => "continuation_started",
        EventV1::ContinuationReminderQueued(_) => "continuation_reminder_queued",
        EventV1::ContinuationStopped(_) => "continuation_stopped",
        EventV1::ContinuationLimitReached(_) => "continuation_limit_reached",
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
    use super::{
        project_background_request, project_resume_plan, project_run_summary,
        project_timeline_index, resolve_background_request_ref, BackgroundRequestProjectionError,
        ChildSessionTerminalState, ProjectionError, RunStatus,
    };
    use crate::event::{
        ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationEvent,
        BackgroundTaskNotificationStatus, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, RunFailedEvent, RunFinishedEvent,
        RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskResultLateEvent,
        TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStatus, SCHEMA_VERSION,
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

    #[test]
    fn background_projection_resolves_lineage_and_terminal_result_from_events() {
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_parent".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                2,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor.clone(),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    tool_id: "read".to_string(),
                    args_summary: "{}".to_string(),
                    args_digest: "argsdigest".to_string(),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                5,
                Some("req_child"),
                child_actor.clone(),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read ok".to_string()),
                    output_digest: Some("outdigest".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                6,
                Some("req_child"),
                child_actor,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "child done".to_string(),
                    result_digest: "resultdigest".to_string(),
                    metadata: None,
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .expect("authorized child request");
        let projection =
            project_background_request(events.iter(), &request_ref).expect("project background");

        assert_eq!(projection.request_id, "req_child");
        assert_eq!(projection.session_id.as_deref(), Some("agent_child"));
        assert_eq!(projection.scheduler_task_id.as_deref(), Some("task_000001"));
        assert_eq!(projection.status, "completed");
        assert!(projection.terminal);
        assert_eq!(projection.result_summary.as_deref(), Some("child done"));
        assert_eq!(projection.tool_calls.requested, 1);
        assert_eq!(projection.tool_calls.succeeded, 1);
        assert_eq!(projection.tool_calls.failed, 0);
    }

    #[test]
    fn background_notification_projects_failed_resume_and_request_state() {
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_parent".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                2,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
            envelope(
                4,
                Some("background_task_notification:req_child"),
                EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                    parent_session_id: "agent_parent".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: "agent_child".to_string(),
                    child_request_id: "req_child".to_string(),
                    task_id: "task_000001".to_string(),
                    description: "investigate".to_string(),
                    status: BackgroundTaskNotificationStatus::Failed,
                    summary: "provider failed closed".to_string(),
                    terminal_event_id: "evt-terminal".to_string(),
                    terminal_task_id: "task_000001".to_string(),
                    delivered_turn_request_id: Some("req_parent_notice".to_string()),
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .expect("authorized child request");
        let projection =
            project_background_request(events.iter(), &request_ref).expect("project background");
        assert_eq!(projection.status, "failed");
        assert!(projection.terminal);
        assert_eq!(projection.session_id.as_deref(), Some("agent_child"));
        assert_eq!(
            projection.failure_summary.as_deref(),
            Some("provider failed closed")
        );

        let plan = project_resume_plan(events.iter(), "run_projection").expect("resume plan");
        assert!(plan.tasks_in_flight.is_empty());
        let child = plan
            .child_sessions
            .get("agent_child")
            .expect("child session snapshot");
        assert_eq!(
            child.terminal_state,
            Some(ChildSessionTerminalState::Failed)
        );
        assert_eq!(
            child.terminal_reason.as_deref(),
            Some("provider failed closed")
        );
        let notification = child
            .background_notification
            .as_ref()
            .expect("notification snapshot");
        assert_eq!(
            notification.status,
            BackgroundTaskNotificationStatus::Failed
        );
        assert_eq!(notification.terminal_event_id, "evt-terminal");
        assert_eq!(
            notification.delivered_turn_request_id.as_deref(),
            Some("req_parent_notice")
        );
    }

    #[test]
    fn background_projection_denies_requests_outside_worker_lineage() {
        let other_actor = EventActor::new(ActorKind::Worker, Some("agent_other".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Queued,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
        ];

        let err =
            resolve_background_request_ref(events.iter(), &other_actor, Some("req_child"), None)
                .expect_err("unrelated worker cannot read child request");
        assert_eq!(err, BackgroundRequestProjectionError::Unauthorized);
    }

    #[test]
    fn background_request_resolution_prefers_explicit_request_id_over_session_hint() {
        let actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_first"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_second"),
                child_actor,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
        ];

        let request_ref = resolve_background_request_ref(
            events.iter(),
            &actor,
            Some("req_first"),
            Some("agent_child"),
        )
        .expect("explicit request id should resolve");

        assert_eq!(request_ref.request_id, "req_first");
    }

    #[test]
    fn background_projection_preserves_cancelled_late_result_state() {
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000001".to_string(),
                    reason: "cancelled by test".to_string(),
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor,
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id: "task_000001".to_string(),
                    result_digest: "latedigest".to_string(),
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .expect("authorized child request");
        let projection =
            project_background_request(events.iter(), &request_ref).expect("project background");

        assert_eq!(projection.status, "cancelled");
        assert!(projection.terminal);
        assert!(projection.late_result);
        assert_eq!(
            projection.cancel_reason.as_deref(),
            Some("cancelled by test")
        );
        assert_eq!(
            projection.failure_summary.as_deref(),
            Some("cancelled by test")
        );
    }

    #[test]
    fn background_projection_ignores_correlated_tool_task_terminal_events() {
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:read".to_string()),
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: "tool done".to_string(),
                    result_digest: "tooldigest".to_string(),
                    metadata: None,
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .expect("authorized child request");
        let projection =
            project_background_request(events.iter(), &request_ref).expect("project background");

        assert_eq!(projection.scheduler_task_id.as_deref(), Some("task_000001"));
        assert_eq!(projection.status, "running");
        assert!(!projection.terminal);
        assert_eq!(projection.result_summary, None);
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

    fn envelope_with_actor(
        seq: u64,
        correlation_id: Option<&str>,
        actor: EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            actor,
            ..envelope(seq, correlation_id, payload)
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
