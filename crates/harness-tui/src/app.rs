use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
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
use crate::text::{non_empty_trimmed, trimmed_json_string_field};
use crate::theme::Theme;
use crate::ui::{
    OperatorSidebarKeyboardTarget, OperatorSidebarKeyboardTargetKind, OperatorSidebarSelection,
    OperatorSidebarSelectionCell, TranscriptMouseTarget, TranscriptScrollbarHit,
    TranscriptSelection, TranscriptSelectionCell, WheelTarget,
};
use crate::view_model;
use crate::{clipboard, ui};

mod activity;
pub mod auth_dialog;
mod auth_display;
mod child_session;
mod composer;
mod composer_editing;
#[cfg(test)]
mod exact_tests;
mod file_mentions;
mod key_interaction;
mod lifecycle;
mod lineage;
mod model_favorites;
mod model_metadata;
mod model_switcher;
mod mouse_interaction;
mod operator_sidebar;
pub(crate) mod palette_controller;
mod pending_live;
mod permission_prompt;
pub(crate) mod permissions;
mod prompt_history;
mod prompt_input;
mod prompt_stash;
mod prompt_stash_actions;
mod question_prompt;
pub(crate) mod session_history;
pub(crate) mod session_navigation;
mod session_pins;
mod session_projection;
mod session_slash;
mod session_stack;
mod terminal_panel;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use exact_tests::*;
mod toggles;
mod tool_call;
mod tool_output;
mod transcript_cache;
mod transcript_state;
mod transcript_view;
mod workspace_display;

