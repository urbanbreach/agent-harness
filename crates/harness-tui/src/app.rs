use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    EventEnvelopeV1, EventV1, ProviderRequestStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;

use crate::keybindings::{Action, KeyMap};

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;

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
    pub status: ToolCallDisplayStatus,
    pub output_summary: Option<String>,
    pub truncated_output: Option<String>,
    pub first_seq: u64,
    pub last_seq: u64,
}

pub struct ActivityEntry {
    pub request_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub status: ActivityStatus,
    pub user_message: Option<UserMessageSubmittedEvent>,
    pub request_data: Option<ProviderRequestStartedEvent>,
    pub transcript_text: String,
    pub error_message: Option<String>,
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
    Details,
    Events,
    Diff,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    Primary,
    Drawer,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceDescriptor {
    pub tab: Tab,
    pub label: &'static str,
    pub role: SurfaceRole,
}

const LIVE_SURFACE_REGISTRY: [SurfaceDescriptor; 5] = [
    SurfaceDescriptor {
        tab: Tab::Run,
        label: "Conversation",
        role: SurfaceRole::Primary,
    },
    SurfaceDescriptor {
        tab: Tab::Details,
        label: "Details",
        role: SurfaceRole::Drawer,
    },
    SurfaceDescriptor {
        tab: Tab::Events,
        label: "Events",
        role: SurfaceRole::Secondary,
    },
    SurfaceDescriptor {
        tab: Tab::Diff,
        label: "Diff",
        role: SurfaceRole::Secondary,
    },
    SurfaceDescriptor {
        tab: Tab::Help,
        label: "Help",
        role: SurfaceRole::Secondary,
    },
];

const REPLAY_SURFACE_REGISTRY: [SurfaceDescriptor; 4] = [
    SurfaceDescriptor {
        tab: Tab::Run,
        label: "Conversation",
        role: SurfaceRole::Primary,
    },
    SurfaceDescriptor {
        tab: Tab::Events,
        label: "Events",
        role: SurfaceRole::Secondary,
    },
    SurfaceDescriptor {
        tab: Tab::Diff,
        label: "Diff",
        role: SurfaceRole::Secondary,
    },
    SurfaceDescriptor {
        tab: Tab::Help,
        label: "Help",
        role: SurfaceRole::Secondary,
    },
];

pub fn surface_registry(replay_mode: bool) -> &'static [SurfaceDescriptor] {
    if replay_mode {
        &REPLAY_SURFACE_REGISTRY
    } else {
        &LIVE_SURFACE_REGISTRY
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
    SubmitPrompt {
        text: String,
    },
    QuitRequested,
}

#[derive(Debug, Clone)]
struct PendingPermission {
    seq: u64,
    summary: String,
}

pub struct SessionProjection {
    pub(crate) events: Vec<EventEnvelopeV1>,
    pub(crate) activities: VecDeque<ActivityEntry>,
    pub(crate) memory_caps: MemoryCaps,
    pub(crate) events_trimmed_count: usize,
    pub(crate) transcript_trimmed_count: usize,
    seen_seqs: BTreeSet<u64>,
    pending_permissions: BTreeMap<String, PendingPermission>,
    run_terminal_seen: bool,
}

impl Default for SessionProjection {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            activities: VecDeque::new(),
            memory_caps: MemoryCaps::default(),
            events_trimmed_count: 0,
            transcript_trimmed_count: 0,
            seen_seqs: BTreeSet::new(),
            pending_permissions: BTreeMap::new(),
            run_terminal_seen: false,
        }
    }
}

