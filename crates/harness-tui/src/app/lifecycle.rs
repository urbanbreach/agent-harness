// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::*;

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
            Self::Events => "shortcuts",
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
    BackgroundForegroundSubagents,
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
    RevertWorkspace {
        snapshot_request_id: String,
    },
    UpdateSessionTitle {
        title: String,
    },
    DeleteSession {
        run_id: String,
        run_dir: PathBuf,
    },
    ExportSession,
    RunShellCommand {
        command: String,
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

    pub(in crate::app) const fn previous(self) -> Self {
        match self {
            Self::NewSession => Self::ReplaySession,
            Self::ContinueSession => Self::NewSession,
            Self::ReplaySession => Self::ContinueSession,
        }
    }

    pub(in crate::app) const fn next(self) -> Self {
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

impl AppState {
    pub(in crate::app) fn launch_value_is_unknown(value: &str) -> bool {
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
        let session_pins_path = session_path
            .as_deref()
            .map(session_pins::session_pins_path_for_session_dir);
        let model_favorites_path = session_path
            .as_deref()
            .map(model_favorites::model_favorites_path_for_session_dir);
        let model_recents_path = session_path
            .as_deref()
            .map(model_favorites::model_recents_path_for_session_dir);
        let prompt_stash_path = session_path
            .as_deref()
            .map(prompt_stash::prompt_stash_path_for_session_dir);
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
        state.set_prompt_history_path(prompt_history_path);
        state.set_session_history_entries(session_history_entries);
        state.set_session_pins_path(session_pins_path);
        state.set_model_favorites_path(model_favorites_path);
        state.set_model_recents_path(model_recents_path);
        state.set_prompt_stash_path(prompt_stash_path);
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
        let session_dir = prompt_history_path
            .as_deref()
            .and_then(|path| path.parent())
            .and_then(|parent| parent.parent())
            .map(PathBuf::from);
        let session_pins_path = session_dir
            .as_deref()
            .map(session_pins::session_pins_path_for_session_dir);
        let model_favorites_path = session_dir
            .as_deref()
            .map(model_favorites::model_favorites_path_for_session_dir);
        let model_recents_path = session_dir
            .as_deref()
            .map(model_favorites::model_recents_path_for_session_dir);
        let prompt_stash_path = session_dir
            .as_deref()
            .map(prompt_stash::prompt_stash_path_for_session_dir);
        state.set_prompt_history_path(prompt_history_path);
        state.set_session_pins_path(session_pins_path);
        state.set_model_favorites_path(model_favorites_path);
        state.set_model_recents_path(model_recents_path);
        state.set_prompt_stash_path(prompt_stash_path);
        if let Some(launch_metadata) = take_pending_live_launch_metadata() {
            state.set_launch_metadata(launch_metadata);
        }
        state.set_session_history_entries(session_history_entries);
        if let Some(pending_prompt) = take_pending_live_prompt() {
            state.replace_prompt_input(pending_prompt.text);
        }
        let connect_providers = take_pending_connect_providers();
        state.set_connect_dialog_providers(connect_providers);
        state
    }
}

impl AppState {
    fn set_prompt_history_path(&mut self, path: Option<PathBuf>) {
        self.composer.prompt_history_path = path;
        let Some(path) = self.composer.prompt_history_path.as_deref() else {
            return;
        };
        match prompt_history::load_prompt_history(path) {
            Ok(history) => {
                self.composer.prompt_history = history;
            }
            Err(err) => {
                self.status_banner = Some(err);
            }
        }
    }

    fn set_session_pins_path(&mut self, path: Option<PathBuf>) {
        self.session_pins_path = path;
        if let Some(path) = self.session_pins_path.as_deref() {
            match session_pins::load_session_pins(path) {
                Ok(pins) => {
                    self.session_pins = pins;
                }
                Err(err) => {
                    self.status_banner = Some(err);
                }
            }
        }
    }

    fn set_model_favorites_path(&mut self, path: Option<PathBuf>) {
        self.model_favorites_path = path;
        if let Some(path) = self.model_favorites_path.as_deref() {
            match model_favorites::load_model_favorites(path) {
                Ok(favorites) => {
                    self.model_favorites = favorites;
                }
                Err(err) => {
                    self.status_banner = Some(err);
                }
            }
        }
    }

    fn set_model_recents_path(&mut self, path: Option<PathBuf>) {
        self.model_recents_path = path;
        if let Some(path) = self.model_recents_path.as_deref() {
            match model_favorites::load_model_recents(path) {
                Ok(recents) => {
                    self.model_recents = recents;
                }
                Err(err) => {
                    self.status_banner = Some(err);
                }
            }
        }
    }

    fn set_prompt_stash_path(&mut self, path: Option<PathBuf>) {
        self.prompt_stash.path = path;
        if let Some(path) = self.prompt_stash.path.as_deref() {
            match prompt_stash::load_prompt_stash(path) {
                Ok(entries) => {
                    self.prompt_stash.entries = entries;
                }
                Err(err) => {
                    self.status_banner = Some(err);
                }
            }
        }
    }
}

impl AppState {
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
            activity_count: self.runtime_state_activity_count(),
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

    pub(crate) fn is_first_run(&self) -> bool {
        self.startup_mode && self.session_history_entries.is_empty()
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
}

impl AppState {
    pub fn default_shell_registry(&self) -> &'static [ShellDescriptor] {
        default_shell_registry(self.replay_mode)
    }

    pub fn details_drawer_open(&self) -> bool {
        !self.replay_mode && self.active_tab == Tab::Run && self.live_details_drawer_open
    }

    pub(in crate::app) fn session_shell_operator_rail_interactive(&self) -> bool {
        self.details_drawer_open() || (!self.replay_mode && self.operator_rail_has_sections())
    }

    pub fn review_surface(&self) -> Option<ReviewSurface> {
        self.active_review_surface
    }

    pub(crate) fn overlay_state(&self) -> OverlayState {
        OverlayState {
            details_drawer_open: self.details_drawer_open(),
            slash_visible: self.slash_overlay_should_render(),
            file_mention_visible: self.file_mention_overlay_should_render(),
            palette_visible: self.palette_visible,
            status_dialog_visible: self.secondary_surfaces.status_dialog_visible(),
            subagent_actions_visible: self.subagent_actions_session_id.is_some(),
            session_history_visible: self.session_history_visible,
            model_switcher_visible: self.model_switcher_visible,
            toggles_menu_visible: self.toggles_menu_visible,
            lineage_browser_visible: self.lineage_browser_visible,
            fork_selector_visible: self.fork_selector_visible,
            permission_pending: self.active_permission().is_some(),
            theme_dialog_visible: self.theme_dialog_visible,
            error_details_visible: self.error_details_visible,
            prompt_stash_list_visible: self.prompt_stash.list_visible,
            auth_dialog_visible: self.connect_dialog.visible,
        }
    }

    pub fn overlay_stack(&self) -> OverlayStack {
        OverlayStack::from_state(self.overlay_state())
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

    pub fn most_recent_workspace_snapshot_request_id(&self) -> Option<String> {
        self.events.iter().rev().find_map(|envelope| {
            if let harness_core::event::EventV1::WorkspaceSnapshot(snapshot) = &envelope.payload {
                Some(snapshot.request_id.to_string())
            } else {
                None
            }
        })
    }

    pub fn request_workspace_revert(&mut self) {
        if let Some(snapshot_request_id) = self.most_recent_workspace_snapshot_request_id() {
            self.emit_ui_intent(UiIntent::RevertWorkspace {
                snapshot_request_id,
            });
        } else {
            self.status_banner = Some("no workspace snapshot available to revert to".to_string());
        }
    }

    pub(crate) fn hidden_delegated_child_request_ids_in_current_view(&self) -> BTreeSet<&str> {
        self.delegated_child_request_ids_for_parent_view(self.current_session_id())
    }

    pub(in crate::app) fn active_turn_in_progress(&self) -> bool {
        let hidden_child_request_ids = self.hidden_delegated_child_request_ids_in_current_view();
        self.activities
            .iter()
            .filter(|activity| !hidden_child_request_ids.contains(activity.request_id.as_str()))
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
            .is_some_and(|deadline| self.now() < deadline)
            && !active_task_ids.is_empty()
            && self.interrupt_confirm_task_ids == *active_task_ids
    }

    pub(in crate::app) fn clear_expired_interrupt_confirmation(&mut self) {
        if self
            .interrupt_confirm_deadline
            .is_some_and(|deadline| self.now() >= deadline)
        {
            self.reset_interrupt_confirmation();
        }
    }

    fn reset_interrupt_confirmation(&mut self) {
        self.interrupt_confirm_deadline = None;
        self.interrupt_confirm_task_ids.clear();
    }

    pub(in crate::app) fn handle_interrupt_escape(&mut self) -> bool {
        let active_task_ids = self.active_interrupt_task_ids();
        if !self.interrupt_hint_visible() || active_task_ids.is_empty() {
            self.reset_interrupt_confirmation();
            return false;
        }

        self.reset_interrupt_confirmation();
        true
    }

    pub(in crate::app) fn handle_ctrl_c_clear_or_cancel(&mut self) -> bool {
        if !self.composer_disabled() && !self.composer.prompt_buffer.is_empty() {
            self.composer.push_undo();
            self.clear_prompt_input();
            if self.composer.shell_mode {
                self.composer.shell_mode = false;
            }
            return true;
        }

        let active_task_ids = self.active_interrupt_task_ids();
        if !self.interrupt_hint_visible() || active_task_ids.is_empty() {
            self.reset_interrupt_confirmation();
            return false;
        }

        let task_ids = active_task_ids.into_iter().collect();
        self.emit_ui_intent(UiIntent::InterruptSession { task_ids });
        self.reset_interrupt_confirmation();
        true
    }

    pub(in crate::app) fn now(&self) -> Instant {
        (self.now_fn)()
    }

    #[cfg(test)]
    pub(crate) fn set_now_fn_for_test(
        &mut self,
        now_fn: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) {
        self.now_fn = now_fn;
    }

    #[cfg(test)]
    pub(crate) fn clear_prompt_hint_for_test() -> &'static str {
        CLEAR_PROMPT_HINT
    }

    fn runtime_state_activity(&self) -> Option<&ActivityEntry> {
        let hidden_child_request_ids = self.hidden_delegated_child_request_ids_in_current_view();
        self.activities
            .iter()
            .rev()
            .filter(|activity| !hidden_child_request_ids.contains(activity.request_id.as_str()))
            .find(|activity| activity.status == ActivityStatus::Streaming)
            .or_else(|| {
                self.activities.iter().rev().find(|activity| {
                    !hidden_child_request_ids.contains(activity.request_id.as_str())
                })
            })
    }

    fn runtime_state_activity_count(&self) -> usize {
        let hidden_child_request_ids = self.hidden_delegated_child_request_ids_in_current_view();
        self.activities
            .iter()
            .filter(|activity| !hidden_child_request_ids.contains(activity.request_id.as_str()))
            .count()
    }
}
