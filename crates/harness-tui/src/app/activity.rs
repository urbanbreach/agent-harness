// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::*;

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
    pub thinking_first_mono_ms: Option<u64>,
    pub thinking_last_mono_ms: Option<u64>,
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
    pub revision: u64,
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

pub(in crate::app) struct NewStreamingActivityEntryArgs {
    pub(in crate::app) request_id: String,
    pub(in crate::app) profile_label: String,
    pub(in crate::app) model_id: String,
    pub(in crate::app) provider_id: String,
    pub(in crate::app) user_message: Option<UserMessageSubmittedEvent>,
    pub(in crate::app) user_timestamp: Option<String>,
    pub(in crate::app) request_data: Option<ProviderRequestStartedEvent>,
    pub(in crate::app) transcript_text: String,
    pub(in crate::app) first_seq: u64,
    pub(in crate::app) first_mono_ms: u64,
}

pub(in crate::app) fn new_streaming_activity_entry(
    args: NewStreamingActivityEntryArgs,
) -> ActivityEntry {
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
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
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
        revision: 0,
    }
}

impl ActivityEntry {
    pub fn duration_ms(&self) -> Option<u64> {
        (self.last_mono_ms >= self.first_mono_ms)
            .then_some(self.last_mono_ms.saturating_sub(self.first_mono_ms))
    }

    /// Mono span of reasoning deltas only — Grok "Thought for" duration.
    pub fn thinking_duration_ms(&self) -> Option<u64> {
        match (self.thinking_first_mono_ms, self.thinking_last_mono_ms) {
            (Some(first), Some(last)) if last >= first => Some(last.saturating_sub(first)),
            _ => None,
        }
    }

    pub(in crate::app) fn note_thinking_mono(&mut self, mono_ms: u64) {
        if self.thinking_first_mono_ms.is_none() {
            self.thinking_first_mono_ms = Some(mono_ms);
        }
        self.thinking_last_mono_ms = Some(mono_ms);
    }

    pub(in crate::app) fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
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

pub(in crate::app) fn mark_activity_event(entry: &mut ActivityEntry, seq: u64, mono_ms: u64) {
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

    pub(in crate::app) fn sort_rank(self) -> u8 {
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

pub(in crate::app) fn merge_orchestration_task_event(
    row: &mut OrchestrationTaskRow,
    event: &EventEnvelopeV1,
) {
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

pub(in crate::app) fn merge_orchestration_task_completion_metadata(
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
