#[cfg(test)]
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use harness_core::agent::AgentModelRef;
use harness_core::event::{
    ActorKind, EventEnvelopeV1, EventV1, PermissionDecision as EventPermissionDecision,
    ProviderRequestStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;
use harness_core::proj::{SessionCatalogEntry, SessionModeSource};

use crate::keybindings::{Action, KeyMap};
use crate::overlay::{OverlayKind, OverlayStack, OverlayState};
use crate::theme::Theme;
use crate::ui::WheelTarget;
use crate::view_model;

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;
const TOOL_TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 72;
const TOOL_TRANSCRIPT_SUMMARY_MAX_FIELDS: usize = 3;
pub(crate) const SLASH_COMMANDS: [(&str, &str); 2] = [
    ("new", "Return to the home shell"),
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
    pub args_summary: String,
    pub args_digest: String,
    pub status: ToolCallDisplayStatus,
    pub output_summary: Option<String>,
    pub output_digest: Option<String>,
    pub truncated_output: Option<String>,
    pub permissions: Vec<PermissionEntry>,
    pub first_seq: u64,
    pub last_seq: u64,
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
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

pub struct ActivityEntry {
    pub request_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub status: ActivityStatus,
    pub user_message: Option<UserMessageSubmittedEvent>,
    pub request_data: Option<ProviderRequestStartedEvent>,
    pub thinking_text: String,
    pub transcript_text: String,
    pub error_message: Option<String>,
    pub permissions: Vec<PermissionEntry>,
    pub tool_calls: Vec<ToolCallEntry>,
    pub first_seq: u64,
    pub last_seq: u64,
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
    pub first_seq: u64,
    pub last_seq: u64,
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
    Diff,
    Help,
}

impl ReviewSurface {
    pub(crate) fn status_label(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Diff => "diff",
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchMetadata {
    profile: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    mode_label: Option<String>,
    available_models: Vec<ModelOption>,
}

impl LaunchMetadata {
    pub fn new(
        profile: impl Into<String>,
        provider: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            profile: Some(profile.into()),
            provider: Some(provider.into()),
            model,
            mode_label: None,
            available_models: Vec::new(),
        }
    }

    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let model_ref = AgentModelRef::parse(model_ref);
        Self::new(profile, model_ref.provider_id, Some(model_ref.model_id))
    }

    pub fn from_model_option(option: &ModelOption) -> Self {
        Self::new(
            option.profile.clone(),
            option.provider.clone(),
            Some(option.model.clone()),
        )
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
            .unwrap_or("unknown")
    }

    pub fn provider(&self) -> &str {
        self.provider
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("unknown")
    }

    pub fn model(&self) -> Option<&str> {
        self.model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn mode_label(&self) -> Option<&str> {
        self.mode_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn available_models(&self) -> &[ModelOption] {
        &self.available_models
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub profile: String,
    pub provider: String,
    pub model: String,
}

impl ModelOption {
    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let profile = profile.into();
        let model_ref = AgentModelRef::parse(model_ref);
        Self {
            profile,
            provider: model_ref.provider_id,
            model: model_ref.model_id,
        }
    }

    fn matches(&self, input: &str) -> bool {
        if input.is_empty() {
            return true;
        }

        let input = input.to_lowercase();
        self.profile.to_lowercase().contains(&input)
            || self.provider.to_lowercase().contains(&input)
            || self.model.to_lowercase().contains(&input)
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
            .then_with(|| self.profile.cmp(&other.profile))
    }
}

#[cfg(not(test))]
static PENDING_LIVE_LAUNCH_METADATA: OnceLock<Mutex<Option<LaunchMetadata>>> = OnceLock::new();
#[cfg(not(test))]
static PENDING_LIVE_PROMPT_DRAFT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
#[cfg(not(test))]
static PENDING_LIVE_PROMPT_AUTO_SUBMIT: OnceLock<Mutex<bool>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static PENDING_LIVE_LAUNCH_METADATA: RefCell<Option<LaunchMetadata>> = const { RefCell::new(None) };
    static PENDING_LIVE_PROMPT_DRAFT: RefCell<Option<String>> = const { RefCell::new(None) };
    static PENDING_LIVE_PROMPT_AUTO_SUBMIT: RefCell<bool> = const { RefCell::new(false) };
}

struct PendingLivePrompt {
    text: String,
    auto_submit: bool,
}

struct PendingLiveState;

impl PendingLiveState {
    #[cfg(not(test))]
    fn launch_metadata() -> &'static Mutex<Option<LaunchMetadata>> {
        PENDING_LIVE_LAUNCH_METADATA.get_or_init(|| Mutex::new(None))
    }

    #[cfg(not(test))]
    fn prompt_draft() -> &'static Mutex<Option<String>> {
        PENDING_LIVE_PROMPT_DRAFT.get_or_init(|| Mutex::new(None))
    }

    #[cfg(not(test))]
    fn prompt_auto_submit() -> &'static Mutex<bool> {
        PENDING_LIVE_PROMPT_AUTO_SUBMIT.get_or_init(|| Mutex::new(false))
    }

    fn set_launch_metadata(metadata: LaunchMetadata) {
        #[cfg(test)]
        {
            PENDING_LIVE_LAUNCH_METADATA.with(|pending| {
                *pending.borrow_mut() = Some(metadata);
            });
        }

        #[cfg(not(test))]
        {
            *Self::launch_metadata()
                .lock()
                .expect("pending live launch metadata lock poisoned") = Some(metadata);
        }
    }

    fn take_launch_metadata() -> Option<LaunchMetadata> {
        #[cfg(test)]
        {
            PENDING_LIVE_LAUNCH_METADATA.with(|pending| pending.borrow_mut().take())
        }

        #[cfg(not(test))]
        {
            Self::launch_metadata()
                .lock()
                .expect("pending live launch metadata lock poisoned")
                .take()
        }
    }

    fn set_prompt(prompt: Option<String>, auto_submit: bool) {
        #[cfg(test)]
        {
            PENDING_LIVE_PROMPT_DRAFT.with(|pending| {
                *pending.borrow_mut() = prompt;
            });
            PENDING_LIVE_PROMPT_AUTO_SUBMIT.with(|pending| {
                *pending.borrow_mut() = auto_submit;
            });
        }

        #[cfg(not(test))]
        {
            *Self::prompt_draft()
                .lock()
                .expect("pending live prompt draft lock poisoned") = prompt;
            *Self::prompt_auto_submit()
                .lock()
                .expect("pending live prompt auto-submit lock poisoned") = auto_submit;
        }
    }

    fn take_prompt() -> Option<PendingLivePrompt> {
        #[cfg(test)]
        let draft = PENDING_LIVE_PROMPT_DRAFT.with(|pending| pending.borrow_mut().take());
        #[cfg(not(test))]
        let draft = Self::prompt_draft()
            .lock()
            .expect("pending live prompt draft lock poisoned")
            .take();

        #[cfg(test)]
        let auto_submit = PENDING_LIVE_PROMPT_AUTO_SUBMIT
            .with(|pending| std::mem::take(&mut *pending.borrow_mut()));
        #[cfg(not(test))]
        let auto_submit = std::mem::take(
            &mut *Self::prompt_auto_submit()
                .lock()
                .expect("pending live prompt auto-submit lock poisoned"),
        );

        draft.map(|text| PendingLivePrompt { text, auto_submit })
    }
}

