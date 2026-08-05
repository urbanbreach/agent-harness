// allow: SIZE_OK — TUI app state (session stack + permissions + composer + model switcher + tool output routing)
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_core::auto_fallback::{AutoFallbackOutcome, AutoFallbackSummary};
use harness_core::binary_update::{
    BinaryUpdateCheck, BinaryUpdatePolicy, BinaryUpdateSummary, BinaryVersionInfo,
};
use harness_core::browser_oidc::{
    BrowserOidcAvailability, BrowserOidcCompleteResult, BrowserOidcOutcomeSummary,
    BrowserOidcStartResult,
};
use harness_core::code_graph::{
    GraphQuery, GraphQueryBatchSummary, GraphQueryResult, PersistentGraphAvailability,
};
use harness_core::config::SettingsRegistrySummary;
use harness_core::cow_worktree::{CowCloneOutcomeSummary, CowCloneResult, CowWorktreeAvailability};
use harness_core::crash_recovery::{
    CrashRecoveryAction, CrashRecoveryScanSummary, PreviousCrashReport,
};
use harness_core::cron_schedule::{
    CronRegisterOutcome, CronRemoveOutcome, CronSchedule, CronScheduleSummary,
};
use harness_core::edit_attribution::EditAttributionSummary;
use harness_core::event::{
    ActorKind, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    ProviderRequestStartedEvent, ResolvedToolIdentity, RunFinishedEvent, TaskCompletionMetadata,
    TaskLineageMetadata, ToolCallLifecycleState, ToolCallMetadata, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::extension_manifest::{
    ExtensionDiscoverSummary, ExtensionLoadOutcome, ExtensionManifestSummary,
};
use harness_core::foreground_demote::{DemoteOutcomeSummary, DemoteToBackgroundResult};
use harness_core::foreign_session::{
    ForeignDiscoverSummary, ForeignImportOutcome, ForeignSessionCandidate,
};
use harness_core::integrations::{
    AcpBindOutcome, AcpConnectOutcome, AcpConnectionState, AcpConnectionSummary, AcpSessionInfo,
    PluginActivateOutcome, PluginDeactivateOutcome, PluginInstallOutcome, PluginLifecycleSummary,
    PluginRemoveOutcome,
};
use harness_core::jujutsu::{
    JujutsuAvailability, JujutsuCommandOutcome, JujutsuProbe, JujutsuWorkspaceStatus,
};
use harness_core::mcp_oauth::{
    McpOauthBeginResult, McpOauthOutcomeSummary, McpOauthRemoteAvailability,
    McpOauthTokenExchangeResult, McpRemoteTransportOpenResult,
};
use harness_core::perm::{PermissionDecision, PermissionGrantScope};
use harness_core::proj::{SessionCatalogEntry, SessionModeSource};
use harness_core::sandbox::{
    LandlockSupport, OsSandboxProfilesSummary, SandboxFsPlanSummary, SandboxPrepareResult,
};
use harness_core::sleep_wake_auth::{
    SleepWakeCredentialPolicy, SleepWakeHostEvent, SleepWakeObservation,
    SleepWakeObservationSummary, SleepWakeRefreshDecision,
};
use harness_core::team_registry::{
    TeamAddMemberOutcome, TeamCancelOutcome, TeamCreateOutcome, TeamRegistrySummary,
    TeamSendOutcome,
};
use harness_core::workspace::WorkspaceEnvironment;
use harness_core::workspace_hub::{
    WorkspaceHubAvailability, WorkspaceHubBindResult, WorkspaceHubConnectResult,
    WorkspaceHubOutcomeSummary, WorkspaceHubRecoveryResult, WorkspaceHubUploadResult,
};
use ratatui::layout::Rect;

use crate::keybindings::{Action, KeyBinding, KeyMap};
use crate::dashboard::{DashboardEligibilityRules, DashboardReplayRegistry, DashboardSessionInput};
use crate::dashboard_controls::DashboardControlState;
use crate::dashboard_details::{DashboardDetails, RosterState as DetailsRosterState};
use crate::dashboard_integration::{
    DashboardIntegration, DashboardIntegrationParts, DashboardReturnState,
};
use crate::dashboard_peek::DashboardPeek;
use crate::dashboard_roster::RosterState;
use crate::attachment_lifecycle::{
    Attachment, AttachmentError, AttachmentIngestor, AttachmentPolicy, CancellationToken,
};
use crate::completion_controller::{
    CompletionAcceptance, CompletionItem, CompletionRequest, CompletionTrigger, SelectionDirection,
};
use crate::composer_atoms::AttachmentId;
use crate::composer_integration::{ComposerUiIntent, ComposerViewModel};
use crate::design_contract::ViewportId;
use crate::overlay::{OverlayKind, OverlayStack, OverlayState};
use crate::prompt_queue_actions::{QueueAction, QueueError, QueueLifecycle};
use crate::text::{non_empty_trimmed, trimmed_json_string_field};
use crate::theme::{ColorLevel, Theme};
use crate::theme_family::{
    deserialize_choice, serialize_choice, AutoResolver, PersistError, ThemeChoice, ThemeFamily,
    ThemePreview,
};
use crate::transcript_integration::{TranscriptComposite, TranscriptViewModel};
use crate::transcript_identity::{TranscriptFocus, TranscriptScreenMode, TurnId};
use crate::ui::{
    OperatorSidebarKeyboardTarget, OperatorSidebarKeyboardTargetKind, OperatorSidebarSelection,
    OperatorSidebarSelectionCell, SubagentFooterTarget, TranscriptMouseTarget,
    TranscriptScrollbarHit, TranscriptSelection, TranscriptSelectionCell, WheelTarget,
};
use crate::view_model;
use crate::welcome_surface::{WelcomeHitMap, WelcomeLayout, WelcomeState};
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
pub mod footer_state;
mod foreign_import;
pub mod interaction_reducer;
mod key_interaction;
mod lifecycle;
mod lineage;
mod memory_browser;
mod model_favorites;
mod model_metadata;
mod model_switcher;
mod mouse_interaction;
mod new_worktree_dialog;
pub mod notifications;
mod operator_sidebar;
pub(crate) mod palette_controller;
mod pending_live;
mod permission_prompt;
pub(crate) mod permissions;
mod plan_view;
mod prompt_history;
mod prompt_input;
mod prompt_stash;
mod prompt_stash_actions;
mod question_prompt;
pub mod recovery_state;
mod secondary_surfaces;
pub(crate) mod session_history;
mod session_live_routing;
pub(crate) mod session_navigation;
mod session_pins;
mod session_projection;
mod session_slash;
mod session_stack;
mod settings_editor;
pub mod shell_status;
pub mod terminal_diagnostics;
mod terminal_panel;
#[cfg(test)]
mod tests;
pub mod theme_preview;
pub mod tips;
#[cfg(test)]
pub(crate) use exact_tests::*;
mod toggles;
mod tool_call;
mod tool_output;
mod transcript_cache;
mod transcript_state;
mod transcript_view;
mod workspace_display;
mod worktree_picker;

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
    ReviewSurface, SessionMode, ShellDescriptor, ShellKind, StartupLauncherAction, Tab, UiIntent,
};
use self::new_worktree_dialog::NewWorktreeDialogState;
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
pub use self::transcript_state::TranscriptInteractionSnapshot;
pub(crate) use self::transcript_state::{ToastState, ToastVariant};
use self::transcript_view::TranscriptViewState;
use self::workspace_display::{directory_branch_label, workspace_context_labels};

/// Deterministic workspace environment for snapshot/render tests: active when
/// running under a test runner (the `NEXTEST` env var, or the explicit
/// `HARNESS_TUI_TEST_WORKSPACE` escape hatch) so renders are identical across
/// worktrees instead of embedding the live checkout path and branch.
fn test_workspace_env_override() -> Option<WorkspaceEnvironment> {
    let active = cfg!(test)
        || std::env::var_os("NEXTEST").is_some()
        || std::env::var_os("HARNESS_TUI_TEST_WORKSPACE").is_some();
    if !active {
        return None;
    }
    Some(WorkspaceEnvironment {
        working_directory: std::path::PathBuf::from("/workspace/agent-harness"),
        workspace_root: std::path::PathBuf::from("/workspace/agent-harness"),
        is_git_repository: true,
        git_branch: Some("test-workspace".to_owned()),
    })
}
pub use crate::view_model::{ForkSelectorViewModel, LineageBrowserViewModel};
#[cfg(test)]
pub(crate) use file_mentions::FileMentionSelectedTag;
pub(crate) use file_mentions::{
    system_file_mention_now_unix, system_file_mention_workspace_root, FileMentionEntry,
    FileMentionFrecency, FileMentionIndex, FileMentionTag, FileMentionWorkspaceScanner,
    SystemFileMentionWorkspaceScanner,
};
pub use foreign_import::ForeignImportPickerState;
pub use lineage::{ForkSelectorState, LineageBrowserState};
pub use memory_browser::{MemoryBrowserEntry, MemoryBrowserState};
pub use model_metadata::{LaunchMetadata, McpResourceOption, ModelOption};
pub use pending_live::{
    set_pending_connect_providers, set_pending_live_launch_metadata,
    set_pending_live_prompt_auto_submit, set_pending_live_prompt_draft,
    set_pending_settings_project_config,
};
use pending_live::{
    take_pending_connect_providers, take_pending_live_launch_metadata, take_pending_live_prompt,
    take_pending_settings_project_config, PendingLivePrompt,
};
use permissions::permission_display_summary;
pub use permissions::{
    ActivePermissionView, PermissionEntry, QuestionOptionView, QuestionPromptView,
};
pub use prompt_history::prompt_history_path_for_session_dir;
pub use prompt_stash::prompt_stash_path_for_session_dir;
use secondary_surfaces::SecondarySurfaceState;
pub use toggles::{ToggleEntryConfig, ToggleEntryKind, ToggleMenuRow, TogglesConfig};
pub use worktree_picker::WorktreePickerState;

/// Truncation limit for tool output display in the TUI (chars)
const TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;

