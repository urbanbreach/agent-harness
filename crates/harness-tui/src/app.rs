use std::cell::Cell;
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_core::event::{
    ActorKind, EventArtifactRef, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    ProviderRequestStartedEvent, ResolvedToolIdentity, TaskCompletionMetadata, TaskLineageMetadata,
    ToolCallLifecycleState, ToolCallMetadata, ToolCallStatus, UserMessageSubmittedEvent,
};
use harness_core::perm::{PermissionDecision, PermissionGrantScope};
use harness_core::workspace::WorkspaceEnvironment;
use ratatui::layout::Rect;

use crate::keybindings::{Action, KeyMap};
use crate::overlay::{OverlayKind, OverlayStack, OverlayState};
use crate::text::{
    has_trimmed_content, non_empty_trimmed, trimmed_json_nested_string_field,
    trimmed_json_string_field,
};
use crate::theme::Theme;
use crate::ui::{
    OperatorSidebarKeyboardTarget, OperatorSidebarKeyboardTargetKind, OperatorSidebarSelection,
    OperatorSidebarSelectionCell, TranscriptMouseTarget, TranscriptScrollbarHit,
    TranscriptSelection, TranscriptSelectionCell, WheelTarget,
};
use crate::view_model;
use crate::{clipboard, ui};

mod file_mentions;
mod lineage;
mod onboarding;
mod pending_live;
pub(crate) mod permissions;
mod prompt_history;
pub(crate) mod session_navigation;
mod session_projection;
mod terminal_panel;
#[cfg(test)]
mod tests;
mod toggles;

use self::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use self::session_navigation::SessionNavigationSnapshot;
use self::session_projection::SessionProjection;
use self::terminal_panel::terminal_panel_event_is_shell;
pub use self::terminal_panel::{TerminalPanelEntry, TerminalPanelStatus};
pub use crate::view_model::{ForkSelectorViewModel, LineageBrowserViewModel};
#[cfg(test)]
pub(crate) use file_mentions::FileMentionSelectedTag;
pub(crate) use file_mentions::{
    system_file_mention_now_unix, system_file_mention_workspace_root, FileMentionEntry,
    FileMentionFrecency, FileMentionIndex, FileMentionTag, FileMentionWorkspaceScanner,
    SystemFileMentionWorkspaceScanner,
};
pub use lineage::{ForkSelectorState, LineageBrowserState};
pub use onboarding::{OnboardingScreen, OnboardingStep};
pub use pending_live::{
    set_pending_live_launch_metadata, set_pending_live_prompt_auto_submit,
    set_pending_live_prompt_draft,
};
use pending_live::{
    take_pending_live_launch_metadata, take_pending_live_prompt, PendingLivePrompt,
};
use permissions::permission_display_summary;
pub use permissions::{
    ActivePermissionView, PermissionEntry, QuestionOptionView, QuestionPromptView,
};
pub use prompt_history::prompt_history_path_for_session_dir;
use prompt_history::PromptHistoryDraft;
pub use session_navigation::{LaunchMetadata, McpResourceOption, ModelOption, SessionHistoryEntry};
pub use toggles::{ToggleEntryConfig, ToggleEntryKind, ToggleMenuRow, TogglesConfig};

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;
const TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 72;
const TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS: usize = 3;
const INTERRUPT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