pub fn set_pending_live_launch_metadata(metadata: LaunchMetadata) {
    PendingLiveState::set_launch_metadata(metadata);
}

fn take_pending_live_launch_metadata() -> Option<LaunchMetadata> {
    PendingLiveState::take_launch_metadata()
}

pub fn set_pending_live_prompt_draft(draft: Option<String>) {
    PendingLiveState::set_prompt(draft.filter(|value| !value.trim().is_empty()), false);
}

pub fn set_pending_live_prompt_auto_submit(prompt: Option<String>) {
    let prompt = prompt.filter(|value| !value.trim().is_empty());
    let should_auto_submit = prompt.is_some();
    PendingLiveState::set_prompt(prompt, should_auto_submit);
}

fn take_pending_live_prompt() -> Option<PendingLivePrompt> {
    PendingLiveState::take_prompt()
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
    show_successful_tool_details: bool,
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
    dismissed_permissions: BTreeSet<String>,
    submitted_permission_id: Option<String>,
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
            show_successful_tool_details: false,
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
            dismissed_permissions: BTreeSet::new(),
            submitted_permission_id: None,
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
                first_seq: event.seq,
                last_seq: event.seq,
            });

        row.owner_kind = event.actor.kind;
        if let Some(agent_id) = event.actor.agent_id.as_ref() {
            row.owner_agent_id = Some(agent_id.clone());
        }
        row.last_seq = event.seq;
        row
    }

    fn update_orchestration_task<F>(&mut self, event: &EventEnvelopeV1, task_id: &str, update: F)
    where
        F: FnOnce(&mut OrchestrationTaskRow),
    {
        {
            let row = self.orchestration_task_row_mut(event, task_id);
            update(row);
        }
        self.enforce_orchestration_retention();
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
                        if entry.first_seq == 0 {
                            entry.first_seq = event.seq;
                        }
                        entry.last_seq = event.seq;
                    }
                } else {
                    let entry = ActivityEntry {
                        request_id: data.request_id.clone(),
                        model_id: String::new(),
                        provider_id: String::new(),
                        status: ActivityStatus::Streaming,
                        user_message: Some(data.clone()),
                        request_data: None,
                        thinking_text: String::new(),
                        transcript_text: String::new(),
                        error_message: None,
                        permissions: Vec::new(),
                        tool_calls: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                    };
                    self.activities.push_back(entry);
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
                        if entry.first_seq == 0 {
                            entry.first_seq = event.seq;
                        }
                        entry.last_seq = event.seq;
                    }
                } else {
                    let entry = ActivityEntry {
                        request_id: data.request_id.clone(),
                        model_id: data.model_id.clone(),
                        provider_id: data.provider_id.clone(),
                        status: ActivityStatus::Streaming,
                        user_message: None,
                        request_data: Some(data.clone()),
                        thinking_text: String::new(),
                        transcript_text: String::new(),
                        error_message: None,
                        permissions: Vec::new(),
                        tool_calls: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                    };
                    self.activities.push_back(entry);
                }
            }
            EventV1::ProviderStreamDelta(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.transcript_text.push_str(&data.delta);
                        if entry.first_seq == 0 {
                            entry.first_seq = event.seq;
                        }
                        entry.last_seq = event.seq;
                    }
                } else {
                    self.activities.push_back(ActivityEntry {
                        request_id: data.request_id.clone(),
                        model_id: String::new(),
                        provider_id: String::new(),
                        status: ActivityStatus::Streaming,
                        user_message: None,
                        request_data: None,
                        thinking_text: String::new(),
                        transcript_text: data.delta.clone(),
                        error_message: None,
                        permissions: Vec::new(),
                        tool_calls: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                    });
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
                        entry.last_seq = event.seq;
                    }
                }
            }
            EventV1::TaskCompleted(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Completed;
                    row.warning = None;
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
                        args_summary: data.args_summary.clone(),
                        args_digest: data.args_digest.clone(),
                        status: ToolCallDisplayStatus::PendingPermission,
                        output_summary: None,
                        output_digest: None,
                        truncated_output: None,
                        permissions: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                    };
                    entry.tool_calls.push(tool_entry);
                    entry.last_seq = event.seq;
                }
            }
            EventV1::ToolCallStarted(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.status = ToolCallDisplayStatus::Running;
                    tool_entry.last_seq = event.seq;
                }
            }
            EventV1::ToolCallFinished(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.status = match data.status {
                        ToolCallStatus::Succeeded => ToolCallDisplayStatus::Succeeded,
                        ToolCallStatus::Failed => ToolCallDisplayStatus::Failed,
                    };
                    tool_entry.output_summary = data.output_summary.clone();
                    tool_entry.output_digest = data.output_digest.clone();
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_live(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        let mut state = Self::new();
        state.launch_metadata = take_pending_live_launch_metadata().unwrap_or_default();
        state.focus = Focus::Prompt;
        state.live_details_drawer_open = false;
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
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
        state.launch_metadata = take_pending_live_launch_metadata().unwrap_or_default();
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
        self.launch_metadata = launch_metadata;
    }

    pub fn replace_events(&mut self, events: Vec<EventEnvelopeV1>) {
        self.projection.reset();
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;

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
        self.update_transient_state_for_event(&event);
        let trimmed_events = self.projection.ingest_event(event, historical);

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
                    state.summary = format!("decision required · {}", permission.summary);
                    state.detail = Some(permission.summary);
                    state.composer_hint =
                        "Draft preserved under the permission checkpoint — deny to stay fail-closed, or allow once after reviewing the request.".to_string();
                }
            }
            RuntimeStateKind::PermissionPending => {
                if let Some(permission) = self.active_permission_view() {
                    state.summary = format!(
                        "decision submitted · awaiting confirmation · {}",
                        permission.summary
                    );
                    state.detail = Some(permission.summary);
                    state.composer_hint =
                        "Draft preserved while Harness records the permission decision. Wait for confirmation before sending another turn.".to_string();
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
        self.launch_metadata.profile()
    }

    pub fn active_provider(&self) -> &str {
        let provider = self.launch_metadata.provider();
        if provider != "unknown" {
            provider
        } else {
            self.activities
                .back()
                .and_then(|activity| {
                    (!activity.provider_id.trim().is_empty())
                        .then_some(activity.provider_id.as_str())
                })
                .unwrap_or(provider)
        }
    }

    pub fn current_model_label(&self) -> &str {
        self.launch_metadata
            .model()
            .or_else(|| {
                self.activities.back().and_then(|activity| {
                    (!activity.model_id.trim().is_empty()).then_some(activity.model_id.as_str())
                })
            })
            .unwrap_or("-")
    }

    pub fn operator_sidebar_state_label(&self) -> String {
        if self.replay_mode {
            "Replay".to_string()
        } else {
            self.launch_mode_label().unwrap_or("Live").to_string()
        }
    }

    pub fn operator_sidebar_run_identity(&self) -> String {
        format!("run {}", self.run_id().unwrap_or("unknown"))
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

    pub fn operator_sidebar_modified_files(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut files = Vec::new();

        for event in self.events.iter().rev() {
            if let EventV1::EditApplied(edit) = &event.payload {
                if seen.insert(edit.path.clone()) {
                    files.push(edit.path.clone());
                }
            }
        }

        files
    }

    pub(crate) fn is_current_model_option(&self, option: &ModelOption) -> bool {
        option.profile == self.active_profile()
            && option.provider == self.active_provider()
            && option.model == self.current_model_label()
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
        match self.prompt_buffer.trim() {
            "/new" => Some("new"),
            "/exit" => Some("exit"),
            _ => None,
        }
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

        if let Some(model) = self.launch_metadata.model() {
            options.insert(ModelOption {
                profile: self.launch_metadata.profile().to_string(),
                provider: self.launch_metadata.provider().to_string(),
                model: model.to_string(),
            });
        }

        if options.is_empty() {
            for activity in &self.activities {
                if !activity.provider_id.trim().is_empty() && !activity.model_id.trim().is_empty() {
                    options.insert(ModelOption {
                        profile: self.launch_metadata.profile().to_string(),
                        provider: activity.provider_id.clone(),
                        model: activity.model_id.clone(),
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

        let mut launch_metadata = LaunchMetadata::from_model_option(&selected_model)
            .with_available_models(self.launch_metadata.available_models().to_vec());
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }

        self.launch_metadata = launch_metadata.clone();
        set_pending_live_launch_metadata(launch_metadata.clone());
        self.emit_ui_intent(UiIntent::SwitchModel {
            profile: selected_model.profile,
            launch_metadata,
        });
        self.close_palette();
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

    pub(crate) fn successful_tool_details_visible(&self) -> bool {
        self.show_successful_tool_details
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

    pub fn transcript_pending_permissions(&self) -> Vec<(String, String)> {
        let mut pending = self
            .projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .map(|(permission_id, permission)| {
                (
                    permission.seq,
                    permission_id.clone(),
                    permission.summary.clone(),
                )
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
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 0,
            last_seq: 0,
        });
        self.selected_activity_index = self.activities.len().saturating_sub(1);
        self.details_scroll = 0;
        self.transcript_scroll = 0;
    }

    fn dispatch_submitted_prompt(&mut self, text: String) {
        self.prompt_history.push(text.clone());
        self.clear_prompt_input();
        self.echo_submitted_prompt(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt { text });
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
                match action {
                    Action::AllowPermission
                    | Action::DenyPermission
                    | Action::DismissModal
                    | Action::Quit => {
                        self.execute_action(action);
                        self.maybe_auto_exit();
                    }
                    _ => {}
                }
            }
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

        if self.startup_shell_visible()
            && self.focus != Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
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
            if let KeyCode::Char(c) = key.code {
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        let Some(action) = self.keymap.get_action(&key) else {
            return;
        };

        self.execute_action(action);
        self.maybe_auto_exit();
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
            "close_review_surface" => self.execute_action(Action::CloseReviewSurface),
            "open_event_log" => self.execute_action(Action::OpenEventLog),
            "open_diff_review" => self.execute_action(Action::OpenDiffReview),
            "toggle_follow" => self.execute_action(Action::ToggleFollow),
            "show_thinking" => self.show_transcript_thinking = true,
            "hide_thinking" => self.show_transcript_thinking = false,
            "expand_tool_output" => self.show_successful_tool_details = true,
            "collapse_tool_output" => self.show_successful_tool_details = false,
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
            return false;
        }

        if self.startup_shell_visible() {
            matches!(command_id, "new_session" | "resume_session" | "replay_session" | "quit")
        } else if matches!(command_id, "show_thinking" | "hide_thinking") {
            self.active_review_surface.is_none()
                && if command_id == "show_thinking" {
                    !self.show_transcript_thinking
                } else {
                    self.show_transcript_thinking
                }
        } else if matches!(command_id, "expand_tool_output" | "collapse_tool_output") {
            self.active_review_surface.is_none()
                && if command_id == "expand_tool_output" {
                    !self.show_successful_tool_details
                } else {
                    self.show_successful_tool_details
                }
        } else if command_id == "close_review_surface" {
            self.active_review_surface.is_some()
        } else if command_id == "open_event_log" {
            self.active_review_surface != Some(ReviewSurface::Events)
        } else if command_id == "open_diff_review" {
            self.active_review_surface != Some(ReviewSurface::Diff)
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
                self.focus = Focus::List;
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
        }
    }

    fn cycle_focus_forward(&mut self) {
        if self.replay_mode {
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
        if self.active_permission().is_some() {
            match action {
                Action::AllowPermission => {
                    if let Some((permission_id, _)) = self.active_permission() {
                        self.send_permission_intent(permission_id, PermissionDecision::Allow);
                    }
                    return;
                }
                Action::DenyPermission => {
                    if let Some((permission_id, _)) = self.active_permission() {
                        self.send_permission_intent(permission_id, PermissionDecision::Deny);
                    }
                    return;
                }
                Action::DismissModal => {
                    if let Some((permission_id, _)) = self.active_permission() {
                        self.dismissed_permissions.insert(permission_id);
                        self.maybe_auto_exit();
                    }
                    return;
                }
                Action::Quit => {
                    self.should_quit = true;
                    self.emit_ui_intent(UiIntent::QuitRequested);
                    return;
                }
                _ => return,
            }
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
                    if !self.prompt_history.is_empty() {
                        let next_idx = match self.prompt_history_index {
                            Some(idx) => idx.saturating_sub(1),
                            None => self.prompt_history.len().saturating_sub(1),
                        };
                        self.prompt_history_index = Some(next_idx);
                        self.replace_prompt_input(self.prompt_history[next_idx].clone());
                    }
                    return;
                }
                Action::HistoryDown => {
                    if let Some(idx) = self.prompt_history_index {
                        if idx + 1 < self.prompt_history.len() {
                            let next_idx = idx + 1;
                            self.prompt_history_index = Some(next_idx);
                            self.replace_prompt_input(self.prompt_history[next_idx].clone());
                        } else {
                            self.clear_prompt_input();
                        }
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
            Action::OpenDiffReview if self.focus != Focus::Prompt => {
                self.open_review_surface(ReviewSurface::Diff);
            }
            Action::Reload if self.replay_mode => {
                self.reload_requested = true;
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
                self.send_permission_intent(permission_id, PermissionDecision::Allow);
                true
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny);
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

    fn send_permission_intent(&mut self, permission_id: String, decision: PermissionDecision) {
        if self.submitted_permission_id.as_deref() == Some(permission_id.as_str()) {
            return;
        }

        self.emit_ui_intent(UiIntent::ResolvePermission {
            permission_id: permission_id.clone(),
            decision,
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
            self.emit_ui_intent(UiIntent::SubmitPrompt { text });
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

#[cfg(test)]
pub(crate) fn exact_test_startup_slash_commands_execute_without_menu() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app.slash_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), None);
    assert_eq!(
        app.slash_filtered,
        vec!["new".to_string(), "exit".to_string()]
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
mod tests {
    use super::*;
    use crate::overlay::OverlayKind;
    use crate::ui::WheelTarget;
    use crossterm::event::MouseEvent;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
        ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, TaskCompletedEvent,
        UserMessageSubmittedEvent, SCHEMA_VERSION,
    };

    fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_app_{seq:04}"),
            seq,
            run_id: "run_app_tests".to_string(),
            mono_ms: seq,
            ts: Some("2026-02-03T12:00:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("app-tests".to_string())),
            correlation_id: Some(request_id.to_string()),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn overlay_stack_orders_details_palette_permission() {
        let mut app = AppState::new_live(None, false, None);
        app.live_details_drawer_open = true;

        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        assert_eq!(
            app.overlay_stack().ordered(),
            &[OverlayKind::DetailsDrawer, OverlayKind::CommandPalette]
        );

        app.ingest_event(envelope(
            1,
            "req_overlay_stack",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_overlay_stack".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_overlay_stack".to_string()),
                summary: "permission summary".to_string(),
                request_digest: "digest-overlay-stack".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        assert_eq!(
            app.overlay_stack().ordered(),
            &[OverlayKind::DetailsDrawer, OverlayKind::PermissionModal]
        );
    }

    #[test]
    fn overlay_stack_orders_permission_above_commands_and_slash() {
        let mut app = AppState::new_live(None, false, None);
        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        assert_eq!(app.overlay_stack().top(), Some(OverlayKind::CommandPalette));

        app.ingest_event(envelope(
            1,
            "req_overlay_priority",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_overlay_priority".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_overlay_priority".to_string()),
                summary: "permission summary".to_string(),
                request_digest: "digest-overlay-priority".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        assert_eq!(
            app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
    }

    #[test]
    fn permission_modal_preempts_palette() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let intent_sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(intent_sink));
        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        app.handle_key(key(KeyCode::Char('d')));

        app.ingest_event(envelope(
            1,
            "req_overlay_preempt",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_overlay_preempt".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_overlay_preempt".to_string()),
                summary: "permission summary".to_string(),
                request_digest: "digest-overlay-preempt".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        app.handle_key(key_with_modifiers(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));

        assert!(!app.palette_visible);
        assert!(app.palette_input.is_empty());
        assert_eq!(
            app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
        let intents = intents.lock().expect("lock intents");
        assert_eq!(
            intents.as_slice(),
            &[UiIntent::ResolvePermission {
                permission_id: "perm_overlay_preempt".to_string(),
                decision: PermissionDecision::Allow,
            }]
        );
    }

    #[test]
    fn permission_modal_routes_q_to_quit_without_buffering() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let intent_sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(intent_sink));
        app.prompt_buffer = "keep this draft".to_string();
        app.prompt_cursor = app.prompt_buffer.chars().count();
        app.ingest_event(envelope(
            1,
            "req_modal_quit",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_modal_quit".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_modal_quit".to_string()),
                summary: "permission summary".to_string(),
                request_digest: "digest-modal-quit".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        app.handle_key(key(KeyCode::Char('q')));

        assert!(app.should_quit);
        assert_eq!(app.prompt_buffer, "keep this draft");
        let intents = intents.lock().expect("lock intents");
        assert_eq!(intents.as_slice(), &[UiIntent::QuitRequested]);
    }

    #[test]
    fn focus_returns_after_palette_close() {
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Details;

        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        assert!(app.palette_visible);
        assert_eq!(app.focus, Focus::Details);

        app.handle_key(key(KeyCode::Esc));
        assert!(!app.palette_visible);
        assert_eq!(app.focus, Focus::Details);
    }

    #[test]
    fn details_drawer_toggles_without_stealing_transcript_state() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_a",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_a".to_string(),
                text: "First".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_a",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_a".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "First".to_string(),
                request_digest: "digest-a".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_b",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_b".to_string(),
                text: "Second".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_b",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_b".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Second".to_string(),
                request_digest: "digest-b".to_string(),
            }),
        ));

        app.follow_mode = false;
        app.focus = Focus::Details;
        app.selected_activity_index = 0;
        app.details_scroll = 7;

        app.handle_key(key(KeyCode::Char('i')));
        assert!(app.details_drawer_open());
        assert_eq!(app.active_tab, Tab::Run);
        assert_eq!(app.focus, Focus::Details);
        assert!(!app.follow_mode);
        assert_eq!(app.selected_activity_index, 0);
        assert_eq!(app.details_scroll, 7);

        app.handle_key(key(KeyCode::Char('i')));
        assert!(!app.details_drawer_open());
        assert_eq!(app.active_tab, Tab::Run);
        assert_eq!(app.focus, Focus::Details);
        assert!(!app.follow_mode);
        assert_eq!(app.selected_activity_index, 0);
        assert_eq!(app.details_scroll, 7);
    }

    #[test]
    fn config_backed_live_launch_starts_in_session_shell_without_details_drawer() {
        set_pending_live_launch_metadata(
            LaunchMetadata::new("deep", "default", Some("gpt-5.3-codex".to_string()))
                .with_mode_label("Live"),
        );

        let app = AppState::new_live(None, false, None);

        assert!(!app.details_drawer_open());
        assert_eq!(app.focus, Focus::Prompt);
    }

    #[test]
    fn mouse_wheel_scrolls_transcript_without_stealing_focus() {
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Some(WheelTarget::Transcript),
        );
        assert!(!app.follow_mode);
        assert_eq!(app.transcript_scroll, 3);
        assert_eq!(app.focus, Focus::Prompt);

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Some(WheelTarget::Transcript),
        );
        assert_eq!(app.transcript_scroll, 0);
        assert!(app.follow_mode);
        assert_eq!(app.focus, Focus::Prompt);
    }

    #[test]
    fn mouse_wheel_scrolls_inspector_when_hovered() {
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::List;
        app.details_scroll = 2;
        app.transcript_scroll = 4;

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 90,
                row: 12,
                modifiers: KeyModifiers::NONE,
            },
            Some(WheelTarget::Inspector),
        );
        assert_eq!(app.details_scroll, 5);
        assert_eq!(app.transcript_scroll, 4);
        assert_eq!(app.focus, Focus::List);

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 90,
                row: 12,
                modifiers: KeyModifiers::NONE,
            },
            Some(WheelTarget::Inspector),
        );
        assert_eq!(app.details_scroll, 2);
        assert_eq!(app.transcript_scroll, 4);
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn mouse_wheel_ignores_non_scrollable_areas() {
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;
        app.details_scroll = 6;
        app.transcript_scroll = 2;
        app.follow_mode = false;

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 2,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            None,
        );

        assert_eq!(app.details_scroll, 6);
        assert_eq!(app.transcript_scroll, 2);
        assert!(!app.follow_mode);
        assert_eq!(app.focus, Focus::Prompt);
    }

    #[test]
    fn historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink = {
            let intents = intents.clone();
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(sink));
        app.ingest_event(envelope(
            1,
            "req_resume_1",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_resume_1".to_string(),
                text: "previous question".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_resume_1",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_resume_1".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "previous question".to_string(),
                request_digest: "digest-resume-1".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_resume_1",
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_resume_1".to_string(),
                delta: "previous answer".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_resume_1",
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000123".to_string(),
                result_summary: "previous answer".to_string(),
                result_digest: "digest-task-123".to_string(),
            }),
        ));

        assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);

        for c in "next".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));

        let intents = intents.lock().expect("lock intents");
        assert!(
            intents
                .iter()
                .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text } if text == "next")),
            "historical streaming residue should not block first resumed submit"
        );
    }

    #[test]
    fn historical_terminal_events_stay_in_session_shell_after_live_finish() {
        let mut app = AppState::new_live(
            Some(PathBuf::from("/tmp/sessions/run_resume")),
            true,
            Some(Arc::new(|_| {})),
        );

        app.ingest_historical_event(envelope(
            1,
            "req_resume_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "previous run complete".to_string(),
            }),
        ));

        assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!app.post_run_handoff_visible());
        assert!(!app.completed_session_shell_active());
        assert!(!app.should_quit);
        assert_eq!(app.events.len(), 1);

        app.ingest_event(envelope(
            2,
            "req_live_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "live run complete".to_string(),
            }),
        ));

        assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!app.post_run_handoff_visible());
        assert!(app.completed_session_shell_active());
        assert_eq!(app.active_tab, Tab::Run);
        assert_eq!(app.focus, Focus::Details);
        assert!(app.should_quit);
    }

    #[test]
    fn continued_quiescent_bootstrap_stays_in_session_shell_without_handoff() {
        set_pending_live_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Continued"),
        );
        let mut app = AppState::new_live(
            Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
            false,
            Some(Arc::new(|_| {})),
        );

        app.ingest_historical_event(envelope(
            1,
            "req_resume_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "previous run complete".to_string(),
            }),
        ));

        assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!app.post_run_handoff_visible());
        assert_eq!(app.active_tab, Tab::Run);
        assert_eq!(app.focus, Focus::Prompt);
        assert!(!app.composer_disabled());
    }

    #[test]
    fn startup_prompt_enter_emits_submit_intent_and_quits_launcher() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_startup(Vec::new(), Some(sink));

        for c in "ship it".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));

        assert!(app.should_quit, "startup submit should leave the launcher");
        assert_eq!(
            intents.lock().expect("lock intents").as_slice(),
            &[UiIntent::SubmitPrompt {
                text: "ship it".to_string(),
            }]
        );
    }

    #[test]
    fn live_bootstrap_auto_submit_echoes_and_emits_first_prompt() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new();
        app.focus = Focus::Prompt;
        app.on_ui_intent = Some(sink);

        app.apply_pending_live_prompt(PendingLivePrompt {
            text: "boot prompt".to_string(),
            auto_submit: true,
        });

        assert!(app.prompt_buffer.is_empty());
        assert_eq!(app.prompt_history, vec!["boot prompt".to_string()]);
        assert_eq!(
            app.activities
                .back()
                .and_then(|activity| activity.user_message.as_ref())
                .map(|message| message.text.as_str()),
            Some("boot prompt")
        );
        assert_eq!(
            intents.lock().expect("lock intents").as_slice(),
            &[UiIntent::SubmitPrompt {
                text: "boot prompt".to_string(),
            }]
        );
    }

    #[test]
    fn replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits() {
        let mut app = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Details);

        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::List);

        app.focus = Focus::Prompt;
        app.handle_key(key(KeyCode::Char('x')));
        assert!(app.prompt_buffer.is_empty());
    }

    #[test]
    fn slash_menu_closes_after_whitespace() {
        let mut app = AppState::new_startup(Vec::new(), None);

        app.handle_key(key(KeyCode::Char('/')));
        app.handle_key(key(KeyCode::Char('n')));
        assert!(app.slash_visible);

        app.handle_key(key(KeyCode::Char(' ')));

        assert!(!app.slash_visible);
        assert_eq!(app.prompt_buffer, "/n ");
    }

    #[test]
    fn slash_exit_matches_quit_requested_behavior() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_startup(Vec::new(), Some(sink));
        for ch in "/exit".chars() {
            app.handle_key(key(KeyCode::Char(ch)));
        }
        app.handle_key(key(KeyCode::Enter));

        assert!(app.should_quit);
        assert_eq!(
            intents.lock().expect("lock intents").as_slice(),
            &[UiIntent::QuitRequested]
        );
    }

    #[test]
    fn startup_mode_uses_pending_launch_metadata() {
        set_pending_live_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
        );

        let app = AppState::new_startup(Vec::new(), None);

        assert_eq!(app.active_profile(), "worker");
        assert_eq!(app.active_provider(), "mock");
        assert_eq!(app.current_model_label(), "model-1");
        assert_eq!(app.launch_mode_label(), Some("Demo"));
    }

    #[test]
    fn lifecycle_shell_state_transitions() {
        let mut startup = AppState::new_startup(Vec::new(), None);
        startup.prompt_buffer = "draft prompt".to_string();

        assert_eq!(
            startup.lifecycle_shell_state(),
            LifecycleShellState::Startup
        );
        assert!(startup.startup_shell_visible());
        assert!(!startup.post_run_handoff_visible());
        assert!(startup.lifecycle_shell_actions_visible());
        assert_eq!(
            startup.runtime_state().summary,
            "startup ready · type below or use Ctrl+P for saved runs"
        );

        let live = AppState::new_live(None, false, None);

        assert_eq!(live.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!live.startup_shell_visible());
        assert!(!live.post_run_handoff_visible());
        assert!(!live.lifecycle_shell_actions_visible());

        let mut finished =
            AppState::new_live(Some(PathBuf::from("/tmp/live-finished")), false, None);
        finished.ingest_event(envelope(
            1,
            "req_lifecycle_finished",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ));

        assert_eq!(finished.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!finished.startup_shell_visible());
        assert!(!finished.post_run_handoff_visible());
        assert!(!finished.lifecycle_shell_actions_visible());
        assert!(finished.completed_session_shell_active());
        assert!(!finished.composer_disabled());

        let mut failed = AppState::new_live(Some(PathBuf::from("/tmp/live-failed")), false, None);
        failed.ingest_event(envelope(
            1,
            "req_lifecycle_failed",
            EventV1::RunFailed(RunFailedEvent {
                error: "boom".to_string(),
            }),
        ));

        assert_eq!(failed.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!failed.post_run_handoff_visible());
        assert!(!failed.lifecycle_shell_actions_visible());
        assert!(failed.completed_session_shell_active());

        let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
        let mut missing_session_path = AppState::new_live(None, false, Some(fallback_sink));
        missing_session_path.ingest_event(envelope(
            1,
            "req_lifecycle_missing_path",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done without persisted path".to_string(),
            }),
        ));

        assert_eq!(
            missing_session_path.lifecycle_shell_state(),
            LifecycleShellState::None
        );
        assert!(!missing_session_path.post_run_handoff_visible());
        assert!(missing_session_path.completed_session_shell_active());
        assert!(!missing_session_path.composer_disabled());

        let mut terminal_without_routing = AppState::new_live(None, false, None);
        terminal_without_routing.ingest_event(envelope(
            1,
            "req_lifecycle_without_routing",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done without lifecycle routing".to_string(),
            }),
        ));

        assert_eq!(
            terminal_without_routing.lifecycle_shell_state(),
            LifecycleShellState::None
        );
        assert!(!terminal_without_routing.post_run_handoff_visible());
        assert!(terminal_without_routing.completed_session_shell_active());
        assert!(!terminal_without_routing.composer_disabled());
    }

    #[test]
    fn default_shell_registry_exposes_home_and_session_shell_only() {
        let live_registry = default_shell_registry(false);
        assert_eq!(
            live_registry,
            &[
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
            ]
        );

        let replay_registry = default_shell_registry(true);
        assert_eq!(
            replay_registry,
            &[
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
            ]
        );
    }

    #[test]
    fn post_run_handoff_ignores_completed_turns_without_terminal_event() {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(envelope(
            1,
            "req_completed_turn",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_completed_turn".to_string(),
                text: "status?".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_completed_turn",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_completed_turn".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "status?".to_string(),
                request_digest: "digest-completed-turn".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_completed_turn",
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_completed_turn".to_string(),
                result_summary: "all done".to_string(),
                result_digest: "digest-task-completed-turn".to_string(),
            }),
        ));

        assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
        assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!app.startup_shell_visible());
        assert!(!app.post_run_handoff_visible());
        assert!(!app.lifecycle_shell_actions_visible());
    }

    #[test]
    fn replay_mode_never_reports_lifecycle_shell_actions() {
        let replay = AppState::new_replay(
            PathBuf::from("/tmp/replay-session"),
            vec![envelope(
                1,
                "req_replay_terminal",
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            )],
        );

        assert_eq!(replay.lifecycle_shell_state(), LifecycleShellState::None);
        assert!(!replay.startup_shell_visible());
        assert!(!replay.post_run_handoff_visible());
        assert!(!replay.lifecycle_shell_actions_visible());
    }
}
