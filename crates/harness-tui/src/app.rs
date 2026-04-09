use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use harness_core::agent::AgentModelRef;
use harness_core::config::{registered_profile_model_metadata, ResolvedProfileModelMetadata};
use harness_core::event::{
    ActorKind, EventArtifactRef, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    PermissionDecision as EventPermissionDecision, ProviderRequestStartedEvent,
    ResolvedToolIdentity, TaskCompletionMetadata, TaskLineageMetadata, ToolCallLifecycleState,
    ToolCallMetadata, ToolCallStatus, UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;
use harness_core::proj::{
    inspect_resume_plan, RunMetadata, SessionCatalogEntry, SessionModeSource,
};
use serde::Deserialize;

use crate::keybindings::{Action, KeyMap};
use crate::overlay::{OverlayKind, OverlayStack, OverlayState};
use crate::theme::Theme;
use crate::ui::WheelTarget;
use crate::view_model;

mod pending_live;
#[cfg(test)]
mod tests;

pub use pending_live::{
    set_pending_live_launch_metadata, set_pending_live_prompt_auto_submit,
    set_pending_live_prompt_draft,
};
use pending_live::{
    take_pending_live_launch_metadata, take_pending_live_prompt, PendingLivePrompt,
};

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;
const TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 72;
const TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS: usize = 3;
pub(crate) const SLASH_COMMANDS: [(&str, &str); 8] = [
    ("new", "Return to the home shell"),
    ("resume", "Continue a saved session"),
    ("replay", "Replay a saved session"),
    ("model", "Switch model"),
    ("events", "Open the event log review"),
    ("shell", "Return to the session shell"),
    ("follow", "Toggle follow mode"),
    ("exit", "Quit Harness"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDisplayStatus {
    PendingPermission,
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl std::fmt::Display for ToolCallDisplayStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallDisplayStatus::PendingPermission => write!(f, "pending permission"),
            ToolCallDisplayStatus::Queued => write!(f, "queued"),
            ToolCallDisplayStatus::Running => write!(f, "running"),
            ToolCallDisplayStatus::Succeeded => write!(f, "succeeded"),
            ToolCallDisplayStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub tool_call_id: String,
    pub tool_id: String,
    pub canonical_tool_id: Option<String>,
    pub alias_source_tool_id: Option<String>,
    pub resolved_tool_identity: Option<ResolvedToolIdentity>,
    pub args_summary: String,
    pub args_digest: String,
    pub lifecycle_state: Option<ToolCallLifecycleState>,
    pub status: ToolCallDisplayStatus,
    pub output_summary: Option<String>,
    pub output_digest: Option<String>,
    pub output_json: Option<serde_json::Value>,
    pub truncated_output: Option<String>,
    pub edit: Option<EditEntry>,
    pub lineage: Option<TaskLineageEntry>,
    pub artifact_refs: Vec<ToolArtifactEntry>,
    pub timing_elapsed_ms: Option<u64>,
    pub permissions: Vec<PermissionEntry>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_mono_ms: u64,
    pub last_mono_ms: u64,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLineageEntry {
    pub parent_tool_call_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub child_session_id: Option<String>,
    pub child_request_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionNavigationSnapshot {
    session_path: PathBuf,
    events: Vec<EventEnvelopeV1>,
    launch_metadata: LaunchMetadata,
    child_session_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArtifactEntry {
    pub path: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlanExitHandoffEnvelope {
    plan_exit_handoff: PlanExitHandoff,
}

#[derive(Debug, Clone, Deserialize)]
struct PlanExitHandoff {
    source_profile: String,
    target_profile: String,
    prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDisplayStatus {
    Proposed,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditEntry {
    pub edit_id: String,
    pub path: String,
    pub status: EditDisplayStatus,
    pub summary: Option<String>,
    pub patch_digest: Option<String>,
    pub new_file_digest: Option<String>,
    pub diff_rel_path: Option<String>,
    pub diff_digest: Option<String>,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionEntry {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<String>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: EventPermissionDecision,
    pub resolved_decision: Option<EventPermissionDecision>,
    pub resolution_reason: Option<String>,
    pub first_seq: u64,
    pub last_seq: u64,
}

impl ToolCallEntry {
    pub fn duration_ms(&self) -> Option<u64> {
        self.timing_elapsed_ms.or_else(|| {
            (self.last_mono_ms >= self.first_mono_ms)
                .then_some(self.last_mono_ms.saturating_sub(self.first_mono_ms))
        })
    }

    pub fn invoked_tool_id(&self) -> &str {
        self.resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref())
            .unwrap_or(&self.tool_id)
    }

    pub fn effective_tool_id(&self) -> &str {
        self.resolved_tool_identity
            .as_ref()
            .and_then(|identity| {
                identity
                    .effective_tool_id
                    .as_deref()
                    .or(identity.canonical_tool_id.as_deref())
            })
            .or(self.canonical_tool_id.as_deref())
            .unwrap_or(&self.tool_id)
    }

    pub fn resolved_canonical_tool_id(&self) -> Option<&str> {
        if let Some(identity) = self.resolved_tool_identity.as_ref() {
            identity.canonical_tool_id.as_deref()
        } else {
            self.canonical_tool_id.as_deref()
        }
    }

    pub fn resolved_alias_source_tool_id(&self) -> Option<&str> {
        if let Some(identity) = self.resolved_tool_identity.as_ref() {
            identity
                .alias_source_tool_id
                .as_deref()
                .or(self.alias_source_tool_id.as_deref())
        } else {
            self.alias_source_tool_id.as_deref()
        }
    }

    pub fn canonical_tool_id(&self) -> &str {
        self.resolved_canonical_tool_id()
            .unwrap_or_else(|| self.effective_tool_id())
    }

    pub fn lifecycle_state(&self) -> ToolCallLifecycleState {
        self.lifecycle_state.unwrap_or(match self.status {
            ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
                ToolCallLifecycleState::Pending
            }
            ToolCallDisplayStatus::Running => ToolCallLifecycleState::Running,
            ToolCallDisplayStatus::Succeeded => ToolCallLifecycleState::Completed,
            ToolCallDisplayStatus::Failed => ToolCallLifecycleState::Error,
        })
    }

    pub fn is_compat_alias(&self) -> bool {
        self.resolved_alias_source_tool_id()
            .is_some_and(|alias_source| alias_source != self.effective_tool_id())
    }

    fn sync_display_status(&mut self) {
        self.status = display_status_for_tool_call(self.lifecycle_state(), &self.permissions);
    }

    pub fn transcript_timestamp(&self) -> Option<&str> {
        self.last_timestamp
            .as_deref()
            .or(self.first_timestamp.as_deref())
    }

    pub fn transcript_summary(&self) -> Option<String> {
        match self.status {
            ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => self
                .output_summary
                .as_deref()
                .and_then(compact_tool_payload_for_transcript)
                .or_else(|| compact_tool_payload_for_transcript(&self.args_summary)),
            ToolCallDisplayStatus::PendingPermission
            | ToolCallDisplayStatus::Queued
            | ToolCallDisplayStatus::Running => {
                compact_tool_payload_for_transcript(&self.args_summary)
            }
        }
    }

    pub fn edit_path_display(&self) -> Option<String> {
        self.edit
            .as_ref()
            .map(|edit| edit.path.clone())
            .or_else(|| tool_path_summary(&self.args_summary))
    }
}

fn tool_path_summary(args_summary: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    let object = value.as_object()?;
    ["path", "filePath"]
        .iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn compact_tool_payload_for_transcript(payload: &str) -> Option<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let compact = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => compact_json_value_for_transcript(&value),
        Err(_) => collapse_whitespace(trimmed),
    };

    Some(truncate_for_transcript(&compact))
}

fn compact_json_value_for_transcript(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }

            let mut parts = Vec::new();
            for (idx, (key, value)) in map.iter().enumerate() {
                if idx >= TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS {
                    parts.push("…".to_string());
                    break;
                }
                parts.push(format!("{key}={}", compact_json_leaf_for_transcript(value)));
            }
            parts.join(", ")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }

            let mut parts = Vec::new();
            for (idx, item) in items.iter().enumerate() {
                if idx >= TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS {
                    parts.push("…".to_string());
                    break;
                }
                parts.push(compact_json_leaf_for_transcript(item));
            }
            format!("[{}]", parts.join(", "))
        }
        _ => compact_json_leaf_for_transcript(value),
    }
}

fn compact_json_leaf_for_transcript(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => collapse_whitespace(text),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(items) => format!(
            "[{} item{}]",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        serde_json::Value::Object(fields) => format!(
            "{{{} field{}}}",
            fields.len(),
            if fields.len() == 1 { "" } else { "s" }
        ),
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut parts = text.split_whitespace();
    let Some(first) = parts.next() else {
        return String::new();
    };

    let mut compact = String::from(first);
    for part in parts {
        compact.push(' ');
        compact.push_str(part);
    }
    compact
}

fn truncate_for_transcript(text: &str) -> String {
    if text.chars().count() <= TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS {
        return text.to_string();
    }

    let truncated: String = text
        .chars()
        .take(TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS.saturating_sub(1))
        .collect();
    format!("{truncated}…")
}

fn task_lineage_entry_from_metadata(metadata: &TaskLineageMetadata) -> TaskLineageEntry {
    TaskLineageEntry {
        parent_tool_call_id: metadata.parent_tool_call_id.clone(),
        parent_task_id: metadata.parent_task_id.clone(),
        parent_request_id: metadata.parent_request_id.clone(),
        child_session_id: metadata.child_session_id.clone(),
        child_request_id: metadata.child_request_id.clone(),
    }
}

fn tool_artifact_entry_from_metadata(artifact: &EventArtifactRef) -> ToolArtifactEntry {
    ToolArtifactEntry {
        path: artifact.path.clone(),
        digest: artifact.digest.clone(),
    }
}

fn merge_tool_call_metadata(entry: &mut ToolCallEntry, metadata: Option<&ToolCallMetadata>) {
    let Some(metadata) = metadata else {
        return;
    };

    if entry.canonical_tool_id.is_none() {
        entry.canonical_tool_id = metadata.canonical_tool_id.clone();
    }
    if entry.alias_source_tool_id.is_none() {
        entry.alias_source_tool_id = metadata.alias_source_tool_id.clone();
    }
    if entry.lineage.is_none() {
        entry.lineage = metadata
            .lineage
            .as_ref()
            .map(task_lineage_entry_from_metadata);
    }
    if entry.timing_elapsed_ms.is_none() {
        entry.timing_elapsed_ms = metadata
            .timing
            .as_ref()
            .and_then(execution_timing_elapsed_ms);
    }

    for artifact in &metadata.artifact_refs {
        let artifact = tool_artifact_entry_from_metadata(artifact);
        if !entry
            .artifact_refs
            .iter()
            .any(|existing| existing.path == artifact.path && existing.digest == artifact.digest)
        {
            entry.artifact_refs.push(artifact);
        }
    }
}

fn merge_resolved_tool_identity(entry: &mut ToolCallEntry, incoming: ResolvedToolIdentity) {
    if incoming.is_empty() {
        return;
    }

    let identity = entry
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

fn display_status_for_tool_call(
    lifecycle_state: ToolCallLifecycleState,
    permissions: &[PermissionEntry],
) -> ToolCallDisplayStatus {
    if permissions
        .iter()
        .any(|permission| permission.resolved_decision.is_none())
    {
        return ToolCallDisplayStatus::PendingPermission;
    }

    match lifecycle_state {
        ToolCallLifecycleState::Pending => ToolCallDisplayStatus::Queued,
        ToolCallLifecycleState::Running => ToolCallDisplayStatus::Running,
        ToolCallLifecycleState::Completed => ToolCallDisplayStatus::Succeeded,
        ToolCallLifecycleState::Error => ToolCallDisplayStatus::Failed,
    }
}

fn execution_timing_elapsed_ms(timing: &ExecutionTimingMetadata) -> Option<u64> {
    timing
        .elapsed_ms
        .or_else(|| match (timing.started_mono_ms, timing.finished_mono_ms) {
            (Some(started), Some(finished)) if finished >= started => {
                Some(finished.saturating_sub(started))
            }
            _ => None,
        })
}

pub struct ActivityEntry {
    pub request_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub status: ActivityStatus,
    pub user_message: Option<UserMessageSubmittedEvent>,
    pub user_timestamp: Option<String>,
    pub request_data: Option<ProviderRequestStartedEvent>,
    pub thinking_text: String,
    pub transcript_text: String,
    pub usage: Option<ActivityUsage>,
    pub error_message: Option<String>,
    pub permissions: Vec<PermissionEntry>,
    pub tool_calls: Vec<ToolCallEntry>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_mono_ms: u64,
    pub last_mono_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

struct NewStreamingActivityEntryArgs {
    request_id: String,
    model_id: String,
    provider_id: String,
    user_message: Option<UserMessageSubmittedEvent>,
    user_timestamp: Option<String>,
    request_data: Option<ProviderRequestStartedEvent>,
    transcript_text: String,
    first_seq: u64,
    first_mono_ms: u64,
}

fn new_streaming_activity_entry(args: NewStreamingActivityEntryArgs) -> ActivityEntry {
    let NewStreamingActivityEntryArgs {
        request_id,
        model_id,
        provider_id,
        user_message,
        user_timestamp,
        request_data,
        transcript_text,
        first_seq,
        first_mono_ms,
    } = args;
    ActivityEntry {
        request_id,
        model_id,
        provider_id,
        status: ActivityStatus::Streaming,
        user_message,
        user_timestamp,
        request_data,
        thinking_text: String::new(),
        transcript_text,
        usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq,
        last_seq: first_seq,
        first_mono_ms,
        last_mono_ms: first_mono_ms,
    }
}

impl ActivityEntry {
    pub fn duration_ms(&self) -> Option<u64> {
        (self.last_mono_ms >= self.first_mono_ms)
            .then_some(self.last_mono_ms.saturating_sub(self.first_mono_ms))
    }
}

fn mark_activity_event(entry: &mut ActivityEntry, seq: u64, mono_ms: u64) {
    if entry.first_seq == 0 {
        entry.first_seq = seq;
    }
    entry.last_seq = seq;
    if entry.first_mono_ms == 0 {
        entry.first_mono_ms = mono_ms;
    }
    entry.last_mono_ms = mono_ms;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityStatus {
    Streaming,
    Done,
    Error,
}

impl std::fmt::Display for ActivityStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivityStatus::Streaming => write!(f, "streaming…"),
            ActivityStatus::Done => write!(f, "done"),
            ActivityStatus::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationTaskState {
    Queued,
    Running,
    Stale,
    Completed,
    Cancelled,
    LateResult,
}

impl OrchestrationTaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::LateResult)
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::Stale => 0,
            Self::Running => 1,
            Self::Queued => 2,
            Self::Completed | Self::Cancelled | Self::LateResult => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationTaskRow {
    pub task_id: String,
    pub queue_key: Option<String>,
    pub state: OrchestrationTaskState,
    pub warning: Option<String>,
    pub owner_kind: ActorKind,
    pub owner_agent_id: Option<String>,
    pub request_id: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub child_session_id: Option<String>,
    pub child_request_id: Option<String>,
    pub result_summary: Option<String>,
    pub child_tool_call_count: usize,
    pub timing_elapsed_ms: Option<u64>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_mono_ms: u64,
    pub last_mono_ms: u64,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
}

impl OrchestrationTaskRow {
    pub fn duration_ms(&self) -> Option<u64> {
        self.timing_elapsed_ms.or_else(|| {
            (self.last_mono_ms >= self.first_mono_ms)
                .then_some(self.last_mono_ms.saturating_sub(self.first_mono_ms))
        })
    }

    pub fn transcript_timestamp(&self) -> Option<&str> {
        self.last_timestamp
            .as_deref()
            .or(self.first_timestamp.as_deref())
    }

    pub(crate) fn effective_child_session_id(&self) -> Option<&str> {
        self.child_session_id
            .as_deref()
            .or(self.owner_agent_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn effective_child_request_id(&self) -> Option<&str> {
        self.child_request_id
            .as_deref()
            .or(self.request_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

fn merge_orchestration_task_event(row: &mut OrchestrationTaskRow, event: &EventEnvelopeV1) {
    if row.first_seq == 0 {
        row.first_seq = event.seq;
    }
    row.last_seq = event.seq;
    if row.first_mono_ms == 0 {
        row.first_mono_ms = event.mono_ms;
    }
    row.last_mono_ms = event.mono_ms;
    if row.first_timestamp.is_none() {
        row.first_timestamp = event.ts.clone();
    }
    row.last_timestamp = event.ts.clone();
    if row.request_id.is_none() {
        row.request_id = event.correlation_id.clone();
    }
    if row.child_session_id.is_none() {
        row.child_session_id = event.actor.agent_id.clone();
    }
}

fn merge_orchestration_task_lineage(
    row: &mut OrchestrationTaskRow,
    lineage: Option<&TaskLineageMetadata>,
) {
    let Some(lineage) = lineage else {
        return;
    };

    if row.parent_tool_call_id.is_none() {
        row.parent_tool_call_id = lineage.parent_tool_call_id.clone();
    }
    if row.parent_request_id.is_none() {
        row.parent_request_id = lineage.parent_request_id.clone();
    }
    if row.child_session_id.is_none() {
        row.child_session_id = lineage.child_session_id.clone();
    }
    if row.child_request_id.is_none() {
        row.child_request_id = lineage.child_request_id.clone();
    }
    if row.request_id.is_none() {
        row.request_id = lineage.child_request_id.clone();
    }
}

fn merge_orchestration_task_completion_metadata(
    row: &mut OrchestrationTaskRow,
    metadata: Option<&TaskCompletionMetadata>,
) {
    let Some(metadata) = metadata else {
        return;
    };

    merge_orchestration_task_lineage(row, metadata.lineage.as_ref());
    if row.timing_elapsed_ms.is_none() {
        row.timing_elapsed_ms = metadata
            .timing
            .as_ref()
            .and_then(execution_timing_elapsed_ms);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrchestrationSummary {
    pub active_agents: usize,
    pub queued: usize,
    pub running: usize,
    pub stale: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationOwnerLabels {
    pub label: String,
    pub profile: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStateKind {
    Ready,
    Sending,
    Streaming,
    Success,
    Failure,
    Cancelled,
    PermissionBlocked,
    PermissionPending,
    Degraded,
    Disconnected,
}

impl RuntimeStateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Sending => "Sending",
            Self::Streaming => "Streaming",
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Cancelled => "Cancelled",
            Self::PermissionBlocked => "Permission blocked",
            Self::PermissionPending => "Permission pending",
            Self::Degraded => "Degraded",
            Self::Disconnected => "Disconnected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeState {
    pub kind: RuntimeStateKind,
    pub summary: String,
    pub detail: Option<String>,
    pub composer_disabled: bool,
    pub composer_hint: String,
}

#[derive(Debug, Clone)]
pub struct MemoryCaps {
    pub max_events: usize,
    pub max_transcript_chars: usize,
}

impl Default for MemoryCaps {
    fn default() -> Self {
        Self {
            max_events: 25_000,
            max_transcript_chars: 200_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Run,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSurface {
    Events,
    Help,
}

impl ReviewSurface {
    pub(crate) fn status_label(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Help => "shortcuts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Home,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellDescriptor {
    pub kind: ShellKind,
    pub label: &'static str,
    pub read_only: bool,
}

const LIVE_DEFAULT_SHELL_REGISTRY: [ShellDescriptor; 2] = [
    ShellDescriptor {
        kind: ShellKind::Home,
        label: "Home",
        read_only: false,
    },
    ShellDescriptor {
        kind: ShellKind::Session,
        label: "Session",
        read_only: false,
    },
];

const REPLAY_DEFAULT_SHELL_REGISTRY: [ShellDescriptor; 2] = [
    ShellDescriptor {
        kind: ShellKind::Home,
        label: "Home",
        read_only: false,
    },
    ShellDescriptor {
        kind: ShellKind::Session,
        label: "Replay",
        read_only: true,
    },
];

pub fn default_shell_registry(replay_mode: bool) -> &'static [ShellDescriptor] {
    if replay_mode {
        &REPLAY_DEFAULT_SHELL_REGISTRY
    } else {
        &LIVE_DEFAULT_SHELL_REGISTRY
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    List,
    Details,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIntent {
    ResolvePermission {
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    },
    SwitchModel {
        profile: String,
        launch_metadata: LaunchMetadata,
    },
    NewSession,
    ReplaySession {
        run_id: String,
        run_dir: PathBuf,
    },
    ContinueSession {
        run_id: String,
        run_dir: PathBuf,
    },
    SubmitPrompt {
        text: String,
        launch_metadata: LaunchMetadata,
    },
    QuitRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartupLauncherAction {
    #[default]
    NewSession,
    ReplaySession,
    ContinueSession,
}

impl StartupLauncherAction {
    pub const ORDERED: [Self; 3] = [Self::NewSession, Self::ContinueSession, Self::ReplaySession];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NewSession => "New session",
            Self::ContinueSession => "Continue session",
            Self::ReplaySession => "Replay session",
        }
    }

    const fn previous(self) -> Self {
        match self {
            Self::NewSession => Self::ReplaySession,
            Self::ContinueSession => Self::NewSession,
            Self::ReplaySession => Self::ContinueSession,
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::NewSession => Self::ContinueSession,
            Self::ContinueSession => Self::ReplaySession,
            Self::ReplaySession => Self::NewSession,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostRunHandoffAction {
    #[default]
    ContinueSession,
    ReplayRun,
    StartAnotherSession,
    Quit,
}

impl PostRunHandoffAction {
    pub const ORDERED: [Self; 4] = [
        Self::ContinueSession,
        Self::ReplayRun,
        Self::StartAnotherSession,
        Self::Quit,
    ];

    pub const FALLBACK_ORDERED: [Self; 2] = [Self::StartAnotherSession, Self::Quit];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ContinueSession => "Continue this session",
            Self::ReplayRun => "Replay this run",
            Self::StartAnotherSession => "Start another session",
            Self::Quit => "Quit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifecycleShellState {
    #[default]
    None,
    Startup,
    PostRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
}

pub(crate) fn session_history_run_name(entry: &SessionHistoryEntry) -> &str {
    entry.catalog.run_name.as_deref().unwrap_or("<unavailable>")
}

pub(crate) fn session_history_status_label(entry: &SessionHistoryEntry) -> &'static str {
    match entry.catalog.status {
        Some(harness_core::proj::RunStatus::Running) => "running",
        Some(harness_core::proj::RunStatus::Finished) => "finished",
        Some(harness_core::proj::RunStatus::Failed) => "failed",
        None => "<unavailable>",
    }
}

pub(crate) fn session_history_recency_label(entry: &SessionHistoryEntry) -> String {
    entry
        .catalog
        .last_updated_at
        .as_deref()
        .map(format_session_history_timestamp)
        .unwrap_or_else(|| "updated <unavailable>".to_string())
}

pub(crate) fn session_history_profile_label(entry: &SessionHistoryEntry) -> &str {
    entry
        .catalog
        .profile_preset
        .as_deref()
        .unwrap_or("<unavailable>")
}

pub(crate) fn session_history_provider_model_label(entry: &SessionHistoryEntry) -> &str {
    entry
        .catalog
        .provider_model
        .as_deref()
        .unwrap_or("<unavailable>")
}

pub(crate) fn session_history_resumability_label(entry: &SessionHistoryEntry) -> String {
    if entry.catalog.is_resumable {
        "continue ready".to_string()
    } else {
        entry
            .catalog
            .resume_disabled_reason
            .as_deref()
            .map(|reason| format!("continue blocked · {reason}"))
            .unwrap_or_else(|| "continue blocked".to_string())
    }
}

fn artifact_count_label(count: usize) -> String {
    match count {
        0 => "no artifacts".to_string(),
        1 => "1 artifact".to_string(),
        count => format!("{count} artifacts"),
    }
}

fn lineage_label(child_session_count: usize, parent_session_id: Option<&str>) -> String {
    let mut parts = Vec::new();
    if child_session_count > 0 {
        let child_label = if child_session_count == 1 {
            "1 child".to_string()
        } else {
            format!("{child_session_count} children")
        };
        parts.push(child_label);
    }
    if let Some(parent_session_id) = parent_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("parent {parent_session_id}"));
    }

    if parts.is_empty() {
        "root session".to_string()
    } else {
        parts.join(" · ")
    }
}

pub(crate) fn session_history_artifact_label(entry: &SessionHistoryEntry) -> String {
    artifact_count_label(entry.catalog.artifact_count)
}

pub(crate) fn session_history_lineage_label(entry: &SessionHistoryEntry) -> String {
    lineage_label(
        entry.catalog.child_session_count,
        entry.catalog.parent_session_id.as_deref(),
    )
}

fn session_history_entry_matches_action(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> bool {
    match action {
        StartupLauncherAction::ContinueSession => matches!(
            entry.catalog.mode_source,
            SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
        ),
        StartupLauncherAction::ReplaySession | StartupLauncherAction::NewSession => !matches!(
            entry.catalog.mode_source,
            SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
        ),
    }
}

const fn session_history_action_sort_bucket(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> u8 {
    match action {
        StartupLauncherAction::ContinueSession if !entry.catalog.is_resumable => 1,
        _ => 0,
    }
}

fn format_session_history_timestamp(timestamp: &str) -> String {
    let trimmed = timestamp.trim();
    if trimmed.len() >= 16 && trimmed.as_bytes().get(10) == Some(&b'T') {
        format!("updated {}", trimmed[..16].replace('T', " "))
    } else if trimmed.is_empty() {
        "updated <unavailable>".to_string()
    } else {
        format!("updated {trimmed}")
    }
}

fn session_history_filter_matches(entry: &SessionHistoryEntry, input: &str) -> bool {
    if input.is_empty() {
        return true;
    }

    let candidates = [
        session_history_run_name(entry).to_lowercase(),
        entry.catalog.run_id.to_lowercase(),
        session_history_status_label(entry).to_string(),
        session_history_recency_label(entry).to_lowercase(),
        session_history_profile_label(entry).to_lowercase(),
        session_history_provider_model_label(entry).to_lowercase(),
        session_history_resumability_label(entry).to_lowercase(),
        session_history_artifact_label(entry).to_lowercase(),
        session_history_lineage_label(entry).to_lowercase(),
    ];

    candidates.iter().any(|candidate| candidate.contains(input))
}

#[derive(Debug, Clone)]
struct PendingPermission {
    seq: u64,
    kind: String,
    summary: String,
    request_digest: String,
    timeout_ms: u64,
    default_decision: EventPermissionDecision,
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePermissionView {
    pub permission_id: String,
    pub kind: String,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: EventPermissionDecision,
    pub tool_call_id: Option<String>,
    pub tool_label: Option<String>,
    pub question_prompts: Option<Vec<QuestionPromptView>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionPromptView {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOptionView>,
    pub multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOptionView {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchMetadata {
    profile: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    variant: Option<String>,
    display_label: Option<String>,
    token_window_label: Option<String>,
    context_window_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    description: Option<String>,
    reasoning_effort: Option<String>,
    text_verbosity: Option<String>,
    recommended_for: Option<String>,
    mode_label: Option<String>,
    available_models: Vec<ModelOption>,
}

impl LaunchMetadata {
    pub fn new(
        profile: impl Into<String>,
        provider: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        let profile = profile.into();
        let provider = provider.into();
        let model = model.filter(|value| !value.trim().is_empty());
        let mut metadata = Self {
            profile: Some(profile.clone()),
            provider: Some(provider.clone()),
            model,
            variant: None,
            display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
            mode_label: None,
            available_models: Vec::new(),
        };
        metadata.apply_registered_metadata();
        metadata
    }

    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let profile = profile.into();
        let model_ref = AgentModelRef::parse(model_ref);
        Self::new(profile, model_ref.provider_id, Some(model_ref.model_id))
    }

    pub fn from_model_option(option: &ModelOption) -> Self {
        Self {
            profile: Some(option.profile.clone()),
            provider: Some(option.provider.clone()),
            model: Some(option.model.clone()),
            variant: option.variant.clone(),
            display_label: option.display_label.clone(),
            token_window_label: option.token_window_label.clone(),
            context_window_tokens: option.context_window_tokens,
            max_input_tokens: option.max_input_tokens,
            max_output_tokens: option.max_output_tokens,
            description: option.description.clone(),
            reasoning_effort: option.reasoning_effort.clone(),
            text_verbosity: option.text_verbosity.clone(),
            recommended_for: option.recommended_for.clone(),
            mode_label: None,
            available_models: Vec::new(),
        }
    }

    pub fn with_mode_label(mut self, mode_label: impl Into<String>) -> Self {
        self.mode_label = Some(mode_label.into());
        self
    }

    pub fn without_mode_label(mut self) -> Self {
        self.mode_label = None;
        self
    }

    pub fn with_available_models(mut self, available_models: Vec<ModelOption>) -> Self {
        self.available_models = available_models;
        self
    }

    pub fn profile(&self) -> &str {
        self.profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
    }

    pub fn provider(&self) -> &str {
        self.provider
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("local")
    }

    pub fn model(&self) -> Option<&str> {
        self.model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn variant(&self) -> Option<&str> {
        self.variant
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.variant())
            })
    }

    pub fn display_label(&self) -> Option<&str> {
        self.display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.display_label())
            })
    }

    pub fn token_window_label(&self) -> Option<&str> {
        self.token_window_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.token_window_label())
            })
    }

    pub fn context_window_tokens(&self) -> Option<u32> {
        self.context_window_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.context_window_tokens)
        })
    }

    pub fn max_input_tokens(&self) -> Option<u32> {
        self.max_input_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.max_input_tokens)
        })
    }

    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.max_output_tokens)
        })
    }

    pub fn description(&self) -> Option<&str> {
        self.description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.description())
            })
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.reasoning_effort())
            })
    }

    pub fn text_verbosity(&self) -> Option<&str> {
        self.text_verbosity
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.text_verbosity())
            })
    }

    pub fn recommended_for(&self) -> Option<&str> {
        self.recommended_for
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.recommended_for())
            })
    }

    pub fn mode_label(&self) -> Option<&str> {
        self.mode_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn available_models(&self) -> &[ModelOption] {
        &self.available_models
    }

    fn to_model_option(&self) -> Option<ModelOption> {
        Some(ModelOption {
            profile: self.profile().to_string(),
            provider: self.provider().to_string(),
            model: self.model()?.to_string(),
            variant: self.variant().map(str::to_string),
            display_label: self.display_label().map(str::to_string),
            token_window_label: self.token_window_label().map(str::to_string),
            context_window_tokens: self.context_window_tokens(),
            max_input_tokens: self.max_input_tokens(),
            max_output_tokens: self.max_output_tokens(),
            description: self.description().map(str::to_string),
            reasoning_effort: self.reasoning_effort().map(str::to_string),
            text_verbosity: self.text_verbosity().map(str::to_string),
            recommended_for: self.recommended_for().map(str::to_string),
        })
    }

    fn apply_registered_metadata(&mut self) {
        let profile = self.profile();
        let provider = self.provider();
        let model = self.model();
        let Some(metadata) = metadata_for_profile_identity(profile, provider, model) else {
            return;
        };
        self.apply_resolved_metadata(&metadata);
    }

    fn apply_resolved_metadata(&mut self, metadata: &ResolvedProfileModelMetadata) {
        self.variant = metadata.variant.clone();
        self.display_label = Some(metadata.display_label.clone());
        self.token_window_label = metadata.token_window_label.clone();
        self.context_window_tokens = metadata.context_window_tokens;
        self.max_input_tokens = metadata.max_input_tokens;
        self.max_output_tokens = metadata.max_output_tokens;
        self.description = metadata.description.clone();
        self.reasoning_effort = metadata.reasoning_effort.clone();
        self.text_verbosity = metadata.text_verbosity.clone();
        self.recommended_for = metadata.recommended_for.clone();
    }

    fn matching_available_model(&self) -> Option<&ModelOption> {
        let profile = self.profile();
        let provider = self.provider();
        let model = self.model();
        let variant = self
            .variant
            .as_deref()
            .filter(|value| !value.trim().is_empty());

        let mut exact_profile_matches = self.available_models.iter().filter(|option| {
            option.profile == profile
                && option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
                && option.variant() == variant
        });
        if let Some(first) = exact_profile_matches.next() {
            return Some(first);
        }

        let mut exact_variant_matches = self.available_models.iter().filter(|option| {
            option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
                && option.variant() == variant
        });
        if let Some(first) = exact_variant_matches.next() {
            if exact_variant_matches.next().is_none() {
                return Some(first);
            }
        }

        let mut profile_matches = self.available_models.iter().filter(|option| {
            option.profile == profile
                && option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
        });
        if let Some(first) = profile_matches.next() {
            if profile_matches.next().is_none() {
                return Some(first);
            }
        }

        let mut matches = self.available_models.iter().filter(|option| {
            option.provider == provider && model.is_some_and(|model_id| option.model == model_id)
        });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub display_label: Option<String>,
    pub token_window_label: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
}

impl ModelOption {
    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let profile = profile.into();
        let model_ref = AgentModelRef::parse(model_ref);
        let mut option = Self {
            profile,
            provider: model_ref.provider_id,
            model: model_ref.model_id,
            variant: None,
            display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };
        option.apply_registered_metadata();
        option
    }

    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = profile.into();
        self
    }

    fn matches(&self, input: &str) -> bool {
        if input.is_empty() {
            return true;
        }

        let input = input.to_lowercase();
        self.profile.to_lowercase().contains(&input)
            || self.provider.to_lowercase().contains(&input)
            || self.model.to_lowercase().contains(&input)
            || self
                .variant()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .display_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .token_window_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .description()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .reasoning_effort()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .text_verbosity()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .recommended_for()
                .is_some_and(|value| value.to_lowercase().contains(&input))
    }

    pub fn variant(&self) -> Option<&str> {
        self.variant
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn display_label(&self) -> Option<&str> {
        self.display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn token_window_label(&self) -> Option<&str> {
        self.token_window_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn description(&self) -> Option<&str> {
        self.description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn text_verbosity(&self) -> Option<&str> {
        self.text_verbosity
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn recommended_for(&self) -> Option<&str> {
        self.recommended_for
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    fn apply_registered_metadata(&mut self) {
        let Some(metadata) = metadata_for_profile_identity(
            self.profile.as_str(),
            self.provider.as_str(),
            Some(self.model.as_str()),
        ) else {
            return;
        };
        self.variant = metadata.variant;
        self.display_label = Some(metadata.display_label);
        self.token_window_label = metadata.token_window_label;
        self.context_window_tokens = metadata.context_window_tokens;
        self.max_input_tokens = metadata.max_input_tokens;
        self.max_output_tokens = metadata.max_output_tokens;
        self.description = metadata.description;
        self.reasoning_effort = metadata.reasoning_effort;
        self.text_verbosity = metadata.text_verbosity;
        self.recommended_for = metadata.recommended_for;
    }
}

impl PartialOrd for ModelOption {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModelOption {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.provider
            .cmp(&other.provider)
            .then_with(|| self.model.cmp(&other.model))
            .then_with(|| self.variant.cmp(&other.variant))
            .then_with(|| self.profile.cmp(&other.profile))
    }
}

fn metadata_for_profile_identity(
    profile: &str,
    provider: &str,
    model: Option<&str>,
) -> Option<ResolvedProfileModelMetadata> {
    let metadata = registered_profile_model_metadata(profile)?;
    if metadata.provider != provider {
        return None;
    }
    if let Some(model_id) = model {
        if metadata.model != model_id {
            return None;
        }
    }
    Some(metadata)
}

#[derive(Default)]
pub struct SessionProjection {
    pub(crate) events: Vec<EventEnvelopeV1>,
    pub(crate) activities: VecDeque<ActivityEntry>,
    pub(crate) memory_caps: MemoryCaps,
    pub(crate) events_trimmed_count: usize,
    pub(crate) transcript_trimmed_count: usize,
    orchestration_tasks: BTreeMap<String, OrchestrationTaskRow>,
    agent_profiles: BTreeMap<String, String>,
    seen_seqs: BTreeSet<u64>,
    pending_permissions: BTreeMap<String, PendingPermission>,
    run_terminal_seen: bool,
}

pub struct AppState {
    pub selected_event_index: usize,
    pub focus: Focus,
    pub follow_mode: bool,
    pub active_tab: Tab,
    pub(crate) active_review_surface: Option<ReviewSurface>,
    pub(crate) live_details_drawer_open: bool,
    projection: SessionProjection,
    pub should_quit: bool,
    pub replay_mode: bool,
    pub session_path: Option<PathBuf>,
    pub status_banner: Option<String>,
    pub details_scroll: u16,
    pub transcript_scroll: u16,
    transcript_animation_phase: usize,
    pub auto_exit_on_finish: bool,
    pub prompt_buffer: String,
    pub prompt_cursor: usize,
    pub prompt_history: Vec<String>,
    pub prompt_history_index: Option<usize>,
    pub selected_activity_index: usize,
    pub palette_visible: bool,
    pub palette_input: String,
    pub palette_cursor: usize,
    pub palette_filtered: Vec<String>,
    pub palette_selected: usize,
    palette_focus_return: Option<Focus>,
    show_transcript_thinking: bool,
    show_transcript_timestamps: bool,
    show_tool_details: bool,
    show_generic_tool_output: bool,
    stacked_transcript_diffs: bool,
    expanded_tool_outputs: BTreeSet<String>,
    pub startup_mode: bool,
    pub startup_launcher_action: StartupLauncherAction,
    post_run_handoff_action: PostRunHandoffAction,
    continued_post_run_handoff_active: bool,
    continued_live_reopen_surface_active: bool,
    pub session_history_visible: bool,
    pub model_switcher_visible: bool,
    pub session_history_entries: Vec<SessionHistoryEntry>,
    pub session_history_filtered: Vec<usize>,
    pub session_history_selected: usize,
    pub model_options: Vec<ModelOption>,
    pub model_filtered: Vec<usize>,
    pub model_selected: usize,
    pub slash_visible: bool,
    pub slash_filtered: Vec<String>,
    pub slash_selected: usize,
    slash_draft_snapshot: Option<String>,
    pub continue_disabled_banner: Option<String>,
    pub keymap: KeyMap,
    theme: Theme,
    launch_metadata: LaunchMetadata,
    runtime_context_metadata: Option<LaunchMetadata>,
    session_navigation_stack: Vec<SessionNavigationSnapshot>,
    dismissed_permissions: BTreeSet<String>,
    submitted_permission_id: Option<String>,
    question_answer_permission_id: Option<String>,
    question_answer_buffer: String,
    question_answer_cursor: usize,
    question_answer_error: Option<String>,
    reload_requested: bool,
    on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected_event_index: 0,
            focus: Focus::default(),
            follow_mode: true,
            active_tab: Tab::default(),
            active_review_surface: None,
            live_details_drawer_open: false,
            projection: SessionProjection::default(),
            should_quit: false,
            replay_mode: false,
            session_path: None,
            status_banner: None,
            details_scroll: 0,
            transcript_scroll: 0,
            transcript_animation_phase: 0,
            auto_exit_on_finish: false,
            prompt_buffer: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            prompt_history_index: None,
            selected_activity_index: 0,
            palette_visible: false,
            palette_input: String::new(),
            palette_cursor: 0,
            palette_filtered: Vec::new(),
            palette_selected: 0,
            palette_focus_return: None,
            show_transcript_thinking: true,
            show_transcript_timestamps: false,
            show_tool_details: true,
            show_generic_tool_output: false,
            stacked_transcript_diffs: false,
            expanded_tool_outputs: BTreeSet::new(),
            startup_mode: false,
            startup_launcher_action: StartupLauncherAction::default(),
            post_run_handoff_action: PostRunHandoffAction::default(),
            continued_post_run_handoff_active: false,
            continued_live_reopen_surface_active: false,
            session_history_visible: false,
            model_switcher_visible: false,
            session_history_entries: Vec::new(),
            session_history_filtered: Vec::new(),
            session_history_selected: 0,
            model_options: Vec::new(),
            model_filtered: Vec::new(),
            model_selected: 0,
            slash_visible: false,
            slash_filtered: Vec::new(),
            slash_selected: 0,
            slash_draft_snapshot: None,
            continue_disabled_banner: None,
            keymap: KeyMap::default(),
            theme: Theme::default(),
            launch_metadata: LaunchMetadata::default(),
            runtime_context_metadata: None,
            session_navigation_stack: Vec::new(),
            dismissed_permissions: BTreeSet::new(),
            submitted_permission_id: None,
            question_answer_permission_id: None,
            question_answer_buffer: String::new(),
            question_answer_cursor: 0,
            question_answer_error: None,
            reload_requested: false,
            on_ui_intent: None,
        }
    }
}

impl Deref for AppState {
    type Target = SessionProjection;

    fn deref(&self) -> &Self::Target {
        &self.projection
    }
}

impl DerefMut for AppState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.projection
    }
}

impl SessionProjection {
    fn reset(&mut self) {
        self.events.clear();
        self.activities.clear();
        self.orchestration_tasks.clear();
        self.agent_profiles.clear();
        self.seen_seqs.clear();
        self.pending_permissions.clear();
        self.run_terminal_seen = false;
        self.events_trimmed_count = 0;
        self.transcript_trimmed_count = 0;
    }

    fn has_seen_seq(&self, seq: u64) -> bool {
        self.seen_seqs.contains(&seq)
    }

    fn ingest_event(&mut self, event: EventEnvelopeV1, historical: bool) -> usize {
        self.seen_seqs.insert(event.seq);
        self.update_derived_state_for_event(&event, historical);
        self.events.push(event);
        self.enforce_event_memory_cap()
    }

    fn find_tool_call_mut(&mut self, tool_call_id: &str) -> Option<&mut ToolCallEntry> {
        for activity in &mut self.activities {
            if let Some(tool_call) = activity
                .tool_calls
                .iter_mut()
                .find(|tc| tc.tool_call_id == tool_call_id)
            {
                return Some(tool_call);
            }
        }
        None
    }

    fn activity_index_for_request(&self, request_id: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| activity.request_id == request_id)
    }

    fn adopt_local_prompt_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        let last_index = self.activities.len().checked_sub(1)?;
        let entry = self.activities.get_mut(last_index)?;
        if !entry.request_id.is_empty() {
            return None;
        }

        entry.request_id = request_id.to_string();
        if entry.first_seq == 0 {
            entry.first_seq = seq;
        }
        entry.last_seq = seq;
        Some(last_index)
    }

    fn activity_index_or_local_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        self.activity_index_for_request(request_id)
            .or_else(|| self.adopt_local_prompt_echo(request_id, seq))
    }

    fn attach_permission_request(&mut self, event: &EventEnvelopeV1) {
        let EventV1::PermissionRequested(data) = &event.payload else {
            return;
        };

        let permission_entry = PermissionEntry {
            permission_id: data.permission_id.clone(),
            kind: data.kind.clone(),
            tool_call_id: data.tool_call_id.clone(),
            summary: data.summary.clone(),
            request_digest: data.request_digest.clone(),
            timeout_ms: data.timeout_ms,
            default_decision: data.default_decision,
            resolved_decision: None,
            resolution_reason: None,
            first_seq: event.seq,
            last_seq: event.seq,
        };

        if let Some(tool_call_id) = data.tool_call_id.as_deref() {
            if let Some(tool_entry) = self.find_tool_call_mut(tool_call_id) {
                tool_entry.permissions.push(permission_entry);
                tool_entry.sync_display_status();
                return;
            }
        }

        // Find target activity without holding borrows across or_else
        let found_by_correlation = event.correlation_id.as_deref().and_then(|request_id| {
            self.activities
                .iter()
                .position(|activity| activity.request_id == request_id)
        });

        if let Some(idx) = found_by_correlation {
            if let Some(activity) = self.activities.get_mut(idx) {
                activity.permissions.push(permission_entry);
                activity.last_seq = event.seq;
            }
        } else if let Some(activity) = self.activities.back_mut() {
            activity.permissions.push(permission_entry);
            activity.last_seq = event.seq;
        }
    }

    fn update_permission_resolution(
        &mut self,
        permission_id: &str,
        decision: EventPermissionDecision,
        reason: Option<&str>,
        seq: u64,
    ) {
        for activity in &mut self.activities {
            for permission in &mut activity.permissions {
                if permission.permission_id == permission_id {
                    permission.resolved_decision = Some(decision);
                    permission.resolution_reason = reason.map(str::to_owned);
                    permission.last_seq = seq;
                    activity.last_seq = seq;
                    return;
                }
            }

            for tool_call in &mut activity.tool_calls {
                for permission in &mut tool_call.permissions {
                    if permission.permission_id == permission_id {
                        permission.resolved_decision = Some(decision);
                        permission.resolution_reason = reason.map(str::to_owned);
                        permission.last_seq = seq;
                        tool_call.sync_display_status();
                        tool_call.last_seq = seq;
                        activity.last_seq = seq;
                        return;
                    }
                }
            }
        }
    }

    fn orchestration_task_row_mut(
        &mut self,
        event: &EventEnvelopeV1,
        task_id: &str,
    ) -> &mut OrchestrationTaskRow {
        let row = self
            .orchestration_tasks
            .entry(task_id.to_string())
            .or_insert_with(|| OrchestrationTaskRow {
                task_id: task_id.to_string(),
                queue_key: None,
                state: OrchestrationTaskState::Queued,
                warning: None,
                owner_kind: event.actor.kind,
                owner_agent_id: event.actor.agent_id.clone(),
                request_id: event.correlation_id.clone(),
                parent_tool_call_id: None,
                parent_request_id: None,
                child_session_id: event.actor.agent_id.clone(),
                child_request_id: event.correlation_id.clone(),
                result_summary: None,
                child_tool_call_count: 0,
                timing_elapsed_ms: None,
                first_seq: event.seq,
                last_seq: event.seq,
                first_mono_ms: event.mono_ms,
                last_mono_ms: event.mono_ms,
                first_timestamp: event.ts.clone(),
                last_timestamp: event.ts.clone(),
            });

        row.owner_kind = event.actor.kind;
        if let Some(agent_id) = event.actor.agent_id.as_ref() {
            row.owner_agent_id = Some(agent_id.clone());
        }
        row
    }

    fn update_orchestration_task<F>(&mut self, event: &EventEnvelopeV1, task_id: &str, update: F)
    where
        F: FnOnce(&mut OrchestrationTaskRow),
    {
        {
            let row = self.orchestration_task_row_mut(event, task_id);
            merge_orchestration_task_event(row, event);
            update(row);
        }
        self.enforce_orchestration_retention();
    }

    fn note_child_task_tool_call(&mut self, event: &EventEnvelopeV1) {
        let Some(request_id) = event.correlation_id.as_deref() else {
            return;
        };

        for row in self.orchestration_tasks.values_mut() {
            if row.effective_child_request_id() == Some(request_id) {
                row.child_tool_call_count = row.child_tool_call_count.saturating_add(1);
                row.last_seq = event.seq;
                row.last_mono_ms = event.mono_ms;
                row.last_timestamp = event.ts.clone();
            }
        }
    }

    fn transcript_task_row_for_tool_call(
        &self,
        tool_call: &ToolCallEntry,
    ) -> Option<OrchestrationTaskRow> {
        let child_request_id = tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.clone())
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_request_id", "request_id"],
                )
            });
        let child_session_id = tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.clone())
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_session_id", "session_id"],
                )
            });

        self.orchestration_tasks
            .values()
            .filter_map(|row| {
                let mut score = 0u8;
                if row.parent_tool_call_id.as_deref() == Some(tool_call.tool_call_id.as_str()) {
                    score += 8;
                }
                if child_request_id
                    .as_deref()
                    .is_some_and(|request_id| row.effective_child_request_id() == Some(request_id))
                {
                    score += 4;
                }
                if child_session_id
                    .as_deref()
                    .is_some_and(|session_id| row.effective_child_session_id() == Some(session_id))
                {
                    score += 2;
                }
                (score > 0).then_some((score, !row.state.is_terminal(), row.last_seq, row.clone()))
            })
            .max_by_key(|(score, active, last_seq, _)| (*score, *active, *last_seq))
            .map(|(_, _, _, row)| row)
    }

    fn enforce_orchestration_retention(&mut self) {
        let mut terminal_rows = self
            .orchestration_tasks
            .iter()
            .filter(|(_, row)| row.state.is_terminal())
            .map(|(task_id, row)| (task_id.clone(), row.last_seq))
            .collect::<Vec<_>>();

        if terminal_rows.len() <= 5 {
            return;
        }

        terminal_rows.sort_by_key(|(task_id, last_seq)| (Reverse(*last_seq), task_id.clone()));
        for (task_id, _) in terminal_rows.into_iter().skip(5) {
            self.orchestration_tasks.remove(&task_id);
        }
    }

    pub fn orchestration_summary(&self) -> OrchestrationSummary {
        let mut summary = OrchestrationSummary::default();
        let mut active_agents = BTreeSet::new();

        for row in self.orchestration_tasks.values() {
            if row.state.is_terminal() {
                continue;
            }

            if row.owner_kind == ActorKind::Worker {
                if let Some(agent_id) = row.owner_agent_id.as_deref() {
                    active_agents.insert(agent_id);
                }
            }

            match row.state {
                OrchestrationTaskState::Queued => summary.queued += 1,
                OrchestrationTaskState::Running => summary.running += 1,
                OrchestrationTaskState::Stale => summary.stale += 1,
                OrchestrationTaskState::Completed
                | OrchestrationTaskState::Cancelled
                | OrchestrationTaskState::LateResult => {}
            }
        }

        summary.active_agents = active_agents.len();
        summary
    }

    pub fn orchestration_latest_warning(&self) -> Option<&str> {
        self.orchestration_tasks
            .values()
            .filter_map(|row| {
                row.warning
                    .as_ref()
                    .map(|warning| (row.last_seq, warning.as_str()))
            })
            .max_by_key(|(last_seq, _)| *last_seq)
            .map(|(_, warning)| warning)
    }

    pub fn orchestration_visible_rows(&self) -> Vec<OrchestrationTaskRow> {
        let mut rows = self
            .orchestration_tasks
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| {
            (
                row.state.is_terminal(),
                row.state.sort_rank(),
                Reverse(row.last_seq),
                row.task_id.clone(),
            )
        });
        rows
    }

    pub fn orchestration_owner_labels(
        &self,
        row: &OrchestrationTaskRow,
    ) -> OrchestrationOwnerLabels {
        match row.owner_kind {
            ActorKind::Worker => {
                let label = row
                    .owner_agent_id
                    .clone()
                    .unwrap_or_else(|| "worker".to_string());
                let profile = self
                    .agent_profiles
                    .get(label.as_str())
                    .cloned()
                    .unwrap_or_else(|| "n/a".to_string());
                OrchestrationOwnerLabels { label, profile }
            }
            ActorKind::Supervisor => OrchestrationOwnerLabels {
                label: "supervisor".to_string(),
                profile: "n/a".to_string(),
            },
            ActorKind::System | ActorKind::User => OrchestrationOwnerLabels {
                label: "system".to_string(),
                profile: "n/a".to_string(),
            },
        }
    }

    fn update_derived_state_for_event(&mut self, event: &EventEnvelopeV1, historical: bool) {
        match &event.payload {
            EventV1::PermissionRequested(data) => {
                self.pending_permissions.insert(
                    data.permission_id.clone(),
                    PendingPermission {
                        seq: event.seq,
                        kind: data.kind.clone(),
                        summary: data.summary.clone(),
                        request_digest: data.request_digest.clone(),
                        timeout_ms: data.timeout_ms,
                        default_decision: data.default_decision,
                        tool_call_id: data.tool_call_id.clone(),
                    },
                );
                self.attach_permission_request(event);
            }
            EventV1::PermissionResolved(data) => {
                self.pending_permissions.remove(&data.permission_id);
                self.update_permission_resolution(
                    &data.permission_id,
                    data.decision,
                    data.reason.as_deref(),
                    event.seq,
                );
            }
            EventV1::RunFinished(_) => {
                if !historical {
                    self.run_terminal_seen = true;
                }
            }
            EventV1::RunFailed(data) => {
                if !historical {
                    self.run_terminal_seen = true;
                }
                if let Some(entry) = self.activities.back_mut() {
                    entry.status = ActivityStatus::Error;
                    entry.error_message = Some(data.error.clone());
                }
            }
            EventV1::AgentSpawned(data) => {
                self.agent_profiles
                    .insert(data.agent_id.clone(), data.profile.clone());
            }
            EventV1::UserMessageSubmitted(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.user_message = Some(data.clone());
                        entry.user_timestamp = event.ts.clone();
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: Some(data.clone()),
                            user_timestamp: event.ts.clone(),
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                }
            }
            EventV1::ProviderRequestStarted(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.model_id = data.model_id.clone();
                        entry.provider_id = data.provider_id.clone();
                        entry.request_data = Some(data.clone());
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
                            model_id: data.model_id.clone(),
                            provider_id: data.provider_id.clone(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: Some(data.clone()),
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                }
            }
            EventV1::ProviderStreamDelta(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.transcript_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: data.delta.clone(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                }
                self.enforce_transcript_memory_cap();
            }
            EventV1::ProviderReasoningDelta(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.thinking_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                    if let Some(entry) = self.activities.back_mut() {
                        entry.thinking_text = data.delta.clone();
                    }
                }
                self.enforce_transcript_memory_cap();
            }
            EventV1::ProviderRequestFinished(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        if entry.tool_calls.is_empty()
                            && entry.transcript_text.is_empty()
                            && !entry.thinking_text.is_empty()
                        {
                            entry.transcript_text = std::mem::take(&mut entry.thinking_text);
                        }
                        entry.status = ActivityStatus::Done;
                        entry.usage = data.usage.as_ref().map(|usage| ActivityUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            total_tokens: usage.total_tokens,
                        });
                        entry.last_seq = event.seq;
                        entry.last_mono_ms = event.mono_ms;
                    }
                }
            }
            EventV1::TaskCompleted(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Completed;
                    row.warning = None;
                    row.result_summary = Some(data.result_summary.clone());
                    merge_orchestration_task_completion_metadata(row, data.metadata.as_ref());
                });

                if let Some(request_id) = event.correlation_id.as_deref() {
                    if let Some(index) = self.activity_index_or_local_echo(request_id, event.seq) {
                        if let Some(entry) = self.activities.get_mut(index) {
                            entry.status = ActivityStatus::Done;
                            if entry.transcript_text.is_empty()
                                && !data.result_summary.trim().is_empty()
                            {
                                entry.transcript_text = data.result_summary.clone();
                            }
                            entry.last_seq = event.seq;
                        }
                    }
                }
            }
            EventV1::TaskScheduled(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    if let Some(queue_key) = data.queue_key.as_ref() {
                        row.queue_key = Some(queue_key.clone());
                    }
                    row.warning = None;
                    if row.child_request_id.is_none() {
                        row.child_request_id = event.correlation_id.clone();
                    }
                    row.state = match data.state {
                        harness_core::event::TaskScheduleState::Queued => {
                            OrchestrationTaskState::Queued
                        }
                        harness_core::event::TaskScheduleState::Started => {
                            OrchestrationTaskState::Running
                        }
                    };
                });
            }
            EventV1::TaskCancelled(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Cancelled;
                    row.warning = (!data.reason.trim().is_empty()).then(|| data.reason.clone());
                });
            }
            EventV1::TaskResultLate(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::LateResult;
                    row.warning = Some("late result after stale cancellation".to_string());
                });
            }
            EventV1::StaleDetected(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Stale;
                    row.warning = Some(format!("stale for {} ms", data.stale_for_ms));
                });
            }
            EventV1::ToolCallRequested(data) => {
                let target_corr_id = event.correlation_id.clone();
                let use_back = self
                    .activities
                    .back()
                    .is_none_or(|entry| target_corr_id.is_none() || entry.request_id.is_empty());

                let entry = if use_back {
                    self.activities.back_mut()
                } else if let Some(corr) = &target_corr_id {
                    self.activities
                        .iter_mut()
                        .find(|activity| &activity.request_id == corr)
                } else {
                    None
                };

                if let Some(entry) = entry {
                    if entry.tool_calls.is_empty()
                        && entry.thinking_text.is_empty()
                        && !entry.transcript_text.is_empty()
                    {
                        entry.thinking_text = std::mem::take(&mut entry.transcript_text);
                    }
                    let tool_entry = ToolCallEntry {
                        tool_call_id: data.tool_call_id.clone(),
                        tool_id: data.tool_id.clone(),
                        canonical_tool_id: None,
                        alias_source_tool_id: None,
                        resolved_tool_identity: None,
                        args_summary: data.args_summary.clone(),
                        args_digest: data.args_digest.clone(),
                        lifecycle_state: Some(ToolCallLifecycleState::Pending),
                        status: ToolCallDisplayStatus::Queued,
                        output_summary: None,
                        output_digest: None,
                        output_json: None,
                        truncated_output: None,
                        edit: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing_elapsed_ms: None,
                        permissions: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                        first_mono_ms: event.mono_ms,
                        last_mono_ms: event.mono_ms,
                        first_timestamp: event.ts.clone(),
                        last_timestamp: event.ts.clone(),
                    };
                    let mut tool_entry = tool_entry;
                    merge_resolved_tool_identity(
                        &mut tool_entry,
                        ResolvedToolIdentity::from_tool_call(
                            Some(data.tool_id.as_str()),
                            data.metadata.as_ref(),
                        ),
                    );
                    merge_tool_call_metadata(&mut tool_entry, data.metadata.as_ref());
                    tool_entry.sync_display_status();
                    entry.tool_calls.push(tool_entry);
                    entry.last_seq = event.seq;
                }
                self.note_child_task_tool_call(event);
            }
            EventV1::ToolCallStarted(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.lifecycle_state = Some(ToolCallLifecycleState::Running);
                    tool_entry.sync_display_status();
                    tool_entry.last_seq = event.seq;
                    tool_entry.last_mono_ms = event.mono_ms;
                    tool_entry.last_timestamp = event.ts.clone();
                }
            }
            EventV1::ToolCallFinished(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.lifecycle_state =
                        Some(ToolCallLifecycleState::from_finish_status(data.status));
                    tool_entry.output_summary = data.output_summary.clone();
                    tool_entry.output_digest = data.output_digest.clone();
                    tool_entry.output_json = data.output_json.clone();
                    if let Some(summary) = &data.output_summary {
                        let display_text =
                            if summary.chars().count() > TOOL_OUTPUT_DISPLAY_MAX_CHARS {
                                let truncated: String = summary
                                    .chars()
                                    .take(TOOL_OUTPUT_DISPLAY_MAX_CHARS)
                                    .collect();
                                format!("{}…", truncated)
                            } else {
                                summary.clone()
                            };
                        tool_entry.truncated_output = Some(display_text);
                    }
                    merge_resolved_tool_identity(
                        tool_entry,
                        ResolvedToolIdentity::from_tool_call(
                            Some(tool_entry.tool_id.as_str()),
                            data.metadata.as_ref(),
                        ),
                    );
                    merge_tool_call_metadata(tool_entry, data.metadata.as_ref());
                    tool_entry.sync_display_status();
                    tool_entry.last_seq = event.seq;
                    tool_entry.last_mono_ms = event.mono_ms;
                    tool_entry.last_timestamp = event.ts.clone();
                }
            }
            EventV1::EditProposed(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    tool_entry.edit = Some(EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: EditDisplayStatus::Proposed,
                        summary: Some(data.summary.clone()),
                        patch_digest: Some(data.patch_digest.clone()),
                        new_file_digest: None,
                        diff_rel_path: None,
                        diff_digest: None,
                        rejection_reason: None,
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            EventV1::EditApplied(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    let summary = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.summary.clone());
                    let patch_digest = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.patch_digest.clone());
                    tool_entry.edit = Some(EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: EditDisplayStatus::Applied,
                        summary,
                        patch_digest,
                        new_file_digest: Some(data.new_file_digest.clone()),
                        diff_rel_path: data.diff_rel_path.clone(),
                        diff_digest: data.diff_digest.clone(),
                        rejection_reason: None,
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            EventV1::EditRejected(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    let summary = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.summary.clone());
                    let patch_digest = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.patch_digest.clone());
                    tool_entry.edit = Some(EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: EditDisplayStatus::Rejected,
                        summary,
                        patch_digest,
                        new_file_digest: None,
                        diff_rel_path: None,
                        diff_digest: None,
                        rejection_reason: Some(data.reason.clone()),
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            _ => {}
        }
    }

    fn enforce_event_memory_cap(&mut self) -> usize {
        let max_events = self.memory_caps.max_events;
        if self.events.len() > max_events {
            let to_remove = self.events.len() - max_events;
            self.events.drain(0..to_remove);
            self.events_trimmed_count += to_remove;
            to_remove
        } else {
            0
        }
    }

    fn enforce_transcript_memory_cap(&mut self) {
        let max_chars = self.memory_caps.max_transcript_chars;
        let total_chars: usize = self
            .activities
            .iter()
            .map(|activity| activity.thinking_text.len() + activity.transcript_text.len())
            .sum();
        if total_chars > max_chars {
            let excess = total_chars - max_chars;
            let mut trimmed = 0;
            while trimmed < excess && !self.activities.is_empty() {
                if let Some(first) = self.activities.front_mut() {
                    for chunk in [&mut first.thinking_text, &mut first.transcript_text] {
                        if trimmed >= excess {
                            break;
                        }
                        if chunk.len() <= excess - trimmed {
                            trimmed += chunk.len();
                            chunk.clear();
                        } else {
                            let to_trim = excess - trimmed;
                            *chunk = chunk.split_off(to_trim);
                            trimmed = excess;
                        }
                    }
                    if trimmed >= excess {
                        break;
                    }
                }
                if trimmed < excess {
                    self.activities.pop_front();
                }
            }
            self.transcript_trimmed_count += trimmed;
        }
    }
}