pub(crate) use self::activity::{
    humanize_profile_label, task_completed_updates_assistant_transcript,
};
pub(in crate::app) use self::activity::{
    mark_activity_event, merge_orchestration_task_completion_metadata,
    merge_orchestration_task_event, new_streaming_activity_entry, NewStreamingActivityEntryArgs,
};
pub use self::activity::{
    ActiveContextUsage, ActivityCacheUsage, ActivityEntry, ActivityStatus, ActivityUsage,
    CompactionState, CompactionStatus, CompactionUsageMetrics, OrchestrationOwnerLabels,
    OrchestrationSummary, OrchestrationTaskRow, OrchestrationTaskState, RuntimeState,
    RuntimeStateKind,
};
pub use self::auth_dialog::{ConnectDialogState, ConnectProviderOption};
use self::auth_display::auth_status_banner;
use self::composer::ComposerState;
pub use self::lifecycle::{
    default_shell_registry, Focus, LifecycleShellState, MemoryCaps, PostRunHandoffAction,
    ReviewSurface, ShellDescriptor, ShellKind, StartupLauncherAction, Tab, UiIntent,
};
use self::permission_prompt::PermissionPromptState;
use self::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use self::prompt_stash::{PromptStashEntry, PromptStashState};
use self::question_prompt::QuestionPromptState;
pub use self::session_history::SessionHistoryEntry;
use self::session_projection::SessionProjection;
use self::session_stack::SessionNavigationSnapshot;
use self::terminal_panel::terminal_panel_event_is_shell;
use self::terminal_panel::TerminalPanelState;
pub use self::terminal_panel::{TerminalPanelEntry, TerminalPanelStatus};
pub(in crate::app) use self::tool_call::{
    execution_timing_elapsed_ms, merge_resolved_tool_identity, merge_tool_call_metadata,
};
pub use self::tool_call::{
    EditDisplayStatus, EditEntry, TaskLineageEntry, ToolArtifactEntry, ToolCallDisplayStatus,
    ToolCallEntry,
};
use self::tool_output::{
    json_string_field, task_child_request_id_from_output, task_child_session_id_from_output,
    tool_call_has_expandable_output,
};
pub(crate) use self::transcript_state::{ToastState, ToastVariant};
use self::transcript_view::TranscriptViewState;
use self::workspace_display::{directory_branch_label, workspace_context_labels};
pub use crate::view_model::{ForkSelectorViewModel, LineageBrowserViewModel};
#[cfg(test)]
pub(crate) use file_mentions::FileMentionSelectedTag;
pub(crate) use file_mentions::{
    system_file_mention_now_unix, system_file_mention_workspace_root, FileMentionEntry,
    FileMentionFrecency, FileMentionIndex, FileMentionTag, FileMentionWorkspaceScanner,
    SystemFileMentionWorkspaceScanner,
};
pub use lineage::{ForkSelectorState, LineageBrowserState};
pub use model_metadata::{LaunchMetadata, McpResourceOption, ModelOption};
use operator_sidebar::OperatorSidebarState;
pub use pending_live::{
    set_pending_connect_providers, set_pending_live_launch_metadata,
    set_pending_live_prompt_auto_submit, set_pending_live_prompt_draft,
};
use pending_live::{
    take_pending_connect_providers, take_pending_live_launch_metadata, take_pending_live_prompt,
    PendingLivePrompt,
};
use permissions::permission_display_summary;
pub use permissions::{
    ActivePermissionView, PermissionEntry, QuestionOptionView, QuestionPromptView,
};
pub use prompt_history::prompt_history_path_for_session_dir;
pub use prompt_stash::prompt_stash_path_for_session_dir;
pub use toggles::{ToggleEntryConfig, ToggleEntryKind, ToggleMenuRow, TogglesConfig};

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;
const INTERRUPT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum OperatorSidebarSection {
    Todo,
    Subagents,
    Mcp,
    Lsp,
    ModifiedFiles,
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
pub(crate) struct TranscriptScrollbarDragState {
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

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

const NO_PROVIDER_BANNER: &str =
    "No provider connected. Run `harness auth login` in a terminal or use /connect to set up a provider.";

pub struct AppState {
    pub selected_event_index: usize,
    pub focus: Focus,
    pub active_tab: Tab,
    pub(crate) active_review_surface: Option<ReviewSurface>,
    pub(crate) live_details_drawer_open: bool,
    projection: SessionProjection,
    pub should_quit: bool,
    pub replay_mode: bool,
    pub session_path: Option<PathBuf>,
    pub status_banner: Option<String>,
    pub connect_dialog: ConnectDialogState,
    toast: Option<ToastState>,
    pub details_scroll: u16,
    pub(crate) terminal_panel: TerminalPanelState,
    last_frame_area: Option<Rect>,
    hovered_subagent_footer_target: Option<SubagentFooterTarget>,
    pub(crate) operator_sidebar: OperatorSidebarState,
    pub(crate) transcript_view: TranscriptViewState,
    pub auto_exit_on_finish: bool,
    pub composer: ComposerState,
    pub prompt_stash: PromptStashState,
    pub queued_prompt_count: usize,
    pub palette_visible: bool,
    pub palette_input: String,
    pub palette_cursor: usize,
    pub palette_filtered: Vec<String>,
    pub palette_selected: usize,
    pub palette_log: Vec<palette_controller::PaletteLogEntry>,
    palette_focus_return: Option<Focus>,
    pub(crate) status_dialog_visible: bool,
    error_details_visible: bool,
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
    pub session_pins: BTreeSet<String>,
    session_pins_path: Option<PathBuf>,
    pub session_delete_armed_run_id: Option<String>,
    pub session_rename_visible: bool,
    pub session_rename_input: String,
    pub session_rename_cursor: usize,
    pub session_rename_target_run_id: Option<String>,
    pub theme_dialog_visible: bool,
    pub theme_dialog_selected: usize,
    pub theme_name: String,
    pub model_options: Vec<ModelOption>,
    pub model_filtered: Vec<usize>,
    pub model_selected: usize,
    pub model_favorites: BTreeSet<String>,
    model_favorites_path: Option<PathBuf>,
    pub model_recents: Vec<String>,
    model_recents_path: Option<PathBuf>,
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
    pub(crate) permission_prompt: PermissionPromptState,
    pub(crate) question_prompt: QuestionPromptState,
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
            active_tab: Tab::default(),
            active_review_surface: None,
            live_details_drawer_open: false,
            projection: SessionProjection::default(),
            should_quit: false,
            replay_mode: false,
            session_path: None,
            status_banner: None,
            connect_dialog: ConnectDialogState::default(),
            toast: None,
            details_scroll: 0,
            terminal_panel: TerminalPanelState::default(),
            last_frame_area: None,
            hovered_subagent_footer_target: None,
            operator_sidebar: OperatorSidebarState::default(),
            transcript_view: TranscriptViewState::default(),
            auto_exit_on_finish: false,
            composer: ComposerState::default(),
            prompt_stash: PromptStashState::default(),
            queued_prompt_count: 0,
            palette_visible: false,
            palette_input: String::new(),
            palette_cursor: 0,
            palette_filtered: Vec::new(),
            palette_selected: 0,
            palette_log: Vec::new(),
            palette_focus_return: None,
            status_dialog_visible: false,
            error_details_visible: false,
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
            session_pins: BTreeSet::new(),
            session_pins_path: None,
            session_delete_armed_run_id: None,
            session_rename_visible: false,
            session_rename_input: String::new(),
            session_rename_cursor: 0,
            session_rename_target_run_id: None,
            theme_dialog_visible: false,
            theme_dialog_selected: 0,
            theme_name: "default".to_string(),
            model_options: Vec::new(),
            model_filtered: Vec::new(),
            model_selected: 0,
            model_favorites: BTreeSet::new(),
            model_favorites_path: None,
            model_recents: Vec::new(),
            model_recents_path: None,
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
            permission_prompt: PermissionPromptState::default(),
            question_prompt: QuestionPromptState::default(),
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
    pub fn apply_auth_backend_result(&mut self, success: bool) {
        self.apply_connect_dialog_auth_result(success);
        if success {
            if self.status_banner.as_deref() == Some(NO_PROVIDER_BANNER) {
                self.status_banner = None;
            }
        } else {
            self.status_banner = Some(
                "auth backend failed; run `harness auth login` in a terminal or use /connect"
                    .to_string(),
            );
        }
    }
    pub fn maybe_set_no_provider_banner(&mut self) {
        if self.replay_mode || !self.startup_mode {
            return;
        }
        if self.launch_metadata.available_models().is_empty() && self.status_banner.is_none() {
            self.status_banner = Some(NO_PROVIDER_BANNER.to_string());
        }
    }
    pub fn apply_keybindings(&mut self, bindings: std::collections::BTreeMap<String, String>) {
        self.keymap.apply_overrides(&bindings);
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub(crate) fn apply_theme_by_name(&mut self, name: &str) {
        match Theme::by_name(name) {
            Some(theme) => {
                self.theme = theme;
                self.theme_name = name.to_string();
                self.bump_transcript_render_epoch();
            }
            None => {
                self.theme = Theme::default();
                self.theme_name = "default".to_string();
                self.bump_transcript_render_epoch();
                self.status_banner =
                    Some(format!("unknown theme {name:?}; falling back to default"));
            }
        }
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
        self.permission_prompt.permission_id = None;
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.selection = PermissionModalSelection::AllowOnce;
        self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt.tab = 0;
        self.question_prompt.selection = 0;
        self.question_prompt.answers.clear();
        self.question_prompt.custom.clear();
        self.question_prompt.editing = false;
        self.transcript_view.expanded_tool_outputs.clear();
        self.transcript_view.expanded_patch_file_outputs.clear();

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
        self.transcript_view.transcript_scroll = 0;
        self.terminal_panel.scroll = 0;
        self.terminal_panel.follow = true;
        self.update_queued_prompt_count();
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
            self.operator_sidebar
                .collapsed_sections
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
        self.transcript_view.selected_activity_index = self
            .transcript_view
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

        if self.transcript_view.follow_mode && !self.projection.events.is_empty() {
            self.selected_event_index = self.projection.events.len() - 1;
            self.transcript_view.selected_activity_index =
                self.projection.activities.len().saturating_sub(1);
            self.details_scroll = 0;
            self.transcript_view.transcript_scroll = 0;
        }

        if terminal_panel_follow_event && self.terminal_panel.follow {
            self.terminal_panel.scroll = 0;
        }

        if terminal_event && !historical {
            self.close_palette();
            self.close_review_surface();
            if self.focus == Focus::Prompt {
                self.focus = Focus::Details;
            }
        }

        self.update_queued_prompt_count();
        self.maybe_auto_exit();
    }

    pub fn set_status_banner(&mut self, status: Option<String>) {
        self.status_banner = status;
    }

    pub(crate) fn set_compact_session_supported(&mut self, supported: bool) {
        self.compact_session_supported = supported;
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

    fn update_queued_prompt_count(&mut self) {
        let hidden_child_request_ids = self.hidden_delegated_child_request_ids_in_current_view();
        self.queued_prompt_count = self
            .activities
            .iter()
            .filter(|activity| {
                activity.status == ActivityStatus::Queued
                    && !hidden_child_request_ids.contains(activity.request_id.as_str())
            })
            .count();
    }

    pub(crate) fn has_revert_message(&self) -> bool {
        false
    }

    pub(crate) fn has_share_url(&self) -> bool {
        false
    }

    pub(crate) fn provider_disconnected(&self) -> bool {
        !self.launch_metadata.has_provider()
    }
}