const CLEAR_PROMPT_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);
const CLEAR_PROMPT_HINT: &str = "press again to clear";

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct TranscriptScrollbarDragState {
    track: Rect,
    thumb_height: u16,
    pointer_offset_y: u16,
    max_scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum OperatorSidebarPendingClick {
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
    pub(crate) quit_confirmation_pending: bool,
    pub(crate) quit_confirmation_shortcut: Option<KeyBinding>,
    pub(crate) quit_confirmation_expires_at: Option<Instant>,
    pub replay_mode: bool,
    pub session_path: Option<PathBuf>,
    pub status_banner: Option<String>,
    pub connect_dialog: ConnectDialogState,
    toast: Option<ToastState>,
    pub details_scroll: u16,
    mouse_wheel_lines_per_tick: u16,
    pub(crate) terminal_panel: TerminalPanelState,
    last_frame_area: Option<Rect>,
    pub(crate) secondary_surfaces: SecondarySurfaceState,
    dashboard: Option<DashboardIntegration>,
    dashboard_return_focus: Option<Focus>,
    pub(crate) transcript_view: TranscriptViewState,
    pub(crate) transcript_integration: Option<TranscriptComposite>,
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
    pub(crate) subagent_actions_session_id: Option<String>,
    pub(crate) hovered_subagent_footer_target: Option<SubagentFooterTarget>,
    pub(crate) pending_subagent_footer_target: Option<SubagentFooterTarget>,
    error_details_visible: bool,
    pub startup_mode: bool,
    starting_session_seed: bool,
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
    pub settings_editor_visible: bool,
    pub settings_editor_selected: usize,
    pub(crate) settings_project_config_path: Option<PathBuf>,
    pub(crate) settings_hashline_edit: bool,
    pub(crate) settings_compaction_enabled: bool,
    pub(crate) settings_compaction_auto_retry_overflow: bool,
    pub(crate) settings_compaction_structured_summary_contract: bool,
    pub(crate) settings_compaction_estimated_token_triggers: bool,
    pub(crate) settings_deterministic_enabled: bool,
    /// Optional operator-facing plugin lifecycle counts for the status dialog.
    pub(crate) plugin_lifecycle_summary: Option<PluginLifecycleSummary>,
    /// Last plugin install attempt (diagnostics; does not load package code).
    pub(crate) plugin_last_install: Option<PluginInstallOutcome>,
    /// Last plugin activate attempt (permission-before-execution; no code load).
    pub(crate) plugin_last_activate: Option<PluginActivateOutcome>,
    /// Last plugin deactivate attempt (diagnostics; no package code load).
    pub(crate) plugin_last_deactivate: Option<PluginDeactivateOutcome>,
    /// Last plugin remove attempt (diagnostics; no package code load).
    pub(crate) plugin_last_remove: Option<PluginRemoveOutcome>,
    /// First installed plugin one-line (if any).
    pub(crate) plugin_first_line: Option<String>,
    /// Optional operator-facing multi-run crash scan counts for the status dialog.
    pub(crate) crash_recovery_scan_summary: Option<CrashRecoveryScanSummary>,
    pub(crate) crash_recovery_first_report: Option<PreviousCrashReport>,
    /// Resolved crash-recovery action policy (resumable vs reopen).
    pub(crate) crash_recovery_resolved_action: Option<CrashRecoveryAction>,
    /// First previous-crash report one-line (if any).
    pub(crate) crash_recovery_first_report_line: Option<String>,
    pub(crate) edit_attribution_summary: Option<EditAttributionSummary>,
    /// First attributed edit one-line (session-local; not VCS blame).
    pub(crate) edit_attribution_first_line: Option<String>,
    /// Last attributed edit one-line (session-local; not VCS blame).
    pub(crate) edit_attribution_last_line: Option<String>,
    /// Optional operator-facing multi-agent team registry counts for the status dialog.
    pub(crate) team_registry_summary: Option<TeamRegistrySummary>,
    /// Last team create outcome (diagnostics; not Team Mode product).
    pub(crate) team_last_create: Option<TeamCreateOutcome>,
    /// First registered team one-line (if any).
    pub(crate) team_first_line: Option<String>,
    /// Last team mailbox send outcome (diagnostics; not process IPC).
    pub(crate) team_last_send: Option<TeamSendOutcome>,
    /// Last team mailbox message one-line (if any).
    pub(crate) team_last_message_line: Option<String>,
    /// Last team add-member attempt (diagnostics; not Team Mode product).
    pub(crate) team_last_add_member: Option<TeamAddMemberOutcome>,
    /// Last team cancel attempt (diagnostics; not Team Mode product).
    pub(crate) team_last_cancel: Option<TeamCancelOutcome>,
    /// Optional operator-facing cron schedule registry counts for the status dialog.
    pub(crate) cron_schedule_summary: Option<CronScheduleSummary>,
    /// Last cron schedule registration outcome (diagnostics; not timer execution).
    pub(crate) cron_last_register: Option<CronRegisterOutcome>,
    /// First registered cron schedule one-line (if any).
    pub(crate) cron_first_schedule_line: Option<String>,
    /// Last cron remove outcome (diagnostics; fail-closed missing ok).
    pub(crate) cron_last_remove: Option<CronRemoveOutcome>,
    /// Optional operator-facing demote-outcome counts for the status dialog.
    pub(crate) demote_outcome_summary: Option<DemoteOutcomeSummary>,
    /// Last foreground→background demote attempt (diagnostics; not shell demote product).
    pub(crate) demote_last_result: Option<DemoteToBackgroundResult>,
    /// Last task-registry demote attempt (diagnostics; not shell demote product).
    pub(crate) demote_last_task_result: Option<DemoteToBackgroundResult>,
    /// Optional operator-facing auto-fallback chain counts for the status dialog.
    pub(crate) auto_fallback_summary: Option<AutoFallbackSummary>,
    /// Last auto-fallback resolution outcome (operator diagnostics; not a live switch).
    pub(crate) auto_fallback_last_outcome: Option<AutoFallbackOutcome>,
    /// Last operator banner for auto-fallback (format_auto_fallback_banner / describe).
    pub(crate) auto_fallback_last_banner: Option<String>,
    /// Resolved model-ref chain label (primary → fallback…); diagnostics only.
    pub(crate) auto_fallback_chain_label: Option<String>,
    /// Optional operator-facing extension descriptor counts for the status dialog.
    pub(crate) extension_manifest_summary: Option<ExtensionManifestSummary>,
    /// Extension descriptor discovery counts (diagnostics; not code load).
    pub(crate) extension_discover_summary: Option<ExtensionDiscoverSummary>,
    /// Last extension.manifest.json load attempt (fail-closed diagnostics).
    pub(crate) extension_last_load: Option<ExtensionLoadOutcome>,
    /// Optional operator-facing workspace-hub outcome counts for the status dialog.
    pub(crate) workspace_hub_outcome_summary: Option<WorkspaceHubOutcomeSummary>,
    /// Remote workspace hub availability (always unavailable in MVP).
    pub(crate) workspace_hub_availability: Option<WorkspaceHubAvailability>,
    /// Last hub connect attempt (fail-closed unavailable in MVP).
    pub(crate) workspace_hub_last_connect: Option<WorkspaceHubConnectResult>,
    /// Last workspace hub bind result (diagnostics; honest unavailable MVP).
    pub(crate) workspace_hub_last_bind: Option<WorkspaceHubBindResult>,
    /// Last workspace hub upload result (diagnostics; honest unavailable MVP).
    pub(crate) workspace_hub_last_upload: Option<WorkspaceHubUploadResult>,
    /// Last workspace hub recovery result (diagnostics; honest unavailable MVP).
    pub(crate) workspace_hub_last_recover: Option<WorkspaceHubRecoveryResult>,
    /// Optional operator-facing graph-query batch counts for the status dialog.
    pub(crate) graph_query_batch_summary: Option<GraphQueryBatchSummary>,
    /// Last thin graph query result (always unavailable in MVP; no hits claimed).
    pub(crate) graph_query_last_result: Option<GraphQueryResult>,
    /// First result one_line from last multi-kind batch (diagnostics; no hits claimed).
    pub(crate) graph_query_batch_first_line: Option<String>,
    pub(crate) persistent_graph_availability: Option<PersistentGraphAvailability>,
    /// Optional operator-facing COW-clone outcome counts for the status dialog.
    pub(crate) cow_clone_outcome_summary: Option<CowCloneOutcomeSummary>,
    /// Last single-file COW clone attempt (diagnostics; not git worktree product).
    pub(crate) cow_clone_last_result: Option<CowCloneResult>,
    pub(crate) cow_worktree_availability: Option<CowWorktreeAvailability>,
    /// Optional operator-facing browser-OIDC outcome counts for the status dialog.
    pub(crate) browser_oidc_outcome_summary: Option<BrowserOidcOutcomeSummary>,
    /// Browser/device OIDC availability (always unavailable in MVP).
    pub(crate) browser_oidc_availability: Option<BrowserOidcAvailability>,
    /// Last browser OIDC start attempt (fail-closed unavailable in MVP).
    pub(crate) browser_oidc_last_start: Option<BrowserOidcStartResult>,
    /// Last browser OIDC complete attempt (fail-closed unavailable in MVP).
    pub(crate) browser_oidc_last_complete: Option<BrowserOidcCompleteResult>,
    /// Optional operator-facing MCP OAuth outcome counts for the status dialog.
    pub(crate) mcp_oauth_outcome_summary: Option<McpOauthOutcomeSummary>,
    /// MCP OAuth remote transport availability (always unavailable in MVP).
    pub(crate) mcp_oauth_remote_availability: Option<McpOauthRemoteAvailability>,
    /// Last MCP OAuth begin attempt (fail-closed unavailable in MVP).
    pub(crate) mcp_oauth_last_begin: Option<McpOauthBeginResult>,
    /// Last MCP OAuth token exchange attempt (fail-closed unavailable in MVP).
    pub(crate) mcp_oauth_last_exchange: Option<McpOauthTokenExchangeResult>,
    /// Last MCP remote transport open attempt (fail-closed unavailable in MVP).
    pub(crate) mcp_oauth_last_open: Option<McpRemoteTransportOpenResult>,
    /// Optional operator-facing sleep/wake observation counts for the status dialog.
    pub(crate) sleep_wake_observation_summary: Option<SleepWakeObservationSummary>,
    /// Last host sleep/wake observation (recorded-noop only in MVP).
    pub(crate) sleep_wake_last_observation: Option<SleepWakeObservation>,
    /// Accumulated host sleep/wake observations for operator summary.
    pub(crate) sleep_wake_observation_log: Vec<SleepWakeObservation>,
    /// Last credential refresh decision for a host sleep/wake event (always Skip in MVP).
    pub(crate) sleep_wake_last_decision: Option<SleepWakeRefreshDecision>,
    /// Sleep/wake credential refresh policy echo (noop/unavailable; never Active).
    pub(crate) sleep_wake_credential_policy: Option<SleepWakeCredentialPolicy>,
    /// Sleep/wake credential refresh availability alias (Unavailable MVP).
    pub(crate) sleep_wake_availability: Option<SleepWakeCredentialPolicy>,
    /// Optional operator-facing binary-update check counts for the status dialog.
    pub(crate) binary_update_summary: Option<BinaryUpdateSummary>,
    /// Operator update policy echo (channel/min-version; diagnostics only).
    pub(crate) binary_update_policy: Option<BinaryUpdatePolicy>,
    /// Last offline update check (structured unavailable; never claims success).
    pub(crate) binary_update_check: Option<BinaryUpdateCheck>,
    /// Current binary package version (compile-time; always available).
    pub(crate) binary_version_info: Option<BinaryVersionInfo>,
    /// Optional operator-facing settings-registry composition counts for the status dialog.
    pub(crate) settings_registry_summary: Option<SettingsRegistrySummary>,
    /// Optional operator-facing foreign-session discover counts for the status dialog.
    pub(crate) foreign_discover_summary: Option<ForeignDiscoverSummary>,
    /// First importable foreign-session candidate from the latest discover scan.
    pub(crate) foreign_import_first_candidate: Option<ForeignSessionCandidate>,
    /// Last foreign-session import attempt (fail-closed diagnostics).
    pub(crate) foreign_import_last_outcome: Option<ForeignImportOutcome>,
    /// Optional operator-facing jujutsu CLI/workspace probe for the status dialog.
    pub(crate) jujutsu_probe: Option<JujutsuProbe>,
    /// Jujutsu CLI availability component (from probe; not worktree product).
    pub(crate) jujutsu_cli: Option<JujutsuAvailability>,
    /// Jujutsu workspace repo status component (from probe; not worktree product).
    pub(crate) jujutsu_workspace: Option<JujutsuWorkspaceStatus>,
    pub(crate) jujutsu_last_command: Option<JujutsuCommandOutcome>,
    /// Optional operator-facing sandbox FS-plan summary for the status dialog.
    pub(crate) sandbox_fs_plan_summary: Option<SandboxFsPlanSummary>,
    pub(crate) landlock_support: Option<LandlockSupport>,
    /// OS sandbox profile listing summary (diagnostics; not enforcement proof).
    pub(crate) os_sandbox_profiles_summary: Option<OsSandboxProfilesSummary>,
    /// First OS sandbox profile one-line (diagnostics; not enforcement proof).
    pub(crate) os_sandbox_first_profile_line: Option<String>,
    /// Last sandbox prepare result (diagnostics; honest unavailable/not-required).
    pub(crate) sandbox_last_prepare: Option<SandboxPrepareResult>,
    /// Optional operator-facing ACP connection summary for the status dialog.
    pub(crate) acp_connection_summary: Option<AcpConnectionSummary>,
    /// Last ACP connection state (state machine only; not full protocol).
    pub(crate) acp_connection_state: Option<AcpConnectionState>,
    /// Bound ACP session metadata when present (local bookkeeping only).
    pub(crate) acp_session_info: Option<AcpSessionInfo>,
    /// Last ACP connect outcome (diagnostics; mock/fail-closed MVP).
    pub(crate) acp_last_connect: Option<AcpConnectOutcome>,
    /// Last ACP session bind outcome (diagnostics; fail-closed when disconnected).
    pub(crate) acp_last_bind: Option<AcpBindOutcome>,
    pub plan_view_visible: bool,
    pub trust_folder_prompt_visible: bool,
    pub plan_view_selected: usize,
    pub plan_view_preview: Option<String>,
    pub theme_name: String,
    theme_choice: ThemeChoice,
    theme_family: ThemeFamily,
    theme_preview: ThemePreview,
    auto_theme_resolver: AutoResolver,
    theme_color_level: ColorLevel,
    welcome: WelcomeState,
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
    always_approve_mode: bool,
    session_mode: SessionMode,
    runtime_toggles: toggles::RuntimeTogglesState,
    pub lineage_browser: LineageBrowserState,
    pub lineage_browser_visible: bool,
    pub fork_selector: ForkSelectorState,
    pub fork_selector_visible: bool,
    pub memory_browser: MemoryBrowserState,
    pub worktree_picker: WorktreePickerState,
    pub(crate) new_worktree_dialog: NewWorktreeDialogState,
    pub foreign_import_picker: ForeignImportPickerState,
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
    clear_prompt_confirm_deadline: Option<Instant>,
    live_turn_started_at: Option<Instant>,
    live_turn_phase_started_at: Option<Instant>,
    live_turn_request_id: Option<String>,
    now_fn: Arc<dyn Fn() -> Instant + Send + Sync>,
    on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
}

impl Default for AppState {
    fn default() -> Self {
        let auto_theme_resolver = AutoResolver::default();
        let initial_theme_family = ThemeFamily::Dark;
        Self {
            selected_event_index: 0,
            focus: Focus::default(),
            active_tab: Tab::default(),
            active_review_surface: None,
            live_details_drawer_open: false,
            projection: SessionProjection::default(),
            should_quit: false,
            quit_confirmation_pending: false,
            quit_confirmation_shortcut: None,
            quit_confirmation_expires_at: None,
            replay_mode: false,
            session_path: None,
            status_banner: None,
            connect_dialog: ConnectDialogState::default(),
            toast: None,
            details_scroll: 0,
            mouse_wheel_lines_per_tick: 3,
            terminal_panel: TerminalPanelState::default(),
            last_frame_area: None,
            secondary_surfaces: SecondarySurfaceState::default(),
            dashboard: None,
            dashboard_return_focus: None,
            transcript_view: TranscriptViewState::default(),
            transcript_integration: None,
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
            subagent_actions_session_id: None,
            hovered_subagent_footer_target: None,
            pending_subagent_footer_target: None,
            error_details_visible: false,
            startup_mode: false,
            starting_session_seed: false,
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
            settings_editor_visible: false,
            settings_editor_selected: 0,
            settings_project_config_path: None,
            settings_hashline_edit: true,
            settings_compaction_enabled: true,
            settings_compaction_auto_retry_overflow: true,
            settings_compaction_structured_summary_contract: true,
            settings_compaction_estimated_token_triggers: true,
            settings_deterministic_enabled: false,
            plugin_lifecycle_summary: None,
            plugin_last_install: None,
            plugin_last_activate: None,
            plugin_last_deactivate: None,
            plugin_last_remove: None,
            plugin_first_line: None,
            crash_recovery_scan_summary: None,
            crash_recovery_first_report: None,
            crash_recovery_resolved_action: None,
            crash_recovery_first_report_line: None,
            edit_attribution_summary: None,
            edit_attribution_first_line: None,
            edit_attribution_last_line: None,
            team_registry_summary: None,
            team_last_create: None,
            team_first_line: None,
            team_last_send: None,
            team_last_message_line: None,
            team_last_add_member: None,
            team_last_cancel: None,
            cron_schedule_summary: None,
            cron_last_register: None,
            cron_first_schedule_line: None,
            cron_last_remove: None,
            demote_outcome_summary: None,
            demote_last_result: None,
            demote_last_task_result: None,
            auto_fallback_summary: None,
            auto_fallback_last_outcome: None,
            auto_fallback_last_banner: None,
            auto_fallback_chain_label: None,
            extension_manifest_summary: None,
            extension_discover_summary: None,
            extension_last_load: None,
            workspace_hub_outcome_summary: None,
            workspace_hub_availability: None,
            workspace_hub_last_connect: None,
            workspace_hub_last_bind: None,
            workspace_hub_last_upload: None,
            workspace_hub_last_recover: None,
            graph_query_batch_summary: None,
            graph_query_last_result: None,
            graph_query_batch_first_line: None,
            persistent_graph_availability: None,
            cow_clone_outcome_summary: None,
            cow_clone_last_result: None,
            cow_worktree_availability: None,
            browser_oidc_outcome_summary: None,
            browser_oidc_availability: None,
            browser_oidc_last_start: None,
            browser_oidc_last_complete: None,
            mcp_oauth_outcome_summary: None,
            mcp_oauth_remote_availability: None,
            mcp_oauth_last_begin: None,
            mcp_oauth_last_exchange: None,
            mcp_oauth_last_open: None,
            sleep_wake_observation_summary: None,
            sleep_wake_last_observation: None,
            sleep_wake_observation_log: Vec::new(),
            sleep_wake_last_decision: None,
            sleep_wake_credential_policy: None,
            sleep_wake_availability: None,
            binary_update_summary: None,
            binary_update_policy: None,
            binary_update_check: None,
            binary_version_info: None,
            settings_registry_summary: None,
            foreign_discover_summary: None,
            foreign_import_first_candidate: None,
            foreign_import_last_outcome: None,
            jujutsu_probe: None,
            jujutsu_cli: None,
            jujutsu_workspace: None,
            jujutsu_last_command: None,
            sandbox_fs_plan_summary: None,
            landlock_support: None,
            os_sandbox_profiles_summary: None,
            os_sandbox_first_profile_line: None,
            sandbox_last_prepare: None,
            acp_connection_summary: None,
            acp_connection_state: None,
            acp_session_info: None,
            acp_last_connect: None,
            acp_last_bind: None,
            plan_view_visible: false,
            trust_folder_prompt_visible: false,
            plan_view_selected: 0,
            plan_view_preview: None,
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
            always_approve_mode: false,
            session_mode: SessionMode::Normal,
            runtime_toggles: toggles::RuntimeTogglesState::default(),
            lineage_browser: LineageBrowserState::default(),
            lineage_browser_visible: false,
            fork_selector: ForkSelectorState::default(),
            fork_selector_visible: false,
            memory_browser: MemoryBrowserState::default(),
            worktree_picker: WorktreePickerState::default(),
            new_worktree_dialog: NewWorktreeDialogState::default(),
            foreign_import_picker: ForeignImportPickerState::default(),
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
            theme: Theme::from_family(initial_theme_family, ColorLevel::TrueColor),
            theme_choice: ThemeChoice::Auto,
            theme_family: initial_theme_family,
            theme_preview: ThemePreview::new(initial_theme_family),
            auto_theme_resolver,
            theme_color_level: ColorLevel::TrueColor,
            welcome: WelcomeState::new(4, false),
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
            clear_prompt_confirm_deadline: None,
            live_turn_started_at: None,
            live_turn_phase_started_at: None,
            live_turn_request_id: None,
            now_fn: Arc::new(Instant::now),
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
    pub fn open_status_dashboard(&mut self) {
        let viewport = self
            .last_frame_area
            .and_then(crate::dashboard_integration::dashboard_viewport)
            .unwrap_or(Rect::new(0, 0, 100, 36));
        let return_focus = self.focus;
        match self.build_dashboard_integration(viewport) {
            Ok(mut dashboard) => {
                dashboard.capture_return_state(DashboardReturnState::new(
                    TranscriptFocus::Transcript,
                    self.transcript_view.follow_mode,
                    None,
                ));
                self.dashboard = Some(dashboard);
                self.dashboard_return_focus = Some(return_focus);
                self.secondary_surfaces.open_status_dialog();
            }
            Err(error) => {
                self.status_banner = Some(format!("dashboard unavailable: {error}"));
            }
        }
    }

    pub fn close_status_dashboard(&mut self) {
        self.dashboard = None;
        self.secondary_surfaces.close_status_dialog();
        if let Some(focus) = self.dashboard_return_focus.take() {
            self.focus = focus;
        }
    }

    pub fn status_dashboard_is_active(&self) -> bool {
        self.secondary_surfaces.status_dialog_visible() && self.dashboard.is_some()
    }

    pub fn status_dashboard_focus(&self) -> Option<crate::dashboard_integration::DashboardPane> {
        self.dashboard
            .as_ref()
            .map(DashboardIntegration::focus)
    }

    pub(crate) fn status_dashboard(&self) -> Option<&DashboardIntegration> {
        self.dashboard.as_ref()
    }

    pub(crate) fn handle_status_dashboard_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.close_status_dashboard();
            return;
        }
        let result = self
            .dashboard
            .as_mut()
            .map(|dashboard| dashboard.handle_key(key));
        if let Some(Err(error)) = result.as_ref() {
            self.status_banner = Some(error.to_string());
        }
    }

    pub(crate) fn handle_status_dashboard_mouse(&mut self, mouse: MouseEvent) -> bool {
        let result = self
            .dashboard
            .as_mut()
            .map(|dashboard| dashboard.handle_mouse(mouse));
        if let Some(Err(error)) = result.as_ref() {
            self.status_banner = Some(error.to_string());
        }
        result.is_some()
    }

    fn build_dashboard_integration(
        &self,
        viewport: Rect,
    ) -> Result<DashboardIntegration, String> {
        let mut events_by_run = BTreeMap::<String, Vec<EventEnvelopeV1>>::new();
        for event in &self.events {
            events_by_run
                .entry(event.run_id.to_string())
                .or_default()
                .push(event.clone());
        }

        let mode_source = if self.replay_mode {
            SessionModeSource::ReplayOnly
        } else {
            SessionModeSource::InteractiveLive
        };
        let sessions = events_by_run
            .into_iter()
            .map(|(run_id, events)| {
                let run_name = events.iter().find_map(|event| match &event.payload {
                    EventV1::RunStarted(data) => Some(data.run_name.clone()),
                    _ => None,
                });
                let workspace_root = events.iter().find_map(|event| match &event.payload {
                    EventV1::RunStarted(data) => Some(data.workspace_root.clone()),
                    _ => None,
                });
                let parent_session_id = events
                    .iter()
                    .find_map(|event| event.lineage_parent_session_id().map(str::to_owned));
                let last_updated_at = events.iter().rev().find_map(|event| event.ts.clone());
                DashboardSessionInput::new(
                    SessionCatalogEntry {
                        run_id: run_id.clone(),
                        run_name: Some(
                            run_name
                                .map(|name| name.to_string())
                                .unwrap_or_else(|| run_id.clone()),
                        ),
                        status: None,
                        last_updated_at,
                        workspace_root,
                        profile_preset: Some(self.active_profile().to_string()),
                        provider_model: None,
                        mode_source,
                        is_resumable: !self.replay_mode,
                        resume_disabled_reason: None,
                        artifact_count: 0,
                        child_session_count: 0,
                        parent_session_id,
                    },
                    events,
                )
            })
            .collect::<Vec<_>>();
        let registry = DashboardReplayRegistry::from_sessions(sessions);
        let rules = DashboardEligibilityRules::default();
        let model = crate::dashboard::build_dashboard_read_model(&registry, &rules)
            .map_err(|error| error.to_string())?;
        let selected = model.fallback_selection(None);
        let roster = RosterState {
            selected: selected.clone(),
            ..RosterState::default()
        };
        let mut peek = DashboardPeek::new(8.0).map_err(|error| error.to_string())?;
        if let (Some(selection), Some(view)) = (
            selected.as_ref(),
            self.transcript_view_model(),
        ) {
            let _ = peek.replace_from_view(selection, view);
        }
        let details = selected.clone().and_then(|selection| {
            DashboardDetails::new(
                &registry,
                &rules,
                selection.clone(),
                DetailsRosterState::new(selection, "", 0, ""),
            )
            .ok()
        });
        let controls = DashboardControlState::new(model.clone(), selected, "")
            .with_replay_mode(self.replay_mode);
        DashboardIntegration::new(
            DashboardIntegrationParts {
                dashboard: model,
                roster,
                peek,
                details,
                controls,
            },
            viewport,
        )
        .map_err(|error| error.to_string())
    }

    fn refresh_status_dashboard(&mut self) {
        let Some(current) = self.dashboard.as_ref() else {
            return;
        };
        let focus = current.focus();
        let return_state = current.leave();
        let viewport = self
            .last_frame_area
            .and_then(crate::dashboard_integration::dashboard_viewport)
            .unwrap_or(Rect::new(0, 0, 100, 36));
        if let Ok(mut next) = self.build_dashboard_integration(viewport) {
            next.capture_return_state(return_state);
            next.set_focus(focus);
            self.dashboard = Some(next);
        }
    }

    pub fn composer_view_model(&self, viewport: ViewportId) -> ComposerViewModel {
        self.composer.slice.view_model(viewport)
    }

    pub fn composer_view_model_for_area(&self, area: Rect) -> ComposerViewModel {
        let viewport = ViewportId::ALL
            .into_iter()
            .find(|viewport| viewport.dimensions() == (area.width, area.height))
            .unwrap_or(ViewportId::Standard100x30);
        self.composer_view_model(viewport)
    }

    pub fn composer_submission(
        &self,
    ) -> Result<ComposerUiIntent, crate::composer_integration::ComposerSliceError> {
        self.composer.slice.submit()
    }

    pub(crate) fn composer_render_text(&self) -> String {
        let parity_text = self.composer.parity_text();
        if parity_text.is_empty() && !self.composer.prompt_buffer.is_empty() {
            self.composer.prompt_buffer.clone()
        } else {
            parity_text
        }
    }

    pub(crate) fn composer_render_cursor(&self) -> usize {
        self.composer.parity_cursor()
    }

    pub fn composer_hit_map(
        &self,
        viewport: ViewportId,
    ) -> crate::composer_integration::ComposerHitMap {
        self.composer.slice.hit_map(viewport)
    }

    pub fn composer_queue_state(&self) -> &crate::prompt_queue_actions::QueueState {
        self.composer.slice.queue_state()
    }

    pub fn composer_apply_queue_action(&mut self, action: QueueAction) -> Result<(), QueueError> {
        self.composer
            .slice
            .apply_queue_action(action)
            .map_err(|error| match error {
                crate::composer_integration::ComposerSliceError::Queue(error) => error,
                _ => QueueError::Disabled {
                    action: "composer",
                    lifecycle: self.composer.slice.queue_state().lifecycle,
                },
            })
    }

    pub fn composer_set_queue_state(
        &mut self,
        state: crate::prompt_queue_actions::QueueState,
    ) -> Result<(), crate::composer_integration::ComposerSliceError> {
        self.composer.slice.set_queue_state(state)
    }

    pub fn composer_request_suggestion(
        &mut self,
        context: impl Into<String>,
    ) -> Result<crate::ghost_suggestions::Request, crate::composer_integration::ComposerSliceError>
    {
        self.composer.slice.request_suggestion(context)
    }

    pub fn composer_advance_suggestion_clock(&self, milliseconds: u64) -> u64 {
        self.composer.slice.advance_flush(milliseconds)
    }

    pub fn composer_ready_suggestion(&self) -> Option<crate::ghost_suggestions::Request> {
        self.composer.slice.ready_suggestion()
    }

    pub fn composer_apply_suggestion_response(
        &mut self,
        request: &crate::ghost_suggestions::Request,
        text: impl Into<String>,
    ) -> Result<(), crate::composer_integration::ComposerSliceError> {
        self.composer.slice.apply_suggestion_response(request, text)
    }

    pub fn composer_accept_full_suggestion(
        &mut self,
    ) -> Result<(), crate::composer_integration::ComposerSliceError> {
        self.composer.slice.accept_full_suggestion()
    }

    pub fn handle_composer_mouse(&mut self, mouse: MouseEvent, frame_area: Rect) -> bool {
        self.handle_composer_mouse_event(mouse, frame_area)
    }

    pub(crate) fn tick_composer_runtime(&mut self) {
        let animation_active = self.has_active_animations_for_evidence();
        let _ = self.composer.slice.schedule_motion(animation_active, true);
    }

    pub fn composer_begin_completion(&mut self, trigger: CompletionTrigger) -> CompletionRequest {
        self.composer.slice.begin_completion(trigger)
    }

    pub fn composer_apply_completion_results(
        &mut self,
        request: &CompletionRequest,
        results: Vec<CompletionItem>,
    ) -> Result<(), crate::composer_integration::ComposerSliceError> {
        self.composer.slice.apply_completion_results(request, results)
    }

    pub fn composer_accept_completion_keyboard(
        &mut self,
    ) -> Result<CompletionAcceptance, crate::composer_integration::ComposerSliceError> {
        let acceptance = self.composer.slice.accept_completion_keyboard()?;
        self.composer.sync_legacy_from_parity();
        Ok(acceptance)
    }

    pub fn composer_accept_completion_mouse(
        &mut self,
        index: usize,
    ) -> Result<CompletionAcceptance, crate::composer_integration::ComposerSliceError> {
        let acceptance = self.composer.slice.accept_completion_mouse(index)?;
        self.composer.sync_legacy_from_parity();
        Ok(acceptance)
    }

    pub fn composer_move_completion(&mut self, direction: SelectionDirection) {
        self.composer.slice.move_completion(direction);
    }

    pub fn composer_cancel_completion(&mut self) {
        self.composer.slice.cancel_completion();
    }

    pub(crate) fn composer_completion_active(&self) -> bool {
        !matches!(
            self.composer.slice.completion().status(),
            crate::completion_controller::CompletionStatus::Hidden
        )
    }

    pub fn composer_attach(
        &mut self,
        id: AttachmentId,
        attachment: Attachment,
    ) -> Result<(), crate::composer_integration::ComposerSliceError> {
        self.composer.slice.attach(id, attachment)?;
        self.composer.sync_legacy_from_parity();
        Ok(())
    }

    pub fn composer_ingest_file(
        &mut self,
        id: AttachmentId,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), AttachmentError> {
        let root = self
            .file_mention_workspace_root
            .as_deref()
            .ok_or(AttachmentError::RootUnavailable)?;
        let policy = AttachmentPolicy::new(root)?;
        let attachment = AttachmentIngestor::new(policy).ingest_file(path, cancellation)?;
        self.composer_attach(id, attachment)
            .map_err(|_| AttachmentError::Io { operation: "attaching" })
    }

    pub fn composer_ingest_clipboard(
        &mut self,
        id: AttachmentId,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<(), AttachmentError> {
        let root = self
            .file_mention_workspace_root
            .as_deref()
            .ok_or(AttachmentError::RootUnavailable)?;
        let policy = AttachmentPolicy::new(root)?;
        let attachment = AttachmentIngestor::new(policy).ingest_clipboard(bytes, cancellation)?;
        self.composer_attach(id, attachment)
            .map_err(|_| AttachmentError::Io { operation: "attaching" })
    }

    pub fn note_auth_backend_failure(&mut self, message: &str) {
        if self.connect_dialog.visible
            && self.connect_dialog.step == auth_dialog::ConnectDialogStep::Waiting
            && !message.trim().is_empty()
        {
            self.connect_dialog.error_message = Some(message.to_string());
        }
    }

    pub fn apply_auth_backend_result(&mut self, success: bool, message: &str) {
        let message = if !success && is_auth_backend_failure_summary(message) {
            self.connect_dialog
                .error_message
                .clone()
                .unwrap_or_else(|| message.to_string())
        } else {
            message.to_string()
        };
        self.apply_connect_dialog_auth_result(success, &message);
        if success {
            if self.status_banner.as_deref() == Some(NO_PROVIDER_BANNER) {
                self.status_banner = None;
            }
        } else {
            self.status_banner = Some(if message.trim().is_empty() {
                "auth backend failed; run `harness auth login` in a terminal or use /connect"
                    .to_string()
            } else {
                message
            });
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

    pub fn theme_choice(&self) -> ThemeChoice {
        self.theme_choice
    }

    pub fn theme_family(&self) -> ThemeFamily {
        self.theme_family
    }

    pub fn persist_theme_choice(&self) -> Result<String, PersistError> {
        serialize_choice(self.theme_choice)
    }

    pub fn restore_theme_choice(&mut self, serialized: &str) -> Result<(), PersistError> {
        let choice = deserialize_choice(serialized)?;
        self.resolve_theme_choice(choice);
        self.theme_name = choice.label().to_string();
        Ok(())
    }

    pub(crate) fn welcome_state(&self) -> &WelcomeState {
        &self.welcome
    }

    pub(crate) fn welcome_layout(&self, area: Rect) -> WelcomeLayout {
        WelcomeLayout::compute(area.width, area.height)
    }

    pub(crate) fn welcome_hit_map(&self, area: Rect) -> WelcomeHitMap {
        const MENU_LABELS: [&str; 4] = ["New worktree", "Resume session", "Changelog", "Quit"];
        WelcomeHitMap::new(self.welcome_layout(area), &MENU_LABELS)
    }

    pub(crate) fn set_color_level(&mut self, level: ColorLevel) {
        if self.theme_color_level == level {
            return;
        }
        self.theme_color_level = level;
        self.theme = Theme::from_family(self.theme_family, level);
        self.bump_transcript_render_epoch();
    }

    fn resolve_theme_choice(&mut self, choice: ThemeChoice) {
        let family = match choice {
            ThemeChoice::Dark => ThemeFamily::Dark,
            ThemeChoice::Light => ThemeFamily::Light,
            ThemeChoice::Auto => self.auto_theme_resolver.resolve(),
        };
        self.theme_choice = choice;
        self.theme_family = family;
        self.theme_preview.begin_preview(family);
        self.theme_preview.commit();
        self.theme = Theme::from_family(family, self.theme_color_level);
        self.bump_transcript_render_epoch();
    }

    pub(crate) fn apply_theme_by_name(&mut self, name: &str) {
        let choice = match name {
            "default" | "harness-chat" | "harness-dark" => Some(ThemeChoice::Dark),
            "harness-light" | "light" => Some(ThemeChoice::Light),
            _ => None,
        };
        if let Some(choice) = choice {
            self.resolve_theme_choice(choice);
            self.theme_name = name.to_string();
            return;
        }

        match Theme::by_name(name) {
            Some(theme) => {
                self.theme = theme.for_color_level(self.theme_color_level);
                self.theme_name = name.to_string();
                self.bump_transcript_render_epoch();
            }
            None => {
                self.resolve_theme_choice(ThemeChoice::Dark);
                self.theme_name = "default".to_string();
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
        self.projection
            .set_fallback_profile_label(self.active_profile().to_string());
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_prompt.permission_id = None;
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.selection = PermissionModalSelection::AllowAlways;
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
        self.sync_transcript_integration();

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
        if !historical && self.route_live_event_while_viewing_child(&event) {
            return;
        }

        if self.projection.has_seen_seq(event.seq) {
            return;
        }

        self.starting_session_seed = false;
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
            self.secondary_surfaces
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
            self.note_live_turn_status_timing(&event);
        }
        self.update_transient_state_for_event(&event);
        let trimmed_events = self.projection.ingest_event(event.clone(), historical);
        self.update_composer_queue_lifecycle(&event);
        self.seed_patch_file_expansions(&event);
        if !historical {
            if let Some(notice) = self.projection.pending_status_notice.take() {
                self.status_banner = Some(notice);
            }
        } else {
            self.projection.pending_status_notice = None;
        }
        self.transcript_view.selected_activity_index = self
            .transcript_view
            .selected_activity_index
            .min(self.projection.activities.len().saturating_sub(1));
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
        if !historical {
            self.maybe_auto_allow_active_permission();
        }
        self.sync_transcript_integration();
        if self.status_dashboard_is_active() {
            self.refresh_status_dashboard();
        }
        self.maybe_auto_exit();
    }

    fn note_live_turn_status_timing(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::UserMessageSubmitted(data) => {
                self.begin_live_turn_timing(Some(data.request_id.as_str()));
            }
            EventV1::ProviderRequestStarted(data) => {
                self.begin_live_turn_timing(Some(
                    event
                        .correlation_id
                        .as_deref()
                        .unwrap_or(data.request_id.as_str()),
                ));
            }
            EventV1::ProviderReasoningDelta(data) => {
                let turn_id = event
                    .correlation_id
                    .as_deref()
                    .unwrap_or(data.request_id.as_str());
                let starts_thinking = self
                    .activities
                    .iter()
                    .rev()
                    .find_map(|activity| (activity.request_id == turn_id).then_some(activity))
                    .is_none_or(|activity| {
                        activity.thinking_text.is_empty() && activity.transcript_text.is_empty()
                    });
                if starts_thinking {
                    self.restart_live_turn_phase_timing();
                }
            }
            EventV1::ProviderStreamDelta(data) => {
                let turn_id = event
                    .correlation_id
                    .as_deref()
                    .unwrap_or(data.request_id.as_str());
                let starts_responding = self
                    .activities
                    .iter()
                    .rev()
                    .find_map(|activity| (activity.request_id == turn_id).then_some(activity))
                    .is_none_or(|activity| activity.transcript_text.is_empty());
                if starts_responding {
                    self.restart_live_turn_phase_timing();
                }
            }
            EventV1::ToolCallStarted(_) => self.restart_live_turn_phase_timing(),
            _ => {}
        }
    }

    pub fn set_status_banner(&mut self, status: Option<String>) {
        self.status_banner = status;
    }

    pub fn set_plugin_lifecycle_summary(&mut self, summary: Option<PluginLifecycleSummary>) {
        self.plugin_lifecycle_summary = summary;
    }

    pub fn plugin_lifecycle_summary(&self) -> Option<PluginLifecycleSummary> {
        self.plugin_lifecycle_summary
    }

    pub fn set_plugin_last_install(&mut self, outcome: Option<PluginInstallOutcome>) {
        self.plugin_last_install = outcome;
    }

    pub fn plugin_last_install(&self) -> Option<&PluginInstallOutcome> {
        self.plugin_last_install.as_ref()
    }

    pub fn set_plugin_last_activate(&mut self, outcome: Option<PluginActivateOutcome>) {
        self.plugin_last_activate = outcome;
    }

    pub fn plugin_last_activate(&self) -> Option<&PluginActivateOutcome> {
        self.plugin_last_activate.as_ref()
    }

    pub fn set_plugin_last_deactivate(&mut self, outcome: Option<PluginDeactivateOutcome>) {
        self.plugin_last_deactivate = outcome;
    }

    pub fn plugin_last_deactivate(&self) -> Option<&PluginDeactivateOutcome> {
        self.plugin_last_deactivate.as_ref()
    }

    pub fn set_plugin_last_remove(&mut self, outcome: Option<PluginRemoveOutcome>) {
        self.plugin_last_remove = outcome;
    }

    pub fn plugin_last_remove(&self) -> Option<&PluginRemoveOutcome> {
        self.plugin_last_remove.as_ref()
    }

    pub fn set_plugin_first_line(&mut self, line: Option<String>) {
        self.plugin_first_line = line;
    }

    pub fn plugin_first_line(&self) -> Option<&str> {
        self.plugin_first_line.as_deref()
    }

    pub fn set_crash_recovery_scan_summary(&mut self, summary: Option<CrashRecoveryScanSummary>) {
        self.crash_recovery_scan_summary = summary;
    }

    pub fn crash_recovery_scan_summary(&self) -> Option<CrashRecoveryScanSummary> {
        self.crash_recovery_scan_summary
    }

    pub fn set_crash_recovery_first_report(&mut self, report: Option<PreviousCrashReport>) {
        self.crash_recovery_first_report = report;
    }

    pub fn crash_recovery_first_report(&self) -> Option<&PreviousCrashReport> {
        self.crash_recovery_first_report.as_ref()
    }

    pub fn set_crash_recovery_resolved_action(&mut self, action: Option<CrashRecoveryAction>) {
        self.crash_recovery_resolved_action = action;
    }

    pub fn crash_recovery_resolved_action(&self) -> Option<CrashRecoveryAction> {
        self.crash_recovery_resolved_action
    }

    pub fn set_crash_recovery_first_report_line(&mut self, line: Option<String>) {
        self.crash_recovery_first_report_line = line;
    }

    pub fn crash_recovery_first_report_line(&self) -> Option<&str> {
        self.crash_recovery_first_report_line.as_deref()
    }

    pub fn set_edit_attribution_summary(&mut self, summary: Option<EditAttributionSummary>) {
        self.edit_attribution_summary = summary;
    }

    pub fn edit_attribution_summary(&self) -> Option<EditAttributionSummary> {
        self.edit_attribution_summary
    }

    pub fn set_edit_attribution_first_line(&mut self, line: Option<String>) {
        self.edit_attribution_first_line = line;
    }

    pub fn edit_attribution_first_line(&self) -> Option<&str> {
        self.edit_attribution_first_line.as_deref()
    }

    pub fn set_edit_attribution_last_line(&mut self, line: Option<String>) {
        self.edit_attribution_last_line = line;
    }

    pub fn edit_attribution_last_line(&self) -> Option<&str> {
        self.edit_attribution_last_line.as_deref()
    }

    /// Rebuild session-local edit attribution counts from EditApplied events + on-disk digests.
    pub fn refresh_edit_attribution_summary(&mut self) {
        use harness_core::edit_attribution::{AttributedEdit, EditSource};
        use harness_core::event::EventV1;
        use std::collections::BTreeMap;
        use std::path::Path;

        let mut applied: BTreeMap<String, String> = BTreeMap::new();
        for event in &self.events {
            if let EventV1::EditApplied(edit) = &event.payload {
                applied.insert(edit.path.clone(), edit.new_file_digest.clone());
            }
        }
        if applied.is_empty() {
            self.edit_attribution_summary = Some(EditAttributionSummary::default());
            self.edit_attribution_first_line = None;
            self.edit_attribution_last_line = None;
            return;
        }

        let workspace_root = self.file_mention_workspace_root_opt();
        let mut agent_tool = 0usize;
        let mut external = 0usize;
        let mut first_line: Option<String> = None;
        let mut last_line: Option<String> = None;
        for (rel_path, expected_digest) in &applied {
            let source = if let Some(root) = workspace_root.as_ref() {
                let candidate = {
                    let input = Path::new(rel_path);
                    if input.is_absolute() {
                        input.to_path_buf()
                    } else {
                        root.join(input)
                    }
                };
                match harness_core::edit_attribution::path_content_digest12(&candidate) {
                    Ok(actual) if actual == *expected_digest => {
                        agent_tool += 1;
                        EditSource::AgentTool
                    }
                    Ok(_) => {
                        external += 1;
                        EditSource::External
                    }
                    Err(_) => {
                        agent_tool += 1;
                        EditSource::AgentTool
                    }
                }
            } else {
                agent_tool += 1;
                EditSource::AgentTool
            };
            let entry = AttributedEdit {
                path: rel_path.clone(),
                source,
                content_sha256: expected_digest.clone(),
                mtime_unix_ms: None,
            };
            let line = entry.one_line();
            if first_line.is_none() {
                first_line = Some(line.clone());
            }
            last_line = Some(line);
        }
        self.edit_attribution_summary = Some(EditAttributionSummary {
            agent_tool,
            external,
            drift: 0,
            total: agent_tool.saturating_add(external),
        });
        self.edit_attribution_first_line = first_line;
        self.edit_attribution_last_line = last_line;
    }

    pub fn set_team_registry_summary(&mut self, summary: Option<TeamRegistrySummary>) {
        self.team_registry_summary = summary;
    }

    pub fn team_registry_summary(&self) -> Option<TeamRegistrySummary> {
        self.team_registry_summary
    }

    pub fn set_team_last_create(&mut self, outcome: Option<TeamCreateOutcome>) {
        self.team_last_create = outcome;
    }

    pub fn team_last_create(&self) -> Option<&TeamCreateOutcome> {
        self.team_last_create.as_ref()
    }

    pub fn set_team_first_line(&mut self, line: Option<String>) {
        self.team_first_line = line;
    }

    pub fn team_first_line(&self) -> Option<&str> {
        self.team_first_line.as_deref()
    }

    pub fn set_team_last_send(&mut self, outcome: Option<TeamSendOutcome>) {
        self.team_last_send = outcome;
    }

    pub fn team_last_send(&self) -> Option<&TeamSendOutcome> {
        self.team_last_send.as_ref()
    }

    pub fn set_team_last_message_line(&mut self, line: Option<String>) {
        self.team_last_message_line = line;
    }

    pub fn team_last_message_line(&self) -> Option<&str> {
        self.team_last_message_line.as_deref()
    }

    pub fn set_team_last_add_member(&mut self, outcome: Option<TeamAddMemberOutcome>) {
        self.team_last_add_member = outcome;
    }

    pub fn team_last_add_member(&self) -> Option<&TeamAddMemberOutcome> {
        self.team_last_add_member.as_ref()
    }

    pub fn set_team_last_cancel(&mut self, outcome: Option<TeamCancelOutcome>) {
        self.team_last_cancel = outcome;
    }

    pub fn team_last_cancel(&self) -> Option<&TeamCancelOutcome> {
        self.team_last_cancel.as_ref()
    }

    pub fn set_cron_schedule_summary(&mut self, summary: Option<CronScheduleSummary>) {
        self.cron_schedule_summary = summary;
    }

    pub fn cron_schedule_summary(&self) -> Option<CronScheduleSummary> {
        self.cron_schedule_summary
    }

    pub fn set_cron_last_register(&mut self, outcome: Option<CronRegisterOutcome>) {
        self.cron_last_register = outcome;
    }

    pub fn cron_last_register(&self) -> Option<&CronRegisterOutcome> {
        self.cron_last_register.as_ref()
    }

    pub fn set_cron_first_schedule_line(&mut self, line: Option<String>) {
        self.cron_first_schedule_line = line;
    }

    pub fn cron_first_schedule_line(&self) -> Option<&str> {
        self.cron_first_schedule_line.as_deref()
    }

    pub fn set_cron_last_remove(&mut self, outcome: Option<CronRemoveOutcome>) {
        self.cron_last_remove = outcome;
    }

    pub fn cron_last_remove(&self) -> Option<&CronRemoveOutcome> {
        self.cron_last_remove.as_ref()
    }

    pub fn set_demote_outcome_summary(&mut self, summary: Option<DemoteOutcomeSummary>) {
        self.demote_outcome_summary = summary;
    }

    pub fn demote_outcome_summary(&self) -> Option<DemoteOutcomeSummary> {
        self.demote_outcome_summary
    }

    pub fn set_demote_last_result(&mut self, result: Option<DemoteToBackgroundResult>) {
        self.demote_last_result = result;
    }

    pub fn demote_last_result(&self) -> Option<&DemoteToBackgroundResult> {
        self.demote_last_result.as_ref()
    }

    pub fn set_demote_last_task_result(&mut self, result: Option<DemoteToBackgroundResult>) {
        self.demote_last_task_result = result;
    }

    pub fn demote_last_task_result(&self) -> Option<&DemoteToBackgroundResult> {
        self.demote_last_task_result.as_ref()
    }

    pub fn set_auto_fallback_summary(&mut self, summary: Option<AutoFallbackSummary>) {
        self.auto_fallback_summary = summary;
    }

    pub fn auto_fallback_summary(&self) -> Option<AutoFallbackSummary> {
        self.auto_fallback_summary
    }

    pub fn set_auto_fallback_last_outcome(&mut self, outcome: Option<AutoFallbackOutcome>) {
        self.auto_fallback_last_outcome = outcome;
    }

    pub fn auto_fallback_last_outcome(&self) -> Option<&AutoFallbackOutcome> {
        self.auto_fallback_last_outcome.as_ref()
    }

    pub fn set_auto_fallback_last_banner(&mut self, banner: Option<String>) {
        self.auto_fallback_last_banner = banner;
    }

    pub fn auto_fallback_last_banner(&self) -> Option<&str> {
        self.auto_fallback_last_banner.as_deref()
    }

    pub fn set_auto_fallback_chain_label(&mut self, label: Option<String>) {
        self.auto_fallback_chain_label = label;
    }

    pub fn auto_fallback_chain_label(&self) -> Option<&str> {
        self.auto_fallback_chain_label.as_deref()
    }

    pub fn set_extension_manifest_summary(&mut self, summary: Option<ExtensionManifestSummary>) {
        self.extension_manifest_summary = summary;
    }

    pub fn extension_manifest_summary(&self) -> Option<&ExtensionManifestSummary> {
        self.extension_manifest_summary.as_ref()
    }

    pub fn set_extension_discover_summary(&mut self, summary: Option<ExtensionDiscoverSummary>) {
        self.extension_discover_summary = summary;
    }

    pub fn extension_discover_summary(&self) -> Option<ExtensionDiscoverSummary> {
        self.extension_discover_summary
    }

    pub fn set_extension_last_load(&mut self, outcome: Option<ExtensionLoadOutcome>) {
        self.extension_last_load = outcome;
    }

    pub fn extension_last_load(&self) -> Option<&ExtensionLoadOutcome> {
        self.extension_last_load.as_ref()
    }

    pub fn set_workspace_hub_outcome_summary(
        &mut self,
        summary: Option<WorkspaceHubOutcomeSummary>,
    ) {
        self.workspace_hub_outcome_summary = summary;
    }

    pub fn workspace_hub_outcome_summary(&self) -> Option<WorkspaceHubOutcomeSummary> {
        self.workspace_hub_outcome_summary
    }

    pub fn set_workspace_hub_availability(
        &mut self,
        availability: Option<WorkspaceHubAvailability>,
    ) {
        self.workspace_hub_availability = availability;
    }

    pub fn workspace_hub_availability(&self) -> Option<&WorkspaceHubAvailability> {
        self.workspace_hub_availability.as_ref()
    }

    pub fn set_workspace_hub_last_connect(&mut self, result: Option<WorkspaceHubConnectResult>) {
        self.workspace_hub_last_connect = result;
    }

    pub fn workspace_hub_last_connect(&self) -> Option<&WorkspaceHubConnectResult> {
        self.workspace_hub_last_connect.as_ref()
    }

    pub fn set_workspace_hub_last_bind(&mut self, result: Option<WorkspaceHubBindResult>) {
        self.workspace_hub_last_bind = result;
    }

    pub fn workspace_hub_last_bind(&self) -> Option<&WorkspaceHubBindResult> {
        self.workspace_hub_last_bind.as_ref()
    }

    pub fn set_workspace_hub_last_upload(&mut self, result: Option<WorkspaceHubUploadResult>) {
        self.workspace_hub_last_upload = result;
    }

    pub fn workspace_hub_last_upload(&self) -> Option<&WorkspaceHubUploadResult> {
        self.workspace_hub_last_upload.as_ref()
    }

    pub fn set_workspace_hub_last_recover(&mut self, result: Option<WorkspaceHubRecoveryResult>) {
        self.workspace_hub_last_recover = result;
    }

    pub fn workspace_hub_last_recover(&self) -> Option<&WorkspaceHubRecoveryResult> {
        self.workspace_hub_last_recover.as_ref()
    }

    pub fn set_graph_query_batch_summary(&mut self, summary: Option<GraphQueryBatchSummary>) {
        self.graph_query_batch_summary = summary;
    }

    pub fn graph_query_batch_summary(&self) -> Option<GraphQueryBatchSummary> {
        self.graph_query_batch_summary
    }

    pub fn set_graph_query_last_result(&mut self, result: Option<GraphQueryResult>) {
        self.graph_query_last_result = result;
    }

    pub fn graph_query_last_result(&self) -> Option<&GraphQueryResult> {
        self.graph_query_last_result.as_ref()
    }

    pub fn set_graph_query_batch_first_line(&mut self, line: Option<String>) {
        self.graph_query_batch_first_line = line;
    }

    pub fn graph_query_batch_first_line(&self) -> Option<&str> {
        self.graph_query_batch_first_line.as_deref()
    }

    pub fn set_persistent_graph_availability(
        &mut self,
        availability: Option<PersistentGraphAvailability>,
    ) {
        self.persistent_graph_availability = availability;
    }

    pub fn persistent_graph_availability(&self) -> Option<&PersistentGraphAvailability> {
        self.persistent_graph_availability.as_ref()
    }

    pub fn set_cow_clone_outcome_summary(&mut self, summary: Option<CowCloneOutcomeSummary>) {
        self.cow_clone_outcome_summary = summary;
    }

    pub fn cow_clone_outcome_summary(&self) -> Option<CowCloneOutcomeSummary> {
        self.cow_clone_outcome_summary
    }

    pub fn set_cow_clone_last_result(&mut self, result: Option<CowCloneResult>) {
        self.cow_clone_last_result = result;
    }

    pub fn cow_clone_last_result(&self) -> Option<&CowCloneResult> {
        self.cow_clone_last_result.as_ref()
    }

    pub fn set_cow_worktree_availability(&mut self, availability: Option<CowWorktreeAvailability>) {
        self.cow_worktree_availability = availability;
    }

    pub fn cow_worktree_availability(&self) -> Option<&CowWorktreeAvailability> {
        self.cow_worktree_availability.as_ref()
    }

    pub fn set_browser_oidc_outcome_summary(&mut self, summary: Option<BrowserOidcOutcomeSummary>) {
        self.browser_oidc_outcome_summary = summary;
    }

    pub fn browser_oidc_outcome_summary(&self) -> Option<BrowserOidcOutcomeSummary> {
        self.browser_oidc_outcome_summary
    }

    pub fn set_browser_oidc_availability(&mut self, availability: Option<BrowserOidcAvailability>) {
        self.browser_oidc_availability = availability;
    }

    pub fn browser_oidc_availability(&self) -> Option<&BrowserOidcAvailability> {
        self.browser_oidc_availability.as_ref()
    }

    pub fn set_browser_oidc_last_start(&mut self, result: Option<BrowserOidcStartResult>) {
        self.browser_oidc_last_start = result;
    }

    pub fn browser_oidc_last_start(&self) -> Option<&BrowserOidcStartResult> {
        self.browser_oidc_last_start.as_ref()
    }

    pub fn set_browser_oidc_last_complete(&mut self, result: Option<BrowserOidcCompleteResult>) {
        self.browser_oidc_last_complete = result;
    }

    pub fn browser_oidc_last_complete(&self) -> Option<&BrowserOidcCompleteResult> {
        self.browser_oidc_last_complete.as_ref()
    }

    pub fn set_mcp_oauth_outcome_summary(&mut self, summary: Option<McpOauthOutcomeSummary>) {
        self.mcp_oauth_outcome_summary = summary;
    }

    pub fn mcp_oauth_outcome_summary(&self) -> Option<McpOauthOutcomeSummary> {
        self.mcp_oauth_outcome_summary
    }

    pub fn set_mcp_oauth_remote_availability(
        &mut self,
        availability: Option<McpOauthRemoteAvailability>,
    ) {
        self.mcp_oauth_remote_availability = availability;
    }

    pub fn mcp_oauth_remote_availability(&self) -> Option<&McpOauthRemoteAvailability> {
        self.mcp_oauth_remote_availability.as_ref()
    }

    pub fn set_mcp_oauth_last_begin(&mut self, result: Option<McpOauthBeginResult>) {
        self.mcp_oauth_last_begin = result;
    }

    pub fn mcp_oauth_last_begin(&self) -> Option<&McpOauthBeginResult> {
        self.mcp_oauth_last_begin.as_ref()
    }

    pub fn set_mcp_oauth_last_exchange(&mut self, result: Option<McpOauthTokenExchangeResult>) {
        self.mcp_oauth_last_exchange = result;
    }

    pub fn mcp_oauth_last_exchange(&self) -> Option<&McpOauthTokenExchangeResult> {
        self.mcp_oauth_last_exchange.as_ref()
    }

    pub fn set_mcp_oauth_last_open(&mut self, result: Option<McpRemoteTransportOpenResult>) {
        self.mcp_oauth_last_open = result;
    }

    pub fn mcp_oauth_last_open(&self) -> Option<&McpRemoteTransportOpenResult> {
        self.mcp_oauth_last_open.as_ref()
    }

    pub fn set_sleep_wake_observation_summary(
        &mut self,
        summary: Option<SleepWakeObservationSummary>,
    ) {
        self.sleep_wake_observation_summary = summary;
    }

    pub fn sleep_wake_observation_summary(&self) -> Option<SleepWakeObservationSummary> {
        self.sleep_wake_observation_summary
    }

    pub fn set_sleep_wake_last_observation(&mut self, observation: Option<SleepWakeObservation>) {
        self.sleep_wake_last_observation = observation;
    }

    pub fn sleep_wake_last_observation(&self) -> Option<&SleepWakeObservation> {
        self.sleep_wake_last_observation.as_ref()
    }

    pub fn sleep_wake_last_decision(&self) -> Option<&SleepWakeRefreshDecision> {
        self.sleep_wake_last_decision.as_ref()
    }

    pub fn sleep_wake_observation_log(&self) -> &[SleepWakeObservation] {
        &self.sleep_wake_observation_log
    }

    pub fn set_sleep_wake_credential_policy(&mut self, policy: Option<SleepWakeCredentialPolicy>) {
        self.sleep_wake_credential_policy = policy;
    }

    pub fn sleep_wake_credential_policy(&self) -> Option<&SleepWakeCredentialPolicy> {
        self.sleep_wake_credential_policy.as_ref()
    }

    pub fn set_sleep_wake_availability(&mut self, availability: Option<SleepWakeCredentialPolicy>) {
        self.sleep_wake_availability = availability;
    }

    pub fn sleep_wake_availability(&self) -> Option<&SleepWakeCredentialPolicy> {
        self.sleep_wake_availability.as_ref()
    }

    /// Product path: observe a host sleep/wake event, decide refresh, update status surfaces.
    ///
    /// Without an expiry snapshot this always skips. Near-expiry wake/resume can recommend
    /// refresh via [`Self::apply_sleep_wake_host_event_with_expiry`]. OS/power adapters inject
    /// events through the core hook event source; proactive OAuth execution lives in
    /// `harness_core::sleep_wake_auth` (`execute_sleep_wake_refresh_decision`).
    pub fn apply_sleep_wake_host_event(
        &mut self,
        event: SleepWakeHostEvent,
    ) -> SleepWakeRefreshDecision {
        self.apply_sleep_wake_host_event_with_expiry(event, None)
    }

    /// Observe a host event and evaluate refresh with optional credential expiry.
    ///
    /// When `expiry` shows near-expiry on wake/resume, decision is
    /// [`SleepWakeRefreshDecision::Refresh`]. Credential write/refresh is performed by the
    /// core execution path when a credential manager is supplied.
    pub fn apply_sleep_wake_host_event_with_expiry(
        &mut self,
        event: SleepWakeHostEvent,
        expiry: Option<&harness_core::sleep_wake_auth::CredentialExpirySnapshot>,
    ) -> SleepWakeRefreshDecision {
        let (observation, decision) =
            harness_core::sleep_wake_auth::observe_and_decide_sleep_wake_host_event_for(
                event, expiry,
            );
        self.sleep_wake_observation_log.push(observation.clone());
        self.sleep_wake_last_observation = Some(observation);
        self.sleep_wake_last_decision = Some(decision.clone());
        self.sleep_wake_observation_summary = Some(
            harness_core::sleep_wake_auth::summarize_sleep_wake_observations(
                &self.sleep_wake_observation_log,
            ),
        );
        self.sleep_wake_credential_policy =
            Some(harness_core::sleep_wake_auth::evaluate_sleep_wake_credential_refresh());
        self.sleep_wake_availability =
            Some(harness_core::sleep_wake_auth::sleep_wake_credential_refresh_availability());
        decision
    }

    pub fn set_binary_update_summary(&mut self, summary: Option<BinaryUpdateSummary>) {
        self.binary_update_summary = summary;
    }

    pub fn binary_update_summary(&self) -> Option<BinaryUpdateSummary> {
        self.binary_update_summary
    }

    pub fn set_binary_update_policy(&mut self, policy: Option<BinaryUpdatePolicy>) {
        self.binary_update_policy = policy;
    }

    pub fn binary_update_policy(&self) -> Option<&BinaryUpdatePolicy> {
        self.binary_update_policy.as_ref()
    }

    pub fn set_binary_update_check(&mut self, check: Option<BinaryUpdateCheck>) {
        self.binary_update_check = check;
    }

    pub fn binary_update_check(&self) -> Option<&BinaryUpdateCheck> {
        self.binary_update_check.as_ref()
    }

    pub fn set_binary_version_info(&mut self, info: Option<BinaryVersionInfo>) {
        self.binary_version_info = info;
    }

    pub fn binary_version_info(&self) -> Option<&BinaryVersionInfo> {
        self.binary_version_info.as_ref()
    }

    pub fn set_settings_registry_summary(&mut self, summary: Option<SettingsRegistrySummary>) {
        self.settings_registry_summary = summary;
    }

    pub fn settings_registry_summary(&self) -> Option<SettingsRegistrySummary> {
        self.settings_registry_summary
    }

    pub fn set_foreign_discover_summary(&mut self, summary: Option<ForeignDiscoverSummary>) {
        self.foreign_discover_summary = summary;
    }

    pub fn foreign_discover_summary(&self) -> Option<ForeignDiscoverSummary> {
        self.foreign_discover_summary
    }

    pub fn set_foreign_import_first_candidate(
        &mut self,
        candidate: Option<ForeignSessionCandidate>,
    ) {
        self.foreign_import_first_candidate = candidate;
    }

    pub fn foreign_import_first_candidate(&self) -> Option<&ForeignSessionCandidate> {
        self.foreign_import_first_candidate.as_ref()
    }

    pub fn set_foreign_import_last_outcome(&mut self, outcome: Option<ForeignImportOutcome>) {
        self.foreign_import_last_outcome = outcome;
    }

    pub fn foreign_import_last_outcome(&self) -> Option<&ForeignImportOutcome> {
        self.foreign_import_last_outcome.as_ref()
    }

    pub fn set_jujutsu_probe(&mut self, probe: Option<JujutsuProbe>) {
        self.jujutsu_probe = probe;
    }

    pub fn jujutsu_probe(&self) -> Option<&JujutsuProbe> {
        self.jujutsu_probe.as_ref()
    }

    pub fn set_jujutsu_cli(&mut self, cli: Option<JujutsuAvailability>) {
        self.jujutsu_cli = cli;
    }

    pub fn jujutsu_cli(&self) -> Option<&JujutsuAvailability> {
        self.jujutsu_cli.as_ref()
    }

    pub fn set_jujutsu_workspace(&mut self, workspace: Option<JujutsuWorkspaceStatus>) {
        self.jujutsu_workspace = workspace;
    }

    pub fn jujutsu_workspace(&self) -> Option<&JujutsuWorkspaceStatus> {
        self.jujutsu_workspace.as_ref()
    }

    pub fn set_jujutsu_last_command(&mut self, outcome: Option<JujutsuCommandOutcome>) {
        self.jujutsu_last_command = outcome;
    }

    pub fn jujutsu_last_command(&self) -> Option<&JujutsuCommandOutcome> {
        self.jujutsu_last_command.as_ref()
    }

    pub fn set_sandbox_fs_plan_summary(&mut self, summary: Option<SandboxFsPlanSummary>) {
        self.sandbox_fs_plan_summary = summary;
    }

    pub fn sandbox_fs_plan_summary(&self) -> Option<&SandboxFsPlanSummary> {
        self.sandbox_fs_plan_summary.as_ref()
    }

    pub fn set_landlock_support(&mut self, support: Option<LandlockSupport>) {
        self.landlock_support = support;
    }

    pub fn landlock_support(&self) -> Option<&LandlockSupport> {
        self.landlock_support.as_ref()
    }

    pub fn set_os_sandbox_profiles_summary(&mut self, summary: Option<OsSandboxProfilesSummary>) {
        self.os_sandbox_profiles_summary = summary;
    }

    pub fn os_sandbox_profiles_summary(&self) -> Option<OsSandboxProfilesSummary> {
        self.os_sandbox_profiles_summary
    }

    pub fn set_os_sandbox_first_profile_line(&mut self, line: Option<String>) {
        self.os_sandbox_first_profile_line = line;
    }

    pub fn os_sandbox_first_profile_line(&self) -> Option<&str> {
        self.os_sandbox_first_profile_line.as_deref()
    }

    pub fn set_sandbox_last_prepare(&mut self, result: Option<SandboxPrepareResult>) {
        self.sandbox_last_prepare = result;
    }

    pub fn sandbox_last_prepare(&self) -> Option<&SandboxPrepareResult> {
        self.sandbox_last_prepare.as_ref()
    }

    pub fn set_acp_connection_summary(&mut self, summary: Option<AcpConnectionSummary>) {
        self.acp_connection_summary = summary;
    }

    pub fn acp_connection_summary(&self) -> Option<&AcpConnectionSummary> {
        self.acp_connection_summary.as_ref()
    }

    pub fn set_acp_connection_state(&mut self, state: Option<AcpConnectionState>) {
        self.acp_connection_state = state;
    }

    pub fn acp_connection_state(&self) -> Option<&AcpConnectionState> {
        self.acp_connection_state.as_ref()
    }

    pub fn set_acp_session_info(&mut self, session: Option<AcpSessionInfo>) {
        self.acp_session_info = session;
    }

    pub fn acp_session_info(&self) -> Option<&AcpSessionInfo> {
        self.acp_session_info.as_ref()
    }

    pub fn set_acp_last_connect(&mut self, outcome: Option<AcpConnectOutcome>) {
        self.acp_last_connect = outcome;
    }

    pub fn acp_last_connect(&self) -> Option<&AcpConnectOutcome> {
        self.acp_last_connect.as_ref()
    }

    pub fn set_acp_last_bind(&mut self, outcome: Option<AcpBindOutcome>) {
        self.acp_last_bind = outcome;
    }

    pub fn acp_last_bind(&self) -> Option<&AcpBindOutcome> {
        self.acp_last_bind.as_ref()
    }

    /// Seed operator-facing host/session probes for the status dialog (diagnostics only).
    ///
    /// Binds offline binary-update counts, optional jujutsu CLI/workspace probe, optional
    /// sandbox FS plan (plan-only, not enforcement), optional crash-scan summary, and optional
    /// foreign-discover summary. Does not claim product install, jj workflows, OS sandbox
    /// confinement, recovery UX, or import ownership.
    ///
    /// Test-only: writes synthetic fixtures and must never be reachable from production TUI startup.
    #[cfg(test)]
    pub fn seed_operator_host_probes(&mut self, workspace_root: Option<&std::path::Path>) {
        self.seed_operator_host_probes_with_roots(workspace_root, None, None);
    }

    /// Seed host/session probes with explicit optional roots (tests only).
    #[cfg(test)]
    #[allow(
        clippy::expect_used,
        clippy::missing_panics_doc,
        reason = "hardcoded probe IDs always parse successfully; panic indicates programmer error"
    )]
    pub fn seed_operator_host_probes_with_roots(
        &mut self,
        workspace_root: Option<&std::path::Path>,
        sessions_root: Option<&std::path::Path>,
        foreign_scan_root: Option<&std::path::Path>,
    ) {
        let binary_update =
            harness_core::binary_update::run_offline_multi_channel_update_checks(None);
        self.set_binary_version_info(Some(binary_update.version.clone()));
        self.set_binary_update_policy(Some(binary_update.policy.clone()));
        if let Some(last) = binary_update.checks.last() {
            self.set_binary_update_check(Some(last.clone()));
        }
        self.set_binary_update_summary(Some(binary_update.summary));
        self.set_settings_registry_summary(Some(
            harness_core::config::summarize_settings_registry(),
        ));

        // Multi-policy OS sandbox product path (detect → list → prepare; FS plans when roots known).
        // Child confinement: apply_landlock_fs_plan via bash pre_exec when HARNESS_OS_SANDBOX_POLICY is non-Off.
        {
            let sandbox_roots = workspace_root.map(|root| {
                let harness_state_dir = self
                    .session_path
                    .clone()
                    .unwrap_or_else(|| root.join(".agent-harness"));
                harness_core::sandbox::SandboxPathRoots {
                    workspace_root: root.to_path_buf(),
                    harness_state_dir,
                    temp_dir: std::env::temp_dir(),
                }
            });
            let sandbox = harness_core::sandbox::probe_os_sandbox_product(sandbox_roots.as_ref());
            self.set_landlock_support(Some(sandbox.landlock));
            self.set_os_sandbox_profiles_summary(Some(sandbox.profiles_summary));
            if let Some(first) = sandbox.profiles.first() {
                self.set_os_sandbox_first_profile_line(Some(first.one_line()));
            }
            self.set_sandbox_last_prepare(Some(sandbox.last_prepare));
            if let Some(summary) = sandbox.last_fs_plan {
                self.set_sandbox_fs_plan_summary(Some(summary));
            }
        }

        {
            let acp =
                harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product("cat");
            self.set_acp_last_connect(Some(acp.last_connect));
            self.set_acp_last_bind(Some(acp.last_bind));
            self.set_acp_connection_summary(Some(acp.summary));
            self.set_acp_connection_state(Some(acp.state));
            self.set_acp_session_info(acp.session);
        }

        {
            use harness_core::config::{ResolvedModelSelection, ResolvedModelTarget};
            use harness_core::model_resolution::ModelResolution;
            let probe_target = |model_ref: &str| ResolvedModelTarget {
                model_ref: model_ref.to_string(),
                provider: "probe".into(),
                model: model_ref
                    .rsplit_once(':')
                    .map(|(_, m)| m)
                    .unwrap_or(model_ref)
                    .into(),
                variant: None,
                reasoning_effort: None,
                text_verbosity: None,
                reasoning_summary: None,
                thinking: None,
                resolution: ModelResolution::default(),
            };
            let selection = ResolvedModelSelection {
                selector: "(probe)".into(),
                profile: Some("(probe)".into()),
                primary: probe_target("(probe):primary"),
                fallback: vec![
                    probe_target("(probe):fb1"),
                    probe_target("(probe):fb2"),
                    probe_target("(probe):fb3"),
                    probe_target("(probe):fb4"),
                ],
            };
            let walk = harness_core::auto_fallback::orchestrate_fallback_chain(
                &selection,
                "(probe):primary",
            );
            let _provider_path = harness_core::auto_fallback::orchestrate_provider_failure_fallback(
                &selection,
                "(probe):primary",
                "provider_error",
            );
            let outcome = walk
                .steps
                .last()
                .map(|step| step.outcome.clone())
                .unwrap_or_else(|| {
                    harness_core::auto_fallback::resolve_next_fallback(&selection, "(probe):fb4")
                });
            let banner = harness_core::auto_fallback::describe_auto_fallback_outcome(&outcome);
            self.set_auto_fallback_summary(Some(walk.terminal_summary));
            self.set_auto_fallback_last_outcome(Some(outcome));
            self.set_auto_fallback_last_banner(Some(banner));
            self.set_auto_fallback_chain_label(Some(walk.chain_label));
        }

        {
            let durable_ok = workspace_root
                .map(|root| root.to_path_buf())
                .or_else(|| self.file_mention_workspace_root.clone())
                .and_then(|root| {
                    harness_core::team_mailbox_journal::run_durable_multi_agent_team_product(&root)
                        .ok()
                })
                .map(|product| {
                    self.set_team_last_create(Some(product.last_create));
                    self.set_team_last_add_member(Some(product.last_add_member));
                    self.set_team_last_send(Some(product.last_send));
                    self.set_team_last_cancel(Some(product.last_cancel));
                    self.set_team_registry_summary(Some(product.summary));
                    if let Some(line) = product.first_line {
                        self.set_team_first_line(Some(line));
                    }
                    if let Some(line) = product.last_message_line {
                        self.set_team_last_message_line(Some(line));
                    }
                    true
                })
                .unwrap_or(false);
            if !durable_ok {
                let mut team_registry = harness_core::team_registry::TeamRegistry::new();
                let outcome =
                    harness_core::team_registry::create_team_outcome(&mut team_registry, "(probe)");
                self.set_team_last_create(Some(outcome));
                let _ = harness_core::team_registry::create_team_outcome(
                    &mut team_registry,
                    "(probe-active)",
                );
                let teams_snapshot = team_registry.list_teams();
                if let Some(first) = teams_snapshot.first() {
                    let add = harness_core::team_registry::add_team_member_outcome(
                        &mut team_registry,
                        &first.team_id,
                        "probe-agent",
                        "operator",
                    );
                    self.set_team_last_add_member(Some(add));
                    let _ = harness_core::team_registry::add_team_member_outcome(
                        &mut team_registry,
                        &first.team_id,
                        "probe-worker",
                        "worker",
                    );
                    let send = harness_core::team_registry::send_team_message_outcome(
                        &mut team_registry,
                        &first.team_id,
                        "probe-agent",
                        None,
                        "(probe mailbox)",
                    );
                    self.set_team_last_send(Some(send));
                    if let Ok(msgs) = team_registry.peek_inbox(&first.team_id, "probe-agent") {
                        if let Some(last) = msgs.last() {
                            self.set_team_last_message_line(Some(last.one_line()));
                        }
                    }
                    let cancel = harness_core::team_registry::cancel_team_outcome(
                        &mut team_registry,
                        &first.team_id,
                    );
                    self.set_team_last_cancel(Some(cancel));
                }
                if let Some(first) = team_registry.list_teams().first() {
                    self.set_team_first_line(Some(first.one_line()));
                }
                self.set_team_registry_summary(Some(team_registry.summary()));
            }
        }
        {
            let mut cron_registry = harness_core::cron_schedule::CronScheduleRegistry::new();
            let probe = harness_core::cron_schedule::CronSchedule {
                id: harness_core::cron_schedule::ScheduleId::from_static_literal("(probe)"),
                expression: "0 * * * *".to_string(),
                label: Some("probe".to_string()),
                payload_hint: "(probe)".to_string(),
            };
            let probe_id = probe.id.clone();
            let _ = harness_core::cron_schedule::register_cron_schedule(&mut cron_registry, probe);
            let probe2 = harness_core::cron_schedule::CronSchedule {
                id: harness_core::cron_schedule::ScheduleId::from_static_literal("(probe-2)"),
                expression: "30 * * * *".to_string(),
                label: Some("probe-2".to_string()),
                payload_hint: "(probe-2)".to_string(),
            };
            let _ = harness_core::cron_schedule::register_cron_schedule(&mut cron_registry, probe2);
            let probe3 = harness_core::cron_schedule::CronSchedule {
                id: harness_core::cron_schedule::ScheduleId::from_static_literal("(probe-3)"),
                expression: "15 */2 * * *".to_string(),
                label: Some("probe-3".to_string()),
                payload_hint: "(probe-3)".to_string(),
            };
            let _ = harness_core::cron_schedule::register_cron_schedule(&mut cron_registry, probe3);
            let probe4 = harness_core::cron_schedule::CronSchedule {
                id: harness_core::cron_schedule::ScheduleId::from_static_literal("(probe-4)"),
                expression: "45 1 * * *".to_string(),
                label: Some("probe-4".to_string()),
                payload_hint: "(probe-4)".to_string(),
            };
            let _ = harness_core::cron_schedule::register_cron_schedule(&mut cron_registry, probe4);
            let probe5 = harness_core::cron_schedule::CronSchedule {
                id: harness_core::cron_schedule::ScheduleId::from_static_literal("(probe-5)"),
                expression: "5 3 * * 1".to_string(),
                label: None,
                payload_hint: "(probe-5-unlabeled)".to_string(),
            };
            let last_register =
                harness_core::cron_schedule::register_cron_schedule(&mut cron_registry, probe5);
            self.set_cron_last_register(Some(last_register));
            let remove_outcome =
                harness_core::cron_schedule::remove_cron_schedule(&mut cron_registry, &probe_id);
            self.set_cron_last_remove(Some(remove_outcome));
            let journal_dir = self
                .session_path
                .clone()
                .map(|p| p.join("cron-journal"))
                .or_else(|| {
                    workspace_root.map(|root| root.join(".agent-harness").join("cron-journal"))
                });
            if let Some(dir) = journal_dir {
                let mut executor = harness_core::cron_execute::CronExecutor::with_journal_dir(dir);
                if let Ok(now) = harness_core::cron_execute::CronCivilTime::new(30, 12, 1, 1, 3) {
                    let _ = executor.fire_due(&cron_registry, now);
                }
            }
            if let Some(first) = cron_registry.list().first() {
                self.set_cron_first_schedule_line(Some(first.one_line()));
            }
            self.set_cron_schedule_summary(Some(cron_registry.summary()));
        }
        // Diagnostic multi demote probes: shell unavailable + task rejected + task demoted.
        {
            let shell_req = harness_core::foreground_demote::DemoteToBackgroundRequest::new(
                "(probe)",
                harness_core::foreground_demote::ForegroundKind::Shell,
            );
            let shell_result = harness_core::foreground_demote::default_demote_policy(&shell_req)
                .unwrap_or_else(|_| {
                    harness_core::foreground_demote::DemoteToBackgroundResult::Unavailable {
                        handle_id: "(probe)".to_string(),
                        reason: "demote policy validation failed".to_string(),
                    }
                });
            let task_rejected =
                harness_core::foreground_demote::demote_task_handle_against_registry(
                    "(probe-task)",
                    &[],
                )
                .unwrap_or_else(|_| {
                    harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                        handle_id: "(probe-task)".to_string(),
                        reason: "demote registry validation failed".to_string(),
                    }
                });
            let task_demoted =
                harness_core::foreground_demote::demote_task_handle_against_registry(
                    "(probe-task-ok)",
                    &["(probe-task-ok)"],
                )
                .unwrap_or_else(|_| {
                    harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                        handle_id: "(probe-task-ok)".to_string(),
                        reason: "demote registry validation failed".to_string(),
                    }
                });
            let task_demoted_2 =
                harness_core::foreground_demote::demote_task_handle_against_registry(
                    "(probe-task-ok-2)",
                    &["(probe-task-ok)", "(probe-task-ok-2)"],
                )
                .unwrap_or_else(|_| {
                    harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                        handle_id: "(probe-task-ok-2)".to_string(),
                        reason: "demote registry validation failed".to_string(),
                    }
                });
            let task_rejected_2 =
                harness_core::foreground_demote::demote_task_handle_against_registry(
                    "(probe-task-missing)",
                    &["(probe-task-ok)", "(probe-task-ok-2)"],
                )
                .unwrap_or_else(|_| {
                    harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                        handle_id: "(probe-task-missing)".to_string(),
                        reason: "demote registry validation failed".to_string(),
                    }
                });
            let results = [
                shell_result.clone(),
                task_rejected,
                task_demoted.clone(),
                task_demoted_2,
                task_rejected_2,
            ];
            self.set_demote_outcome_summary(Some(
                harness_core::foreground_demote::summarize_demote_outcomes(&results),
            ));
            self.set_demote_last_result(Some(shell_result));
            self.set_demote_last_task_result(Some(task_demoted));
        }
        {
            let hub = harness_core::workspace_hub::probe_workspace_hub_product();
            self.set_workspace_hub_availability(Some(hub.availability));
            self.set_workspace_hub_last_connect(Some(hub.last_connect));
            self.set_workspace_hub_last_bind(Some(hub.last_bind));
            self.set_workspace_hub_last_upload(Some(hub.last_upload));
            self.set_workspace_hub_last_recover(Some(hub.last_recover));
            self.set_workspace_hub_outcome_summary(Some(hub.summary));
        }
        {
            let oidc = harness_core::browser_oidc::probe_browser_oidc_product();
            self.set_browser_oidc_availability(Some(oidc.availability));
            self.set_browser_oidc_last_start(Some(oidc.last_start));
            self.set_browser_oidc_last_complete(Some(oidc.last_complete));
            self.set_browser_oidc_outcome_summary(Some(oidc.summary));
        }
        {
            let mcp = harness_core::mcp_oauth::probe_mcp_oauth_remote_product();
            self.set_mcp_oauth_remote_availability(Some(mcp.availability));
            self.set_mcp_oauth_last_begin(Some(mcp.last_begin));
            self.set_mcp_oauth_last_exchange(Some(mcp.last_exchange));
            self.set_mcp_oauth_last_open(Some(mcp.last_open));
            self.set_mcp_oauth_outcome_summary(Some(mcp.summary));
        }
        {
            // Dual-cycle host-event product path (observe + decide; Active hook policy).
            for _cycle in 0..2 {
                for event in [
                    SleepWakeHostEvent::Sleep,
                    SleepWakeHostEvent::Wake,
                    SleepWakeHostEvent::Resume,
                    SleepWakeHostEvent::Suspend,
                ] {
                    let _ = self.apply_sleep_wake_host_event(event);
                }
            }
        }

        let probe_root = workspace_root
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.file_mention_workspace_root.clone());
        if let Some(root) = probe_root.clone() {
            self.file_mention_workspace_root = Some(root.clone());
            let plans_dir = root.join(harness_core::plan::PLAN_DIR);
            let _ = std::fs::create_dir_all(&plans_dir);
            let plan_primary = plans_dir.join("harness-probe-plan.md");
            let plan_alt = plans_dir.join("harness-probe-plan-alt.md");
            let plan_extra = plans_dir.join("harness-probe-plan-extra.md");
            let plan_ops = plans_dir.join("harness-probe-plan-ops.md");
            let plan_active = root.join(harness_core::plan::plan_file_relative_path(
                "harness-probe-run",
            ));
            if !plan_primary.is_file() {
                let _ = std::fs::write(
                    &plan_primary,
                    "# Harness probe plan\n\n- step one\n- step two\n",
                );
            }
            if !plan_alt.is_file() {
                let _ = std::fs::write(&plan_alt, "# Harness probe plan alt\n\n- alt step\n");
            }
            if !plan_extra.is_file() {
                let _ = std::fs::write(&plan_extra, "# Harness probe plan extra\n\n- extra step\n");
            }
            if !plan_ops.is_file() {
                let _ = std::fs::write(&plan_ops, "# Harness probe plan ops\n\n- ops step\n");
            }
            if !plan_active.is_file() {
                let _ = std::fs::write(
                    &plan_active,
                    "# Harness probe active-run plan\n\n- active step\n",
                );
            }
            if self.run_id().is_none() {
                self.ingest_historical_event(EventEnvelopeV1 {
                    schema_version: SCHEMA_VERSION,
                    event_id: "evt_harness_probe_plan_active".to_string(),
                    seq: 1,
                    run_id: "harness-probe-run".into(),
                    mono_ms: 1,
                    ts: None,
                    actor: EventActor::new(ActorKind::System, None),
                    correlation_id: None,
                    causation_id: None,
                    stream_key: Some("run:harness-probe-run".to_string()),
                    payload: EventV1::RunFinished(RunFinishedEvent {
                        summary: "probe-active-plan".to_string(),
                    }),
                });
            }
            let settings_path = root.join("harness.json");
            if !settings_path.is_file() {
                let body = r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": {
        "gpt-4o-mini": {
          "display_name": "GPT-4o mini"
        }
      }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": {
      "edit": "ask",
      "shell": "ask",
      "network": "deny"
    }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": {
      "enabled": false,
      "seed": 42
    },
    "compaction": {
      "enabled": true,
      "auto_retry_overflow": true,
      "structured_summary_contract": true,
      "estimated_token_triggers": true
    }
  },
  "integrations": {
    "remote_search": {
      "endpoint": "https://mcp.exa.ai/mcp"
    }
  },
  "hashline_edit": true
}"#;
                let _ = std::fs::write(&settings_path, body);
            }
            let _ = harness_core::config::write_project_hashline_edit(&settings_path, false);
            let _ = harness_core::config::write_project_compaction_enabled(&settings_path, false);
            let _ = harness_core::config::write_project_compaction_auto_retry_overflow(
                &settings_path,
                false,
            );
            let _ = harness_core::config::write_project_compaction_structured_summary_contract(
                &settings_path,
                false,
            );
            let _ = harness_core::config::write_project_compaction_estimated_token_triggers(
                &settings_path,
                false,
            );
            let _ = harness_core::config::write_project_deterministic_enabled(&settings_path, true);
            let _ = harness_core::config::reset_project_hashline_edit(&settings_path);
            let _ = harness_core::config::reset_project_compaction_enabled(&settings_path);
            let _ =
                harness_core::config::reset_project_compaction_auto_retry_overflow(&settings_path);
            let _ = harness_core::config::reset_project_compaction_structured_summary_contract(
                &settings_path,
            );
            let _ = harness_core::config::reset_project_compaction_estimated_token_triggers(
                &settings_path,
            );
            let _ = harness_core::config::reset_project_deterministic_enabled(&settings_path);
            let _ = harness_core::config::write_project_hashline_edit(&settings_path, true);
            let _ = harness_core::config::write_project_compaction_enabled(&settings_path, true);
            let _ = harness_core::config::write_project_compaction_auto_retry_overflow(
                &settings_path,
                true,
            );
            let _ = harness_core::config::write_project_compaction_structured_summary_contract(
                &settings_path,
                true,
            );
            let _ = harness_core::config::write_project_compaction_estimated_token_triggers(
                &settings_path,
                true,
            );
            let _ =
                harness_core::config::write_project_deterministic_enabled(&settings_path, false);
            let _ = harness_core::config::settings_registry_json();
            let hashline_edit =
                harness_core::config::read_effective_hashline_edit(&settings_path).unwrap_or(true);
            let compaction_enabled =
                harness_core::config::read_effective_compaction_enabled(&settings_path)
                    .unwrap_or(true);
            let compaction_auto_retry_overflow =
                harness_core::config::read_effective_compaction_auto_retry_overflow(&settings_path)
                    .unwrap_or(true);
            let compaction_structured_summary_contract =
                harness_core::config::read_effective_compaction_structured_summary_contract(
                    &settings_path,
                )
                .unwrap_or(true);
            let compaction_estimated_token_triggers =
                harness_core::config::read_effective_compaction_estimated_token_triggers(
                    &settings_path,
                )
                .unwrap_or(true);
            let deterministic_enabled =
                harness_core::config::read_effective_deterministic_enabled(&settings_path)
                    .unwrap_or(false);
            self.bind_settings_project_config(
                &settings_path,
                hashline_edit,
                compaction_enabled,
                compaction_auto_retry_overflow,
                compaction_structured_summary_contract,
                compaction_estimated_token_triggers,
                deterministic_enabled,
            );
            let _ = harness_core::jujutsu::ensure_jujutsu_repo_marker(&root);
            let (jj_walk, _jj_receipt) =
                harness_core::jujutsu::run_jujutsu_product_with_receipt(&root);
            self.set_jujutsu_cli(Some(jj_walk.probe.cli.clone()));
            self.set_jujutsu_workspace(Some(jj_walk.probe.workspace.clone()));
            self.set_jujutsu_last_command(Some(jj_walk.last_command));
            self.set_jujutsu_probe(Some(jj_walk.probe));
            self.set_cow_worktree_availability(Some(
                harness_core::cow_worktree::detect_cow_worktree_fastpath(&root),
            ));
            let cow_probe_dir = root.join(".harness-cow-probe");
            let cow_src = cow_probe_dir.join("src.bin");
            let cow_dst = cow_probe_dir.join("dst.bin");
            let cow_missing_src = cow_probe_dir.join("missing-src.bin");
            let cow_missing_dst = cow_probe_dir.join("dst-missing.bin");
            let cow_exists_dst = cow_probe_dir.join("dst-exists.bin");
            let _ = std::fs::remove_file(&cow_dst);
            let _ = std::fs::remove_file(&cow_missing_dst);
            if let Some(parent) = cow_src.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if !cow_src.is_file() {
                let _ = std::fs::write(&cow_src, b"harness-cow-probe\n");
            }
            let _ = std::fs::write(&cow_exists_dst, b"preexisting-dest\n");
            let cow_src_2 = cow_probe_dir.join("src2.bin");
            let cow_dst_2 = cow_probe_dir.join("dst2.bin");
            let cow_missing_src_2 = cow_probe_dir.join("missing-src-2.bin");
            let cow_missing_dst_2 = cow_probe_dir.join("dst-missing-2.bin");
            let _ = std::fs::remove_file(&cow_dst_2);
            let _ = std::fs::remove_file(&cow_missing_dst_2);
            if !cow_src_2.is_file() {
                let _ = std::fs::write(&cow_src_2, b"harness-cow-probe-2\n");
            }
            let cow_tree_src = cow_probe_dir.join("tree-src");
            let cow_tree_dst = cow_probe_dir.join("tree-dst");
            let _ = std::fs::remove_dir_all(&cow_tree_dst);
            if !cow_tree_src.is_dir() {
                let _ = std::fs::create_dir_all(cow_tree_src.join("nested"));
                let _ = std::fs::write(cow_tree_src.join("nested/leaf.bin"), b"cow-tree-leaf\n");
            }
            let cow_tree =
                harness_core::cow_worktree::try_cow_clone_tree(&cow_tree_src, &cow_tree_dst);
            let cow_overlay = harness_core::cow_worktree::apply_cow_worktree_fastpath(
                &root,
                &cow_probe_dir,
                &[".harness-cow-overlay-missing"],
            );
            let cow_results = [
                harness_core::cow_worktree::try_cow_clone_file(&cow_src, &cow_dst),
                harness_core::cow_worktree::try_cow_clone_file(&cow_missing_src, &cow_missing_dst),
                harness_core::cow_worktree::try_cow_clone_file(&cow_src, &cow_exists_dst),
                harness_core::cow_worktree::try_cow_clone_file(&cow_src_2, &cow_dst_2),
                harness_core::cow_worktree::try_cow_clone_file(
                    &cow_missing_src_2,
                    &cow_missing_dst_2,
                ),
            ];
            let _ = (cow_tree, cow_overlay);
            self.set_cow_clone_outcome_summary(Some(
                harness_core::cow_worktree::summarize_cow_clone_outcomes(&cow_results),
            ));
            self.set_cow_clone_last_result(Some(cow_results[2].clone()));
            {
                let probe = harness_core::code_graph::probe_persistent_graph_product(
                    &root,
                    &["(probe)", "(probe-alt)", "(probe-module)"],
                );
                self.set_graph_query_batch_summary(Some(probe.summary()));
                if let Some(first) = probe.batch.results.first() {
                    self.set_graph_query_batch_first_line(Some(first.one_line()));
                }
                let last = probe.batch.results.last().cloned().unwrap_or_else(|| {
                    harness_core::code_graph::query_persistent_graph(
                        &root,
                        &harness_core::code_graph::GraphQuery::symbol_def("(probe)"),
                    )
                });
                self.set_graph_query_last_result(Some(last));
                self.set_persistent_graph_availability(Some(probe.availability));
            }
            let plugins = harness_core::integrations::run_multi_plugin_lifecycle_product(&root);
            self.set_plugin_last_install(Some(plugins.last_install));
            self.set_plugin_last_activate(Some(plugins.last_activate));
            self.set_plugin_last_deactivate(Some(plugins.last_deactivate));
            self.set_plugin_last_remove(Some(plugins.last_remove));
            if let Some(first) = plugins.first_line {
                self.set_plugin_first_line(Some(first));
            }
            self.set_plugin_lifecycle_summary(Some(plugins.summary));

            let extensions =
                harness_core::integrations::run_multi_descriptor_discover_product(&root);
            self.set_extension_discover_summary(Some(extensions.discover));
            if let Some(summary) = extensions.primary {
                self.set_extension_manifest_summary(Some(summary));
            }
            self.set_extension_last_load(Some(extensions.last_load));
        }

        // FS plan summary already bound via probe_os_sandbox_product when workspace_root was set.
        // Re-probe with roots if seed was called with workspace only after early sandbox bind.
        if self.sandbox_fs_plan_summary().is_none() {
            if let Some(ref workspace) = probe_root {
                let harness_state_dir = self
                    .session_path
                    .clone()
                    .unwrap_or_else(|| workspace.join(".agent-harness"));
                let roots = harness_core::sandbox::SandboxPathRoots {
                    workspace_root: workspace.clone(),
                    harness_state_dir,
                    temp_dir: std::env::temp_dir(),
                };
                let sandbox = harness_core::sandbox::probe_os_sandbox_product(Some(&roots));
                if let Some(summary) = sandbox.last_fs_plan {
                    self.set_sandbox_fs_plan_summary(Some(summary));
                }
            }
        }

        let sessions = sessions_root
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                self.session_path
                    .as_ref()
                    .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            })
            .or_else(|| {
                probe_root
                    .as_ref()
                    .map(|root| root.join(".harness-sessions-probe"))
            });
        if let Some(root) = sessions {
            let _ = std::fs::create_dir_all(&root);
            if root.is_dir() {
                let clean = root.join("harness_probe_clean");
                let crashed = root.join("harness_probe_crashed");
                let stale = root.join("harness_probe_stale");
                let crashed_with_events = root.join("harness_probe_crashed_events");
                let stale_with_events = root.join("harness_probe_stale_events");
                if !clean.is_dir() {
                    let _ = std::fs::create_dir_all(&clean);
                    let _ = std::fs::write(clean.join("events.jsonl"), b"");
                }
                if !crashed.is_dir() {
                    let _ = std::fs::create_dir_all(&crashed);
                    let _ = std::fs::write(crashed.join(".writer.lock.recovering"), b"");
                }
                if !stale.is_dir() {
                    let _ = std::fs::create_dir_all(&stale);
                    let _ = std::fs::write(stale.join(".writer.lock"), b"");
                }
                if !crashed_with_events.is_dir() {
                    let _ = std::fs::create_dir_all(&crashed_with_events);
                    let _ =
                        std::fs::write(crashed_with_events.join(".writer.lock.recovering"), b"");
                    let _ = std::fs::write(crashed_with_events.join("events.jsonl"), b"");
                }
                if !stale_with_events.is_dir() {
                    let _ = std::fs::create_dir_all(&stale_with_events);
                    let _ = std::fs::write(stale_with_events.join(".writer.lock"), b"");
                    let _ = std::fs::write(stale_with_events.join("events.jsonl"), b"");
                }
                let reports = harness_core::crash_recovery::scan_previous_crashes(&root);
                self.set_crash_recovery_scan_summary(Some(
                    harness_core::crash_recovery::summarize_crash_reports(&reports),
                ));
                let first = reports
                    .into_iter()
                    .find(|report| report.previous_crash_detected);
                if let Some(ref report) = first {
                    self.set_crash_recovery_first_report_line(Some(report.one_line()));
                    let action = harness_core::crash_recovery::resolve_crash_recovery_action(
                        report.events_log_present,
                    );
                    self.set_crash_recovery_resolved_action(Some(action));
                }
                self.set_crash_recovery_first_report(first);
            }
        }

        let foreign_root = foreign_scan_root
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                self.session_path
                    .as_ref()
                    .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            })
            .or_else(|| {
                probe_root
                    .as_ref()
                    .map(|root| root.join(".harness-foreign-probe-root"))
            });
        if let Some(root) = foreign_root {
            let _ = std::fs::create_dir_all(&root);
            let foreign_event_body = |event_id: &str, run_id: &str, summary: &str| {
                format!(
                    concat!(
                        r#"{{"schema_version":1,"event_id":"{event_id}","seq":1,"#,
                        r#""run_id":"{run_id}","mono_ms":1,"actor":{{"kind":"system"}},"#,
                        r#""payload":{{"event_type":"run_finished","data":{{"summary":"{summary}"}}}}}}"#,
                        "\n",
                    ),
                    event_id = event_id,
                    run_id = run_id,
                    summary = summary,
                )
            };
            for (dir_name, event_id, run_id, summary) in [
                (
                    "harness-foreign-probe",
                    "evt_probe_foreign_1",
                    "run_probe_foreign",
                    "probe-import",
                ),
                (
                    "harness-foreign-probe-2",
                    "evt_probe_foreign_2",
                    "run_probe_foreign_2",
                    "probe-import-2",
                ),
                (
                    "harness-foreign-probe-3",
                    "evt_probe_foreign_3",
                    "run_probe_foreign_3",
                    "probe-import-3",
                ),
            ] {
                let probe_src = root.join(dir_name);
                let _ = std::fs::create_dir_all(&probe_src);
                let events = probe_src.join("events.jsonl");
                if !events.is_file() {
                    let _ = std::fs::write(&events, foreign_event_body(event_id, run_id, summary));
                }
            }
            let corrupt_src = root.join("harness-foreign-probe-corrupt");
            let _ = std::fs::create_dir_all(&corrupt_src);
            let corrupt_events = corrupt_src.join("events.jsonl");
            if !corrupt_events.is_file() {
                let _ = std::fs::write(&corrupt_events, "{not-valid-json\n");
            }
            if let Ok(candidates) = harness_core::foreign_session::discover_foreign_sessions(&root)
            {
                self.set_foreign_discover_summary(Some(
                    harness_core::foreign_session::summarize_discover_candidates(&candidates),
                ));
                let first = candidates
                    .into_iter()
                    .find(|candidate| candidate.is_importable());
                let import_src = first
                    .as_ref()
                    .map(|candidate| candidate.path().to_path_buf())
                    .unwrap_or_else(|| root.join("harness-foreign-probe"));
                self.set_foreign_import_first_candidate(first);
                let probe_dest = root.join("harness-foreign-import-dest");
                let _ = std::fs::remove_dir_all(&probe_dest);
                self.set_foreign_import_last_outcome(Some(
                    harness_core::foreign_session::import_foreign_session_outcome(
                        &import_src,
                        &probe_dest,
                    ),
                ));
            }
        }

        self.refresh_edit_attribution_summary();
        if self
            .edit_attribution_summary()
            .map(|summary| summary.total == 0)
            .unwrap_or(true)
        {
            if let Some(root) = self.file_mention_workspace_root_opt() {
                if let Ok(product) =
                    harness_core::edit_attribution::run_multi_path_edit_attribution_product(&root)
                {
                    self.set_edit_attribution_summary(Some(product.summary));
                    self.set_edit_attribution_first_line(product.first_line);
                    self.set_edit_attribution_last_line(product.last_line);
                } else if let Ok(journal) =
                    harness_core::edit_attribution::EditAttributionJournal::open(&root)
                {
                    let summary = journal.summary();
                    if summary.total > 0 {
                        let entries = journal.list();
                        self.set_edit_attribution_summary(Some(summary));
                        self.set_edit_attribution_first_line(entries.first().map(|e| e.one_line()));
                        self.set_edit_attribution_last_line(entries.last().map(|e| e.one_line()));
                    }
                }
            }
        }
    }

    pub fn always_approve_mode(&self) -> bool {
        self.always_approve_mode
    }

    pub(crate) fn enable_always_approve_mode(&mut self) {
        self.always_approve_mode = true;
        self.session_mode = SessionMode::AlwaysApprove;
    }

    pub(in crate::app) fn toggle_always_approve_mode(&mut self) {
        self.always_approve_mode = !self.always_approve_mode;
        self.session_mode = if self.always_approve_mode {
            SessionMode::AlwaysApprove
        } else {
            SessionMode::Normal
        };
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
        directory_branch_label(
            &test_workspace_env_override().unwrap_or_else(WorkspaceEnvironment::current),
            false,
        )
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

    fn update_composer_queue_lifecycle(&mut self, event: &EventEnvelopeV1) {
        let lifecycle = if matches!(&event.payload, EventV1::UserMessageSubmitted(_)) {
            Some(QueueLifecycle::Streaming)
        } else if matches!(&event.payload, EventV1::ToolCallRequested(_)) {
            Some(QueueLifecycle::Tool)
        } else if matches!(&event.payload, EventV1::PermissionRequested(_)) {
            Some(QueueLifecycle::Waiting)
        } else if matches!(&event.payload, EventV1::TaskCancelled(_)) {
            Some(QueueLifecycle::Cancelling)
        } else if matches!(&event.payload, EventV1::RunFinished(_)) {
            Some(QueueLifecycle::Completed)
        } else if matches!(&event.payload, EventV1::RunFailed(_)) {
            Some(QueueLifecycle::Failed)
        } else if matches!(&event.payload, EventV1::AssistantMessageFinished(_)) {
            Some(QueueLifecycle::Idle)
        } else {
            None
        };
        let Some(lifecycle) = lifecycle else {
            return;
        };
        let mut state = self.composer.slice.queue_state().clone();
        state.lifecycle = lifecycle;
        if lifecycle != QueueLifecycle::Cancelling {
            state.cancel_stage = None;
        }
        let _ = self.composer_set_queue_state(state);
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

    // -----------------------------------------------------------------------
    // Scrollback test helpers (compilation shims for scrollback_state_test)
    // -----------------------------------------------------------------------

    /// Record the maximum scroll offset for the current transcript content.
    pub fn record_transcript_max_scroll(&mut self, max_scroll: usize) {
        self.transcript_view
            .last_transcript_max_scroll
            .set(max_scroll);
    }

    /// Returns `true` when follow mode is active (scroll pinned to bottom).
    pub fn follow_mode_active(&self) -> bool {
        self.transcript_view.follow_mode
    }

    /// Current scroll offset (0 = bottom / most recent content).
    pub fn transcript_scroll_offset(&self) -> usize {
        self.transcript_view.transcript_scroll
    }

    /// Scroll up (away from bottom) by `viewport` rows, breaking follow mode.
    pub fn scroll_page_up(&mut self, viewport: usize) {
        if let Some(composite) = self.transcript_integration.as_mut() {
            let amount = u64::try_from(viewport.max(1)).unwrap_or(u64::MAX);
            let amount = f64::from(u32::try_from(amount).unwrap_or(u32::MAX));
            let _ = composite.scroll_by(amount);
            return;
        }
        self.transcript_view.follow_mode = false;
        self.transcript_view.transcript_scroll = self
            .transcript_view
            .transcript_scroll
            .saturating_add(viewport.max(1));
    }

    /// Scroll down (toward bottom) by `viewport` rows. Re-engages follow at 0.
    pub fn scroll_page_down(&mut self, viewport: usize) {
        if let Some(composite) = self.transcript_integration.as_mut() {
            let amount = u64::try_from(viewport.max(1)).unwrap_or(u64::MAX);
            let amount = f64::from(u32::try_from(amount).unwrap_or(u32::MAX));
            let _ = composite.scroll_by(-amount);
            return;
        }
        self.transcript_view.transcript_scroll = self
            .transcript_view
            .transcript_scroll
            .saturating_sub(viewport.max(1));
        if self.transcript_view.transcript_scroll == 0 {
            self.transcript_view.follow_mode = true;
        }
    }

    /// Scroll up by half of `viewport` (rounded up), breaking follow mode.
    pub fn scroll_half_page_up(&mut self, viewport: usize) {
        let half = viewport.div_ceil(2);
        self.scroll_page_up(half);
    }

    /// Scroll down by half of `viewport` (rounded up). Re-engages follow at 0.
    pub fn scroll_half_page_down(&mut self, viewport: usize) {
        let half = viewport.div_ceil(2);
        self.scroll_page_down(half);
    }

    pub fn set_mouse_wheel_lines_per_tick(&mut self, lines: u16) {
        self.mouse_wheel_lines_per_tick = lines.max(1);
    }

    /// Jump to the top (oldest content). Breaks follow mode.
    pub fn scroll_goto_top(&mut self) {
        let max = self.transcript_view.last_transcript_max_scroll.get();
        self.transcript_view.transcript_scroll = max;
        self.transcript_view.follow_mode = false;
    }

    /// Jump to the bottom (newest content). Re-engages follow mode.
    pub fn scroll_goto_bottom(&mut self) {
        self.transcript_view.transcript_scroll = 0;
        self.transcript_view.follow_mode = true;
    }

    /// Called when new content arrives. If in follow mode, scroll stays at 0.
    /// If not in follow mode, scroll position is unchanged.
    pub fn follow_mode_content_arrived(&mut self) {
        if self.transcript_view.follow_mode {
            self.transcript_view.transcript_scroll = 0;
        }
    }

    // -----------------------------------------------------------------------
    // Tool output fold test helpers (compilation shims for scrollback_state_test)
    // -----------------------------------------------------------------------

    /// Toggle the expansion state of a single tool output by id.
    pub fn toggle_tool_output_for_test(&mut self, tool_call_id: &str) {
        if !self
            .transcript_view
            .expanded_tool_outputs
            .insert(tool_call_id.to_string())
        {
            self.transcript_view
                .expanded_tool_outputs
                .remove(tool_call_id);
        }
        self.bump_transcript_render_epoch();
    }

    /// Check whether a tool output is expanded.
    pub fn is_tool_output_expanded_for_test(&self, tool_call_id: &str) -> bool {
        self.transcript_view
            .expanded_tool_outputs
            .contains(tool_call_id)
    }

    pub fn set_generic_tool_output_visible_for_test(&mut self, visible: bool) {
        self.transcript_view.show_generic_tool_output = visible;
        self.bump_transcript_render_epoch();
    }

    /// Return all expanded tool-output ids.
    pub fn expanded_tool_output_ids_for_test(&self) -> Vec<String> {
        self.transcript_view
            .expanded_tool_outputs
            .iter()
            .cloned()
            .collect()
    }

    /// Expand all known tool-call outputs.
    pub fn expand_all_tool_outputs_for_test(&mut self) {
        for activity in &self.projection.activities {
            for tc in &activity.tool_calls {
                self.transcript_view
                    .expanded_tool_outputs
                    .insert(tc.tool_call_id.clone());
            }
        }
    }

    /// Collapse all tool-call outputs.
    pub fn collapse_all_tool_outputs_for_test(&mut self) {
        self.transcript_view.expanded_tool_outputs.clear();
    }
}

fn is_auth_backend_failure_summary(message: &str) -> bool {
    message.starts_with("auth backend failed (exit ") && !message.contains('\n')
}