impl AppState {
    fn launch_value_is_unknown(value: &str) -> bool {
        let trimmed = value.trim();
        trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown")
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_live(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        let mut state = Self::new();
        state.focus = Focus::Prompt;
        state.live_details_drawer_open = false;
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
        if let Some(launch_metadata) = take_pending_live_launch_metadata() {
            state.set_launch_metadata(launch_metadata);
        }
        if let Some(pending_prompt) = take_pending_live_prompt() {
            state.apply_pending_live_prompt(pending_prompt);
        }
        state
    }

    pub fn new_replay(session_path: PathBuf, events: Vec<EventEnvelopeV1>) -> Self {
        let mut state = Self::new();
        state.replay_mode = true;
        state.session_path = Some(session_path);
        state.replace_events(events);
        state.normalize_focus_for_active_surface();
        state
    }

    pub fn new_startup(
        session_history_entries: Vec<SessionHistoryEntry>,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        let mut state = Self::new();
        state.focus = Focus::List;
        state.startup_mode = true;
        state.on_ui_intent = on_ui_intent;
        if let Some(launch_metadata) = take_pending_live_launch_metadata() {
            state.set_launch_metadata(launch_metadata);
        }
        state.set_session_history_entries(session_history_entries);
        if let Some(pending_prompt) = take_pending_live_prompt() {
            state.replace_prompt_input(pending_prompt.text);
        }
        state
    }

    pub fn apply_keybindings(&mut self, bindings: std::collections::BTreeMap<String, String>) {
        self.keymap.apply_overrides(&bindings);
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    #[cfg(test)]
    pub(crate) fn set_theme_for_test(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_launch_metadata(&mut self, launch_metadata: LaunchMetadata) {
        let refresh_runtime_context = self.startup_mode
            || self.replay_mode
            || self.runtime_context_metadata.is_none()
            || (self.events.is_empty() && self.activities.is_empty());
        self.launch_metadata = launch_metadata.clone();
        if refresh_runtime_context {
            self.runtime_context_metadata = Some(launch_metadata);
        }
    }

    fn current_session_id(&self) -> Option<&str> {
        self.session_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn child_session_ids(&self) -> Vec<String> {
        let mut child_session_ids = BTreeSet::new();

        for activity in &self.activities {
            for tool_call in &activity.tool_calls {
                let child_session_id = tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        json_string_field(
                            tool_call.output_json.as_ref(),
                            &["child_session_id", "session_id"],
                        )
                    });

                if let Some(child_session_id) = child_session_id {
                    child_session_ids.insert(child_session_id);
                }
            }
        }

        child_session_ids.into_iter().collect()
    }

    fn current_parent_session_id(&self) -> Option<String> {
        parent_session_id_from_events(&self.events)
    }

    fn build_launch_metadata_for_option(&self, selected_model: &ModelOption) -> LaunchMetadata {
        let mut launch_metadata = LaunchMetadata::from_model_option(selected_model)
            .with_available_models(self.launch_metadata.available_models().to_vec());
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }
        launch_metadata
    }

    fn apply_selected_model_option(&mut self, selected_model: ModelOption, emit_intent: bool) {
        let launch_metadata = self.build_launch_metadata_for_option(&selected_model);
        self.launch_metadata = launch_metadata.clone();

        if emit_intent {
            set_pending_live_launch_metadata(launch_metadata.clone());
            self.emit_ui_intent(UiIntent::SwitchModel {
                profile: selected_model.profile,
                launch_metadata,
            });
        }
    }

    fn cycle_variant(&mut self) {
        if self.replay_mode {
            return;
        }

        let profile_id = self.launch_metadata.profile().to_string();
        let Some(model_id) = self.launch_metadata.model().map(str::to_owned) else {
            return;
        };
        let provider_id = self.launch_metadata.provider().to_string();
        let mut variants = self
            .launch_metadata
            .available_models()
            .iter()
            .filter(|option| {
                option.profile == profile_id
                    && option.provider == provider_id
                    && option.model == model_id
            })
            .cloned()
            .collect::<Vec<_>>();

        let explicit_variants_exist = variants.iter().any(|option| option.variant().is_some());
        if explicit_variants_exist {
            variants.retain(|option| option.variant().is_some());
        }

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            if current_option.profile == profile_id
                && current_option.provider == provider_id
                && current_option.model == model_id
                && (!explicit_variants_exist || current_option.variant().is_some())
                && !variants.iter().any(|option| option == &current_option)
            {
                variants.push(current_option);
            }
        }

        variants.sort();
        variants.dedup();
        if variants.is_empty() {
            return;
        }

        let selected_model = match variants
            .iter()
            .position(|option| self.is_current_model_option(option))
        {
            Some(_) if variants.len() < 2 => return,
            Some(current_index) => {
                let next_index = (current_index + 1) % variants.len();
                variants[next_index].clone()
            }
            None => variants[0].clone(),
        };
        self.apply_selected_model_option(selected_model, !self.replay_mode);
    }