static NEXT_TRANSCRIPT_CACHE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static TRANSCRIPT_RENDER_KEY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDisplayStatus {
    PendingPermission,
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum OperatorSidebarSection {
    Todo,
    Subagents,
    Mcp,
    Lsp,
    ModifiedFiles,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubagentSessionInfo {
    pub label: String,
    pub title: String,
    pub parent_label: String,
    pub index: usize,
    pub total: usize,
    pub usage: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentFooterTarget {
    Parent,
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy)]
struct TranscriptScrollbarDragState {
    track: Rect,
    thumb_height: u16,
    pointer_offset_y: u16,
    max_scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorSidebarPendingClick {
    Section(OperatorSidebarSection),
    SubagentGroup(String),
    SubagentSession(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArtifactEntry {
    pub path: String,
    pub digest: Option<String>,
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
    trimmed_json_string_field(Some(&value), &["path", "filePath"])
}

fn compact_tool_payload_for_transcript(payload: &str) -> Option<String> {
    crate::text_compact::compact_payload(
        payload,
        TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS,
        TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS,
    )
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
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
    pub profile_label: String,
    pub model_id: String,
    pub provider_id: String,
    pub status: ActivityStatus,
    pub user_message: Option<UserMessageSubmittedEvent>,
    pub user_timestamp: Option<String>,
    pub request_data: Option<ProviderRequestStartedEvent>,
    pub thinking_text: String,
    pub transcript_text: String,
    pub usage: Option<ActivityUsage>,
    pub cache_usage: Option<ActivityCacheUsage>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityCacheUsage {
    pub read_tokens: u32,
    pub write_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveContextUsage {
    pub tokens: Option<u32>,
    pub compacted_pending_refresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionStatus {
    pub agent_id: String,
    pub checkpoint_id: Option<String>,
    pub trigger_reason: String,
    pub state: CompactionState,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompactionUsageMetrics {
    pub completed_count: u32,
    pub summary_tokens_estimate: u64,
    pub reduction_tokens_estimate: u64,
    pub last_tokens_before_estimate: Option<u32>,
    pub last_tokens_after_estimate: Option<u32>,
    pub last_reduction_percent_estimate: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionState {
    Requested,
    Written,
    Applied,
    Failed,
}

impl ActiveContextUsage {
    pub const fn estimate(tokens: u32) -> Self {
        Self {
            tokens: Some(tokens),
            compacted_pending_refresh: false,
        }
    }

    pub const fn compacted_pending_refresh() -> Self {
        Self {
            tokens: None,
            compacted_pending_refresh: true,
        }
    }
}

struct NewStreamingActivityEntryArgs {
    request_id: String,
    profile_label: String,
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
        profile_label,
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
        profile_label,
        model_id,
        provider_id,
        status: ActivityStatus::Streaming,
        user_message,
        user_timestamp,
        request_data,
        thinking_text: String::new(),
        transcript_text,
        usage: None,
        cache_usage: None,
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

pub(crate) fn humanize_profile_label(profile: &str) -> String {
    let words = profile
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return profile.to_string();
    }
    words.join(" ")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityStatus {
    Queued,
    Streaming,
    Done,
    Error,
}

impl std::fmt::Display for ActivityStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivityStatus::Queued => write!(f, "queued"),
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
    Failed,
    TimedOut,
    LateResult,
}

impl OrchestrationTaskState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::TimedOut | Self::LateResult
        )
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::Stale => 0,
            Self::Running => 1,
            Self::Queued => 2,
            Self::Completed
            | Self::Cancelled
            | Self::Failed
            | Self::TimedOut
            | Self::LateResult => 0,
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
    pub current_child_tool_title: Option<String>,
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
            .and_then(non_empty_trimmed)
    }

    pub(crate) fn effective_child_request_id(&self) -> Option<&str> {
        self.child_request_id
            .as_deref()
            .or(self.request_id.as_deref())
            .and_then(non_empty_trimmed)
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

pub(crate) fn task_completed_updates_assistant_transcript(
    data: &harness_core::event::TaskCompletedEvent,
) -> bool {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.lineage.as_ref())
        .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
        .is_none()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastVariant {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToastState {
    pub message: String,
    pub variant: ToastVariant,
    remaining_frames: u16,
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
    Terminal,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIntent {
    ResolvePermission {
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
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
        selected_file_tags: Vec<harness_core::file_tag::SelectedFileTag>,
        selected_agent_tags: Vec<harness_core::file_tag::SelectedAgentTag>,
        selected_resource_tags: Vec<harness_core::file_tag::SelectedResourceTag>,
        launch_metadata: LaunchMetadata,
    },
    CompactSession,
    OpenAuthManager {
        args: Vec<String>,
        stdin: Option<String>,
    },
    InterruptSession {
        task_ids: Vec<String>,
    },
    ForkSession {
        source_run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
        stable_prefix: harness_core::session_lineage::StableSessionPrefix,
        prompt_text: String,
    },
    CloneSession {
        source_run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
        stable_prefix: harness_core::session_lineage::StableSessionPrefix,
    },
    QuitRequested,
}

pub(super) fn auth_status_banner(args: &[String]) -> String {
    format!(
        "auth backend requested: harness auth {}",
        display_auth_args_for_status(args)
    )
}

fn display_auth_args_for_status(args: &[String]) -> String {
    let mut display = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            display.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        if auth_arg_redacts_next(arg) {
            display.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_auth_arg_value(arg) {
            display.push(redacted);
            continue;
        }
        display.push(arg.clone());
    }
    display.join(" ")
}

fn auth_arg_redacts_next(arg: &str) -> bool {
    matches!(
        arg,
        "--mock-token" | "--mock-refresh-token" | "--enterprise-url"
    )
}

fn redact_auth_arg_value(arg: &str) -> Option<String> {
    [
        "--mock-token=",
        "--mock-refresh-token=",
        "--enterprise-url=",
    ]
    .into_iter()
    .find_map(|prefix| {
        arg.strip_prefix(prefix)
            .map(|_| format!("{prefix}<redacted>"))
    })
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

fn workspace_context_labels(environment: &WorkspaceEnvironment) -> Vec<String> {
    let full = directory_branch_label(environment, false);
    let short = directory_branch_label(environment, true);
    if full == short {
        vec![full]
    } else {
        vec![full, short]
    }
}

fn directory_branch_label(environment: &WorkspaceEnvironment, short: bool) -> String {
    let path = if short {
        environment
            .working_directory
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| home_shortened_path(&environment.working_directory))
    } else {
        home_shortened_path(&environment.working_directory)
    };

    match environment.git_branch.as_deref() {
        Some(branch) if !branch.trim().is_empty() => format!("{path}:{branch}"),
        _ => path,
    }
}

fn home_shortened_path(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home.as_deref().filter(|home| !home.as_os_str().is_empty()) {
        if path == home {
            return "~".to_string();
        }
        if let Ok(stripped) = path.strip_prefix(home) {
            return format!("~/{}", stripped.display());
        }
    }

    path.display().to_string()
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
    toast: Option<ToastState>,
    pub details_scroll: u16,
    pub transcript_scroll: usize,
    pub terminal_panel_scroll: usize,
    pub terminal_panel_follow: bool,
    pub(crate) last_transcript_max_scroll: Cell<usize>,
    pub(crate) last_terminal_panel_max_scroll: Cell<usize>,
    last_frame_area: Option<Rect>,
    transcript_scrollbar_drag: Option<TranscriptScrollbarDragState>,
    selected_diff_hunk_row: Option<usize>,
    hovered_transcript_target: Option<TranscriptMouseTarget>,
    hovered_subagent_footer_target: Option<SubagentFooterTarget>,
    transcript_click_activated_on_down: bool,
    transcript_selection: Option<TranscriptSelection>,
    transcript_selection_dragging: bool,
    operator_sidebar_selection: Option<OperatorSidebarSelection>,
    selected_operator_sidebar_keyboard_index: Option<usize>,
    operator_sidebar_selection_dragging: bool,
    operator_sidebar_pending_click: Option<OperatorSidebarPendingClick>,
    transcript_cache_instance_id: u64,
    transcript_render_epoch: u64,
    transcript_render_key_cache: Cell<Option<(u64, u64)>>,
    transcript_animation_phase: usize,
    pub auto_exit_on_finish: bool,
    pub prompt_buffer: String,
    pub prompt_cursor: usize,
    pub prompt_history: Vec<String>,
    pub prompt_history_index: Option<usize>,
    prompt_history_path: Option<PathBuf>,
    prompt_history_draft: Option<PromptHistoryDraft>,
    pub selected_activity_index: usize,
    pub palette_visible: bool,
    pub palette_input: String,
    pub palette_cursor: usize,
    pub palette_filtered: Vec<String>,
    pub palette_selected: usize,
    palette_focus_return: Option<Focus>,
    pub(crate) status_dialog_visible: bool,
    show_transcript_thinking: bool,
    show_transcript_timestamps: bool,
    show_tool_details: bool,
    show_generic_tool_output: bool,
    terminal_panel_visible: bool,
    stacked_transcript_diffs: bool,
    expanded_tool_outputs: BTreeSet<String>,
    expanded_patch_file_outputs: BTreeSet<String>,
    collapsed_operator_sidebar_sections: BTreeSet<OperatorSidebarSection>,
    expanded_operator_sidebar_subagent_groups: BTreeSet<String>,
    pub startup_mode: bool,
    pub startup_launcher_action: StartupLauncherAction,
    pub(crate) onboarding_visible: bool,
    pub(crate) onboarding_step: OnboardingStep,
    pub(crate) onboarding_selected: usize,
    pub(crate) onboarding_skipped_for_launch: bool,
    pub(crate) onboarding_auth_in_progress: bool,
    pub(crate) onboarding_secret_input: String,
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
    pub toggles_menu_visible: bool,
    pub toggles_selected: usize,
    toggles_yolo_confirm_visible: bool,
    runtime_toggles: toggles::RuntimeTogglesState,
    pub lineage_browser: LineageBrowserState,
    pub lineage_browser_visible: bool,
    pub fork_selector: ForkSelectorState,
    pub fork_selector_visible: bool,
    pub slash_visible: bool,
    pub slash_filtered: Vec<String>,
    pub slash_selected: usize,
    slash_draft_snapshot: Option<String>,
    pub(crate) file_mention_visible: bool,
    pub(crate) file_mention_entries: Vec<FileMentionEntry>,
    pub(crate) file_mention_selected: usize,
    file_mention_trigger: Option<usize>,
    file_mention_workspace_root: Option<PathBuf>,
    file_mention_workspace_root_provider: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    file_mention_scanner: Arc<dyn FileMentionWorkspaceScanner>,
    file_mention_now_unix: Arc<dyn Fn() -> u64 + Send + Sync>,
    workspace_context_labels: Vec<String>,
    file_mention_index: Option<FileMentionIndex>,
    pub(crate) file_mention_tags: Vec<FileMentionTag>,
    file_mention_frecency: BTreeMap<String, FileMentionFrecency>,
    pub continue_disabled_banner: Option<String>,
    pub keymap: KeyMap,
    theme: Theme,
    launch_metadata: LaunchMetadata,
    runtime_context_metadata: Option<LaunchMetadata>,
    session_navigation_stack: Vec<SessionNavigationSnapshot>,
    dismissed_permissions: BTreeSet<String>,
    submitted_permission_id: Option<String>,
    permission_modal_permission_id: Option<String>,
    permission_modal_stage: PermissionModalStage,
    permission_modal_selection: PermissionModalSelection,
    permission_modal_confirm_selection: PermissionConfirmSelection,
    question_answer_permission_id: Option<String>,
    question_prompt_tab: usize,
    question_prompt_selection: usize,
    question_prompt_answers: Vec<Vec<String>>,
    question_prompt_custom: Vec<String>,
    question_prompt_editing: bool,
    question_answer_buffer: String,
    question_answer_cursor: usize,
    question_answer_error: Option<String>,
    reload_requested: bool,
    compact_session_supported: bool,
    replay_navigation_handoff_enabled: bool,
    interrupt_confirm_deadline: Option<Instant>,
    interrupt_confirm_task_ids: BTreeSet<String>,
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
            toast: None,
            details_scroll: 0,
            transcript_scroll: 0,
            terminal_panel_scroll: 0,
            terminal_panel_follow: true,
            last_transcript_max_scroll: Cell::new(0),
            last_terminal_panel_max_scroll: Cell::new(0),
            last_frame_area: None,
            transcript_scrollbar_drag: None,
            selected_diff_hunk_row: None,
            hovered_transcript_target: None,
            hovered_subagent_footer_target: None,
            transcript_click_activated_on_down: false,
            transcript_selection: None,
            transcript_selection_dragging: false,
            operator_sidebar_selection: None,
            selected_operator_sidebar_keyboard_index: None,
            operator_sidebar_selection_dragging: false,
            operator_sidebar_pending_click: None,
            transcript_cache_instance_id: NEXT_TRANSCRIPT_CACHE_INSTANCE_ID
                .fetch_add(1, Ordering::Relaxed),
            transcript_render_epoch: 0,
            transcript_render_key_cache: Cell::new(None),
            transcript_animation_phase: 0,
            auto_exit_on_finish: false,
            prompt_buffer: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            prompt_history_index: None,
            prompt_history_path: None,
            prompt_history_draft: None,
            selected_activity_index: 0,
            palette_visible: false,
            palette_input: String::new(),
            palette_cursor: 0,
            palette_filtered: Vec::new(),
            palette_selected: 0,
            palette_focus_return: None,
            status_dialog_visible: false,
            show_transcript_thinking: true,
            show_transcript_timestamps: false,
            show_tool_details: true,
            show_generic_tool_output: false,
            terminal_panel_visible: false,
            stacked_transcript_diffs: false,
            expanded_tool_outputs: BTreeSet::new(),
            expanded_patch_file_outputs: BTreeSet::new(),
            collapsed_operator_sidebar_sections: BTreeSet::from([
                OperatorSidebarSection::ModifiedFiles,
            ]),
            expanded_operator_sidebar_subagent_groups: BTreeSet::new(),
            startup_mode: false,
            startup_launcher_action: StartupLauncherAction::default(),
            onboarding_visible: false,
            onboarding_step: OnboardingStep::StartSplash,
            onboarding_selected: 0,
            onboarding_skipped_for_launch: false,
            onboarding_auth_in_progress: false,
            onboarding_secret_input: String::new(),
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
            toggles_menu_visible: false,
            toggles_selected: 0,
            toggles_yolo_confirm_visible: false,
            runtime_toggles: toggles::RuntimeTogglesState::default(),
            lineage_browser: LineageBrowserState::default(),
            lineage_browser_visible: false,
            fork_selector: ForkSelectorState::default(),
            fork_selector_visible: false,
            slash_visible: false,
            slash_filtered: Vec::new(),
            slash_selected: 0,
            slash_draft_snapshot: None,
            file_mention_visible: false,
            file_mention_entries: Vec::new(),
            file_mention_selected: 0,
            file_mention_trigger: None,
            file_mention_workspace_root: None,
            file_mention_workspace_root_provider: Arc::new(system_file_mention_workspace_root),
            file_mention_scanner: Arc::new(SystemFileMentionWorkspaceScanner),
            file_mention_now_unix: Arc::new(system_file_mention_now_unix),
            workspace_context_labels: Vec::new(),
            file_mention_index: None,
            file_mention_tags: Vec::new(),
            file_mention_frecency: BTreeMap::new(),
            continue_disabled_banner: None,
            keymap: KeyMap::default(),
            theme: Theme::default(),
            launch_metadata: LaunchMetadata::default(),
            runtime_context_metadata: None,
            session_navigation_stack: Vec::new(),
            dismissed_permissions: BTreeSet::new(),
            submitted_permission_id: None,
            permission_modal_permission_id: None,
            permission_modal_stage: PermissionModalStage::default(),
            permission_modal_selection: PermissionModalSelection::default(),
            permission_modal_confirm_selection: PermissionConfirmSelection::default(),
            question_answer_permission_id: None,
            question_prompt_tab: 0,
            question_prompt_selection: 0,
            question_prompt_answers: Vec::new(),
            question_prompt_custom: Vec::new(),
            question_prompt_editing: false,
            question_answer_buffer: String::new(),
            question_answer_cursor: 0,
            question_answer_error: None,
            reload_requested: false,
            compact_session_supported: false,
            replay_navigation_handoff_enabled: false,
            interrupt_confirm_deadline: None,
            interrupt_confirm_task_ids: BTreeSet::new(),
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

impl AppState {
    fn launch_value_is_unknown(value: &str) -> bool {
        let trimmed = value.trim();
        trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown")
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn active_context_usage(&self) -> Option<ActiveContextUsage> {
        self.projection.active_context_usage
    }

    pub(crate) fn compaction_status(&self) -> Option<&CompactionStatus> {
        self.projection.compaction_status.as_ref()
    }

    pub(crate) fn compaction_usage_metrics(&self) -> CompactionUsageMetrics {
        self.projection.compaction_usage_metrics
    }

    pub fn new_live(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        Self::new_live_with_session_history(
            session_path,
            auto_exit_on_finish,
            on_ui_intent,
            Vec::new(),
        )
    }

    pub fn new_live_with_prompt_history_path(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
        prompt_history_path: Option<PathBuf>,
    ) -> Self {
        Self::new_live_with_session_history_and_prompt_history_path(
            session_path,
            auto_exit_on_finish,
            on_ui_intent,
            Vec::new(),
            prompt_history_path,
        )
    }

    pub fn new_live_with_session_history(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
        session_history_entries: Vec<SessionHistoryEntry>,
    ) -> Self {
        Self::new_live_with_session_history_and_prompt_history_path(
            session_path,
            auto_exit_on_finish,
            on_ui_intent,
            session_history_entries,
            None,
        )
    }

    pub fn new_live_with_session_history_and_prompt_history_path(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
        session_history_entries: Vec<SessionHistoryEntry>,
        prompt_history_path: Option<PathBuf>,
    ) -> Self {
        let mut state = Self::new();
        state.focus = Focus::Prompt;
        state.live_details_drawer_open = false;
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
        state.set_prompt_history_path(prompt_history_path);
        state.set_session_history_entries(session_history_entries);
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

    pub(crate) fn enable_replay_navigation_handoff(
        &mut self,
        on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync>,
    ) {
        self.replay_navigation_handoff_enabled = true;
        self.on_ui_intent = Some(on_ui_intent);
    }

    pub fn new_startup(
        session_history_entries: Vec<SessionHistoryEntry>,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        Self::new_startup_with_prompt_history_path(session_history_entries, on_ui_intent, None)
    }

    pub fn new_startup_with_prompt_history_path(
        session_history_entries: Vec<SessionHistoryEntry>,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
        prompt_history_path: Option<PathBuf>,
    ) -> Self {
        let mut state = Self::new();
        state.focus = Focus::List;
        state.startup_mode = true;
        state.on_ui_intent = on_ui_intent;
        state.set_prompt_history_path(prompt_history_path);
        if let Some(launch_metadata) = take_pending_live_launch_metadata() {
            state.set_launch_metadata(launch_metadata);
        }
        state.set_session_history_entries(session_history_entries);
        if let Some(pending_prompt) = take_pending_live_prompt() {
            state.replace_prompt_input(pending_prompt.text);
        }
        state
    }

    pub fn set_onboarding_required(&mut self, required: bool) {
        self.onboarding_visible = required && !self.onboarding_skipped_for_launch;
        if self.onboarding_visible {
            self.focus = Focus::List;
            self.onboarding_step = OnboardingStep::StartSplash;
            self.onboarding_selected = 0;
            self.onboarding_auth_in_progress = false;
            self.onboarding_secret_input.clear();
        }
    }

    pub fn onboarding_screen(&self) -> Option<OnboardingScreen> {
        self.onboarding_visible
            .then(|| onboarding::screen_for(self.onboarding_step, self.onboarding_selected))
    }

    pub fn set_onboarding_step_for_test(&mut self, step: OnboardingStep) {
        self.onboarding_visible = true;
        self.onboarding_step = step;
        self.onboarding_selected = 0;
        self.onboarding_auth_in_progress = false;
        self.onboarding_secret_input.clear();
        self.focus = Focus::List;
    }

    pub fn apply_auth_backend_result(&mut self, success: bool) {
        if !self.onboarding_visible || !self.onboarding_auth_in_progress {
            return;
        }
        self.onboarding_auth_in_progress = false;
        self.onboarding_secret_input.clear();
        self.onboarding_step = if success {
            OnboardingStep::LoginSuccess
        } else {
            OnboardingStep::LoginErrorTimeout
        };
        self.onboarding_selected = 0;
        self.focus = Focus::List;
    }

    fn onboarding_accepts_hidden_text(&self) -> bool {
        self.onboarding_visible
            && matches!(
                self.onboarding_step,
                OnboardingStep::ApiKeyEntry | OnboardingStep::CopilotEnterpriseDevice
            )
    }

    fn set_prompt_history_path(&mut self, path: Option<PathBuf>) {
        self.prompt_history_path = path;
        let Some(path) = self.prompt_history_path.as_deref() else {
            return;
        };
        match prompt_history::load_prompt_history(path) {
            Ok(history) => {
                self.prompt_history = history;
            }
            Err(err) => {
                self.status_banner = Some(err);
            }
        }
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
        self.bump_transcript_render_epoch();
    }

    #[cfg(test)]
    pub(crate) fn mark_transcript_dirty_for_test(&mut self) {
        self.bump_transcript_render_epoch();
    }

    pub fn replace_events(&mut self, events: Vec<EventEnvelopeV1>) {
        self.bump_transcript_render_epoch();
        self.projection.reset();
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_modal_permission_id = None;
        self.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_modal_selection = PermissionModalSelection::AllowOnce;
        self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt_tab = 0;
        self.question_prompt_selection = 0;
        self.question_prompt_answers.clear();
        self.question_prompt_custom.clear();
        self.question_prompt_editing = false;
        self.expanded_tool_outputs.clear();
        self.expanded_patch_file_outputs.clear();

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
        self.terminal_panel_scroll = 0;
        self.terminal_panel_follow = true;
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

        self.bump_transcript_render_epoch();

        if matches!(&event.payload, EventV1::PermissionRequested(_)) {
            self.close_palette();
            self.clear_slash_menu();
            self.clear_file_mention_menu();
        }

        if let EventV1::RunStarted(data) = &event.payload {
            self.file_mention_workspace_root = Some(PathBuf::from(&data.workspace_root));
            let environment = WorkspaceEnvironment::discover(&data.workspace_root);
            self.workspace_context_labels = workspace_context_labels(&environment);
            self.file_mention_index = None;
        }

        if matches!(&event.payload, EventV1::EditApplied(_)) {
            self.collapsed_operator_sidebar_sections
                .remove(&OperatorSidebarSection::ModifiedFiles);
            self.file_mention_index = None;
        }

        let terminal_panel_follow_event = terminal_panel_event_is_shell(&event.payload);

        let terminal_event = matches!(
            &event.payload,
            EventV1::RunFinished(_) | EventV1::RunFailed(_)
        );
        if !historical {
            self.continued_live_reopen_surface_active = false;
        }
        let completed_tool_call_id = match &event.payload {
            EventV1::ToolCallFinished(data) if data.status == ToolCallStatus::Succeeded => {
                Some(data.tool_call_id.clone())
            }
            _ => None,
        };
        self.update_transient_state_for_event(&event);
        let trimmed_events = self.projection.ingest_event(event, historical);
        self.selected_activity_index = self
            .selected_activity_index
            .min(self.projection.activities.len().saturating_sub(1));
        if let Some(tool_call_id) = completed_tool_call_id.as_deref() {
            self.seed_apply_patch_file_outputs_for_tool_call(tool_call_id);
        }
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

        if terminal_panel_follow_event && self.terminal_panel_follow {
            self.terminal_panel_scroll = 0;
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

    pub(crate) fn set_compact_session_supported(&mut self, supported: bool) {
        self.compact_session_supported = supported;
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
            latest_activity: self.runtime_state_activity(),
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

    pub(crate) fn startup_directory_branch_label(&self) -> String {
        directory_branch_label(&WorkspaceEnvironment::current(), false)
    }

    pub(crate) fn sidebar_directory_branch_label(&self) -> Option<&str> {
        if self.replay_mode {
            return None;
        }
        self.workspace_context_labels.last().map(String::as_str)
    }

    pub fn launch_mode_label(&self) -> Option<&str> {
        self.launch_metadata.mode_label()
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

    pub(crate) fn hovered_transcript_target(&self) -> Option<&TranscriptMouseTarget> {
        self.hovered_transcript_target.as_ref()
    }

    pub(crate) fn hovered_subagent_footer_target(&self) -> Option<SubagentFooterTarget> {
        self.hovered_subagent_footer_target
    }

    pub(crate) fn transcript_cache_instance_id(&self) -> u64 {
        self.transcript_cache_instance_id
    }

    pub(crate) fn transcript_render_cache_key(&self) -> u64 {
        let stamp = self.transcript_render_cache_stamp();
        if let Some((cached_stamp, cached_key)) = self.transcript_render_key_cache.get() {
            if cached_stamp == stamp {
                return cached_key;
            }
        }

        let key = self.compute_transcript_render_cache_key();
        self.transcript_render_key_cache.set(Some((stamp, key)));

        #[cfg(test)]
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(|count| count.set(count.get().saturating_add(1)));

        key
    }

    fn transcript_render_cache_stamp(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_render_settings(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        hasher.finish()
    }

    fn compute_transcript_render_cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_render_settings(&mut hasher);
        self.hash_transcript_content(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        for (permission_id, summary) in self.transcript_pending_permissions() {
            permission_id.hash(&mut hasher);
            summary.hash(&mut hasher);
        }

        hasher.finish()
    }

    fn hash_transcript_render_settings(&self, hasher: &mut impl Hasher) {
        self.replay_mode.hash(hasher);
        self.selected_activity_index.hash(hasher);
        self.show_transcript_thinking.hash(hasher);
        self.show_transcript_timestamps.hash(hasher);
        self.show_tool_details.hash(hasher);
        self.show_generic_tool_output.hash(hasher);
        self.stacked_transcript_diffs.hash(hasher);
        self.transcript_animation_phase.hash(hasher);
        self.hovered_transcript_target.hash(hasher);
        self.transcript_render_epoch.hash(hasher);
        self.active_profile().hash(hasher);
        self.session_path.hash(hasher);
    }

    fn hash_transcript_content(&self, hasher: &mut impl Hasher) {
        for activity in &self.activities {
            activity.request_id.hash(hasher);
            activity.profile_label.hash(hasher);
            activity.model_id.hash(hasher);
            activity.provider_id.hash(hasher);
            activity.status.hash(hasher);
            activity.user_timestamp.hash(hasher);
            activity.thinking_text.hash(hasher);
            activity.transcript_text.hash(hasher);
            activity.error_message.hash(hasher);
            activity.first_seq.hash(hasher);
            activity.last_seq.hash(hasher);

            if let Some(user_message) = activity.user_message.as_ref() {
                user_message.request_id.hash(hasher);
                user_message.text.hash(hasher);
            }

            for permission in &activity.permissions {
                permission.permission_id.hash(hasher);
                permission.kind.hash(hasher);
                permission.tool_call_id.hash(hasher);
                permission.summary.hash(hasher);
                permission.request_digest.hash(hasher);
                permission.timeout_ms.hash(hasher);
                std::mem::discriminant(&permission.default_decision).hash(hasher);
                permission.resolution_reason.hash(hasher);
                permission.first_seq.hash(hasher);
                permission.last_seq.hash(hasher);
            }

            for tool_call in &activity.tool_calls {
                tool_call.tool_call_id.hash(hasher);
                tool_call.tool_id.hash(hasher);
                tool_call.canonical_tool_id.hash(hasher);
                tool_call.alias_source_tool_id.hash(hasher);
                tool_call.args_digest.hash(hasher);
                tool_call.output_digest.hash(hasher);
                tool_call.output_summary.hash(hasher);
                tool_call.first_seq.hash(hasher);
                tool_call.last_seq.hash(hasher);
                std::mem::discriminant(&tool_call.status).hash(hasher);

                if let Some(edit) = tool_call.edit.as_ref() {
                    edit.edit_id.hash(hasher);
                    edit.path.hash(hasher);
                    std::mem::discriminant(&edit.status).hash(hasher);
                    edit.summary.hash(hasher);
                    edit.patch_digest.hash(hasher);
                    edit.new_file_digest.hash(hasher);
                    edit.diff_rel_path.hash(hasher);
                    edit.diff_digest.hash(hasher);
                    edit.rejection_reason.hash(hasher);
                }

                for artifact in &tool_call.artifact_refs {
                    artifact.path.hash(hasher);
                    artifact.digest.hash(hasher);
                }
            }
        }
    }

    fn hash_transcript_render_expansions(&self, hasher: &mut impl Hasher) {
        for tool_call_id in &self.expanded_tool_outputs {
            tool_call_id.hash(hasher);
        }
        for file_key in &self.expanded_patch_file_outputs {
            file_key.hash(hasher);
        }
    }

    #[cfg(test)]
    pub(crate) fn reset_transcript_render_key_metrics_for_test() {
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(|count| count.set(0));
    }

    #[cfg(test)]
    pub(crate) fn transcript_render_key_build_count_for_test() -> usize {
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(Cell::get)
    }

    pub(crate) fn advance_transcript_animation_phase(&mut self) {
        self.transcript_animation_phase = self.transcript_animation_phase.wrapping_add(1);
        self.clear_expired_interrupt_confirmation();
        if let Some(toast) = self.toast.as_mut() {
            toast.remaining_frames = toast.remaining_frames.saturating_sub(1);
            if toast.remaining_frames == 0 {
                self.toast = None;
            }
        }
    }

    pub(crate) fn has_active_animations(&self) -> bool {
        self.active_turn_in_progress()
            || self.toast.is_some()
            || self.interrupt_confirmation_pending()
    }

    fn bump_transcript_render_epoch(&mut self) {
        self.transcript_render_epoch = self.transcript_render_epoch.wrapping_add(1);
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
        self.expanded_tool_outputs.contains(&tool_call.tool_call_id)
    }

    pub(crate) fn patch_file_output_expanded(&self, tool_call_id: &str, file_path: &str) -> bool {
        self.expanded_patch_file_outputs
            .contains(&Self::patch_file_disclosure_key(tool_call_id, file_path))
    }

    fn patch_file_disclosure_key(tool_call_id: &str, file_path: &str) -> String {
        format!("{tool_call_id}\u{1f}{file_path}")
    }

    fn toggle_tool_output(&mut self, tool_call_id: &str) {
        if !self.expanded_tool_outputs.insert(tool_call_id.to_string()) {
            self.expanded_tool_outputs.remove(tool_call_id);
        }
    }

    fn set_tool_output_expanded(&mut self, tool_call_id: &str, expanded: bool) {
        if expanded {
            self.expanded_tool_outputs.insert(tool_call_id.to_string());
        } else {
            self.expanded_tool_outputs.remove(tool_call_id);
        }
    }

    fn toggle_patch_file_output(&mut self, tool_call_id: &str, file_path: &str) {
        let disclosure_key = Self::patch_file_disclosure_key(tool_call_id, file_path);
        if !self
            .expanded_patch_file_outputs
            .insert(disclosure_key.clone())
        {
            self.expanded_patch_file_outputs.remove(&disclosure_key);
        }
    }

    fn set_tool_group_outputs_expanded(&mut self, tool_call_ids: &[String], expanded: bool) {
        for tool_call_id in tool_call_ids {
            self.set_tool_output_expanded(tool_call_id, expanded);
        }
    }

    fn tool_call_entry(&self, tool_call_id: &str) -> Option<&ToolCallEntry> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .find(|tool_call| tool_call.tool_call_id == tool_call_id)
    }

    fn apply_patch_default_expanded_files(tool_call: &ToolCallEntry) -> Vec<String> {
        if tool_call.effective_tool_id() != "apply_patch" {
            return Vec::new();
        }

        let mut seen = BTreeSet::new();
        let mut files = Vec::new();

        if let Some(edits) = tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("edits"))
            .and_then(serde_json::Value::as_array)
        {
            for edit in edits {
                let Some(path) = edit
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_string)
                else {
                    continue;
                };
                let deleted = edit
                    .get("deleted")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if deleted || !seen.insert(path.clone()) {
                    continue;
                }
                files.push(path);
            }
        }

        if !files.is_empty() {
            return files;
        }

        let Some(rows) = tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("files"))
            .and_then(serde_json::Value::as_array)
        else {
            return files;
        };

        for row in rows {
            let Some(row) = row.as_str().map(str::trim).filter(|row| !row.is_empty()) else {
                continue;
            };
            let (status, path) = row
                .split_once(' ')
                .map(|(status, path)| (status.trim(), path.trim()))
                .filter(|(_, path)| !path.is_empty())
                .unwrap_or(("", row));
            if status.eq_ignore_ascii_case("D") {
                continue;
            }
            let path = path.to_string();
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }

        files
    }

    fn seed_apply_patch_file_outputs_for_tool_call(&mut self, tool_call_id: &str) {
        let files = self
            .tool_call_entry(tool_call_id)
            .map(Self::apply_patch_default_expanded_files)
            .unwrap_or_default();
        for file_path in files {
            self.expanded_patch_file_outputs
                .insert(Self::patch_file_disclosure_key(tool_call_id, &file_path));
        }
    }

    #[cfg(test)]
    pub(crate) fn set_patch_file_output_expanded_for_test(
        &mut self,
        tool_call_id: &str,
        file_path: &str,
        expanded: bool,
    ) {
        let disclosure_key = Self::patch_file_disclosure_key(tool_call_id, file_path);
        if expanded {
            self.expanded_patch_file_outputs.insert(disclosure_key);
        } else {
            self.expanded_patch_file_outputs.remove(&disclosure_key);
        }
    }

    fn activate_transcript_mouse_target(&mut self, target: TranscriptMouseTarget) {
        match target {
            TranscriptMouseTarget::FirstSubagentSession => {
                self.navigate_to_first_child_session();
            }
            TranscriptMouseTarget::SubagentSession { session_id } => {
                self.navigate_to_child_session_id(session_id);
            }
            TranscriptMouseTarget::Tool { tool_call_id } => {
                if let Some(child_session_id) = self.task_tool_child_session_id(&tool_call_id) {
                    self.navigate_to_child_session_id(child_session_id);
                    return;
                }
                if self
                    .tool_call_entry(&tool_call_id)
                    .is_some_and(Self::tool_call_is_task_spawn)
                {
                    self.set_status_banner(Some(
                        "subagent session is not available for this task yet".to_string(),
                    ));
                    return;
                }
                self.toggle_tool_output(&tool_call_id);
            }
            TranscriptMouseTarget::ToolGroup { tool_call_ids } => {
                let expand_group = tool_call_ids
                    .iter()
                    .any(|tool_call_id| !self.expanded_tool_outputs.contains(tool_call_id));
                self.set_tool_group_outputs_expanded(&tool_call_ids, expand_group);
            }
            TranscriptMouseTarget::PatchFile {
                tool_call_id,
                file_path,
            } => {
                self.toggle_patch_file_output(&tool_call_id, &file_path);
            }
        }
    }

    fn activate_subagent_footer_target(&mut self, target: SubagentFooterTarget) {
        match target {
            SubagentFooterTarget::Parent => self.navigate_to_parent_session(),
            SubagentFooterTarget::Previous => self.navigate_to_child_sibling(true),
            SubagentFooterTarget::Next => self.navigate_to_child_sibling(false),
        }
    }

    fn task_tool_child_session_id(&self, tool_call_id: &str) -> Option<String> {
        let tool_call = self.tool_call_entry(tool_call_id)?;
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        task_child_session_id_from_output(tool_call.output_json.as_ref())
            .or_else(|| {
                tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.clone())
            })
            .or_else(|| {
                self.transcript_task_row_for_tool_call(tool_call)
                    .and_then(|row| row.effective_child_session_id().map(str::to_string))
            })
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

    pub fn default_shell_registry(&self) -> &'static [ShellDescriptor] {
        default_shell_registry(self.replay_mode)
    }

    pub fn details_drawer_open(&self) -> bool {
        !self.replay_mode && self.active_tab == Tab::Run && self.live_details_drawer_open
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
            slash_visible: self.slash_overlay_should_render(),
            file_mention_visible: self.file_mention_overlay_should_render(),
            palette_visible: self.palette_visible,
            status_dialog_visible: self.status_dialog_visible,
            session_history_visible: self.session_history_visible,
            model_switcher_visible: self.model_switcher_visible,
            toggles_menu_visible: self.toggles_menu_visible,
            lineage_browser_visible: self.lineage_browser_visible,
            fork_selector_visible: self.fork_selector_visible,
            permission_pending: self.active_permission().is_some(),
        })
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

        if self.prompt_history_index.is_none() {
            self.prompt_history_draft = Some(PromptHistoryDraft {
                text: self.prompt_buffer.clone(),
                cursor: self.prompt_cursor,
            });
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

        self.restore_prompt_history_draft_or_clear();
    }

    fn restore_prompt_history_draft_or_clear(&mut self) {
        self.prompt_history_index = None;
        let Some(draft) = self.prompt_history_draft.take() else {
            self.clear_prompt_input();
            return;
        };
        self.prompt_buffer = draft.text;
        self.prompt_cursor = draft.cursor.min(self.prompt_char_count());
        self.clear_file_mention_tags();
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn clear_prompt_input(&mut self) {
        self.prompt_buffer.clear();
        self.prompt_cursor = 0;
        self.clear_file_mention_tags();
        self.prompt_history_index = None;
        self.prompt_history_draft = None;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn replace_prompt_input(&mut self, prompt: String) {
        self.prompt_cursor = prompt.chars().count();
        self.prompt_buffer = prompt;
        self.clear_file_mention_tags();
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
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
        self.adjust_file_mention_tags_for_insert(self.prompt_cursor, 1);
        self.prompt_buffer.insert(byte_idx, c);
        self.prompt_cursor += 1;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn insert_prompt_text(&mut self, text: &str) {
        for c in text.chars() {
            self.insert_prompt_char(c);
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) {
        if self.onboarding_accepts_hidden_text() && !self.onboarding_auth_in_progress {
            self.onboarding_secret_input.push_str(text.trim());
            return;
        }

        if self.composer_disabled() {
            return;
        }

        if self.startup_shell_visible() && self.focus != Focus::Prompt {
            self.focus = Focus::Prompt;
        }

        if self.focus != Focus::Prompt {
            return;
        }

        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_prompt_text(&normalized);
    }

    fn backspace_prompt_char(&mut self) {
        if self.prompt_cursor == 0 {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        self.prompt_cursor -= 1;
        self.adjust_file_mention_tags_for_delete(self.prompt_cursor, self.prompt_cursor + 1);
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn delete_prompt_char(&mut self) {
        if self.prompt_cursor >= self.prompt_char_count() {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        self.adjust_file_mention_tags_for_delete(self.prompt_cursor, self.prompt_cursor + 1);
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn active_turn_in_progress(&self) -> bool {
        self.activities
            .iter()
            .any(|activity| activity.status == ActivityStatus::Streaming)
    }

    fn active_interrupt_task_id(&self) -> Option<&str> {
        self.projection.active_turn_task_id()
    }

    fn active_interrupt_task_ids(&self) -> BTreeSet<String> {
        self.projection
            .active_turn_task_ids()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub(crate) fn interrupt_hint_visible(&self) -> bool {
        !self.replay_mode
            && !self.startup_shell_visible()
            && !self.composer_disabled()
            && !self.slash_visible
            && self.active_interrupt_task_id().is_some()
    }

    pub(crate) fn interrupt_confirmation_pending(&self) -> bool {
        let active_task_ids = self.active_interrupt_task_ids();
        self.interrupt_confirmation_pending_for(&active_task_ids)
    }

    fn interrupt_confirmation_pending_for(&self, active_task_ids: &BTreeSet<String>) -> bool {
        self.interrupt_confirm_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
            && !active_task_ids.is_empty()
            && self.interrupt_confirm_task_ids == *active_task_ids
    }

    fn clear_expired_interrupt_confirmation(&mut self) {
        if self
            .interrupt_confirm_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.reset_interrupt_confirmation();
        }
    }

    fn reset_interrupt_confirmation(&mut self) {
        self.interrupt_confirm_deadline = None;
        self.interrupt_confirm_task_ids.clear();
    }

    fn handle_interrupt_escape(&mut self) -> bool {
        let active_task_ids = self.active_interrupt_task_ids();
        if !self.interrupt_hint_visible() || active_task_ids.is_empty() {
            self.reset_interrupt_confirmation();
            return false;
        }

        if !self.interrupt_confirmation_pending_for(&active_task_ids) {
            self.interrupt_confirm_deadline = Some(Instant::now() + INTERRUPT_CONFIRM_TIMEOUT);
            self.interrupt_confirm_task_ids = active_task_ids;
            return true;
        }

        let task_ids = active_task_ids.into_iter().collect();

        self.emit_ui_intent(UiIntent::InterruptSession { task_ids });
        self.reset_interrupt_confirmation();
        true
    }

    fn runtime_state_activity(&self) -> Option<&ActivityEntry> {
        self.activities
            .iter()
            .rev()
            .find(|activity| activity.status == ActivityStatus::Streaming)
            .or_else(|| self.activities.back())
    }

    fn echo_submitted_prompt(&mut self, text: String, status: ActivityStatus) {
        let profile_label = self.active_profile().to_string();
        self.activities.push_back(ActivityEntry {
            request_id: String::new(),
            profile_label,
            model_id: String::new(),
            provider_id: String::new(),
            status,
            user_message: Some(UserMessageSubmittedEvent {
                request_id: String::new(),
                text,
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
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

    fn record_submitted_prompt_locally(&mut self, text: String) {
        let status = if self.active_turn_in_progress() {
            ActivityStatus::Queued
        } else {
            ActivityStatus::Streaming
        };
        if self.prompt_history.last() != Some(&text) {
            self.prompt_history.push(text.clone());
            self.save_prompt_history();
        }
        self.clear_prompt_input();
        self.echo_submitted_prompt(text.clone(), status);
    }

    fn save_prompt_history(&mut self) {
        let Some(path) = self.prompt_history_path.as_deref() else {
            return;
        };
        if let Err(err) = prompt_history::save_prompt_history(path, &self.prompt_history) {
            self.status_banner = Some(err);
        }
    }

    fn dispatch_submitted_prompt(&mut self, text: String) {
        if self.launch_metadata.model().is_none()
            && self.launch_metadata.provider() == "local"
            && self.launch_metadata.configured_profile().is_some()
        {
            self.status_banner = Some("Connect a provider to send prompts".to_string());
            self.show_toast(
                "Connect a provider to send prompts".to_string(),
                ToastVariant::Error,
            );
            return;
        }
        let selected_file_tags = self.selected_file_tags();
        let selected_agent_tags = self.selected_agent_tags();
        let selected_resource_tags = self.selected_resource_tags();
        self.record_submitted_prompt_locally(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt {
            text,
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            launch_metadata: self.launch_metadata.clone(),
        });
    }

    fn set_transcript_selection(
        &mut self,
        anchor: TranscriptSelectionCell,
        focus: TranscriptSelectionCell,
    ) {
        self.transcript_selection = Some(TranscriptSelection { anchor, focus });
    }

    fn clear_transcript_selection(&mut self) {
        self.transcript_selection = None;
        self.transcript_selection_dragging = false;
    }

    fn set_operator_sidebar_selection(
        &mut self,
        anchor: OperatorSidebarSelectionCell,
        focus: OperatorSidebarSelectionCell,
    ) {
        self.operator_sidebar_selection = Some(OperatorSidebarSelection { anchor, focus });
    }

    fn clear_operator_sidebar_selection(&mut self) {
        self.operator_sidebar_selection = None;
        self.operator_sidebar_selection_dragging = false;
        self.operator_sidebar_pending_click = None;
    }

    pub(crate) fn show_toast(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.toast = Some(ToastState {
            message: message.into(),
            variant,
            remaining_frames: 30,
        });
    }

    fn copy_transcript_selection(&mut self, frame_area: Rect) -> bool {
        let Some(selection) = self.transcript_selection else {
            return false;
        };
        let Some(text) = ui::transcript_selection_text(self, frame_area, selection) else {
            return false;
        };

        match clipboard::copy(&text) {
            Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
            Err(err) => {
                self.show_toast(format!("clipboard copy failed: {err}"), ToastVariant::Error)
            }
        }
        true
    }

    fn copy_operator_sidebar_selection(&mut self, frame_area: Rect) -> bool {
        let Some(selection) = self.operator_sidebar_selection else {
            return false;
        };
        let Some(text) = ui::operator_sidebar_selection_text(self, frame_area, selection) else {
            return false;
        };

        match clipboard::copy(&text) {
            Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
            Err(err) => {
                self.show_toast(format!("clipboard copy failed: {err}"), ToastVariant::Error)
            }
        }
        true
    }

    fn copy_active_selection(&mut self, frame_area: Rect) -> bool {
        self.copy_operator_sidebar_selection(frame_area)
            || self.copy_transcript_selection(frame_area)
    }

    fn maybe_clear_empty_transcript_selection(&mut self, frame_area: Rect) {
        if self
            .transcript_selection
            .and_then(|selection| ui::transcript_selection_text(self, frame_area, selection))
            .is_none()
        {
            self.clear_transcript_selection();
        }
    }

    fn operator_sidebar_selection_has_text(&self, frame_area: Rect) -> bool {
        self.operator_sidebar_selection
            .and_then(|selection| ui::operator_sidebar_selection_text(self, frame_area, selection))
            .is_some()
    }

    fn activate_operator_sidebar_pending_click(&mut self) -> bool {
        let Some(target) = self.operator_sidebar_pending_click.take() else {
            return false;
        };
        match target {
            OperatorSidebarPendingClick::Section(section) => {
                self.toggle_operator_sidebar_section(section)
            }
            OperatorSidebarPendingClick::SubagentGroup(agent_name) => {
                self.toggle_operator_sidebar_subagent_group(agent_name)
            }
            OperatorSidebarPendingClick::SubagentSession(session_id) => {
                self.navigate_to_child_session_id(session_id)
            }
        }
        true
    }

    fn operator_sidebar_keyboard_active(&self) -> bool {
        self.active_review_surface.is_none()
            && self.focus == Focus::List
            && !self.startup_shell_visible()
            && !self.post_run_handoff_visible()
            && self.session_shell_operator_rail_interactive()
    }

    fn operator_sidebar_keyboard_targets(&self) -> Vec<OperatorSidebarKeyboardTarget> {
        ui::operator_sidebar_keyboard_targets(self, self.last_frame_area)
    }

    fn selected_operator_sidebar_keyboard_target(
        &mut self,
    ) -> Option<OperatorSidebarKeyboardTargetKind> {
        let targets = self.operator_sidebar_keyboard_targets();
        if targets.is_empty() {
            self.selected_operator_sidebar_keyboard_index = None;
            return None;
        }

        let index = self
            .selected_operator_sidebar_keyboard_index
            .unwrap_or(0)
            .min(targets.len().saturating_sub(1));
        self.selected_operator_sidebar_keyboard_index = Some(index);
        targets.get(index).map(|target| target.kind.clone())
    }

    fn move_operator_sidebar_keyboard_selection(&mut self, reverse: bool) -> bool {
        let targets = self.operator_sidebar_keyboard_targets();
        if targets.is_empty() {
            self.selected_operator_sidebar_keyboard_index = None;
            return false;
        }

        let next = match self.selected_operator_sidebar_keyboard_index {
            Some(index) if reverse => index.saturating_sub(1),
            Some(index) => (index + 1).min(targets.len().saturating_sub(1)),
            None if reverse => targets.len().saturating_sub(1),
            None => 0,
        };
        self.selected_operator_sidebar_keyboard_index = Some(next);
        self.details_scroll = targets[next].top_row.min(usize::from(u16::MAX)) as u16;
        true
    }

    fn activate_operator_sidebar_keyboard_selection(&mut self) -> bool {
        let Some(target) = self.selected_operator_sidebar_keyboard_target() else {
            return false;
        };

        match target {
            OperatorSidebarKeyboardTargetKind::Section(section) => {
                self.toggle_operator_sidebar_section(section);
            }
            OperatorSidebarKeyboardTargetKind::SubagentGroup(agent_name) => {
                self.toggle_operator_sidebar_subagent_group(agent_name);
            }
            OperatorSidebarKeyboardTargetKind::SubagentSession(session_id) => {
                self.navigate_to_child_session_id(session_id);
            }
        }
        true
    }

    fn handle_operator_sidebar_action(&mut self, action: Action) -> bool {
        if !self.operator_sidebar_keyboard_active() {
            return false;
        }

        match action {
            Action::MoveDown | Action::HistoryDown => {
                self.move_operator_sidebar_keyboard_selection(false)
            }
            Action::MoveUp | Action::HistoryUp => {
                self.move_operator_sidebar_keyboard_selection(true)
            }
            Action::SubmitPrompt => self.activate_operator_sidebar_keyboard_selection(),
            _ => false,
        }
    }

    pub(crate) fn set_frame_area(&mut self, area: Rect) {
        self.last_frame_area = Some(area);
    }

    pub(crate) fn last_frame_area(&self) -> Option<Rect> {
        self.last_frame_area
    }

    pub(crate) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        frame_area: Rect,
        hovered_wheel_target: Option<WheelTarget>,
        clicked_operator_sidebar_section: Option<OperatorSidebarSection>,
        transcript_scrollbar_hit: Option<TranscriptScrollbarHit>,
    ) -> bool {
        if self.file_mention_visible {
            let mention_overlay =
                crate::layout::FrameLayoutPlan::for_app(self, frame_area).slash_overlay;
            if let Some(overlay) =
                mention_overlay.filter(|overlay| rect_contains(*overlay, mouse.column, mouse.row))
            {
                self.handle_file_mention_mouse(mouse, overlay);
                return true;
            }
        }

        if self.slash_visible {
            let slash_overlay =
                crate::layout::FrameLayoutPlan::for_app(self, frame_area).slash_overlay;
            if let Some(overlay) =
                slash_overlay.filter(|overlay| rect_contains(*overlay, mouse.column, mouse.row))
            {
                self.handle_slash_mouse(mouse, overlay);
                return true;
            }
        }

        if self.overlay_stack().blocks_pointer_interaction() {
            let changed = self.transcript_scrollbar_drag.is_some()
                || self.hovered_transcript_target.is_some()
                || self.hovered_subagent_footer_target.is_some()
                || self.transcript_selection.is_some()
                || self.operator_sidebar_selection.is_some();
            self.transcript_scrollbar_drag = None;
            self.hovered_transcript_target = None;
            self.hovered_subagent_footer_target = None;
            self.clear_transcript_selection();
            self.clear_operator_sidebar_selection();
            return changed;
        }

        self.set_frame_area(frame_area);

        match mouse.kind {
            MouseEventKind::Moved => {
                let hovered_transcript_target =
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row);
                let hovered_subagent_footer_target =
                    ui::subagent_footer_mouse_target(self, frame_area, mouse.column, mouse.row);
                let changed = self.hovered_transcript_target != hovered_transcript_target
                    || self.hovered_subagent_footer_target != hovered_subagent_footer_target;
                self.hovered_transcript_target = hovered_transcript_target;
                self.hovered_subagent_footer_target = hovered_subagent_footer_target;
                changed
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let copy_on_select_disabled = clipboard::copy_on_select_disabled();
                if copy_on_select_disabled && self.copy_active_selection(frame_area) {
                    self.clear_operator_sidebar_selection();
                    self.clear_transcript_selection();
                }
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.transcript_click_activated_on_down = false;
                self.hovered_transcript_target =
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row);
                self.hovered_subagent_footer_target =
                    ui::subagent_footer_mouse_target(self, frame_area, mouse.column, mouse.row);
                if let Some(scrollbar) = transcript_scrollbar_hit
                    .filter(|scrollbar| rect_contains(scrollbar.thumb, mouse.column, mouse.row))
                {
                    self.begin_transcript_scrollbar_drag(scrollbar, mouse.row);
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }

                self.transcript_scrollbar_drag = None;
                if let Some(target) = self.hovered_subagent_footer_target {
                    self.activate_subagent_footer_target(target);
                    self.transcript_click_activated_on_down = true;
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }
                if let Some(target) =
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                {
                    self.activate_transcript_mouse_target(target);
                    self.transcript_click_activated_on_down = true;
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }
                let transcript_hit =
                    ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                if let Some(cell) = transcript_hit {
                    self.set_transcript_selection(cell, cell);
                    self.transcript_selection_dragging = true;
                    self.clear_operator_sidebar_selection();
                    return true;
                }

                self.clear_transcript_selection();
                let operator_sidebar_session = ui::operator_sidebar_subagent_session_hit_target(
                    self,
                    frame_area,
                    mouse.column,
                    mouse.row,
                );
                let operator_sidebar_group = ui::operator_sidebar_subagent_group_hit_target(
                    self,
                    frame_area,
                    mouse.column,
                    mouse.row,
                );
                let operator_sidebar_cell =
                    ui::operator_sidebar_selection_cell(self, frame_area, mouse.column, mouse.row);
                if let Some(cell) = operator_sidebar_cell {
                    self.set_operator_sidebar_selection(cell, cell);
                    self.operator_sidebar_selection_dragging = true;
                    self.operator_sidebar_pending_click = operator_sidebar_session
                        .map(OperatorSidebarPendingClick::SubagentSession)
                        .or(operator_sidebar_group.map(OperatorSidebarPendingClick::SubagentGroup))
                        .or(clicked_operator_sidebar_section
                            .map(OperatorSidebarPendingClick::Section));
                    return true;
                }
                if let Some(agent_name) = operator_sidebar_group {
                    self.clear_operator_sidebar_selection();
                    self.toggle_operator_sidebar_subagent_group(agent_name);
                    return true;
                }
                if let Some(section) = clicked_operator_sidebar_section {
                    self.clear_operator_sidebar_selection();
                    self.toggle_operator_sidebar_section(section);
                }
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.transcript_scrollbar_drag.is_some() {
                    self.update_transcript_scrollbar_drag(mouse.row);
                    return true;
                }

                if self.transcript_selection_dragging {
                    let transcript_hit =
                        ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                    if let Some(cell) = transcript_hit {
                        if let Some(selection) = self.transcript_selection {
                            self.set_transcript_selection(selection.anchor, cell);
                        }
                    }
                    true
                } else if self.operator_sidebar_selection_dragging {
                    let sidebar_hit = ui::operator_sidebar_selection_cell(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    if let Some(cell) = sidebar_hit {
                        if let Some(selection) = self.operator_sidebar_selection {
                            self.set_operator_sidebar_selection(selection.anchor, cell);
                        }
                    }
                    true
                } else {
                    false
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let operator_sidebar_was_dragging = self.operator_sidebar_selection_dragging;
                if self.operator_sidebar_selection_dragging {
                    let sidebar_hit = ui::operator_sidebar_selection_cell(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    if let Some(cell) = sidebar_hit {
                        if let Some(selection) = self.operator_sidebar_selection {
                            self.set_operator_sidebar_selection(selection.anchor, cell);
                        }
                    }
                    self.operator_sidebar_selection_dragging = false;
                    let copy_on_select_disabled = clipboard::copy_on_select_disabled();
                    if copy_on_select_disabled {
                        if self.operator_sidebar_selection_has_text(frame_area) {
                            self.operator_sidebar_pending_click = None;
                        } else {
                            self.activate_operator_sidebar_pending_click();
                            self.clear_operator_sidebar_selection();
                        }
                    } else {
                        let copied = self.copy_operator_sidebar_selection(frame_area);
                        if copied {
                            self.clear_operator_sidebar_selection();
                        } else {
                            self.activate_operator_sidebar_pending_click();
                            self.clear_operator_sidebar_selection();
                        }
                    }
                }
                if self.transcript_selection_dragging {
                    let transcript_hit =
                        ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                    if let Some(cell) = transcript_hit {
                        if let Some(selection) = self.transcript_selection {
                            self.set_transcript_selection(selection.anchor, cell);
                        }
                    }
                    self.transcript_selection_dragging = false;
                    let copy_on_select_disabled = clipboard::copy_on_select_disabled();
                    if copy_on_select_disabled {
                        self.maybe_clear_empty_transcript_selection(frame_area);
                    } else {
                        let copied = self.copy_transcript_selection(frame_area);
                        self.clear_transcript_selection();
                        if !copied {
                            self.clear_transcript_selection();
                        }
                    }
                }
                if self.transcript_click_activated_on_down {
                    self.transcript_click_activated_on_down = false;
                    self.transcript_scrollbar_drag = None;
                    return true;
                }
                if operator_sidebar_was_dragging {
                    self.transcript_scrollbar_drag = None;
                    return true;
                }
                if self.transcript_scrollbar_drag.is_none() {
                    if let Some(target) =
                        ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                    {
                        self.activate_transcript_mouse_target(target);
                        self.clear_transcript_selection();
                        return true;
                    }
                }
                self.transcript_scrollbar_drag = None;
                true
            }
            MouseEventKind::ScrollUp => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => {
                    self.scroll_transcript_up(3);
                    true
                }
                Some(WheelTarget::Terminal) => {
                    self.scroll_terminal_panel_up(3);
                    true
                }
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self.details_scroll.saturating_sub(3);
                    true
                }
                None => false,
            },
            MouseEventKind::ScrollDown => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => {
                    self.scroll_transcript_down(3);
                    true
                }
                Some(WheelTarget::Terminal) => {
                    self.scroll_terminal_panel_down(3);
                    true
                }
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self.details_scroll.saturating_add(3);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    pub(crate) fn operator_sidebar_section_collapsed(
        &self,
        section: OperatorSidebarSection,
    ) -> bool {
        self.collapsed_operator_sidebar_sections.contains(&section)
    }

    pub(crate) fn operator_sidebar_subagent_group_expanded(&self, agent_name: &str) -> bool {
        self.expanded_operator_sidebar_subagent_groups
            .contains(agent_name)
    }

    pub(crate) fn transcript_scrollbar_dragging(&self) -> bool {
        self.transcript_scrollbar_drag.is_some()
    }

    pub(crate) fn transcript_selection(&self) -> Option<TranscriptSelection> {
        self.transcript_selection
    }

    pub(crate) fn operator_sidebar_selection(&self) -> Option<OperatorSidebarSelection> {
        self.operator_sidebar_selection
    }

    pub(crate) fn selected_operator_sidebar_keyboard_index(&self) -> Option<usize> {
        self.selected_operator_sidebar_keyboard_index
    }

    #[cfg(test)]
    pub(crate) fn selected_operator_sidebar_keyboard_index_for_test(&self) -> Option<usize> {
        self.selected_operator_sidebar_keyboard_index
    }

    pub(crate) fn toast(&self) -> Option<&ToastState> {
        self.toast.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_toast_for_test(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.show_toast(message, variant);
    }

    fn toggle_operator_sidebar_section(&mut self, section: OperatorSidebarSection) {
        if !self.collapsed_operator_sidebar_sections.insert(section) {
            self.collapsed_operator_sidebar_sections.remove(&section);
        }
        self.details_scroll = 0;
    }

    fn toggle_operator_sidebar_subagent_group(&mut self, agent_name: String) {
        if !self
            .expanded_operator_sidebar_subagent_groups
            .insert(agent_name.clone())
        {
            self.expanded_operator_sidebar_subagent_groups
                .remove(&agent_name);
        }
        self.details_scroll = 0;
    }

    fn begin_transcript_scrollbar_drag(
        &mut self,
        scrollbar: TranscriptScrollbarHit,
        pointer_row: u16,
    ) {
        let pointer_offset_y = pointer_row
            .saturating_sub(scrollbar.thumb.y)
            .min(scrollbar.thumb.height.saturating_sub(1));
        self.transcript_scrollbar_drag = Some(TranscriptScrollbarDragState {
            track: scrollbar.track,
            thumb_height: scrollbar.thumb.height,
            pointer_offset_y,
            max_scroll: scrollbar.max_scroll,
        });
    }

    fn update_transcript_scrollbar_drag(&mut self, pointer_row: u16) {
        let Some(drag) = self.transcript_scrollbar_drag else {
            return;
        };

        let max_thumb_top = drag.track.height.saturating_sub(drag.thumb_height);
        let desired_thumb_top = pointer_row
            .saturating_sub(drag.pointer_offset_y)
            .clamp(drag.track.y, drag.track.y.saturating_add(max_thumb_top));
        let thumb_top = desired_thumb_top.saturating_sub(drag.track.y);
        let scroll_top = if drag.max_scroll == 0 || max_thumb_top == 0 {
            drag.max_scroll
        } else {
            ((usize::from(thumb_top) * drag.max_scroll) + usize::from(max_thumb_top) / 2)
                / usize::from(max_thumb_top)
        };
        self.set_transcript_scroll_from_top_with_max(
            scroll_top.min(drag.max_scroll),
            drag.max_scroll,
        );
    }

    fn set_transcript_scroll_from_top_with_max(&mut self, scroll_top: usize, max_scroll: usize) {
        let clamped = scroll_top.min(max_scroll);
        if max_scroll == 0 || clamped >= max_scroll {
            self.follow_mode = true;
            self.transcript_scroll = 0;
            return;
        }

        self.follow_mode = false;
        self.transcript_scroll = max_scroll.saturating_sub(clamped);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.onboarding_accepts_hidden_text()
            && !self.onboarding_auth_in_progress
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char(c) => {
                    self.execute_action(Action::Char(c));
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Backspace => {
                    self.execute_action(Action::Backspace);
                    self.maybe_auto_exit();
                    return;
                }
                _ => {}
            }
        }

        if self.overlay_stack().top() == Some(OverlayKind::PermissionModal) {
            self.handle_permission_modal_key(key);
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::StatusDialog) {
            if key.code == KeyCode::Esc {
                self.status_dialog_visible = false;
            }
            self.maybe_auto_exit();
            return;
        }

        if clipboard::copy_on_select_disabled()
            && (self.transcript_selection.is_some() || self.operator_sidebar_selection.is_some())
        {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            {
                if let Some(frame_area) = self.last_frame_area() {
                    if !self.copy_active_selection(frame_area) {
                        self.clear_operator_sidebar_selection();
                        self.clear_transcript_selection();
                        return;
                    }
                }
                self.clear_operator_sidebar_selection();
                self.clear_transcript_selection();
                self.maybe_auto_exit();
                return;
            }

            if key.code == KeyCode::Esc {
                self.clear_operator_sidebar_selection();
                self.clear_transcript_selection();
                self.maybe_auto_exit();
                return;
            }

            self.clear_operator_sidebar_selection();
            self.clear_transcript_selection();
        }

        if self.handle_navigation_overlay_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.active_review_surface.is_some() && key.code == KeyCode::Esc {
            self.close_review_surface();
            self.maybe_auto_exit();
            return;
        }

        if self.replay_mode && key.code == KeyCode::Esc && !self.session_navigation_stack.is_empty()
        {
            self.navigate_to_parent_session();
            self.maybe_auto_exit();
            return;
        }

        if key.code == KeyCode::Esc && self.handle_interrupt_escape() {
            self.maybe_auto_exit();
            return;
        }

        if self.focus != Focus::Prompt && self.handle_transcript_navigation_key(key) {
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
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
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
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
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

    fn overlay_delete(&mut self, on_change: fn(&mut Self)) {
        if self.palette_cursor >= self.palette_input.chars().count() {
            return;
        }

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

    fn close_review_surface(&mut self) {
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        self.normalize_focus_for_active_surface();
    }

    fn handle_slash_mouse(&mut self, mouse: MouseEvent, overlay: Rect) {
        let list_area = crate::layout::slash_command_overlay_content_area(overlay);
        if self.slash_filtered.is_empty()
            || list_area.height == 0
            || !rect_contains(list_area, mouse.column, mouse.row)
        {
            return;
        }

        let visible_rows = usize::from(list_area.height);
        let selected = self
            .slash_selected
            .min(self.slash_filtered.len().saturating_sub(1));
        let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        let row = usize::from(mouse.row.saturating_sub(list_area.y));
        let Some(next) = scroll
            .checked_add(row)
            .filter(|index| *index < self.slash_filtered.len())
        else {
            return;
        };

        match mouse.kind {
            MouseEventKind::Moved
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Down(MouseButton::Left) => {
                self.slash_selected = next;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.slash_selected = next;
                self.apply_selected_slash_completion();
            }
            _ => {}
        }
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
            } else if (self.focus == Focus::Terminal && !self.terminal_panel_visible())
                || (self.active_review_surface.is_none()
                    && !self.session_shell_operator_rail_interactive()
                    && self.focus == Focus::List)
            {
                self.focus = Focus::Details;
            }
            return;
        }

        if self.post_run_handoff_visible() {
            if matches!(self.focus, Focus::Prompt | Focus::Terminal) || self.active_tab == Tab::Run
            {
                self.focus = Focus::List;
            }
            return;
        }

        if self.active_review_surface.is_some() && self.focus == Focus::Prompt {
            self.focus = Focus::List;
        } else if (self.active_review_surface.is_some() && self.focus == Focus::Terminal)
            || (self.active_review_surface.is_none()
                && !self.startup_shell_visible()
                && !self.session_shell_operator_rail_interactive()
                && self.focus == Focus::List)
        {
            self.focus = Focus::Details;
        }
    }

    fn cycle_focus_forward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = if self.focus == Focus::Details && self.terminal_panel_visible() {
                    Focus::Terminal
                } else {
                    Focus::Details
                };
                return;
            }

            self.focus = match self.focus {
                Focus::List => Focus::Details,
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::Prompt => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
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
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::List => Focus::Prompt,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details => Focus::List,
                Focus::List => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::Prompt => Focus::Details,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Details,
                Focus::Details | Focus::Terminal => Focus::Prompt,
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
                self.focus = if self.focus == Focus::Terminal {
                    Focus::Details
                } else if self.terminal_panel_visible() {
                    Focus::Terminal
                } else {
                    Focus::Details
                };
                return;
            }

            self.focus = match self.focus {
                Focus::List | Focus::Prompt => {
                    if self.terminal_panel_visible() {
                        Focus::Terminal
                    } else {
                        Focus::Details
                    }
                }
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
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
                Focus::Prompt if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Prompt => Focus::Details,
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::List => Focus::Details,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Details => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::List => Focus::Details,
                Focus::Prompt => Focus::List,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Prompt,
                Focus::Details | Focus::Terminal => Focus::List,
                Focus::Prompt => Focus::Details,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }

    fn move_onboarding_selection(&mut self, delta: isize) {
        let len = onboarding::screen_for(self.onboarding_step, self.onboarding_selected)
            .choices
            .len();
        if len == 0 {
            self.onboarding_selected = 0;
            return;
        }
        self.onboarding_selected = if delta < 0 {
            if self.onboarding_selected == 0 {
                len - 1
            } else {
                self.onboarding_selected - 1
            }
        } else {
            (self.onboarding_selected + 1) % len
        };
    }

    fn request_onboarding_auth(&mut self, args: Vec<String>, stdin: Option<String>) {
        self.onboarding_auth_in_progress = true;
        self.status_banner = Some(auth_status_banner(&args));
        self.emit_ui_intent(UiIntent::OpenAuthManager { args, stdin });
    }

    fn execute_onboarding_auth_step(&mut self) {
        match self.onboarding_step {
            OnboardingStep::CodexBrowser => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "browser".to_string(),
                ],
                None,
            ),
            OnboardingStep::CodexDevice => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "device".to_string(),
                ],
                None,
            ),
            OnboardingStep::CopilotPublicDevice => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "github-copilot".to_string(),
                    "--method".to_string(),
                    "device".to_string(),
                ],
                None,
            ),
            OnboardingStep::CopilotEnterpriseDevice => {
                let enterprise_url = self.onboarding_secret_input.trim().to_string();
                if enterprise_url.is_empty() {
                    self.status_banner =
                        Some("enterprise login requires a domain; input stays hidden".to_string());
                    return;
                }
                self.onboarding_secret_input.clear();
                self.request_onboarding_auth(
                    vec![
                        "login".to_string(),
                        "github-copilot".to_string(),
                        "--method".to_string(),
                        "device".to_string(),
                        "--enterprise-url".to_string(),
                        enterprise_url,
                    ],
                    None,
                );
            }
            OnboardingStep::ApiKeyEntry => {
                let secret = self.onboarding_secret_input.trim().to_string();
                if secret.is_empty() {
                    self.status_banner =
                        Some("api-key login requires a pasted key; input stays hidden".to_string());
                    return;
                }
                self.onboarding_secret_input.clear();
                self.request_onboarding_auth(
                    vec![
                        "login".to_string(),
                        "codex".to_string(),
                        "--method".to_string(),
                        "api-key".to_string(),
                        "--api-key-stdin".to_string(),
                    ],
                    Some(secret),
                );
            }
            _ => {}
        }
    }

    fn execute_onboarding_selection(&mut self) {
        if self.onboarding_auth_in_progress {
            self.status_banner = Some("auth backend already running".to_string());
            return;
        }
        match self.onboarding_step {
            OnboardingStep::StartSplash if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::SkipConfirmation;
                self.onboarding_selected = 0;
            }
            OnboardingStep::ProviderPick if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::CopilotTargetPick;
                self.onboarding_selected = 0;
            }
            OnboardingStep::CopilotTargetPick => {
                self.onboarding_step = if self.onboarding_selected == 1 {
                    OnboardingStep::CopilotEnterpriseDevice
                } else {
                    OnboardingStep::CopilotPublicDevice
                };
                self.onboarding_selected = 0;
            }
            OnboardingStep::AuthMethodPick => {
                self.onboarding_step = match self.onboarding_selected {
                    1 => OnboardingStep::CodexBrowser,
                    2 => OnboardingStep::ApiKeyEntry,
                    _ => OnboardingStep::CodexDevice,
                };
                self.onboarding_selected = 0;
            }
            OnboardingStep::LoginErrorTimeout if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::SkipConfirmation;
                self.onboarding_selected = 0;
            }
            OnboardingStep::CodexBrowser
            | OnboardingStep::CodexDevice
            | OnboardingStep::CopilotPublicDevice
            | OnboardingStep::CopilotEnterpriseDevice
            | OnboardingStep::ApiKeyEntry => {
                self.execute_onboarding_auth_step();
            }
            OnboardingStep::SkipConfirmation if self.onboarding_selected == 0 => {
                self.onboarding_visible = false;
                self.onboarding_skipped_for_launch = true;
                self.status_banner = Some(
                    "onboarding skipped for this launch; no credential was written".to_string(),
                );
            }
            OnboardingStep::FirstPromptSuccess => {
                self.onboarding_visible = false;
                self.apply_new_session_launcher_selection();
            }
            _ => {
                self.onboarding_step = self.onboarding_step.next();
                self.onboarding_selected = 0;
            }
        }
    }

    fn handle_onboarding_text_action(&mut self, action: Action) -> bool {
        if !self.onboarding_visible
            || self.onboarding_auth_in_progress
            || !matches!(
                self.onboarding_step,
                OnboardingStep::ApiKeyEntry | OnboardingStep::CopilotEnterpriseDevice
            )
        {
            return false;
        }

        match action {
            Action::Char(c) => {
                if !c.is_control() {
                    self.onboarding_secret_input.push(c);
                }
                true
            }
            Action::Backspace => {
                self.onboarding_secret_input.pop();
                true
            }
            Action::ClearPrompt => {
                self.onboarding_secret_input.clear();
                true
            }
            _ => false,
        }
    }

    fn execute_action(&mut self, action: Action) {
        if self.execute_permission_action(action) {
            return;
        }

        if self.handle_onboarding_text_action(action) {
            return;
        }

        if self.onboarding_visible && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_onboarding_selection();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.move_onboarding_selection(-1);
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.move_onboarding_selection(1);
                    return;
                }
                Action::DismissModal => {
                    self.onboarding_step = OnboardingStep::SkipConfirmation;
                    self.onboarding_selected = 0;
                    return;
                }
                _ => {}
            }
        }

        if self.handle_operator_sidebar_action(action) {
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

        if matches!(action, Action::ToggleTerminalPanel) && !self.startup_shell_visible() {
            self.toggle_terminal_panel();
            return;
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
                        self.sync_file_mention_overlay();
                        return;
                    }

                    if self.prompt_cursor_at_start() {
                        self.select_previous_prompt_history();
                    }
                    return;
                }
                Action::HistoryDown => {
                    if self.move_prompt_cursor_down() {
                        self.sync_file_mention_overlay();
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
                    self.sync_file_mention_overlay();
                    return;
                }
                Action::CursorRight => {
                    if self.prompt_cursor < self.prompt_char_count() {
                        self.prompt_cursor += 1;
                    }
                    self.sync_file_mention_overlay();
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
                self.restore_parent_session_for_quit();
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
                if !self.replay_mode && !self.post_run_handoff_visible() =>
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
            Action::DiffHunkNext => {
                self.navigate_diff_hunk(false);
            }
            Action::DiffHunkPrevious => {
                self.navigate_diff_hunk(true);
            }
            Action::AgentCycle => {
                self.cycle_agent(false);
            }
            Action::AgentCycleReverse => {
                self.cycle_agent(true);
            }
            Action::VariantCycle => {
                self.cycle_variant();
            }
            Action::MoveDown if self.focus != Focus::Prompt => {
                if self.active_review_surface.is_none() && self.focus == Focus::List {
                    self.next_activity();
                } else if self.focus == Focus::List {
                    self.next_event();
                } else if self.focus == Focus::Terminal {
                    self.scroll_terminal_panel_down(1);
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
                } else if self.focus == Focus::Terminal {
                    self.scroll_terminal_panel_up(1);
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

    fn handle_transcript_navigation_key(&mut self, key: KeyEvent) -> bool {
        if self.terminal_panel_surface_active() && key.modifiers == KeyModifiers::NONE {
            return match key.code {
                KeyCode::PageUp => {
                    self.scroll_terminal_panel_up(10);
                    true
                }
                KeyCode::PageDown => {
                    self.scroll_terminal_panel_down(10);
                    true
                }
                KeyCode::Home => {
                    self.terminal_panel_follow = false;
                    self.terminal_panel_scroll = self.last_terminal_panel_max_scroll.get();
                    true
                }
                KeyCode::End => {
                    self.terminal_panel_follow = true;
                    self.terminal_panel_scroll = 0;
                    true
                }
                _ => false,
            };
        }

        if !self.transcript_surface_active() || key.modifiers != KeyModifiers::NONE {
            return false;
        }

        match key.code {
            KeyCode::PageUp => {
                self.scroll_transcript_up(10);
                true
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(10);
                true
            }
            KeyCode::Home => {
                self.follow_mode = false;
                self.transcript_scroll = self.last_transcript_max_scroll.get();
                true
            }
            KeyCode::End => {
                self.follow_mode = true;
                self.transcript_scroll = 0;
                true
            }
            _ => false,
        }
    }

    fn navigate_diff_hunk(&mut self, reverse: bool) -> bool {
        let Some(frame_area) = self.last_frame_area else {
            return false;
        };
        let hunk_rows = ui::transcript_diff_hunk_rows(self, frame_area);
        if hunk_rows.is_empty() {
            return false;
        }

        let max_scroll = self.last_transcript_max_scroll.get();
        let current_top = if self.follow_mode {
            max_scroll
        } else {
            max_scroll
                .saturating_sub(self.transcript_scroll)
                .min(max_scroll)
        };
        let anchor = self.selected_diff_hunk_row.unwrap_or(current_top);
        let target = if reverse {
            hunk_rows
                .iter()
                .rev()
                .copied()
                .find(|row| *row < anchor)
                .unwrap_or_else(|| hunk_rows[0])
        } else {
            hunk_rows
                .iter()
                .copied()
                .find(|row| *row > anchor)
                .unwrap_or_else(|| *hunk_rows.last().expect("non-empty hunk rows"))
        };

        self.selected_diff_hunk_row = Some(target);
        self.follow_mode = false;
        let target_top = target.min(max_scroll);
        self.transcript_scroll = max_scroll.saturating_sub(target_top);
        true
    }

    #[cfg(test)]
    pub(crate) fn selected_diff_hunk_row_for_test(&self) -> Option<usize> {
        self.selected_diff_hunk_row
    }

    fn scroll_transcript_up(&mut self, amount: u16) {
        self.follow_mode = false;
        self.transcript_scroll = self
            .transcript_scroll
            .saturating_add(usize::from(amount.max(1)));
    }

    fn scroll_transcript_down(&mut self, amount: u16) {
        self.transcript_scroll = self
            .transcript_scroll
            .saturating_sub(usize::from(amount.max(1)));
        if self.transcript_scroll == 0 {
            self.follow_mode = true;
        }
    }

    fn submit_prompt(&mut self) {
        if !self.replay_mode && !self.composer_disabled() {
            if let Some(command) = self.typed_slash_command() {
                self.execute_slash_command(command, self.slash_draft_snapshot.clone());
                return;
            }
        }

        if self.prompt_buffer.trim().is_empty() || self.composer_disabled() || self.replay_mode {
            return;
        }

        if self.startup_mode {
            let text = self.prompt_buffer.clone();
            set_pending_live_launch_metadata(self.launch_metadata.clone());
            set_pending_live_prompt_auto_submit(Some(text.clone()));
            self.startup_mode = false;
            self.focus = Focus::Prompt;
            self.record_submitted_prompt_locally(text);
            self.emit_ui_intent(UiIntent::NewSession);
            self.should_quit = true;
            return;
        }

        let text = self.prompt_buffer.clone();
        self.dispatch_submitted_prompt(text);
    }

    fn update_transient_state_for_event(&mut self, event: &EventEnvelopeV1) {
        if let EventV1::PermissionResolved(data) = &event.payload {
            self.dismissed_permissions.remove(&data.permission_id);
            self.clear_permission_modal_selection(&data.permission_id);
            if self.submitted_permission_is_active(&data.permission_id) {
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

fn action_preempts_text_input(action: Action, key: KeyEvent) -> bool {
    matches!(
        action,
        Action::SessionChildCycle | Action::SessionChildCycleReverse | Action::ToggleTerminalPanel
    ) || matches!(
        (action, key.code, key.modifiers),
        (
            Action::ToggleOperatorSidebar,
            KeyCode::Char('2'),
            KeyModifiers::NONE
        )
    )
}

fn json_string_field(output_json: Option<&serde_json::Value>, keys: &[&str]) -> Option<String> {
    trimmed_json_string_field(output_json, keys)
}

fn task_child_session_id_from_output(output_json: Option<&serde_json::Value>) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &[
            "child_session_id",
            "session_id",
            "task_id",
            "childSessionId",
            "sessionId",
            "taskId",
        ],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["child_session", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["childSession", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "sessionId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "sessionId"])
    })
}

fn task_child_request_id_from_output(output_json: Option<&serde_json::Value>) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &[
            "child_request_id",
            "request_id",
            "childRequestId",
            "requestId",
        ],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["child_session", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["childSession", "requestId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "requestId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "requestId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "requestId"])
    })
}

fn tool_call_has_expandable_output(tool_call: &ToolCallEntry) -> bool {
    if matches!(
        tool_call.effective_tool_id(),
        "fs.read" | "read" | "fs.glob" | "glob" | "fs.grep" | "grep" | "fs.ls" | "list"
    ) {
        return true;
    }

    if matches!(tool_call.effective_tool_id(), "shell.run" | "bash")
        && shell_tool_output_for_expansion(tool_call).is_some_and(has_trimmed_content)
    {
        return true;
    }

    if matches!(tool_call.effective_tool_id(), "edit" | "fs.write")
        && serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_some_and(|object| {
                let path = object
                    .get("filePath")
                    .or_else(|| object.get("path"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(has_trimmed_content);
                let inline_preview = match tool_call.effective_tool_id() {
                    "edit" => {
                        object
                            .get("oldString")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                            || object
                                .get("newString")
                                .and_then(serde_json::Value::as_str)
                                .is_some()
                    }
                    "fs.write" => object
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .is_some(),
                    _ => false,
                };
                path && inline_preview
            })
    {
        return true;
    }

    if tool_call.effective_tool_id() == "apply_patch"
        && tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("files"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|files| !files.is_empty())
    {
        return true;
    }

    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && tool_call.effective_tool_id().starts_with("mcp.")
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(has_trimmed_content)
    {
        return true;
    }

    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let line_count = output.lines().count();
    let has_diff_preview = tool_call
        .edit
        .as_ref()
        .and_then(|edit| edit.diff_rel_path.as_ref())
        .is_some()
        || tool_call
            .artifact_refs
            .iter()
            .any(|artifact| artifact.path.ends_with(".diff"));
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" | "bash" => line_count > 10,
            "edit.hashline_apply" | "fs.write" | "edit" | "apply_patch" => has_diff_preview,
            "agent.spawn" => true,
            _ => has_trimmed_content(output) && line_count > 3,
        }
}

fn shell_tool_output_for_expansion(tool_call: &ToolCallEntry) -> Option<&str> {
    tool_call.output_summary.as_deref().or_else(|| {
        let output_json = tool_call.output_json.as_ref()?;
        output_json
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .filter(|stdout| has_trimmed_content(stdout))
            .or_else(|| {
                output_json
                    .get("stderr")
                    .and_then(serde_json::Value::as_str)
                    .filter(|stderr| has_trimmed_content(stderr))
            })
    })
}

#[cfg(test)]
pub(crate) fn exact_test_startup_slash_commands_execute_without_menu() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(app.slash_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::SlashCommands));
    assert_eq!(
        app.slash_filtered,
        vec![
            "auth".to_string(),
            "exit".to_string(),
            "help".to_string(),
            "new".to_string(),
            "replay".to_string(),
            "resume".to_string(),
            "status".to_string(),
            "toggles".to_string(),
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
pub(crate) fn exact_test_slash_resume_opens_history_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.prompt_buffer = "/resume".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("resume this draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.palette_visible);
    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ContinueSession
    );
    assert_eq!(app.prompt_buffer, "resume this draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_events_opens_review_surface() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.prompt_buffer = "/events".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("keep events draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, Some(ReviewSurface::Events));
    assert_eq!(app.prompt_buffer, "keep events draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_status_opens_status_dialog_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.prompt_buffer = "/status".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("status draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.status_dialog_visible);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));
    assert_eq!(app.prompt_buffer, "status draft");
    assert!(!app.slash_visible);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.status_dialog_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_shell_closes_review_surface() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.open_review_surface(ReviewSurface::Events);
    app.focus = Focus::Prompt;
    app.prompt_buffer = "/shell".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("back to shell".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, None);
    assert_eq!(app.prompt_buffer, "back to shell");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_follow_toggles_follow_mode() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.follow_mode = false;
    app.transcript_scroll = 12;
    app.prompt_buffer = "/follow".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("follow draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.follow_mode);
    assert_eq!(app.transcript_scroll, 0);
    assert_eq!(app.prompt_buffer, "follow draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_live_slash_compact_emits_ui_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.set_compact_session_supported(true);
    app.prompt_buffer = "/compact".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("compact draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.prompt_buffer, "compact draft");
    assert!(!app.slash_visible);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::CompactSession]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_auth_slash_and_palette_emit_ui_intent_mid_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut slash = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(sink.clone()),
    );
    slash.prompt_buffer = "/login codex --method device".to_string();
    slash.prompt_cursor = slash.prompt_buffer.chars().count();
    slash.slash_draft_snapshot = Some("draft after auth".to_string());
    slash.sync_slash_overlay();

    slash.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(slash.prompt_buffer, "draft after auth");
    assert!(!slash.slash_visible);
    assert_eq!(
        slash.status_banner.as_deref(),
        Some("auth backend requested: harness auth login codex --method device")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "device".to_string()
            ],
            stdin: None,
        }]
    );

    let mut palette = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    palette.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "auth".chars() {
        palette.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    palette.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!palette.palette_visible);
    assert_eq!(
        palette.status_banner.as_deref(),
        Some("auth backend requested: harness auth list")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::OpenAuthManager {
                args: vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "device".to_string()
                ],
                stdin: None,
            },
            UiIntent::OpenAuthManager {
                args: vec!["list".to_string()],
                stdin: None,
            }
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_inventory_has_focus_hints_redaction_and_skill_selection() {
    for step in OnboardingStep::INVENTORY {
        let screen = onboarding::screen_for(step, 0);
        let text = screen
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !text.to_lowercase().contains("opencode"),
            "onboarding screen {} should use Harness branding only:\n{text}",
            step.snapshot_name()
        );
        assert!(
            !text.contains("oauth-access")
                && !text.contains("refresh")
                && !text.contains("acct-")
                && !text.contains("sk-"),
            "onboarding screen {} should not include secret-like values:\n{text}",
            step.snapshot_name()
        );
        assert!(
            !screen.choices.is_empty(),
            "onboarding screen {} should expose at least one selectable row",
            step.snapshot_name()
        );
        assert!(
            !screen.footer.trim().is_empty(),
            "onboarding screen {} should include key hints",
            step.snapshot_name()
        );
        if matches!(
            step,
            OnboardingStep::CodexBrowser
                | OnboardingStep::CodexDevice
                | OnboardingStep::CopilotPublicDevice
                | OnboardingStep::CopilotEnterpriseDevice
                | OnboardingStep::ApiKeyEntry
                | OnboardingStep::LoginSuccess
        ) {
            assert!(
                text.contains("redacted"),
                "onboarding screen {} should explicitly redact sensitive auth metadata:\n{text}",
                step.snapshot_name()
            );
        }
    }

    let skill_screen = onboarding::screen_for(OnboardingStep::SkillSelection, 0);
    let skill_labels = skill_screen
        .choices
        .iter()
        .map(|choice| choice.label)
        .collect::<Vec<_>>();
    assert_eq!(skill_labels, vec!["build", "plan", "explore"]);
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_skip_is_launch_local_and_writes_no_auth_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_required(true);
    assert!(app.onboarding_screen().is_some());

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen().expect("skip screen").step,
        OnboardingStep::SkipConfirmation
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.onboarding_screen().is_none());
    assert_eq!(
        app.status_banner.as_deref(),
        Some("onboarding skipped for this launch; no credential was written")
    );
    assert!(intents.lock().expect("lock intents").is_empty());

    app.set_onboarding_required(true);
    assert!(
        app.onboarding_screen().is_none(),
        "skip should suppress onboarding only for the current AppState launch"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_auth_waits_for_backend_result() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_step_for_test(OnboardingStep::CodexDevice);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.onboarding_screen().expect("codex device screen").step,
        OnboardingStep::CodexDevice,
        "onboarding must not show success before the backend reports success"
    );
    assert!(app.onboarding_auth_in_progress);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "device".to_string()
            ],
            stdin: None,
        }]
    );

    app.apply_auth_backend_result(true);

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.onboarding_screen().expect("success screen").step,
        OnboardingStep::LoginSuccess
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_api_key_emits_hidden_stdin_without_visible_secret() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_step_for_test(OnboardingStep::ApiKeyEntry);
    let secret = "sk-tui-onboarding-secret-value";

    app.handle_paste(secret);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        !app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains(secret),
        "onboarding status leaked the pasted API key"
    );
    assert!(
        app.onboarding_secret_input.is_empty(),
        "secret buffer should be cleared after auth request handoff"
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string()
            ],
            stdin: Some(secret.to_string()),
        }]
    );
    assert_eq!(
        app.onboarding_screen().expect("api key screen").step,
        OnboardingStep::ApiKeyEntry,
        "success must wait for backend result"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_copilot_enterprise_is_reachable_and_redacts_domain() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    let enterprise_domain = "https://github.example.test";

    app.set_onboarding_required(true);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen().expect("provider screen").step,
        OnboardingStep::ProviderPick
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let target_screen = app.onboarding_screen().expect("copilot target screen");
    assert_eq!(target_screen.step, OnboardingStep::CopilotTargetPick);
    assert_eq!(
        target_screen
            .choices
            .iter()
            .map(|choice| choice.label)
            .collect::<Vec<_>>(),
        vec!["GitHub.com", "Enterprise"]
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen()
            .expect("enterprise device screen")
            .step,
        OnboardingStep::CopilotEnterpriseDevice
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.status_banner.as_deref(),
        Some("enterprise login requires a domain; input stays hidden")
    );
    assert!(
        intents.lock().expect("lock intents").is_empty(),
        "blank Enterprise domain must not emit a public-fallback auth request"
    );

    app.handle_paste(enterprise_domain);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        !app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains(enterprise_domain),
        "onboarding status leaked the enterprise domain"
    );
    assert!(
        app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains("--enterprise-url <redacted>"),
        "onboarding status should redact the enterprise-url value"
    );
    assert!(
        app.onboarding_secret_input.is_empty(),
        "enterprise domain buffer should be cleared after auth request handoff"
    );
    assert_eq!(
        app.onboarding_screen()
            .expect("enterprise device screen")
            .step,
        OnboardingStep::CopilotEnterpriseDevice,
        "success must wait for backend result"
    );
    assert!(app.onboarding_auth_in_progress);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "github-copilot".to_string(),
                "--method".to_string(),
                "device".to_string(),
                "--enterprise-url".to_string(),
                enterprise_domain.to_string(),
            ],
            stdin: None,
        }]
    );

    app.apply_auth_backend_result(true);

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.onboarding_screen().expect("success screen").step,
        OnboardingStep::LoginSuccess
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_slash_compact_appears_when_supported() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.set_compact_session_supported(true);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));
}

#[cfg(test)]
pub(crate) fn exact_test_live_without_compact_support_hides_slash_compact() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));

    app.clear_prompt_input();
    for ch in "/compact".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert!(app.typed_slash_command().is_none());
    assert!(!app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));
}