pub struct AppState {
    pub selected_event_index: usize,
    pub focus: Focus,
    pub follow_mode: bool,
    pub active_tab: Tab,
    live_details_drawer_open: bool,
    projection: SessionProjection,
    pub should_quit: bool,
    pub replay_mode: bool,
    pub session_path: Option<PathBuf>,
    pub status_banner: Option<String>,
    pub details_scroll: u16,
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
    pub keymap: KeyMap,
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
            live_details_drawer_open: false,
            projection: SessionProjection::default(),
            should_quit: false,
            replay_mode: false,
            session_path: None,
            status_banner: None,
            details_scroll: 0,
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
            keymap: KeyMap::default(),
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
        self.seen_seqs.clear();
        self.pending_permissions.clear();
        self.run_terminal_seen = false;
        self.events_trimmed_count = 0;
        self.transcript_trimmed_count = 0;
    }

    fn has_seen_seq(&self, seq: u64) -> bool {
        self.seen_seqs.contains(&seq)
    }

    fn ingest_event(&mut self, event: EventEnvelopeV1) -> usize {
        self.seen_seqs.insert(event.seq);
        self.update_derived_state_for_event(&event);
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

    fn update_derived_state_for_event(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::PermissionRequested(data) => {
                self.pending_permissions.insert(
                    data.permission_id.clone(),
                    PendingPermission {
                        seq: event.seq,
                        summary: data.summary.clone(),
                    },
                );
            }
            EventV1::PermissionResolved(data) => {
                self.pending_permissions.remove(&data.permission_id);
            }
            EventV1::RunFinished(_) => {
                self.run_terminal_seen = true;
            }
            EventV1::RunFailed(data) => {
                self.run_terminal_seen = true;
                if let Some(entry) = self.activities.back_mut() {
                    entry.status = ActivityStatus::Error;
                    entry.error_message = Some(data.error.clone());
                }
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
                        transcript_text: String::new(),
                        error_message: None,
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
                        transcript_text: String::new(),
                        error_message: None,
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
                        transcript_text: data.delta.clone(),
                        error_message: None,
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
                        entry.status = ActivityStatus::Done;
                        entry.last_seq = event.seq;
                    }
                }
            }
            EventV1::TaskScheduled(data) => {
                if data.state == harness_core::event::TaskScheduleState::Queued {
                    if let Some(tool_entry) = self.find_tool_call_mut(&data.task_id) {
                        tool_entry.status = ToolCallDisplayStatus::Queued;
                        tool_entry.last_seq = event.seq;
                    }
                }
            }
            EventV1::ToolCallRequested(data) => {
                let target_corr_id = event.correlation_id.clone();
                let use_back = self.activities.back().map_or(true, |entry| {
                    target_corr_id.is_none() || entry.request_id.is_empty()
                });

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
                    let tool_entry = ToolCallEntry {
                        tool_call_id: data.tool_call_id.clone(),
                        tool_id: data.tool_id.clone(),
                        args_summary: data.args_summary.clone(),
                        status: ToolCallDisplayStatus::PendingPermission,
                        output_summary: None,
                        truncated_output: None,
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
            .map(|activity| activity.transcript_text.len())
            .sum();
        if total_chars > max_chars {
            let excess = total_chars - max_chars;
            let mut trimmed = 0;
            while trimmed < excess && !self.activities.is_empty() {
                if let Some(first) = self.activities.front_mut() {
                    if first.transcript_text.len() <= excess - trimmed {
                        trimmed += first.transcript_text.len();
                        first.transcript_text.clear();
                    } else {
                        let to_trim = excess - trimmed;
                        first.transcript_text = first.transcript_text.split_off(to_trim);
                        trimmed = excess;
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
        state.focus = Focus::Prompt;
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
        state
    }

    pub fn new_replay(session_path: PathBuf, events: Vec<EventEnvelopeV1>) -> Self {
        let mut state = Self::new();
        state.replay_mode = true;
        state.session_path = Some(session_path);
        state.replace_events(events);
        state
    }

    pub fn apply_keybindings(&mut self, bindings: std::collections::BTreeMap<String, String>) {
        self.keymap.apply_overrides(&bindings);
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
        self.maybe_auto_exit();
    }

    pub fn ingest_event(&mut self, event: EventEnvelopeV1) {
        if self.projection.has_seen_seq(event.seq) {
            return;
        }

        self.update_transient_state_for_event(&event);
        let trimmed_events = self.projection.ingest_event(event);

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
        }

        self.maybe_auto_exit();
    }

    pub fn set_status_banner(&mut self, status: Option<String>) {
        self.status_banner = status;
    }

    pub fn prompt_bootstrap_disabled(&self) -> bool {
        if self.replay_mode || !self.events.is_empty() {
            return false;
        }

        self.status_banner.as_deref().is_some_and(|banner| {
            let lower = banner.to_ascii_lowercase();
            lower.contains("lagged")
                || lower.contains("replaying")
                || lower.contains("disconnected")
        })
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

    pub fn active_permission(&self) -> Option<(String, String)> {
        self.projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| (permission_id.clone(), pending.summary.clone()))
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

    pub fn surface_registry(&self) -> &'static [SurfaceDescriptor] {
        surface_registry(self.replay_mode)
    }

    pub fn details_drawer_open(&self) -> bool {
        !self.replay_mode
            && matches!(self.active_tab, Tab::Run | Tab::Details)
            && (self.live_details_drawer_open || self.active_tab == Tab::Details)
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
    }

    fn replace_prompt_input(&mut self, prompt: String) {
        self.prompt_cursor = prompt.chars().count();
        self.prompt_buffer = prompt;
    }

    fn insert_prompt_char(&mut self, c: char) {
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.insert(byte_idx, c);
        self.prompt_cursor += 1;
    }

    fn backspace_prompt_char(&mut self) {
        if self.prompt_cursor == 0 {
            return;
        }

        self.prompt_cursor -= 1;
        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
    }

    fn delete_prompt_char(&mut self) {
        if self.prompt_cursor >= self.prompt_char_count() {
            return;
        }

        let byte_idx = self.prompt_cursor_byte_index();
        self.prompt_buffer.remove(byte_idx);
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
            transcript_text: String::new(),
            error_message: None,
            tool_calls: Vec::new(),
            first_seq: 0,
            last_seq: 0,
        });
        self.selected_activity_index = self.activities.len().saturating_sub(1);
        self.details_scroll = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.palette_visible && self.handle_palette_key(&key) {
            return;
        }

        if self.focus == Focus::Prompt
            && !self.prompt_bootstrap_disabled()
            && self.active_permission().is_none()
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
                if self.palette_cursor > 0 {
                    self.palette_cursor -= 1;
                    let byte_idx = self
                        .palette_input
                        .char_indices()
                        .nth(self.palette_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.palette_input.len());
                    self.palette_input.remove(byte_idx);
                    self.update_palette_filter();
                }
                true
            }
            KeyCode::Char(c) => {
                let byte_idx = self
                    .palette_input
                    .char_indices()
                    .nth(self.palette_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.palette_input.len());
                self.palette_input.insert(byte_idx, c);
                self.palette_cursor += 1;
                self.update_palette_filter();
                true
            }
            _ => false,
        }
    }

    fn update_palette_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        self.palette_filtered = Action::palette_commands()
            .iter()
            .filter(|(cmd, _)| cmd.to_lowercase().starts_with(&input))
            .map(|(cmd, _)| cmd.to_string())
            .collect();
        self.palette_selected = 0;
    }

    fn execute_palette_command(&mut self) {
        let Some(cmd) = self.palette_filtered.get(self.palette_selected) else {
            self.close_palette();
            return;
        };

        match cmd.as_str() {
            "help" => self.execute_action(Action::Help),
            "details" => self.execute_action(Action::ToggleDetailsDrawer),
            "run" => self.execute_action(Action::TabRun),
            "events" => self.execute_action(Action::TabEvents),
            "diff" => self.execute_action(Action::TabDiff),
            "toggle_follow" => self.execute_action(Action::ToggleFollow),
            "quit" => self.execute_action(Action::Quit),
            _ => {}
        }
        self.close_palette();
    }

    fn close_palette(&mut self) {
        self.palette_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.palette_selected = 0;
    }

    fn open_palette(&mut self) {
        self.palette_visible = true;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered = Action::palette_commands()
            .iter()
            .map(|(cmd, _)| cmd.to_string())
            .collect();
        self.palette_selected = 0;
    }

    fn set_active_tab(&mut self, tab: Tab) {
        let requested_tab = if matches!(tab, Tab::Details) {
            Tab::Run
        } else {
            tab
        };

        if !self.replay_mode {
            self.live_details_drawer_open = false;
        }

        self.active_tab = requested_tab;
        self.normalize_focus_for_active_tab();
    }

    fn normalize_focus_for_active_tab(&mut self) {
        if self.replay_mode {
            if self.active_tab != Tab::Run && self.focus == Focus::Prompt {
                self.focus = Focus::List;
            }
            return;
        }

        match self.active_tab {
            Tab::Run => {
                if !self.details_drawer_open() && self.focus == Focus::List {
                    self.focus = Focus::Details;
                }
            }
            Tab::Details => {
                if self.focus == Focus::Prompt {
                    self.focus = Focus::Details;
                }
            }
            Tab::Events | Tab::Diff | Tab::Help => {
                if self.focus == Focus::Prompt {
                    self.focus = Focus::List;
                }
            }
        }
    }

    fn cycle_focus_forward(&mut self) {
        self.focus = if !self.replay_mode && self.active_tab == Tab::Run {
            if self.details_drawer_open() {
                match self.focus {
                    Focus::Prompt => Focus::List,
                    Focus::List => Focus::Details,
                    Focus::Details => Focus::Prompt,
                }
            } else {
                match self.focus {
                    Focus::Prompt => Focus::Details,
                    Focus::List | Focus::Details => Focus::Prompt,
                }
            }
        } else {
            match self.focus {
                Focus::List => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::Prompt => Focus::List,
            }
        };
    }

    fn cycle_focus_backward(&mut self) {
        self.focus = if !self.replay_mode && self.active_tab == Tab::Run {
            if self.details_drawer_open() {
                match self.focus {
                    Focus::Prompt => Focus::Details,
                    Focus::Details => Focus::List,
                    Focus::List => Focus::Prompt,
                }
            } else {
                match self.focus {
                    Focus::Prompt => Focus::Details,
                    Focus::List | Focus::Details => Focus::Prompt,
                }
            }
        } else {
            match self.focus {
                Focus::List => Focus::Prompt,
                Focus::Details => Focus::List,
                Focus::Prompt => Focus::Details,
            }
        };
    }

    fn execute_action(&mut self, action: Action) {
        // Check for modal (permission) first
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
                _ => {}
            }
        }

        // Handle prompt-focused actions
        if self.focus == Focus::Prompt {
            if self.prompt_bootstrap_disabled() {
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
                self.set_active_tab(Tab::Help);
            }
            Action::ToggleFollow => {
                self.follow_mode = !self.follow_mode;
            }
            Action::ToggleDetailsDrawer if !self.replay_mode && self.focus != Focus::Prompt => {
                let opening = self.active_tab != Tab::Run || !self.details_drawer_open();
                self.active_tab = Tab::Run;
                self.live_details_drawer_open = opening;
                if !opening && self.focus == Focus::List {
                    self.focus = Focus::Details;
                } else if opening && self.focus == Focus::Prompt {
                    self.focus = Focus::Details;
                }
            }
            Action::TabRun if self.focus != Focus::Prompt => {
                self.set_active_tab(Tab::Run);
            }
            Action::TabEvents if self.focus != Focus::Prompt => {
                self.set_active_tab(Tab::Events);
            }
            Action::TabDiff if self.focus != Focus::Prompt => {
                self.set_active_tab(Tab::Diff);
            }
            Action::TabHelp if self.focus != Focus::Prompt => {
                self.set_active_tab(Tab::Help);
            }
            Action::Reload if self.replay_mode => {
                self.reload_requested = true;
            }
            Action::MoveDown if self.focus != Focus::Prompt => {
                if matches!(self.active_tab, Tab::Run | Tab::Details) && self.focus == Focus::List {
                    self.next_activity();
                } else if self.focus == Focus::List {
                    self.next_event();
                } else {
                    if self.focus == Focus::Details {
                        self.details_scroll = self.details_scroll.saturating_add(1);
                    }
                }
            }
            Action::MoveUp if self.focus != Focus::Prompt => {
                if matches!(self.active_tab, Tab::Run | Tab::Details) && self.focus == Focus::List {
                    self.previous_activity();
                } else if self.focus == Focus::List {
                    self.previous_event();
                } else {
                    if self.focus == Focus::Details {
                        self.details_scroll = self.details_scroll.saturating_sub(1);
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
        }
    }

    fn previous_activity(&mut self) {
        if self.selected_activity_index > 0 {
            self.selected_activity_index -= 1;
            self.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    fn _handle_modal_key(&mut self, key_code: KeyCode) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match key_code {
            KeyCode::Char('a') => {
                self.send_permission_intent(permission_id, PermissionDecision::Allow);
                true
            }
            KeyCode::Char('d') => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny);
                true
            }
            KeyCode::Esc => {
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
        if self.prompt_buffer.trim().is_empty()
            || self.active_turn_in_progress()
            || self.prompt_bootstrap_disabled()
        {
            return;
        }

        let text = self.prompt_buffer.clone();
        self.prompt_history.push(text.clone());
        self.clear_prompt_input();
        self.echo_submitted_prompt(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt { text });
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