    fn current_session_snapshot(&self) -> Option<SessionNavigationSnapshot> {
        Some(SessionNavigationSnapshot {
            session_path: self.session_path.clone()?,
            events: self.events.clone(),
            launch_metadata: self.launch_metadata.clone(),
            child_session_ids: self.child_session_ids(),
        })
    }

    fn restore_session_snapshot(&mut self, snapshot: SessionNavigationSnapshot) {
        self.replay_mode = true;
        self.session_path = Some(snapshot.session_path);
        self.replace_events(snapshot.events);
        self.set_launch_metadata(snapshot.launch_metadata);
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        self.focus = Focus::Details;
        self.normalize_focus_for_active_surface();
    }

    fn session_path_for_id(&self, session_id: &str) -> Option<PathBuf> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        self.session_path
            .as_deref()
            .and_then(Path::parent)
            .map(|parent| parent.join(session_id))
    }

    fn live_switch_to_session(&mut self, session_id: String, session_path: PathBuf) {
        let resume_plan = inspect_resume_plan(&session_path);
        set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
        if resume_plan.is_resumable {
            self.emit_ui_intent(UiIntent::ContinueSession {
                run_id: session_id,
                run_dir: session_path,
            });
        } else {
            self.emit_ui_intent(UiIntent::ReplaySession {
                run_id: session_id,
                run_dir: session_path,
            });
        }
    }

    fn open_replay_session(&mut self, session_id: String, push_current: bool) {
        let Some(session_path) = self.session_path_for_id(&session_id) else {
            self.set_status_banner(Some(
                "session navigation unavailable: missing session path".to_string(),
            ));
            return;
        };

        let snapshot =
            match session_navigation_snapshot_from_path(&session_path, &self.launch_metadata) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                    return;
                }
            };

        if push_current {
            if let Some(current_snapshot) = self.current_session_snapshot() {
                let already_pushed = self
                    .session_navigation_stack
                    .last()
                    .map(|existing| existing.session_path.as_path())
                    == Some(current_snapshot.session_path.as_path());
                if !already_pushed {
                    self.session_navigation_stack.push(current_snapshot);
                }
            }
        }

        self.restore_session_snapshot(snapshot);
    }

    fn sibling_child_session_target(&self, reverse: bool) -> Option<String> {
        let current_session_id = self.current_session_id()?;
        let siblings = if let Some(parent_snapshot) = self.session_navigation_stack.last() {
            parent_snapshot.child_session_ids.clone()
        } else {
            let parent_session_id = self.current_parent_session_id()?;
            let parent_session_path = self.session_path_for_id(&parent_session_id)?;
            session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
                .ok()?
                .child_session_ids
        };

        sibling_session_id(&siblings, current_session_id, reverse)
    }

    fn navigate_to_first_child_session(&mut self) {
        let Some(session_id) = self.child_session_ids().into_iter().next() else {
            return;
        };

        if self.replay_mode {
            self.open_replay_session(session_id, true);
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&session_id) {
            self.live_switch_to_session(session_id, session_path);
        }
    }

    fn navigate_to_child_sibling(&mut self, reverse: bool) {
        let target_session_id = self.sibling_child_session_target(reverse).or_else(|| {
            let child_session_ids = self.child_session_ids();
            if reverse {
                child_session_ids.into_iter().last()
            } else {
                child_session_ids.into_iter().next()
            }
        });
        let Some(target_session_id) = target_session_id else {
            return;
        };

        if self.replay_mode {
            self.open_replay_session(
                target_session_id,
                self.current_parent_session_id().is_none(),
            );
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&target_session_id) {
            self.live_switch_to_session(target_session_id, session_path);
        }
    }

    fn navigate_to_parent_session(&mut self) {
        let Some(parent_session_id) = self.current_parent_session_id() else {
            return;
        };

        if self.replay_mode {
            if let Some(parent_snapshot) = self.session_navigation_stack.pop() {
                self.restore_session_snapshot(parent_snapshot);
                return;
            }

            let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) else {
                self.set_status_banner(Some(
                    "session navigation unavailable: missing parent session path".to_string(),
                ));
                return;
            };
            match session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
            {
                Ok(snapshot) => self.restore_session_snapshot(snapshot),
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                }
            }
            return;
        }

        if let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) {
            self.live_switch_to_session(parent_session_id, parent_session_path);
        }
    }

    pub fn replace_events(&mut self, events: Vec<EventEnvelopeV1>) {
        self.projection.reset();
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.expanded_tool_outputs.clear();

        for event in events {
            self.ingest_event(event);
        }

        if self.projection.events.is_empty() {
            self.selected_event_index = 0;
        } else {
            self.selected_event_index = self
                .selected_event_index
                .min(self.projection.events.len() - 1);
        }
        self.details_scroll = 0;
        self.transcript_scroll = 0;
        self.maybe_auto_exit();
    }

    pub fn ingest_historical_event(&mut self, event: EventEnvelopeV1) {
        self.ingest_event_internal(event, true);
    }

    pub fn ingest_event(&mut self, event: EventEnvelopeV1) {
        self.ingest_event_internal(event, false);
    }

    fn ingest_event_internal(&mut self, event: EventEnvelopeV1, historical: bool) {
        if self.projection.has_seen_seq(event.seq) {
            return;
        }

        if matches!(&event.payload, EventV1::PermissionRequested(_)) {
            self.close_palette();
            self.session_history_visible = false;
            self.model_switcher_visible = false;
            self.clear_slash_menu();
        }

        let terminal_event = matches!(
            &event.payload,
            EventV1::RunFinished(_) | EventV1::RunFailed(_)
        );
        if !historical {
            self.continued_live_reopen_surface_active = false;
        }
        let plan_exit_output_json = match &event.payload {
            EventV1::ToolCallFinished(data) if data.status == ToolCallStatus::Succeeded => {
                data.output_json.clone()
            }
            _ => None,
        };
        self.update_transient_state_for_event(&event);
        let trimmed_events = self.projection.ingest_event(event, historical);
        self.maybe_apply_plan_exit_handoff(plan_exit_output_json.as_ref(), historical);

        if trimmed_events > 0 {
            if self.selected_event_index >= trimmed_events {
                self.selected_event_index -= trimmed_events;
            } else {
                self.selected_event_index = 0;
            }
        }

        if self.follow_mode && !self.projection.events.is_empty() {
            self.selected_event_index = self.projection.events.len() - 1;
            self.selected_activity_index = self.projection.activities.len().saturating_sub(1);
            self.details_scroll = 0;
            self.transcript_scroll = 0;
        }

        if terminal_event && !historical {
            self.close_palette();
            self.close_review_surface();
            if self.focus == Focus::Prompt {
                self.focus = Focus::Details;
            }
        }

        self.maybe_auto_exit();
    }

    pub fn set_status_banner(&mut self, status: Option<String>) {
        self.status_banner = status;
    }

    pub fn runtime_state(&self) -> RuntimeState {
        let active_permission = self.active_permission().map(|(permission_id, summary)| {
            view_model::PermissionRuntimeInput {
                submission_pending: self.permission_submission_pending(&permission_id),
                summary,
            }
        });

        let state = view_model::runtime_state(view_model::RuntimeStateInput {
            replay_mode: self.replay_mode,
            lifecycle_shell_state: self.lifecycle_shell_state(),
            continue_disabled_banner: self.continue_disabled_banner.as_deref(),
            status_banner: self.status_banner.as_deref(),
            event_count: self.events.len(),
            last_event: self.events.last().map(|event| &event.payload),
            latest_activity: self.activities.back(),
            activity_count: self.activities.len(),
            active_permission,
        });

        self.enhance_runtime_state(state)
    }

    fn enhance_runtime_state(&self, mut state: RuntimeState) -> RuntimeState {
        match state.kind {
            RuntimeStateKind::PermissionBlocked => {
                if let Some(permission) = self.active_permission_view() {
                    let summary = permission_display_summary(&permission);
                    state.summary = format!("decision required · {summary}");
                    state.detail = Some(summary);
                    state.composer_hint =
                        "Draft preserved under the checkpoint — deny stays fail-closed; allow once only after review."
                            .to_string();
                }
            }
            RuntimeStateKind::PermissionPending => {
                if let Some(permission) = self.active_permission_view() {
                    let summary = permission_display_summary(&permission);
                    state.summary =
                        format!("decision submitted · awaiting confirmation · {}", summary);
                    state.detail = Some(summary);
                    state.composer_hint =
                        "Draft preserved while Harness records the decision. Wait for confirmation before sending again."
                            .to_string();
                }
            }
            RuntimeStateKind::Degraded => {
                state.summary = state
                    .detail
                    .as_deref()
                    .map(|detail| format!("recovery in progress · Sending paused · {detail}"))
                    .unwrap_or_else(|| {
                        "recovery in progress · Sending paused until live state catches up"
                            .to_string()
                    });
                state.composer_hint =
                    "Draft preserved locally while recovery completes.".to_string();
            }
            RuntimeStateKind::Disconnected => {
                state.summary = if self.activities.is_empty() {
                    "connection lost · reopen the TUI to establish the live stream".to_string()
                } else {
                    "connection lost · transcript preserved · reopen required before sending"
                        .to_string()
                };
                state.composer_hint =
                    "Draft preserved locally — reopen the TUI to reconnect.".to_string();
            }
            RuntimeStateKind::Failure => {
                if self.status_banner.as_deref().is_some_and(|banner| {
                    let banner = banner.to_ascii_lowercase();
                    banner.contains("failed")
                        || banner.contains("error")
                        || banner.contains("no session path")
                }) {
                    state.summary =
                        "runtime failure · inspect transcript, then retry or continue".to_string();
                    state.composer_hint =
                        "After review, adjust the draft, then retry or continue.".to_string();
                } else if self
                    .activities
                    .back()
                    .is_some_and(|activity| activity.status == ActivityStatus::Error)
                {
                    state.summary =
                        "turn failed · inspect transcript, then retry or continue".to_string();
                    state.composer_hint =
                        "After review, adjust the draft, then retry or continue.".to_string();
                }
            }
            _ => {}
        }

        state
    }

    pub fn lifecycle_shell_state(&self) -> LifecycleShellState {
        if self.replay_mode {
            LifecycleShellState::None
        } else if self.startup_mode {
            LifecycleShellState::Startup
        } else {
            LifecycleShellState::None
        }
    }

    pub fn lifecycle_shell_actions_visible(&self) -> bool {
        self.lifecycle_shell_state() != LifecycleShellState::None
    }

    pub(crate) fn completed_session_shell_active(&self) -> bool {
        !self.replay_mode && !self.startup_mode && self.projection.run_terminal_seen
    }

    pub fn startup_shell_visible(&self) -> bool {
        matches!(self.lifecycle_shell_state(), LifecycleShellState::Startup)
    }

    pub fn post_run_handoff_visible(&self) -> bool {
        matches!(self.lifecycle_shell_state(), LifecycleShellState::PostRun)
    }

    pub fn post_run_handoff_notice(&self) -> Option<&'static str> {
        view_model::post_run_handoff_notice(self.post_run_can_reopen())
    }

    pub fn post_run_handoff_actions(&self) -> &'static [PostRunHandoffAction] {
        if self.post_run_can_reopen() {
            &PostRunHandoffAction::ORDERED
        } else {
            &PostRunHandoffAction::FALLBACK_ORDERED
        }
    }

    pub fn composer_disabled(&self) -> bool {
        self.replay_mode || self.runtime_state().composer_disabled
    }

    pub(crate) fn startup_card_view_model(&self) -> view_model::StartupCardViewModel {
        view_model::startup_card_view_model(
            self.startup_mode,
            self.launch_mode_label(),
            self.active_profile(),
            self.active_provider(),
            self.current_model_label(),
        )
    }

    pub(crate) fn footer_hints_view_model(&self) -> view_model::FooterHintsViewModel {
        view_model::footer_hints_view_model(view_model::FooterHintsInput {
            replay_mode: self.replay_mode,
            review_surface_active: self.active_review_surface.is_some(),
            startup_shell_visible: self.startup_shell_visible(),
            focus: self.focus,
            composer_disabled: self.composer_disabled(),
            completed_session_shell_active: self.completed_session_shell_active(),
            continued_live_run: self.continued_live_run(),
        })
    }

    pub(crate) fn continued_live_run(&self) -> bool {
        !self.replay_mode
            && self
                .launch_mode_label()
                .is_some_and(|label| label.eq_ignore_ascii_case("continued"))
    }

    fn post_run_can_reopen(&self) -> bool {
        self.post_run_reopen_target().is_some()
    }

    fn post_run_reopen_target(&self) -> Option<(&str, &PathBuf)> {
        let run_id = self.run_id().filter(|run_id| !run_id.trim().is_empty())?;
        let session_path = self.session_path.as_ref()?;
        Some((run_id, session_path))
    }

    fn default_post_run_handoff_action(&self) -> PostRunHandoffAction {
        if self.post_run_can_reopen() {
            PostRunHandoffAction::ContinueSession
        } else {
            PostRunHandoffAction::StartAnotherSession
        }
    }

    pub(crate) fn selected_post_run_handoff_action(&self) -> PostRunHandoffAction {
        let selected = self.post_run_handoff_action;
        if self.post_run_handoff_actions().contains(&selected) {
            selected
        } else {
            self.default_post_run_handoff_action()
        }
    }

    fn reset_post_run_handoff_selection(&mut self) {
        self.post_run_handoff_action = self.default_post_run_handoff_action();
    }

    pub fn prompt_bootstrap_disabled(&self) -> bool {
        self.composer_disabled()
    }

    pub fn selected_event(&self) -> Option<&EventEnvelopeV1> {
        self.projection.events.get(self.selected_event_index)
    }

    pub fn run_id(&self) -> Option<&str> {
        self.projection
            .events
            .first()
            .map(|event| event.run_id.as_str())
    }

    pub fn launch_mode_label(&self) -> Option<&str> {
        self.launch_metadata.mode_label()
    }

    pub fn active_profile(&self) -> &str {
        let profile = self.launch_metadata.profile();
        if Self::launch_value_is_unknown(profile) {
            "default"
        } else {
            profile
        }
    }

    pub fn active_provider(&self) -> &str {
        let provider = self.launch_metadata.provider();
        if !Self::launch_value_is_unknown(provider) && provider != "local" {
            provider
        } else {
            self.activities
                .back()
                .and_then(|activity| {
                    (!activity.provider_id.trim().is_empty())
                        .then_some(activity.provider_id.as_str())
                })
                .filter(|value| !Self::launch_value_is_unknown(value))
                .unwrap_or("local")
        }
    }

    fn current_model_id(&self) -> &str {
        self.launch_metadata
            .model()
            .or_else(|| {
                self.activities.back().and_then(|activity| {
                    (!activity.model_id.trim().is_empty()).then_some(activity.model_id.as_str())
                })
            })
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or("-")
    }

    fn current_model_variant(&self) -> Option<&str> {
        self.launch_metadata.variant()
    }

    pub fn current_model_label(&self) -> &str {
        self.launch_metadata
            .display_label()
            .unwrap_or_else(|| self.current_model_id())
    }

    pub fn runtime_context_primary_summary(&self) -> String {
        self.control_dock_view_model().primary_summary
    }

    pub fn runtime_context_summary_segment_text(&self) -> Option<String> {
        self.control_dock_view_model()
            .summary_segment
            .map(|segment| segment.text)
    }

    pub fn runtime_context_provider_display(&self) -> Option<String> {
        self.control_dock_view_model().runtime_context
    }

    pub(crate) fn runtime_context_identity_line(&self) -> String {
        format!(
            "{} · {}/{}",
            self.runtime_context_profile(),
            self.runtime_context_provider(),
            self.runtime_context_model_id()
        )
    }

    fn runtime_context_metadata(&self) -> &LaunchMetadata {
        self.runtime_context_metadata
            .as_ref()
            .unwrap_or(&self.launch_metadata)
    }

    fn runtime_context_profile(&self) -> &str {
        let profile = self.runtime_context_metadata().profile();
        if Self::launch_value_is_unknown(profile) {
            self.active_profile()
        } else {
            profile
        }
    }

    fn runtime_context_provider(&self) -> &str {
        let provider = self.runtime_context_metadata().provider();
        if Self::launch_value_is_unknown(provider) || provider == "local" {
            self.active_provider()
        } else {
            provider
        }
    }

    fn runtime_context_model_id(&self) -> &str {
        self.runtime_context_metadata()
            .model()
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_id())
    }

    fn runtime_context_model_label(&self) -> String {
        self.runtime_context_metadata()
            .display_label()
            .or_else(|| self.runtime_context_metadata().model())
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_label())
            .to_string()
    }

    fn runtime_context_identity(&self) -> String {
        format!(
            "{} · {}",
            self.runtime_context_profile(),
            self.runtime_context_model_label()
        )
    }

    fn runtime_context_label(&self) -> view_model::RuntimeContextLabel {
        if self.startup_shell_visible() {
            view_model::RuntimeContextLabel::Launch
        } else if self.replay_mode {
            view_model::RuntimeContextLabel::RecordedRuntimeReadOnly
        } else if self.continued_live_run() {
            view_model::RuntimeContextLabel::ContinuedRuntime
        } else {
            view_model::RuntimeContextLabel::CurrentRuntime
        }
    }

    fn runtime_identity_for_metadata(metadata: &LaunchMetadata) -> String {
        let model_label = metadata
            .display_label()
            .or_else(|| metadata.model())
            .unwrap_or("-");
        format!("{} · {model_label}", metadata.profile())
    }

    fn runtime_provider_context(&self) -> Option<String> {
        let provider = self.runtime_context_provider().trim();
        (!provider.is_empty()).then(|| provider.to_string())
    }

    fn next_turn_identity(&self) -> Option<String> {
        if self.startup_shell_visible() || self.replay_mode {
            return None;
        }

        let current = self.runtime_context_metadata();
        let next = &self.launch_metadata;
        let changed = current.profile() != next.profile()
            || current.provider() != next.provider()
            || current.model() != next.model()
            || current.variant() != next.variant();
        changed.then(|| Self::runtime_identity_for_metadata(next))
    }

    pub(crate) fn control_dock_view_model(&self) -> view_model::ControlDockViewModel {
        let runtime_state = self.runtime_state();
        let grammar = view_model::runtime_context_grammar(view_model::RuntimeContextGrammarInput {
            label: self.runtime_context_label(),
            identity: self.runtime_context_identity(),
            next_turn_identity: self.next_turn_identity(),
        });
        let runtime_context = self.runtime_provider_context();

        if self.startup_shell_visible() {
            let composer_body = if self.prompt_buffer.is_empty() {
                runtime_state.composer_hint.clone()
            } else {
                self.prompt_buffer.clone()
            };
            return view_model::control_dock_view_model(view_model::ControlDockInput::Startup {
                runtime_context,
                runtime_state,
                primary_summary: grammar.primary_summary,
                composer_body,
                composer_disclosure: String::new(),
                composer_focused: self.focus == Focus::Prompt,
            });
        }

        if self.replay_mode {
            return view_model::control_dock_view_model(
                view_model::ControlDockInput::ReplayReadOnly {
                    runtime_context,
                    runtime_state,
                    primary_summary: grammar.primary_summary,
                    composer_body: "Replay is read-only.".to_string(),
                    composer_disclosure: String::new(),
                    composer_focused: self.focus == Focus::Prompt,
                },
            );
        }

        let composer_body = if self.prompt_buffer.is_empty() {
            String::new()
        } else {
            self.prompt_buffer.clone()
        };
        view_model::control_dock_view_model(view_model::ControlDockInput::Live {
            runtime_context,
            runtime_state,
            primary_summary: grammar.primary_summary,
            summary_segment: grammar.summary_segment,
            composer_body,
            composer_disclosure: String::new(),
            composer_focused: self.focus == Focus::Prompt,
        })
    }

    pub fn operator_sidebar_state_label(&self) -> String {
        if self.replay_mode {
            "Replay".to_string()
        } else {
            self.launch_mode_label().unwrap_or("Live").to_string()
        }
    }

    pub fn operator_sidebar_run_identity(&self) -> String {
        format!("run {}", self.run_id().unwrap_or("pending"))
    }

    pub fn operator_sidebar_pending_permission_lines(&self) -> Vec<String> {
        self.transcript_pending_permissions()
            .into_iter()
            .map(|(_, summary)| summary)
            .collect()
    }

    pub fn operator_sidebar_todo_lines(&self) -> Vec<String> {
        self.orchestration_visible_rows()
            .into_iter()
            .filter(|row| !row.state.is_terminal())
            .map(|row| {
                let owner = self.orchestration_owner_labels(&row);
                let queue = row.queue_key.unwrap_or_else(|| "queue:none".to_string());
                format!(
                    "{} · {} · {}/{}",
                    row.task_id, queue, owner.label, owner.profile
                )
            })
            .collect()
    }

    fn operator_sidebar_artifact_paths(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut artifact_paths = Vec::new();

        for event in self.events.iter().rev() {
            if let EventV1::EditApplied(edit) = &event.payload {
                if let Some(diff_rel_path) = edit
                    .diff_rel_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                {
                    if seen.insert(diff_rel_path.clone()) {
                        artifact_paths.push(diff_rel_path);
                    }
                }
            }
        }

        for activity in self.activities.iter().rev() {
            for tool_call in activity.tool_calls.iter().rev() {
                for artifact_ref in tool_call.artifact_refs.iter().rev() {
                    let path = artifact_ref.path.trim();
                    if !path.is_empty() && seen.insert(path.to_string()) {
                        artifact_paths.push(path.to_string());
                    }
                }
            }
        }

        artifact_paths
    }

    pub fn operator_sidebar_recovery_lines(&self) -> Vec<String> {
        let artifact_paths = self.operator_sidebar_artifact_paths();
        let child_session_ids = self.child_session_ids();
        let parent_session_id = self.current_parent_session_id();
        let has_bundle = self.session_path.is_some() && !self.events.is_empty();
        let show_section = has_bundle
            || !artifact_paths.is_empty()
            || !child_session_ids.is_empty()
            || parent_session_id.is_some();
        let mut lines = Vec::new();

        if !show_section {
            return lines;
        }

        if self.replay_mode {
            lines.push(
                "Replay is read-only — inspect recorded context and use replay navigation."
                    .to_string(),
            );
        }
        if let Some(parent_session_id) = parent_session_id {
            lines.push(format!("Parent session · {parent_session_id}"));
        }
        for child_session_id in child_session_ids {
            lines.push(format!("Child session · {child_session_id}"));
        }
        if has_bundle {
            lines.push("Bundle keeps events.jsonl and artifacts/".to_string());
        }
        for path in artifact_paths {
            lines.push(format!("Artifact · {path}"));
        }

        lines
    }

    pub fn operator_sidebar_modified_files(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut files = Vec::new();

        for event in self.events.iter().rev() {
            if let EventV1::EditApplied(edit) = &event.payload {
                if seen.insert(edit.path.clone()) {
                    let diff_rel_path = edit
                        .diff_rel_path
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    files.push(match diff_rel_path {
                        Some(diff_rel_path) => format!("{} · diff {diff_rel_path}", edit.path),
                        None => edit.path.clone(),
                    });
                }
            }
        }

        files
    }

    pub(crate) fn is_current_model_option(&self, option: &ModelOption) -> bool {
        option.profile == self.active_profile()
            && option.provider == self.active_provider()
            && option.model == self.current_model_id()
            && option.variant() == self.current_model_variant()
    }

    fn active_slash_query(&self) -> Option<&str> {
        let query = self.prompt_buffer.strip_prefix('/')?;
        (!query.chars().any(char::is_whitespace)).then_some(query)
    }

    fn clear_slash_menu(&mut self) {
        self.slash_visible = false;
        self.slash_filtered.clear();
        self.slash_selected = 0;
    }

    fn slash_overlay_should_render(&self) -> bool {
        false
    }

    fn sync_slash_overlay(&mut self) {
        if self.focus != Focus::Prompt
            || self.composer_disabled()
            || self.active_slash_query().is_none()
            || self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.active_permission().is_some()
        {
            if !self.prompt_buffer.starts_with('/') {
                self.slash_draft_snapshot = None;
            }
            self.clear_slash_menu();
            return;
        }

        let slash_query = self.active_slash_query().unwrap_or_default().to_lowercase();

        self.slash_visible = true;
        self.slash_filtered = SLASH_COMMANDS
            .iter()
            .filter(|(command, _)| self.slash_command_available(command))
            .filter(|(command, description)| {
                slash_query.is_empty()
                    || command.starts_with(&slash_query)
                    || description.to_lowercase().contains(&slash_query)
            })
            .map(|(command, _)| (*command).to_string())
            .collect();
        self.slash_selected = self
            .slash_selected
            .min(self.slash_filtered.len().saturating_sub(1));
    }

    fn typed_slash_command(&self) -> Option<&'static str> {
        self.prompt_buffer
            .trim()
            .strip_prefix('/')
            .and_then(|command| {
                SLASH_COMMANDS.iter().find_map(|(name, _)| {
                    (*name == command && self.slash_command_available(name)).then_some(*name)
                })
            })
    }

    fn slash_command_available(&self, command: &str) -> bool {
        match command {
            "new" | "exit" => true,
            "resume" | "replay" | "model" => !self.replay_mode,
            "events" => !self.startup_mode,
            "shell" => self.active_review_surface.is_some(),
            "follow" => !self.replay_mode && !self.startup_mode,
            _ => false,
        }
    }

    fn restore_slash_draft(&mut self, preserved_draft: Option<String>) {
        self.replace_prompt_input(preserved_draft.unwrap_or_default());
    }

    fn navigate_to_home_shell(&mut self, draft: String) {
        self.projection.reset();
        self.selected_event_index = 0;
        self.selected_activity_index = 0;
        self.follow_mode = true;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.startup_mode = true;
        self.startup_launcher_action = StartupLauncherAction::NewSession;
        self.status_banner = None;
        self.details_scroll = 0;
        self.transcript_scroll = 0;
        self.prompt_history.clear();
        self.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.palette_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.palette_selected = 0;
        self.palette_focus_return = None;
        self.session_history_visible = false;
        self.session_history_selected = 0;
        self.model_switcher_visible = false;
        self.model_filtered.clear();
        self.model_selected = 0;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.continue_disabled_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.reload_requested = false;
        self.should_quit = false;
        self.focus = Focus::Prompt;
        self.replace_prompt_input(draft);
    }

    fn execute_slash_command(&mut self, command: &str, preserved_draft: Option<String>) {
        self.clear_slash_menu();
        match command {
            "new" => self.navigate_to_home_shell(preserved_draft.unwrap_or_default()),
            "resume" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            "replay" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            "model" => {
                self.restore_slash_draft(preserved_draft);
                self.open_model_switcher();
            }
            "events" => {
                self.restore_slash_draft(preserved_draft);
                self.open_review_surface(ReviewSurface::Events);
            }
            "shell" => {
                self.restore_slash_draft(preserved_draft);
                self.close_review_surface();
            }
            "follow" => {
                self.restore_slash_draft(preserved_draft);
                self.execute_action(Action::ToggleFollow);
            }
            "exit" => self.execute_action(Action::Quit),
            _ => {}
        }
    }

    fn apply_selected_slash_completion(&mut self) {
        let Some(command) = self.slash_filtered.get(self.slash_selected).cloned() else {
            return;
        };
        self.execute_slash_command(&command, self.slash_draft_snapshot.clone());
    }

    fn rebuild_model_options(&mut self) {
        self.model_options = self.collect_model_options().into_iter().collect();
    }

    fn collect_model_options(&self) -> BTreeSet<ModelOption> {
        let mut options = BTreeSet::new();

        options.extend(self.launch_metadata.available_models().iter().cloned());

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            options.insert(current_option);
        }

        if options.is_empty() {
            for activity in &self.activities {
                if !activity.provider_id.trim().is_empty() && !activity.model_id.trim().is_empty() {
                    options.insert(ModelOption {
                        profile: self.launch_metadata.profile().to_string(),
                        provider: activity.provider_id.clone(),
                        model: activity.model_id.clone(),
                        variant: None,
                        display_label: None,
                        token_window_label: None,
                        context_window_tokens: None,
                        max_input_tokens: None,
                        max_output_tokens: None,
                        description: None,
                        reasoning_effort: None,
                        text_verbosity: None,
                        recommended_for: None,
                    });
                }
            }

            for entry in &self.session_history_entries {
                let Some(provider_model) = entry.catalog.provider_model.as_deref() else {
                    continue;
                };
                let Some((provider, model)) = provider_model.split_once('/') else {
                    continue;
                };
                options.insert(ModelOption {
                    profile: session_history_profile_label(entry).to_string(),
                    provider: provider.to_string(),
                    model: model.to_string(),
                    variant: None,
                    display_label: None,
                    token_window_label: None,
                    context_window_tokens: None,
                    max_input_tokens: None,
                    max_output_tokens: None,
                    description: None,
                    reasoning_effort: None,
                    text_verbosity: None,
                    recommended_for: None,
                });
            }
        }

        options
    }

    fn update_model_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let mut filtered = self
            .model_options
            .iter()
            .enumerate()
            .filter(|(_, option)| option.matches(&input))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            let left_option = &self.model_options[*left];
            let right_option = &self.model_options[*right];
            self.is_current_model_option(left_option)
                .cmp(&self.is_current_model_option(right_option))
                .reverse()
                .then_with(|| left_option.profile.cmp(&right_option.profile))
                .then_with(|| left_option.provider.cmp(&right_option.provider))
                .then_with(|| left_option.model.cmp(&right_option.model))
        });
        self.model_filtered = filtered;
        self.model_selected = 0;
    }

    fn open_model_switcher(&mut self) {
        if !self.model_switcher_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = true;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.rebuild_model_options();
        self.update_model_filter();
        self.sync_slash_overlay();
    }

    fn execute_selected_model(&mut self) {
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            self.close_palette();
            return;
        };

        if self.replay_mode {
            self.close_palette();
            return;
        }

        let Some(selected_model) = self.model_options.get(selected_index).cloned() else {
            self.close_palette();
            return;
        };

        self.apply_selected_model_option(selected_model, true);
        self.close_palette();
    }

    fn maybe_apply_plan_exit_handoff(
        &mut self,
        output_json: Option<&serde_json::Value>,
        historical: bool,
    ) {
        if historical || self.replay_mode || self.on_ui_intent.is_none() {
            return;
        }
        let Some(output_json) = output_json else {
            return;
        };
        let Ok(envelope) = serde_json::from_value::<PlanExitHandoffEnvelope>(output_json.clone())
        else {
            return;
        };
        if envelope.plan_exit_handoff.source_profile != self.active_profile() {
            return;
        }

        let handoff = envelope.plan_exit_handoff;
        let mut available_models = self.launch_metadata.available_models().to_vec();
        let mut launch_metadata = self
            .launch_metadata
            .available_models()
            .iter()
            .find(|option| option.profile == handoff.target_profile)
            .map(LaunchMetadata::from_model_option)
            .unwrap_or_else(|| {
                LaunchMetadata::new(
                    handoff.target_profile.clone(),
                    self.launch_metadata.provider().to_string(),
                    self.launch_metadata.model().map(str::to_owned),
                )
            })
            .with_available_models({
                if !available_models
                    .iter()
                    .any(|option| option.profile == handoff.target_profile)
                {
                    available_models.push(ModelOption {
                        profile: handoff.target_profile.clone(),
                        provider: self.launch_metadata.provider().to_string(),
                        model: self
                            .launch_metadata
                            .model()
                            .map(str::to_string)
                            .unwrap_or_default(),
                        variant: self.launch_metadata.variant().map(str::to_string),
                        display_label: self.launch_metadata.display_label().map(str::to_string),
                        token_window_label: self
                            .launch_metadata
                            .token_window_label()
                            .map(str::to_string),
                        context_window_tokens: self.launch_metadata.context_window_tokens(),
                        max_input_tokens: self.launch_metadata.max_input_tokens(),
                        max_output_tokens: self.launch_metadata.max_output_tokens(),
                        description: self.launch_metadata.description().map(str::to_string),
                        reasoning_effort: self
                            .launch_metadata
                            .reasoning_effort()
                            .map(str::to_string),
                        text_verbosity: self.launch_metadata.text_verbosity().map(str::to_string),
                        recommended_for: self.launch_metadata.recommended_for().map(str::to_string),
                    });
                }
                available_models
            });
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }

        self.launch_metadata = launch_metadata.clone();
        set_pending_live_launch_metadata(launch_metadata.clone());
        self.emit_ui_intent(UiIntent::SwitchModel {
            profile: handoff.target_profile,
            launch_metadata,
        });
        self.emit_ui_intent(UiIntent::SubmitPrompt {
            text: handoff.prompt,
            launch_metadata: self.launch_metadata.clone(),
        });
    }

    pub fn active_permission(&self) -> Option<(String, String)> {
        self.projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| (permission_id.clone(), pending.summary.clone()))
    }

    pub(crate) fn transcript_thinking_visible(&self) -> bool {
        self.show_transcript_thinking
    }

    pub(crate) fn transcript_timestamps_visible(&self) -> bool {
        self.show_transcript_timestamps
    }

    pub(crate) fn transcript_animation_phase(&self) -> usize {
        self.transcript_animation_phase
    }

    pub(crate) fn advance_transcript_animation_phase(&mut self) {
        self.transcript_animation_phase = self.transcript_animation_phase.wrapping_add(1);
    }

    pub(crate) fn tool_details_visible(&self) -> bool {
        self.show_tool_details
    }

    pub(crate) fn generic_tool_output_visible(&self) -> bool {
        self.show_generic_tool_output
    }

    pub(crate) fn stacked_transcript_diffs(&self) -> bool {
        self.stacked_transcript_diffs
    }

    pub(crate) fn tool_output_expanded(&self, tool_call: &ToolCallEntry) -> bool {
        self.show_generic_tool_output
            || self.expanded_tool_outputs.contains(&tool_call.tool_call_id)
    }

    fn selected_activity_expandable_tool_ids(&self) -> Vec<String> {
        self.activities
            .get(self.selected_activity_index)
            .into_iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| tool_call_has_expandable_output(tool_call))
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect()
    }

    fn set_selected_activity_expandable_outputs(&mut self, expanded: bool) {
        for tool_call_id in self.selected_activity_expandable_tool_ids() {
            if expanded {
                self.expanded_tool_outputs.insert(tool_call_id);
            } else {
                self.expanded_tool_outputs.remove(&tool_call_id);
            }
        }
    }

    pub fn active_permission_view(&self) -> Option<ActivePermissionView> {
        self.projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| ActivePermissionView {
                permission_id: permission_id.clone(),
                kind: pending.kind.clone(),
                summary: pending.summary.clone(),
                request_digest: pending.request_digest.clone(),
                timeout_ms: pending.timeout_ms,
                default_decision: pending.default_decision,
                tool_call_id: pending.tool_call_id.clone(),
                tool_label: pending
                    .tool_call_id
                    .as_deref()
                    .and_then(|tool_call_id| self.tool_label_for_call(tool_call_id)),
                question_prompts: parse_question_prompts(&pending.kind, &pending.summary),
            })
    }

    fn tool_label_for_call(&self, tool_call_id: &str) -> Option<String> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .find(|tool_call| tool_call.tool_call_id == tool_call_id)
            .map(|tool_call| tool_call.tool_id.clone())
    }

    pub fn orchestration_summary(&self) -> OrchestrationSummary {
        self.projection.orchestration_summary()
    }

    pub fn orchestration_latest_warning(&self) -> Option<&str> {
        self.projection.orchestration_latest_warning()
    }

    pub fn orchestration_visible_rows(&self) -> Vec<OrchestrationTaskRow> {
        self.projection.orchestration_visible_rows()
    }

    pub fn orchestration_owner_labels(
        &self,
        row: &OrchestrationTaskRow,
    ) -> OrchestrationOwnerLabels {
        self.projection.orchestration_owner_labels(row)
    }

    pub(crate) fn transcript_task_row_for_tool_call(
        &self,
        tool_call: &ToolCallEntry,
    ) -> Option<OrchestrationTaskRow> {
        self.projection.transcript_task_row_for_tool_call(tool_call)
    }

    pub fn transcript_pending_permissions(&self) -> Vec<(String, String)> {
        let mut pending = self
            .projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .map(|(permission_id, permission)| {
                let summary = if permission.kind.eq_ignore_ascii_case("question") {
                    "Question requested".to_string()
                } else {
                    permission.summary.clone()
                };
                (permission.seq, permission_id.clone(), summary)
            })
            .collect::<Vec<_>>();
        pending.sort_by_key(|(seq, _, _)| *seq);
        pending
            .into_iter()
            .map(|(_, permission_id, summary)| (permission_id, summary))
            .collect()
    }

    pub fn default_shell_registry(&self) -> &'static [ShellDescriptor] {
        default_shell_registry(self.replay_mode)
    }

    pub fn details_drawer_open(&self) -> bool {
        !self.replay_mode && self.active_tab == Tab::Run && self.live_details_drawer_open
    }

    pub(crate) fn operator_rail_has_sections(&self) -> bool {
        if self.startup_shell_visible() {
            return false;
        }

        let has_session_title = self.activities.iter().any(|activity| {
            activity
                .user_message
                .as_ref()
                .map(|message| message.text.trim())
                .is_some_and(|text| !text.is_empty())
        });
        let has_usage = self
            .activities
            .iter()
            .any(|activity| activity.usage.is_some());
        let has_modified_files = !self.operator_sidebar_modified_files().is_empty();
        let has_integrations = harness_core::config::registered_integrations_config().is_some();
        let lsp = harness_core::config::registered_lsp_config();
        let has_lsp = lsp.disabled || !lsp.servers.is_empty();

        has_session_title || has_usage || has_modified_files || has_integrations || has_lsp
    }

    fn session_shell_operator_rail_interactive(&self) -> bool {
        self.details_drawer_open() || (!self.replay_mode && self.operator_rail_has_sections())
    }

    pub fn review_surface(&self) -> Option<ReviewSurface> {
        self.active_review_surface
    }

    pub fn overlay_stack(&self) -> OverlayStack {
        OverlayStack::from_state(OverlayState {
            details_drawer_open: self.details_drawer_open(),
            palette_visible: self.palette_visible,
            session_history_visible: self.session_history_visible,
            permission_pending: self.active_permission().is_some(),
        })
    }

    pub fn set_session_history_entries(&mut self, entries: Vec<SessionHistoryEntry>) {
        self.session_history_entries = entries;
        self.update_session_history_filter();
        self.rebuild_model_options();
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
    }

    pub fn selected_session_history_entry(&self) -> Option<&SessionHistoryEntry> {
        self.session_history_filtered
            .get(self.session_history_selected)
            .and_then(|index| self.session_history_entries.get(*index))
    }

    pub fn permission_submission_pending(&self, permission_id: &str) -> bool {
        self.submitted_permission_id.as_deref() == Some(permission_id)
    }

    #[cfg(test)]
    pub(crate) fn exact_test_overlay_stack_orders_permission_above_commands_and_slash() {
        fn permission_event(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
            EventEnvelopeV1 {
                schema_version: harness_core::event::SCHEMA_VERSION,
                event_id: format!("evt_permission_overlay_{seq:04}"),
                seq,
                run_id: "run_overlay_stack_exact".to_string(),
                mono_ms: seq,
                ts: Some("2026-02-03T12:00:00Z".to_string()),
                actor: harness_core::event::EventActor::new(
                    ActorKind::System,
                    Some("overlay-stack-exact".to_string()),
                ),
                correlation_id: Some(permission_id.to_string()),
                causation_id: None,
                stream_key: None,
                payload: EventV1::PermissionRequested(
                    harness_core::event::PermissionRequestedEvent {
                        permission_id: permission_id.to_string(),
                        kind: "edit_fs".to_string(),
                        tool_call_id: Some(tool_call_id.to_string()),
                        summary: "permission summary".to_string(),
                        request_digest: format!("digest-{permission_id}"),
                        timeout_ms: 30_000,
                        default_decision: harness_core::event::PermissionDecision::Deny,
                    },
                ),
            }
        }

        let mut palette_app = AppState::new_live(None, false, None);
        palette_app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(
            palette_app.overlay_stack().top(),
            Some(OverlayKind::CommandPalette)
        );

        palette_app.ingest_event(permission_event(
            1,
            "perm_overlay_priority_palette",
            "tc_overlay_priority_palette",
        ));

        assert_eq!(
            palette_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
        assert!(!palette_app.palette_visible);

        let mut slash_app = AppState::new_live(None, false, None);
        slash_app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(slash_app.slash_visible);
        assert_eq!(slash_app.overlay_stack().top(), None);

        slash_app.ingest_event(permission_event(
            1,
            "perm_overlay_priority_slash",
            "tc_overlay_priority_slash",
        ));

        assert!(!slash_app.slash_visible);
        assert_eq!(
            slash_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );

        slash_app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(slash_app.prompt_buffer, "/");
        assert!(!slash_app.slash_visible);
        assert_eq!(
            slash_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
    }

    pub fn take_reload_requested(&mut self) -> bool {
        let requested = self.reload_requested;
        self.reload_requested = false;
        requested
    }

    pub fn emit_ui_intent(&mut self, intent: UiIntent) {
        if let Some(handler) = &self.on_ui_intent {
            handler(intent);
        }
    }

    fn prompt_char_count(&self) -> usize {
        self.prompt_buffer.chars().count()
    }

    fn prompt_cursor_byte_index(&self) -> usize {
        self.prompt_buffer
            .char_indices()
            .nth(self.prompt_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.prompt_buffer.len())
    }

    fn prompt_cursor_at_start(&self) -> bool {
        self.prompt_cursor == 0
    }

    fn prompt_cursor_at_end(&self) -> bool {
        self.prompt_cursor >= self.prompt_char_count()
    }

    fn prompt_line_starts_and_lengths(&self) -> (Vec<usize>, Vec<usize>) {
        let mut starts = Vec::new();
        let mut lengths = Vec::new();
        let mut start = 0;

        for line in self.prompt_buffer.split('\n') {
            starts.push(start);
            let line_len = line.chars().count();
            lengths.push(line_len);
            start += line_len + 1;
        }

        if starts.is_empty() {
            starts.push(0);
            lengths.push(0);
        }

        (starts, lengths)
    }

    fn prompt_cursor_line_column(&self, starts: &[usize], lengths: &[usize]) -> (usize, usize) {
        let line = starts
            .iter()
            .enumerate()
            .rfind(|(_, start)| self.prompt_cursor >= **start)
            .map(|(line, _)| line)
            .unwrap_or(0)
            .min(lengths.len().saturating_sub(1));
        let column = self
            .prompt_cursor
            .saturating_sub(starts[line])
            .min(lengths[line]);
        (line, column)
    }

    fn move_prompt_cursor_up(&mut self) -> bool {
        let (starts, lengths) = self.prompt_line_starts_and_lengths();
        let (line, column) = self.prompt_cursor_line_column(&starts, &lengths);
        if line == 0 {
            return false;
        }

        self.prompt_cursor = starts[line - 1] + column.min(lengths[line - 1]);
        true
    }

    fn move_prompt_cursor_down(&mut self) -> bool {
        let (starts, lengths) = self.prompt_line_starts_and_lengths();
        let (line, column) = self.prompt_cursor_line_column(&starts, &lengths);
        if line + 1 >= lengths.len() {
            return false;
        }

        self.prompt_cursor = starts[line + 1] + column.min(lengths[line + 1]);
        true
    }

    fn select_previous_prompt_history(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }

        let next_idx = match self.prompt_history_index {
            Some(idx) => idx.saturating_sub(1),
            None => self.prompt_history.len().saturating_sub(1),
        };
        self.prompt_history_index = Some(next_idx);
        self.replace_prompt_input(self.prompt_history[next_idx].clone());
    }

    fn select_next_prompt_history(&mut self) {
        let Some(idx) = self.prompt_history_index else {
            return;
        };

        if idx + 1 < self.prompt_history.len() {
            let next_idx = idx + 1;
            self.prompt_history_index = Some(next_idx);
            self.replace_prompt_input(self.prompt_history[next_idx].clone());
            return;
        }

        self.clear_prompt_input();
    }

    fn clear_prompt_input(&mut self) {
        self.prompt_buffer.clear();
        self.prompt_cursor = 0;
        self.prompt_history_index = None;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
    }

    fn replace_prompt_input(&mut self, prompt: String) {
        self.prompt_cursor = prompt.chars().count();
        self.prompt_buffer = prompt;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
    }

    fn apply_pending_live_prompt(&mut self, pending_prompt: PendingLivePrompt) {
        if pending_prompt.auto_submit {
            self.dispatch_submitted_prompt(pending_prompt.text);
        } else {
            self.replace_prompt_input(pending_prompt.text);
        }
    }

    fn insert_prompt_char(&mut self, c: char) {
        self.continued_live_reopen_surface_active = false;
        if c == '/' && self.prompt_cursor == 0 && !self.prompt_buffer.starts_with('/') {
            self.slash_draft_snapshot = Some(self.prompt_buffer.clone());
        }
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.insert(byte_idx, c);
        self.prompt_cursor += 1;
        self.sync_slash_overlay();
    }

    fn backspace_prompt_char(&mut self) {
        if self.prompt_cursor == 0 {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        self.prompt_cursor -= 1;
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
    }

    fn delete_prompt_char(&mut self) {
        if self.prompt_cursor >= self.prompt_char_count() {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
    }

    fn active_turn_in_progress(&self) -> bool {
        self.activities
            .back()
            .is_some_and(|activity| activity.status == ActivityStatus::Streaming)
    }

    fn echo_submitted_prompt(&mut self, text: String) {
        self.activities.push_back(ActivityEntry {
            request_id: String::new(),
            model_id: String::new(),
            provider_id: String::new(),
            status: ActivityStatus::Streaming,
            user_message: Some(UserMessageSubmittedEvent {
                request_id: String::new(),
                text,
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 0,
            last_seq: 0,
            first_mono_ms: 0,
            last_mono_ms: 0,
        });
        self.selected_activity_index = self.activities.len().saturating_sub(1);
        self.details_scroll = 0;
        self.transcript_scroll = 0;
    }

    fn dispatch_submitted_prompt(&mut self, text: String) {
        self.prompt_history.push(text.clone());
        self.clear_prompt_input();
        self.echo_submitted_prompt(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt {
            text,
            launch_metadata: self.launch_metadata.clone(),
        });
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, hovered_wheel_target: Option<WheelTarget>) {
        if self.overlay_stack().blocks_pointer_interaction() {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => self.scroll_transcript_up(3),
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self.details_scroll.saturating_sub(3);
                }
                None => {}
            },
            MouseEventKind::ScrollDown => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => self.scroll_transcript_down(3),
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self.details_scroll.saturating_add(3);
                }
                None => {}
            },
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.overlay_stack().top() == Some(OverlayKind::PermissionModal) {
            self.handle_permission_modal_key(key);
            return;
        }

        if self.session_history_visible && self.handle_session_history_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.model_switcher_visible && self.handle_model_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.palette_visible && self.handle_palette_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.slash_overlay_should_render() && self.handle_slash_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.active_review_surface.is_some() && key.code == KeyCode::Esc {
            self.close_review_surface();
            self.maybe_auto_exit();
            return;
        }

        let mapped_action = self.keymap.get_action(&key);

        if self.startup_shell_visible()
            && self.focus != Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(action_preempts_text_input) {
                self.execute_action(mapped_action.expect("preempting action"));
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                self.focus = Focus::Prompt;
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        if self.focus == Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(action_preempts_text_input) {
                self.execute_action(mapped_action.expect("preempting action"));
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        let Some(action) = mapped_action else {
            return;
        };

        self.execute_action(action);
        self.maybe_auto_exit();
    }

    fn handle_permission_modal_key(&mut self, key: KeyEvent) {
        if self
            .active_permission_view()
            .as_ref()
            .and_then(|permission| permission.question_prompts.as_ref())
            .is_some()
        {
            self.handle_question_permission_modal_key(key);
            return;
        }

        if !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char('/') => return,
                KeyCode::Char('q') => {}
                KeyCode::Char(c) => {
                    self.insert_prompt_char(c);
                    self.maybe_auto_exit();
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.keymap.get_action(&key) {
            if matches!(
                action,
                Action::AllowPermission
                    | Action::DenyPermission
                    | Action::DismissModal
                    | Action::Quit
            ) {
                self.execute_action(action);
                self.maybe_auto_exit();
            }
        }
    }

    fn handle_question_permission_modal_key(&mut self, key: KeyEvent) {
        let Some(permission) = self.active_permission_view() else {
            return;
        };
        self.ensure_question_answer_state(&permission.permission_id);

        if !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char('q') => {}
                KeyCode::Char(c) => {
                    self.insert_question_answer_char(c);
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Enter => {
                    self.insert_question_answer_char('\n');
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Backspace => {
                    self.backspace_question_answer_char();
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Delete => {
                    self.delete_question_answer_char();
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Left => {
                    self.question_answer_cursor = self.question_answer_cursor.saturating_sub(1);
                    return;
                }
                KeyCode::Right => {
                    self.question_answer_cursor = self
                        .question_answer_cursor
                        .saturating_add(1)
                        .min(self.question_answer_char_count());
                    return;
                }
                KeyCode::Home => {
                    self.question_answer_cursor = 0;
                    return;
                }
                KeyCode::End => {
                    self.question_answer_cursor = self.question_answer_char_count();
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.keymap.get_action(&key) {
            if matches!(
                action,
                Action::AllowPermission
                    | Action::DenyPermission
                    | Action::DismissModal
                    | Action::Quit
            ) {
                self.execute_action(action);
                self.maybe_auto_exit();
            }
        }
    }

    fn handle_palette_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_palette_command();
                true
            }
            KeyCode::Up => {
                if self.palette_selected > 0 {
                    self.palette_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if !self.palette_filtered.is_empty()
                    && self.palette_selected < self.palette_filtered.len() - 1
                {
                    self.palette_selected += 1;
                }
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_palette_filter);
                true
            }
            KeyCode::Char(c) => {
                self.overlay_insert_char(c, Self::update_palette_filter);
                true
            }
            _ => false,
        }
    }

    fn handle_model_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_model();
                true
            }
            KeyCode::Up => {
                if self.model_selected > 0 {
                    self.model_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if self.model_selected + 1 < self.model_filtered.len() {
                    self.model_selected += 1;
                }
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_model_filter);
                true
            }
            KeyCode::Char(c) => {
                self.overlay_insert_char(c, Self::update_model_filter);
                true
            }
            _ => false,
        }
    }

    fn handle_slash_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.clear_slash_menu();
                true
            }
            KeyCode::Enter | KeyCode::Tab => {
                self.apply_selected_slash_completion();
                true
            }
            KeyCode::Up => {
                if self.slash_selected > 0 {
                    self.slash_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if self.slash_selected + 1 < self.slash_filtered.len() {
                    self.slash_selected += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn handle_session_history_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.close_session_history();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_session_launcher_action();
                true
            }
            KeyCode::Up => {
                if self.session_history_selected > 0 {
                    self.session_history_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if self.session_history_selected + 1 < self.session_history_filtered.len() {
                    self.session_history_selected += 1;
                }
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_session_history_filter);
                true
            }
            KeyCode::Char(c) => {
                self.overlay_insert_char(c, Self::update_session_history_filter);
                true
            }
            _ => false,
        }
    }

    fn overlay_backspace(&mut self, on_change: fn(&mut Self)) {
        if self.palette_cursor == 0 {
            return;
        }

        self.palette_cursor -= 1;
        let byte_idx = self
            .palette_input
            .char_indices()
            .nth(self.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.palette_input.len());
        self.palette_input.remove(byte_idx);
        on_change(self);
    }

    fn overlay_insert_char(&mut self, c: char, on_change: fn(&mut Self)) {
        let byte_idx = self
            .palette_input
            .char_indices()
            .nth(self.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.palette_input.len());
        self.palette_input.insert(byte_idx, c);
        self.palette_cursor += 1;
        on_change(self);
    }

    fn update_palette_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let filtered = self
            .palette_commands()
            .iter()
            .enumerate()
            .filter_map(|palette_command| {
                let (index, palette_command) = palette_command;
                let label = palette_command.label.to_lowercase();
                let id = palette_command.id.to_lowercase();
                let description = palette_command.description.to_lowercase();
                let section = palette_command.section.label().to_lowercase();
                let prefix_match = input.is_empty()
                    || label.starts_with(&input)
                    || id.starts_with(&input)
                    || section.starts_with(&input);
                let contains_match = prefix_match
                    || label.contains(&input)
                    || id.contains(&input)
                    || description.contains(&input)
                    || section.contains(&input);
                contains_match.then_some((
                    prefix_match,
                    palette_command.section,
                    index,
                    palette_command.id.to_string(),
                ))
            })
            .collect::<Vec<_>>();
        let has_prefix_matches = filtered.iter().any(|(prefix_match, _, _, _)| *prefix_match);
        let mut filtered = filtered
            .into_iter()
            .filter(|(prefix_match, _, _, _)| !has_prefix_matches || *prefix_match)
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            if input.is_empty() {
                left.1
                    .cmp(&right.1)
                    .then_with(|| left.2.cmp(&right.2))
                    .then_with(|| left.3.cmp(&right.3))
            } else {
                left.3.cmp(&right.3)
            }
        });
        self.palette_filtered = filtered
            .into_iter()
            .map(|(_, _, _, command)| command)
            .collect();
        self.palette_selected = 0;
    }

    fn update_session_history_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let mut filtered = self
            .session_history_entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                session_history_entry_matches_action(entry, self.startup_launcher_action)
            })
            .filter(|(_, entry)| session_history_filter_matches(entry, &input))
            .map(|(index, entry)| {
                (
                    index,
                    session_history_action_sort_bucket(entry, self.startup_launcher_action),
                )
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|(left_index, left_bucket), (right_index, right_bucket)| {
            let left_entry = &self.session_history_entries[*left_index];
            let right_entry = &self.session_history_entries[*right_index];
            left_bucket
                .cmp(right_bucket)
                .then_with(|| {
                    right_entry
                        .catalog
                        .last_updated_at
                        .as_deref()
                        .unwrap_or("")
                        .cmp(left_entry.catalog.last_updated_at.as_deref().unwrap_or(""))
                })
                .then_with(|| {
                    session_history_run_name(left_entry).cmp(session_history_run_name(right_entry))
                })
                .then_with(|| left_entry.catalog.run_id.cmp(&right_entry.catalog.run_id))
        });
        self.session_history_filtered = filtered.into_iter().map(|(index, _)| index).collect();
        self.session_history_selected = 0;
    }

    fn execute_palette_command(&mut self) {
        let Some(cmd) = self.palette_filtered.get(self.palette_selected) else {
            self.close_palette();
            return;
        };

        match cmd.as_str() {
            "new_session" => {
                self.startup_launcher_action = StartupLauncherAction::NewSession;
                self.apply_new_session_launcher_selection();
            }
            "resume_session" => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            "replay_session" => {
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            "switch_model" => {
                self.open_model_switcher();
            }
            "cycle_variant" => self.execute_action(Action::VariantCycle),
            "close_review_surface" => self.execute_action(Action::CloseReviewSurface),
            "open_event_log" => self.execute_action(Action::OpenEventLog),
            "toggle_follow" => self.execute_action(Action::ToggleFollow),
            "show_thinking" => self.show_transcript_thinking = true,
            "hide_thinking" => self.show_transcript_thinking = false,
            "show_timestamps" => self.show_transcript_timestamps = true,
            "hide_timestamps" => self.show_transcript_timestamps = false,
            "show_tool_details" => self.show_tool_details = true,
            "hide_tool_details" => self.show_tool_details = false,
            "show_generic_tool_output" => self.show_generic_tool_output = true,
            "hide_generic_tool_output" => self.show_generic_tool_output = false,
            "expand_selected_turn_results" => self.set_selected_activity_expandable_outputs(true),
            "collapse_selected_turn_results" => {
                self.set_selected_activity_expandable_outputs(false)
            }
            "stack_transcript_diffs" => self.stacked_transcript_diffs = true,
            "split_transcript_diffs" => self.stacked_transcript_diffs = false,
            "quit" => self.execute_action(Action::Quit),
            _ => {}
        }
        if !self.session_history_visible && !self.model_switcher_visible {
            self.close_palette();
        }
    }

    fn close_palette(&mut self) {
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.palette_selected = 0;
        self.session_history_selected = 0;
        self.model_selected = 0;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
        self.sync_slash_overlay();
    }

    fn select_previous_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let previous_index = if current_index == 0 {
            actions.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.post_run_handoff_action = actions[previous_index];
    }

    fn select_next_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let next_index = if current_index + 1 >= actions.len() {
            0
        } else {
            current_index + 1
        };
        self.post_run_handoff_action = actions[next_index];
    }

    fn execute_post_run_handoff_action(&mut self) {
        match self.selected_post_run_handoff_action() {
            PostRunHandoffAction::ContinueSession => {
                if self.continued_post_run_handoff_active {
                    self.continued_post_run_handoff_active = false;
                    self.continued_live_reopen_surface_active = true;
                    self.active_tab = Tab::Run;
                    self.focus = Focus::Prompt;
                    return;
                }
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::ReplayRun => {
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::StartAnotherSession => {
                self.apply_new_session_launcher_selection();
            }
            PostRunHandoffAction::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
            }
        }
    }

    fn open_palette(&mut self) {
        if !self.palette_visible {
            self.palette_focus_return = Some(self.focus);
        }
        self.palette_visible = true;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered = self
            .palette_commands()
            .iter()
            .map(|palette_command| palette_command.id.to_string())
            .collect();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.palette_selected = 0;
        self.sync_slash_overlay();
    }

    fn palette_commands(&self) -> Vec<crate::keybindings::PaletteCommand> {
        Action::grouped_palette_commands_for_overlay()
            .iter()
            .copied()
            .filter(|command| self.palette_command_available(command.id))
            .collect()
    }

    fn palette_command_available(&self, command_id: &str) -> bool {
        if command_id == "switch_model" {
            return !self.replay_mode;
        }

        if command_id == "cycle_variant" {
            return !self.replay_mode;
        }

        if self.startup_shell_visible() {
            matches!(
                command_id,
                "new_session" | "resume_session" | "replay_session" | "quit"
            )
        } else if matches!(command_id, "show_timestamps" | "hide_timestamps") {
            self.active_review_surface.is_none()
                && if command_id == "show_timestamps" {
                    !self.show_transcript_timestamps
                } else {
                    self.show_transcript_timestamps
                }
        } else if matches!(command_id, "show_thinking" | "hide_thinking") {
            self.active_review_surface.is_none()
                && if command_id == "show_thinking" {
                    !self.show_transcript_thinking
                } else {
                    self.show_transcript_thinking
                }
        } else if matches!(command_id, "show_tool_details" | "hide_tool_details") {
            self.active_review_surface.is_none()
                && if command_id == "show_tool_details" {
                    !self.show_tool_details
                } else {
                    self.show_tool_details
                }
        } else if matches!(
            command_id,
            "show_generic_tool_output" | "hide_generic_tool_output"
        ) {
            self.active_review_surface.is_none()
                && if command_id == "show_generic_tool_output" {
                    !self.show_generic_tool_output
                } else {
                    self.show_generic_tool_output
                }
        } else if matches!(
            command_id,
            "expand_selected_turn_results" | "collapse_selected_turn_results"
        ) {
            let expandable_ids = self.selected_activity_expandable_tool_ids();
            self.active_review_surface.is_none()
                && !expandable_ids.is_empty()
                && if command_id == "expand_selected_turn_results" {
                    expandable_ids
                        .iter()
                        .any(|tool_call_id| !self.expanded_tool_outputs.contains(tool_call_id))
                } else {
                    expandable_ids
                        .iter()
                        .any(|tool_call_id| self.expanded_tool_outputs.contains(tool_call_id))
                }
        } else if matches!(
            command_id,
            "stack_transcript_diffs" | "split_transcript_diffs"
        ) {
            self.active_review_surface.is_none()
                && if command_id == "stack_transcript_diffs" {
                    !self.stacked_transcript_diffs
                } else {
                    self.stacked_transcript_diffs
                }
        } else if command_id == "close_review_surface" {
            self.active_review_surface.is_some()
        } else if command_id == "open_event_log" {
            self.active_review_surface != Some(ReviewSurface::Events)
        } else {
            true
        }
    }

    fn begin_session_history_picker(&mut self, action: StartupLauncherAction) {
        self.startup_launcher_action = action;
        self.continue_disabled_banner = None;
        self.palette_visible = true;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.update_session_history_filter();
        self.open_session_history();
    }

    fn open_session_history(&mut self) {
        if !self.session_history_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = true;
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
        self.session_history_visible = true;
        self.sync_slash_overlay();
    }

    fn close_session_history(&mut self) {
        self.close_palette();
    }

    fn execute_selected_session_launcher_action(&mut self) {
        if self.session_history_entries.is_empty() {
            if matches!(
                self.startup_launcher_action,
                StartupLauncherAction::ContinueSession
            ) {
                self.continue_disabled_banner =
                    Some("continue unavailable: no session history entries".to_string());
            } else {
                self.continue_disabled_banner =
                    Some("replay unavailable: no session history entries".to_string());
            }
            self.open_session_history();
            return;
        }

        if self.session_history_filtered.is_empty() {
            self.continue_disabled_banner =
                Some("no sessions match the current filter".to_string());
            self.open_session_history();
            return;
        }

        let Some(selected) = self.selected_session_history_entry() else {
            return;
        };
        let selected_run_id = selected.catalog.run_id.clone();
        let selected_run_dir = selected.run_dir.clone();
        let selected_resumable = selected.catalog.is_resumable;
        let selected_resume_disabled_reason = selected.catalog.resume_disabled_reason.clone();

        match self.startup_launcher_action {
            StartupLauncherAction::NewSession => {
                self.apply_new_session_launcher_selection();
            }
            StartupLauncherAction::ReplaySession => {
                self.continue_disabled_banner = None;
                self.replay_mode = true;
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                if self.startup_mode {
                    self.should_quit = true;
                }
                self.close_session_history();
            }
            StartupLauncherAction::ContinueSession => {
                if !selected_resumable {
                    self.continue_disabled_banner = selected_resume_disabled_reason
                        .map(|reason| format!("continue unavailable: {reason}"))
                        .or_else(|| {
                            Some("continue unavailable for the selected session".to_string())
                        });
                    return;
                }

                self.continue_disabled_banner = None;
                self.replay_mode = false;
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                if self.startup_mode {
                    self.should_quit = true;
                }
                self.close_session_history();
            }
        }
    }

    fn apply_new_session_launcher_selection(&mut self) {
        let lifecycle_exit = self.startup_mode
            || self.post_run_handoff_visible()
            || self.completed_session_shell_active();
        let prompt_buffer = self.prompt_buffer.clone();
        let prompt_cursor = self.prompt_cursor;
        set_pending_live_prompt_draft(Some(prompt_buffer.clone()));
        set_pending_live_launch_metadata(self.launch_metadata.clone());

        self.projection.reset();
        self.selected_event_index = 0;
        self.selected_activity_index = 0;
        self.follow_mode = true;
        self.details_scroll = 0;
        self.transcript_scroll = 0;
        self.status_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.prompt_history.clear();
        self.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.continue_disabled_banner = None;

        self.prompt_buffer = prompt_buffer;
        self.prompt_cursor = prompt_cursor.min(self.prompt_buffer.chars().count());

        self.close_session_history();
        self.emit_ui_intent(UiIntent::NewSession);
        if lifecycle_exit {
            self.should_quit = true;
        }
    }

    fn select_previous_startup_launcher_action(&mut self) {
        self.startup_launcher_action = self.startup_launcher_action.previous();
        self.continue_disabled_banner = None;
    }

    fn select_next_startup_launcher_action(&mut self) {
        self.startup_launcher_action = self.startup_launcher_action.next();
        self.continue_disabled_banner = None;
    }

    fn execute_startup_launcher_action(&mut self) {
        match self.startup_launcher_action {
            StartupLauncherAction::NewSession => self.apply_new_session_launcher_selection(),
            StartupLauncherAction::ReplaySession => {
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            StartupLauncherAction::ContinueSession => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
        }
    }

    fn close_review_surface(&mut self) {
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        self.normalize_focus_for_active_surface();
    }

    fn open_review_surface(&mut self, surface: ReviewSurface) {
        self.active_tab = Tab::Run;
        self.active_review_surface = Some(surface);
        if !self.replay_mode {
            self.live_details_drawer_open = false;
        }
        self.normalize_focus_for_active_surface();
    }

    fn normalize_focus_for_active_surface(&mut self) {
        if self.replay_mode {
            if self.focus == Focus::Prompt {
                self.focus = if self.session_shell_operator_rail_interactive() {
                    Focus::List
                } else {
                    Focus::Details
                };
            } else if self.active_review_surface.is_none()
                && !self.session_shell_operator_rail_interactive()
                && self.focus == Focus::List
            {
                self.focus = Focus::Details;
            }
            return;
        }

        if self.post_run_handoff_visible() {
            if self.focus == Focus::Prompt || self.active_tab == Tab::Run {
                self.focus = Focus::List;
            }
            return;
        }

        if self.active_review_surface.is_some() && self.focus == Focus::Prompt {
            self.focus = Focus::List;
        } else if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
            && self.focus == Focus::List
        {
            self.focus = Focus::Details;
        }
    }

    fn cycle_focus_forward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = Focus::Details;
                return;
            }

            self.focus = match self.focus {
                Focus::List => Focus::Details,
                Focus::Details | Focus::Prompt => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt => Focus::Details,
                Focus::Details | Focus::List => Focus::Prompt,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details => Focus::List,
                Focus::List => Focus::Prompt,
                Focus::Prompt => Focus::Details,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::Prompt => Focus::List,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }

    fn cycle_focus_backward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = Focus::Details;
                return;
            }

            self.focus = match self.focus {
                Focus::List | Focus::Prompt => Focus::Details,
                Focus::Details => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::List => Focus::Details,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details => Focus::Prompt,
                Focus::List => Focus::Details,
                Focus::Prompt => Focus::List,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Prompt,
                Focus::Details => Focus::List,
                Focus::Prompt => Focus::Details,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }

    fn execute_action(&mut self, action: Action) {
        if self.execute_permission_action(action) {
            return;
        }

        if self.post_run_handoff_visible() && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_post_run_handoff_action();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.select_previous_post_run_handoff_action();
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.select_next_post_run_handoff_action();
                    return;
                }
                _ => {}
            }
        }

        if self.startup_shell_visible() && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_startup_launcher_action();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.select_previous_startup_launcher_action();
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.select_next_startup_launcher_action();
                    return;
                }
                _ => {}
            }
        }

        // Handle prompt-focused actions
        if self.focus == Focus::Prompt {
            if self.composer_disabled() {
                match action {
                    Action::SubmitPrompt
                    | Action::InsertNewline
                    | Action::ClearPrompt
                    | Action::HistoryUp
                    | Action::HistoryDown
                    | Action::CursorLeft
                    | Action::CursorRight
                    | Action::Backspace
                    | Action::Delete
                    | Action::Char(_) => return,
                    _ => {}
                }
            }

            match action {
                Action::SubmitPrompt => {
                    self.submit_prompt();
                    return;
                }
                Action::InsertNewline => {
                    self.insert_prompt_char('\n');
                    return;
                }
                Action::ClearPrompt => {
                    self.clear_prompt_input();
                    return;
                }
                Action::HistoryUp => {
                    if self.move_prompt_cursor_up() {
                        return;
                    }

                    if self.prompt_cursor_at_start() {
                        self.select_previous_prompt_history();
                    }
                    return;
                }
                Action::HistoryDown => {
                    if self.move_prompt_cursor_down() {
                        return;
                    }

                    if self.prompt_cursor_at_end() {
                        self.select_next_prompt_history();
                    }
                    return;
                }
                Action::CursorLeft => {
                    if self.prompt_cursor > 0 {
                        self.prompt_cursor -= 1;
                    }
                    return;
                }
                Action::CursorRight => {
                    if self.prompt_cursor < self.prompt_char_count() {
                        self.prompt_cursor += 1;
                    }
                    return;
                }
                Action::Backspace => {
                    self.backspace_prompt_char();
                    return;
                }
                Action::Delete => {
                    self.delete_prompt_char();
                    return;
                }
                Action::Char(c) => {
                    self.insert_prompt_char(c);
                    return;
                }
                _ => {}
            }
        }

        // Handle global actions
        match action {
            Action::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
            }
            Action::Palette => {
                self.open_palette();
            }
            Action::Help => {
                if self.active_review_surface == Some(ReviewSurface::Help) {
                    self.close_review_surface();
                } else {
                    self.open_review_surface(ReviewSurface::Help);
                }
            }
            Action::ToggleFollow => {
                self.follow_mode = !self.follow_mode;
                if self.follow_mode {
                    self.transcript_scroll = 0;
                }
            }
            Action::ToggleOperatorSidebar
                if !self.replay_mode
                    && self.focus != Focus::Prompt
                    && !self.post_run_handoff_visible() =>
            {
                let opening = self.active_review_surface.is_some() || !self.details_drawer_open();
                self.active_tab = Tab::Run;
                self.active_review_surface = None;
                self.live_details_drawer_open = opening;
                if (!opening && self.focus == Focus::List)
                    || (opening && self.focus == Focus::Prompt)
                {
                    self.focus = Focus::Details;
                }
            }
            Action::CloseReviewSurface if self.focus != Focus::Prompt => {
                self.close_review_surface();
            }
            Action::OpenEventLog if self.focus != Focus::Prompt => {
                self.open_review_surface(ReviewSurface::Events);
            }
            Action::Reload if self.replay_mode => {
                self.reload_requested = true;
            }
            Action::SessionChildFirst => {
                self.navigate_to_first_child_session();
            }
            Action::SessionChildCycle => {
                self.navigate_to_child_sibling(false);
            }
            Action::SessionChildCycleReverse => {
                self.navigate_to_child_sibling(true);
            }
            Action::SessionParent => {
                self.navigate_to_parent_session();
            }
            Action::VariantCycle => {
                self.cycle_variant();
            }
            Action::MoveDown if self.focus != Focus::Prompt => {
                if self.active_review_surface.is_none() && self.focus == Focus::List {
                    self.next_activity();
                } else if self.focus == Focus::List {
                    self.next_event();
                } else {
                    if self.focus == Focus::Details {
                        if self.transcript_surface_active() {
                            self.scroll_transcript_up(1);
                        } else {
                            self.details_scroll = self.details_scroll.saturating_add(1);
                        }
                    }
                }
            }
            Action::MoveUp if self.focus != Focus::Prompt => {
                if self.active_review_surface.is_none() && self.focus == Focus::List {
                    self.previous_activity();
                } else if self.focus == Focus::List {
                    self.previous_event();
                } else {
                    if self.focus == Focus::Details {
                        if self.transcript_surface_active() {
                            self.scroll_transcript_down(1);
                        } else {
                            self.details_scroll = self.details_scroll.saturating_sub(1);
                        }
                    }
                }
            }
            Action::FocusNext => {
                self.cycle_focus_forward();
            }
            Action::FocusPrev => {
                self.cycle_focus_backward();
            }
            _ => {}
        }
    }

    fn execute_permission_action(&mut self, action: Action) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match action {
            Action::AllowPermission => {
                let reason = self
                    .active_permission_view()
                    .and_then(|permission| self.build_question_permission_reason(&permission));
                if self.active_permission_view().is_some_and(|permission| {
                    permission.question_prompts.is_some() && reason.is_none()
                }) {
                    return true;
                }
                self.send_permission_intent(permission_id, PermissionDecision::Allow, reason);
                true
            }
            Action::DenyPermission => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
                true
            }
            Action::DismissModal => {
                self.dismissed_permissions.insert(permission_id);
                self.maybe_auto_exit();
                true
            }
            Action::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
                true
            }
            _ => true,
        }
    }

    fn _handle_prompt_key(&mut self, key_code: KeyCode) -> bool {
        match key_code {
            KeyCode::Enter => {
                self.submit_prompt();
                true
            }
            KeyCode::Esc => {
                self.prompt_buffer.clear();
                self.prompt_cursor = 0;
                self.prompt_history_index = None;
                true
            }
            KeyCode::Up => {
                if !self.prompt_history.is_empty() {
                    let next_idx = match self.prompt_history_index {
                        Some(idx) => idx.saturating_sub(1),
                        None => self.prompt_history.len().saturating_sub(1),
                    };
                    self.prompt_history_index = Some(next_idx);
                    self.prompt_buffer = self.prompt_history[next_idx].clone();
                    self.prompt_cursor = self.prompt_buffer.len();
                }
                true
            }
            KeyCode::Down => {
                if let Some(idx) = self.prompt_history_index {
                    if idx + 1 < self.prompt_history.len() {
                        let next_idx = idx + 1;
                        self.prompt_history_index = Some(next_idx);
                        self.prompt_buffer = self.prompt_history[next_idx].clone();
                        self.prompt_cursor = self.prompt_buffer.len();
                    } else {
                        self.prompt_history_index = None;
                        self.prompt_buffer.clear();
                        self.prompt_cursor = 0;
                    }
                }
                true
            }
            KeyCode::Left => {
                if self.prompt_cursor > 0 {
                    self.prompt_cursor -= 1;
                }
                true
            }
            KeyCode::Right => {
                if self.prompt_cursor < self.prompt_buffer.chars().count() {
                    self.prompt_cursor += 1;
                }
                true
            }
            KeyCode::Backspace => {
                if self.prompt_cursor > 0 {
                    self.prompt_cursor -= 1;
                    let byte_idx = self
                        .prompt_buffer
                        .char_indices()
                        .nth(self.prompt_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.prompt_buffer.len());
                    self.prompt_buffer.remove(byte_idx);
                }
                true
            }
            KeyCode::Delete => {
                if self.prompt_cursor < self.prompt_buffer.chars().count() {
                    let byte_idx = self
                        .prompt_buffer
                        .char_indices()
                        .nth(self.prompt_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.prompt_buffer.len());
                    self.prompt_buffer.remove(byte_idx);
                }
                true
            }
            KeyCode::Char(c) => {
                let byte_idx = self
                    .prompt_buffer
                    .char_indices()
                    .nth(self.prompt_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.prompt_buffer.len());
                self.prompt_buffer.insert(byte_idx, c);
                self.prompt_cursor += 1;
                true
            }
            KeyCode::Tab | KeyCode::BackTab => false,
            _ => true,
        }
    }

    pub fn next_event(&mut self) {
        if !self.events.is_empty() && self.selected_event_index < self.events.len() - 1 {
            self.selected_event_index += 1;
            self.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    pub fn previous_event(&mut self) {
        if self.selected_event_index > 0 {
            self.selected_event_index -= 1;
            self.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    fn next_activity(&mut self) {
        if !self.activities.is_empty() && self.selected_activity_index < self.activities.len() - 1 {
            self.selected_activity_index += 1;
            self.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_scroll = 0;
        }
    }

    fn previous_activity(&mut self) {
        if self.selected_activity_index > 0 {
            self.selected_activity_index -= 1;
            self.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_scroll = 0;
        }
    }

    fn transcript_surface_active(&self) -> bool {
        self.active_review_surface.is_none()
            && self.focus == Focus::Details
            && !self.details_drawer_open()
    }

    fn scroll_transcript_up(&mut self, amount: u16) {
        self.follow_mode = false;
        self.transcript_scroll = self.transcript_scroll.saturating_add(amount.max(1));
    }

    fn scroll_transcript_down(&mut self, amount: u16) {
        self.transcript_scroll = self.transcript_scroll.saturating_sub(amount.max(1));
        if self.transcript_scroll == 0 {
            self.follow_mode = true;
        }
    }

    fn _handle_modal_key(&mut self, key: KeyEvent) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Allow, None);
                true
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
                true
            }
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.dismissed_permissions.insert(permission_id);
                self.maybe_auto_exit();
                true
            }
            _ => false,
        }
    }

    fn ensure_question_answer_state(&mut self, permission_id: &str) {
        if self.question_answer_permission_id.as_deref() == Some(permission_id) {
            return;
        }

        self.question_answer_permission_id = Some(permission_id.to_string());
        self.question_answer_buffer.clear();
        self.question_answer_cursor = 0;
        self.question_answer_error = None;
    }

    fn question_answer_char_count(&self) -> usize {
        self.question_answer_buffer.chars().count()
    }

    fn question_answer_cursor_byte_index(&self) -> usize {
        self.question_answer_buffer
            .char_indices()
            .nth(self.question_answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.question_answer_buffer.len())
    }

    fn insert_question_answer_char(&mut self, c: char) {
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.insert(byte_idx, c);
        self.question_answer_cursor += 1;
        self.question_answer_error = None;
    }

    fn backspace_question_answer_char(&mut self) {
        if self.question_answer_cursor == 0 {
            return;
        }

        self.question_answer_cursor -= 1;
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.remove(byte_idx);
        self.question_answer_error = None;
    }

    fn delete_question_answer_char(&mut self) {
        if self.question_answer_cursor >= self.question_answer_char_count() {
            return;
        }

        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.remove(byte_idx);
        self.question_answer_error = None;
    }

    fn build_question_permission_reason(
        &mut self,
        permission: &ActivePermissionView,
    ) -> Option<String> {
        let prompts = permission.question_prompts.as_ref()?;
        match parse_question_answers_from_draft(prompts, &self.question_answer_buffer) {
            Ok(answers) => {
                self.question_answer_error = None;
                serde_json::to_string(&answers).ok()
            }
            Err(err) => {
                self.question_answer_error = Some(err);
                None
            }
        }
    }

    pub(crate) fn question_answer_preview(&self, permission_id: &str) -> String {
        if self.question_answer_permission_id.as_deref() != Some(permission_id) {
            return "█".to_string();
        }

        let mut preview = self.question_answer_buffer.clone();
        let byte_idx = preview
            .char_indices()
            .nth(self.question_answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(preview.len());
        preview.insert(byte_idx, '█');
        preview
    }

    pub(crate) fn question_answer_error(&self, permission_id: &str) -> Option<&str> {
        (self.question_answer_permission_id.as_deref() == Some(permission_id))
            .then_some(self.question_answer_error.as_deref())
            .flatten()
    }

    fn clear_question_answer_state(&mut self, permission_id: &str) {
        if self.question_answer_permission_id.as_deref() != Some(permission_id) {
            return;
        }

        self.question_answer_permission_id = None;
        self.question_answer_buffer.clear();
        self.question_answer_cursor = 0;
        self.question_answer_error = None;
    }

    fn send_permission_intent(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) {
        if self.submitted_permission_id.as_deref() == Some(permission_id.as_str()) {
            return;
        }

        self.emit_ui_intent(UiIntent::ResolvePermission {
            permission_id: permission_id.clone(),
            decision,
            reason,
        });
        self.submitted_permission_id = Some(permission_id);
    }

    fn submit_prompt(&mut self) {
        if !self.replay_mode && !self.composer_disabled() {
            if let Some(command) = self.typed_slash_command() {
                self.execute_slash_command(command, self.slash_draft_snapshot.clone());
                return;
            }
        }

        if self.prompt_buffer.trim().is_empty()
            || self.active_turn_in_progress()
            || self.composer_disabled()
            || self.replay_mode
        {
            return;
        }

        if self.startup_mode {
            let text = self.prompt_buffer.clone();
            self.emit_ui_intent(UiIntent::SubmitPrompt {
                text,
                launch_metadata: self.launch_metadata.clone(),
            });
            self.should_quit = true;
            return;
        }

        let text = self.prompt_buffer.clone();
        self.dispatch_submitted_prompt(text);
    }

    fn update_transient_state_for_event(&mut self, event: &EventEnvelopeV1) {
        if let EventV1::PermissionResolved(data) = &event.payload {
            self.dismissed_permissions.remove(&data.permission_id);
            if self.submitted_permission_id.as_deref() == Some(data.permission_id.as_str()) {
                self.submitted_permission_id = None;
            }
            self.clear_question_answer_state(&data.permission_id);
        }
    }

    fn maybe_auto_exit(&mut self) {
        if self.auto_exit_on_finish
            && self.projection.run_terminal_seen
            && self.active_permission().is_none()
        {
            self.should_quit = true;
        }
    }
}