#[cfg(test)]
pub(crate) fn exact_test_slash_menu_lists_lineage_commands() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    for query in ["fork", "tree", "clone"] {
        app.replace_prompt_input(format!("/{query}"));
        app.sync_slash_overlay();

        assert_eq!(app.slash_filtered, vec![query.to_string()]);
        assert_eq!(app.typed_slash_command(), Some(query));
    }

    assert_eq!(
        crate::keybindings::slash_command_description("fork"),
        "Fork session"
    );
    assert_eq!(
        crate::keybindings::slash_command_description("tree"),
        "View the Harness session tree"
    );
    assert_eq!(
        crate::keybindings::slash_command_description("clone"),
        "Prepare a Harness session clone"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_write_commands_blocked_in_replay() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    app.on_ui_intent = Some(sink);

    app.prompt_buffer = "/fork".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    assert!(app.typed_slash_command().is_none());
    app.execute_slash_command("fork", Some("replay draft".to_string()));
    assert_eq!(app.prompt_buffer, "replay draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session fork blocked: replay mode is read-only")
    );

    app.prompt_buffer = "/clone".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    assert!(app.typed_slash_command().is_none());
    app.execute_slash_command("clone", Some("clone draft".to_string()));
    assert_eq!(app.prompt_buffer, "clone draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session clone blocked: replay mode is read-only")
    );

    app.prompt_buffer = "/tree".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    assert_eq!(app.typed_slash_command(), Some("tree"));
    app.execute_slash_command("tree", Some("tree draft".to_string()));
    assert_eq!(app.prompt_buffer, "tree draft");
    assert!(app.lineage_browser_visible);

    assert!(intents.lock().expect("lock intents").is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_write_commands_blocked_when_live_unstable() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_live_lineage_unstable".to_string(),
        seq: 1,
        run_id: "run_live_lineage_unstable".to_string(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_live_lineage_unstable".to_string()),
        causation_id: None,
        stream_key: Some("req_live_lineage_unstable".to_string()),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_live_lineage_unstable".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect active work".to_string(),
            request_digest: "digest-live-lineage-unstable".to_string(),
            metadata: None,
        }),
    });
    assert!(app.active_turn_in_progress());

    app.replace_prompt_input("/fork".to_string());
    app.sync_slash_overlay();
    assert_eq!(app.typed_slash_command(), Some("fork"));
    assert_eq!(app.slash_filtered, vec!["fork".to_string()]);
    app.execute_slash_command("fork", Some("fork draft".to_string()));
    assert_eq!(app.prompt_buffer, "fork draft");
    assert!(app.fork_selector_visible);

    app.fork_selector_visible = false;
    app.replace_prompt_input("/clone".to_string());
    app.sync_slash_overlay();
    assert!(
        app.typed_slash_command().is_none(),
        "/clone should not type-dispatch while live work is active"
    );
    assert!(
        !app.slash_filtered.iter().any(|entry| entry == "clone"),
        "/clone should be hidden while live work is active"
    );
    app.execute_slash_command("clone", Some("clone draft".to_string()));
    assert_eq!(app.prompt_buffer, "clone draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("Harness session clone blocked: live session has active work")
    );

    app.replace_prompt_input("/tree".to_string());
    app.sync_slash_overlay();
    assert_eq!(app.typed_slash_command(), Some("tree"));
    assert_eq!(app.slash_filtered, vec!["tree".to_string()]);
    assert!(intents.lock().expect("lock intents").is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_descriptions_use_harness_branding() {
    for command in ["tree", "clone"] {
        let description = crate::keybindings::slash_command_description(command);
        assert!(
            description.contains("Harness"),
            "{command} should use Harness branding: {description}"
        );

        let lower = description.to_lowercase();
        for forbidden in [
            ["open", "code"].concat(),
            ["open", "code"].join(" "),
            "codex".to_string(),
        ] {
            assert!(
                !lower.contains(&forbidden),
                "{command} description contains forbidden source brand: {description}"
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn exact_test_compact_operator_rail_skips_focus_cycle() {
    let mut live = AppState::new_live(None, false, None);

    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    let mut live_overlay = AppState::new_live(None, false, None);
    live_overlay.focus = Focus::Details;
    live_overlay.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert!(live_overlay.details_drawer_open());

    live_overlay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live_overlay.focus, Focus::List);
    assert!(live_overlay.details_drawer_open());

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(replay.focus, Focus::Details);
}
