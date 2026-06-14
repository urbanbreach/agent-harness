use std::cell::Cell;
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
use crate::overlay::{OverlayKind, OverlayStack};
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
mod auth_display;
mod child_session;
mod child_session_dialog;
mod error_details;
#[cfg(test)]
mod exact_tests;
mod file_mentions;
mod key_interaction;
mod key_sequence;
mod lifecycle;
mod lineage;
mod model_dialog;
mod model_dialog_state;
mod model_metadata;
mod model_switcher;
mod mouse_interaction;
mod onboarding;
mod pending_live;
pub(crate) mod permissions;
mod prompt_editor;
mod prompt_history;
mod prompt_input;
mod prompt_management_keys;
mod queued_prompts;
pub(crate) mod session_history;
mod session_history_state;
pub(crate) mod session_navigation;
mod session_projection;
mod session_slash;
mod session_stack;
mod state;
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
use self::auth_display::auth_status_banner;
pub use self::lifecycle::{
    default_shell_registry, Focus, LifecycleShellState, MemoryCaps, PostRunHandoffAction,
    ReviewSurface, ShellDescriptor, ShellKind, StartupLauncherAction, Tab, UiIntent,
};
use self::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
pub use self::session_history::SessionHistoryEntry;
use self::session_projection::SessionProjection;
use self::session_stack::SessionNavigationSnapshot;
pub(crate) use self::state::TranscriptScrollbarDragState;
pub(crate) use self::state::{
    ComposerState, KeySequenceState, OverlayStateHolder, PermissionPromptState,
    QuestionPromptState, TranscriptViewState,
};
use self::terminal_panel::terminal_panel_event_is_shell;
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
use self::workspace_display::{directory_branch_label, workspace_context_labels};
pub use crate::view_model::{ForkSelectorViewModel, LineageBrowserViewModel};
pub use child_session_dialog::{ChildSessionDialogRow, ChildSessionDialogViewModel};
pub use error_details::ErrorDetailsViewModel;
#[cfg(test)]
pub(crate) use file_mentions::FileMentionSelectedTag;
pub(crate) use file_mentions::{
    system_file_mention_now_unix, system_file_mention_workspace_root, FileMentionEntry,
    FileMentionFrecency, FileMentionIndex, FileMentionTag, FileMentionWorkspaceScanner,
    SystemFileMentionWorkspaceScanner,
};
pub use lineage::{ForkSelectorState, LineageBrowserState};
use model_dialog_state::ModelDialogState;
pub use model_metadata::{LaunchMetadata, McpResourceOption, ModelOption};
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
pub(in crate::app) use prompt_editor::is_prompt_editor_action;
pub use prompt_editor::ComposerMode;
pub use prompt_history::prompt_history_path_for_session_dir;
use prompt_history::PromptHistoryDraft;
use session_history_state::SessionHistoryUiState;
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
    toast: Option<ToastState>,
    pub details_scroll: u16,
    pub terminal_panel_scroll: usize,
    pub terminal_panel_follow: bool,
    pub(crate) last_terminal_panel_max_scroll: Cell<usize>,
    last_frame_area: Option<Rect>,
    operator_sidebar_selection: Option<OperatorSidebarSelection>,
    selected_operator_sidebar_keyboard_index: Option<usize>,
    operator_sidebar_selection_dragging: bool,
    operator_sidebar_pending_click: Option<OperatorSidebarPendingClick>,
    pub transcript_view: TranscriptViewState,
    pub auto_exit_on_finish: bool,
    pub composer: ComposerState,
    pub selected_activity_index: usize,
    pub palette_input: String,
    pub palette_cursor: usize,
    pub palette_filtered: Vec<String>,
    pub palette_selected: usize,
    palette_focus_return: Option<Focus>,
    terminal_panel_visible: bool,
    collapsed_operator_sidebar_sections: BTreeSet<OperatorSidebarSection>,
    expanded_operator_sidebar_subagent_groups: BTreeSet<String>,
    pub startup_mode: bool,
    pub startup_launcher_action: StartupLauncherAction,
    pub(crate) onboarding_step: OnboardingStep,
    pub(crate) onboarding_selected: usize,
    pub(crate) onboarding_skipped_for_launch: bool,
    pub(crate) onboarding_auth_in_progress: bool,
    pub(crate) onboarding_secret_input: String,
    post_run_handoff_action: PostRunHandoffAction,
    continued_post_run_handoff_active: bool,
    continued_live_reopen_surface_active: bool,
    pub overlay_state: OverlayStateHolder,
    pub session_history_entries: Vec<SessionHistoryEntry>,
    pub session_history_filtered: Vec<usize>,
    pub session_history_selected: usize,
    session_history_ui: SessionHistoryUiState,
    model_dialog_state: ModelDialogState,
    pub model_options: Vec<ModelOption>,
    pub model_filtered: Vec<usize>,
    pub model_selected: usize,
    pub toggles_selected: usize,
    toggles_yolo_confirm_visible: bool,
    runtime_toggles: toggles::RuntimeTogglesState,
    pub lineage_browser: LineageBrowserState,
    pub fork_selector: ForkSelectorState,
    pub slash_filtered: Vec<String>,
    pub slash_selected: usize,
    slash_draft_snapshot: Option<String>,
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
    key_sequence_state: KeySequenceState,
    theme: Theme,
    launch_metadata: LaunchMetadata,
    runtime_context_metadata: Option<LaunchMetadata>,
    session_navigation_stack: Vec<SessionNavigationSnapshot>,
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
            toast: None,
            details_scroll: 0,
            terminal_panel_scroll: 0,
            terminal_panel_follow: true,
            last_terminal_panel_max_scroll: Cell::new(0),
            last_frame_area: None,
            operator_sidebar_selection: None,
            selected_operator_sidebar_keyboard_index: None,
            operator_sidebar_selection_dragging: false,
            operator_sidebar_pending_click: None,
            transcript_view: TranscriptViewState::default(),
            auto_exit_on_finish: false,
            composer: ComposerState::default(),
            selected_activity_index: 0,
            palette_input: String::new(),
            palette_cursor: 0,
            palette_filtered: Vec::new(),
            palette_selected: 0,
            palette_focus_return: None,
            terminal_panel_visible: false,
            collapsed_operator_sidebar_sections: BTreeSet::from([
                OperatorSidebarSection::ModifiedFiles,
            ]),
            expanded_operator_sidebar_subagent_groups: BTreeSet::new(),
            startup_mode: false,
            startup_launcher_action: StartupLauncherAction::default(),
            onboarding_step: OnboardingStep::StartSplash,
            onboarding_selected: 0,
            onboarding_skipped_for_launch: false,
            onboarding_auth_in_progress: false,
            onboarding_secret_input: String::new(),
            post_run_handoff_action: PostRunHandoffAction::default(),
            continued_post_run_handoff_active: false,
            continued_live_reopen_surface_active: false,
            overlay_state: OverlayStateHolder::default(),
            session_history_entries: Vec::new(),
            session_history_filtered: Vec::new(),
            session_history_selected: 0,
            session_history_ui: SessionHistoryUiState::default(),
            model_dialog_state: ModelDialogState::default(),
            model_options: Vec::new(),
            model_filtered: Vec::new(),
            model_selected: 0,
            toggles_selected: 0,
            toggles_yolo_confirm_visible: false,
            runtime_toggles: toggles::RuntimeTogglesState::default(),
            lineage_browser: LineageBrowserState::default(),
            fork_selector: ForkSelectorState::default(),
            slash_filtered: Vec::new(),
            slash_selected: 0,
            slash_draft_snapshot: None,
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
            key_sequence_state: KeySequenceState::default(),
            theme: Theme::default(),
            launch_metadata: LaunchMetadata::default(),
            runtime_context_metadata: None,
            session_navigation_stack: Vec::new(),
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
    pub fn set_onboarding_required(&mut self, required: bool) {
        self.overlay_state.onboarding_visible = required && !self.onboarding_skipped_for_launch;
        if self.overlay_state.onboarding_visible {
            self.focus = Focus::List;
            self.onboarding_step = OnboardingStep::StartSplash;
            self.onboarding_selected = 0;
            self.onboarding_auth_in_progress = false;
            self.onboarding_secret_input.clear();
        }
    }

    pub fn onboarding_screen(&self) -> Option<OnboardingScreen> {
        self.overlay_state
            .onboarding_visible
            .then(|| onboarding::screen_for(self.onboarding_step, self.onboarding_selected))
    }

    pub fn set_onboarding_step_for_test(&mut self, step: OnboardingStep) {
        self.overlay_state.onboarding_visible = true;
        self.onboarding_step = step;
        self.onboarding_selected = 0;
        self.onboarding_auth_in_progress = false;
        self.onboarding_secret_input.clear();
        self.focus = Focus::List;
    }

    pub fn apply_auth_backend_result(&mut self, success: bool) {
        if !self.overlay_state.onboarding_visible || !self.onboarding_auth_in_progress {
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
        self.overlay_state.onboarding_visible
            && matches!(
                self.onboarding_step,
                OnboardingStep::ApiKeyEntry | OnboardingStep::CopilotEnterpriseDevice
            )
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
        self.permission_prompt.dismissed_permissions.clear();
        self.permission_prompt.submitted_permission_id = None;
        self.permission_prompt.permission_modal_permission_id = None;
        self.permission_prompt.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_prompt.permission_modal_selection = PermissionModalSelection::AllowOnce;
        self.permission_prompt.permission_modal_confirm_selection =
            PermissionConfirmSelection::Confirm;
        self.question_prompt.question_prompt_tab = 0;
        self.question_prompt.question_prompt_selection = 0;
        self.question_prompt.question_prompt_answers.clear();
        self.question_prompt.question_prompt_custom.clear();
        self.question_prompt.question_prompt_editing = false;
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

        let queued_prompt_event = queued_prompts::queued_prompt_runtime_event(&event);
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
        if !historical {
            if let Some(event) = queued_prompt_event {
                self.apply_queued_prompt_runtime_event(event);
            }
        }
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

        if self.transcript_view.follow_mode && !self.projection.events.is_empty() {
            self.selected_event_index = self.projection.events.len() - 1;
            self.selected_activity_index = self.projection.activities.len().saturating_sub(1);
            self.details_scroll = 0;
            self.transcript_view.transcript_scroll = 0;
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
            self.permission_prompt
                .dismissed_permissions
                .remove(&data.permission_id);
            self.clear_permission_modal_selection(&data.permission_id);
            if self.submitted_permission_is_active(&data.permission_id) {
                self.permission_prompt.submitted_permission_id = None;
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