fn action_preempts_text_input(action: Action) -> bool {
    matches!(
        action,
        Action::SessionChildCycle | Action::SessionChildCycleReverse
    )
}

fn json_string_field(output_json: Option<&serde_json::Value>, keys: &[&str]) -> Option<String> {
    let object = output_json?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn sibling_session_id(
    session_ids: &[String],
    current_session_id: &str,
    reverse: bool,
) -> Option<String> {
    if session_ids.is_empty() {
        return None;
    }

    let current_index = session_ids
        .iter()
        .position(|session_id| session_id == current_session_id)?;
    let next_index = if reverse {
        current_index
            .checked_sub(1)
            .unwrap_or(session_ids.len().saturating_sub(1))
    } else {
        (current_index + 1) % session_ids.len()
    };
    session_ids.get(next_index).cloned()
}

fn lineage_parent_session_id_from_event(event: &EventEnvelopeV1) -> Option<String> {
    let parent_session_id = match &event.payload {
        EventV1::ToolCallRequested(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        EventV1::ToolCallFinished(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        EventV1::TaskCompleted(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        _ => None,
    }?;

    let parent_session_id = parent_session_id.trim();
    (!parent_session_id.is_empty()).then(|| parent_session_id.to_string())
}

fn parent_session_id_from_events(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().find_map(lineage_parent_session_id_from_event)
}

fn load_session_events(session_path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = session_path.join("events.jsonl");
    let body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    let mut events = Vec::new();
    for (line_number, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = serde_json::from_str(trimmed).map_err(|err| {
            format!(
                "failed to parse {} line {}: {err}",
                events_path.display(),
                line_number + 1
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

fn infer_launch_metadata_from_events(
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let profile = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.clone()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.profile().to_string());
    let (provider, model) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(payload) => {
                Some((payload.provider_id.clone(), Some(payload.model_id.clone())))
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            (
                fallback.provider().to_string(),
                fallback.model().map(str::to_string),
            )
        });

    let mut launch_metadata = LaunchMetadata::new(profile, provider, model)
        .with_available_models(fallback.available_models().to_vec());
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn replay_launch_metadata_from_session(
    session_path: &Path,
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    load_replay_run_metadata(session_path)
        .and_then(|metadata| {
            metadata
                .recorded_runtime_context
                .as_ref()
                .map(|context| launch_metadata_from_recorded_runtime_context(context, fallback))
        })
        .unwrap_or_else(|| infer_launch_metadata_from_events(events, fallback))
}

fn load_replay_run_metadata(session_path: &Path) -> Option<RunMetadata> {
    let meta_path = session_path.join("meta.json");
    let body = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&body).ok()
}

fn launch_metadata_from_recorded_runtime_context(
    recorded_runtime_context: &harness_core::proj::RecordedRuntimeContext,
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let mut launch_metadata = LaunchMetadata::from_model_option(&ModelOption {
        profile: recorded_runtime_context.profile.clone(),
        provider: recorded_runtime_context.provider.clone(),
        model: recorded_runtime_context.model.clone(),
        variant: recorded_runtime_context.variant.clone(),
        display_label: Some(recorded_runtime_context.display_label.clone())
            .filter(|value| !value.trim().is_empty()),
        token_window_label: recorded_runtime_context.token_window_label.clone(),
        context_window_tokens: recorded_runtime_context.context_window_tokens,
        max_input_tokens: recorded_runtime_context.max_input_tokens,
        max_output_tokens: recorded_runtime_context.max_output_tokens,
        description: recorded_runtime_context.description.clone(),
        reasoning_effort: recorded_runtime_context.reasoning_effort.clone(),
        text_verbosity: recorded_runtime_context.text_verbosity.clone(),
        recommended_for: recorded_runtime_context.recommended_for.clone(),
    })
    .with_available_models(fallback.available_models().to_vec());
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn session_navigation_snapshot_from_path(
    session_path: &Path,
    fallback_launch_metadata: &LaunchMetadata,
) -> Result<SessionNavigationSnapshot, String> {
    let events = load_session_events(session_path)?;
    let launch_metadata =
        replay_launch_metadata_from_session(session_path, &events, fallback_launch_metadata);
    let replay = AppState::new_replay(session_path.to_path_buf(), events.clone());

    Ok(SessionNavigationSnapshot {
        session_path: session_path.to_path_buf(),
        events,
        launch_metadata,
        child_session_ids: replay.child_session_ids(),
    })
}

fn parse_question_prompts(kind: &str, summary: &str) -> Option<Vec<QuestionPromptView>> {
    if !kind.eq_ignore_ascii_case("question")
        && !kind.eq_ignore_ascii_case("ask")
        && !kind.eq_ignore_ascii_case("ask_user")
    {
        return None;
    }

    let value = serde_json::from_str::<serde_json::Value>(summary).ok()?;
    let questions = value.get("questions")?.as_array()?;
    let prompts = questions
        .iter()
        .map(|question| {
            Some(QuestionPromptView {
                question: question.get("question")?.as_str()?.to_string(),
                header: question.get("header")?.as_str()?.to_string(),
                options: question
                    .get("options")?
                    .as_array()?
                    .iter()
                    .map(|option| {
                        Some(QuestionOptionView {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option.get("description")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                multiple: question
                    .get("multiple")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(prompts)
}

fn parse_question_answers_from_draft(
    prompts: &[QuestionPromptView],
    draft: &str,
) -> Result<Vec<Vec<String>>, String> {
    let lines = draft.lines().collect::<Vec<_>>();
    let mut answers = Vec::with_capacity(prompts.len());

    for (index, prompt) in prompts.iter().enumerate() {
        let line = lines.get(index).copied().unwrap_or_default().trim();
        if line.is_empty() {
            return Err(format!(
                "Answer question {} ({}) before continuing.",
                index + 1,
                prompt.header
            ));
        }

        let values = if prompt.multiple {
            line.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        } else {
            if line.contains(',') {
                return Err(format!(
                    "Question {} ({}) accepts only one answer.",
                    index + 1,
                    prompt.header
                ));
            }
            vec![line]
        };

        if values.is_empty() {
            return Err(format!(
                "Answer question {} ({}) before continuing.",
                index + 1,
                prompt.header
            ));
        }

        answers.push(
            values
                .into_iter()
                .map(|value| {
                    prompt
                        .options
                        .iter()
                        .find(|option| option.label.eq_ignore_ascii_case(value))
                        .map(|option| option.label.clone())
                        .unwrap_or_else(|| value.to_string())
                })
                .collect(),
        );
    }

    Ok(answers)
}

fn permission_display_summary(permission: &ActivePermissionView) -> String {
    if permission.question_prompts.is_some() {
        "Question requested".to_string()
    } else {
        permission.summary.clone()
    }
}

fn tool_call_has_expandable_output(tool_call: &ToolCallEntry) -> bool {
    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && tool_call.effective_tool_id().starts_with("mcp.")
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(|output| !output.trim().is_empty())
    {
        return true;
    }

    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let line_count = output.lines().count();
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" => line_count > 10,
            "edit.hashline_apply" => tool_call
                .edit
                .as_ref()
                .and_then(|edit| edit.diff_rel_path.as_ref())
                .is_some(),
            "agent.spawn" => true,
            _ => !output.trim().is_empty() && line_count > 3,
        }
}

#[cfg(test)]
pub(crate) fn exact_test_startup_slash_commands_execute_without_menu() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app.slash_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), None);
    assert_eq!(
        app.slash_filtered,
        vec![
            "new".to_string(),
            "resume".to_string(),
            "replay".to_string(),
            "model".to_string(),
            "exit".to_string(),
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_slash_new_preserves_draft_and_returns_home() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.prompt_buffer = "/new".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("carry draft home".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.startup_shell_visible());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.prompt_buffer, "carry draft home");
    assert_eq!(app.prompt_cursor, "carry draft home".chars().count());
    assert!(!app.should_quit);
    assert!(!app.replay_mode);
    assert!(app.session_path.is_none());
}

#[cfg(test)]
pub(crate) fn exact_test_replay_mode_disables_slash_workflow() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    app.focus = Focus::Prompt;
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app.slash_visible);
    assert_eq!(app.overlay_stack().top(), None);
    assert!(app.prompt_buffer.is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_replay_opens_history_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.prompt_buffer = "/replay".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("keep this draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.palette_visible);
    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ReplaySession
    );
    assert_eq!(app.prompt_buffer, "keep this draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_compact_operator_rail_skips_focus_cycle() {
    let mut live = AppState::new_live(None, false, None);

    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    let mut live_overlay = AppState::new_live(None, false, None);
    live_overlay.focus = Focus::Details;
    live_overlay.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert!(live_overlay.details_drawer_open());

    live_overlay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(live_overlay.focus, Focus::List);
    assert!(live_overlay.details_drawer_open());

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
    assert_eq!(replay.focus, Focus::Details);
}
