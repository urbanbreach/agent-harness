use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

use clap::Args;
use harness_core::agent::{AgentModelSettings, AgentProfile};
use harness_core::clock::{Clock, Determinism, FakeClock, RealClock};
use harness_core::config::{
    configured_model_catalog, resolve_profile_model_metadata, AgentMode, HarnessConfig,
    ShellAllowlist,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
    ManualCompactionOutcome,
};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionDecision;
use harness_core::proj::{inspect_resume_plan, RecordedRuntimeContext, SessionModeSource};
use harness_core::redact::{DefaultRedactor, Redactor};
use harness_core::session_lineage::{
    materialize_child_session, ChildSessionMaterializationRequest,
    ChildSessionMaterializationResult, ChildSessionMaterializationSourceKind, StableSessionPrefix,
};
use harness_core::session_title::create_default_title;
use harness_core::store::{EventStore, EventStoreError};
use harness_tools::{coordinator_registry, discover_skill_catalog, SkillCatalogEntry};
use harness_tui::app::{
    prompt_history_path_for_session_dir, set_pending_live_launch_metadata,
    set_pending_live_prompt_auto_submit, LaunchMetadata, ModelOption, SessionHistoryEntry,
    ToggleEntryConfig, ToggleEntryKind, TogglesConfig,
};
use harness_tui::{
    close_preserved_terminal_session, run_tui_with_options, set_pending_replay_launch_metadata,
    LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions, UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::cli_config::{apply_runtime_metadata, load_optional_config_with_digest_context};
use serde::{Deserialize, Serialize};

use crate::bootstrap;
use crate::cli_io::{
    load_events_from_run_dir, load_run_metadata, wait_for_permission_id, wait_for_tool_finished,
    ToolFinishTerminalEvents, DEFAULT_EVENT_WAIT_TIMEOUT,
};
use crate::defaults::{
    DEFAULT_INTERACTIVE_RUN_NAME, DEFAULT_MOCK_PROFILE, DEFAULT_SESSION_DIR,
    RESUME_UNAVAILABLE_FALLBACK_REASON,
};
use crate::logging;
use crate::recovery::{latest_run_name, select_resume_agent_id};
use crate::replay::inspect_session_catalog;
use crate::scenarios::{
    create_workspace, default_permission_policy, deterministic_run_id, golden_path_edit_args,
    golden_path_profiles, golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

const MODEL_SELECTION_STATE_FILE: &str = "model.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedModelSelection {
    schema_version: u8,
    profile: String,
    provider: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
}

fn handoff_profile_file() -> Option<&'static Mutex<std::fs::File>> {
    static PROFILE_FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    PROFILE_FILE
        .get_or_init(|| {
            let path = std::env::var_os("HARNESS_TUI_PROFILE_LOG")?;
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            Some(Mutex::new(file))
        })
        .as_ref()
}

fn handoff_profile_start() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

fn profile_handoff(event: &str) {
    let Some(file) = handoff_profile_file() else {
        return;
    };

    let elapsed_ms = handoff_profile_start().elapsed().as_millis();
    let mut file = match file.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let _ = writeln!(file, "{elapsed_ms:>6}ms {event}");
}

#[derive(Debug, Args, Clone)]
pub struct TuiCommand {
    #[arg(long, conflicts_with = "scenario")]
    pub replay: Option<PathBuf>,

    #[arg(
        long = "continue",
        alias = "continue-session",
        value_name = "SESSION",
        conflicts_with_all = ["replay", "scenario", "mock"]
    )]
    pub continue_session: Option<PathBuf>,

    #[arg(long, value_enum, conflicts_with = "replay")]
    pub scenario: Option<ScenarioName>,

    #[arg(long, default_value_t = false, conflicts_with_all = ["replay", "continue_session", "scenario"])]
    pub mock: bool,

    #[arg(long, default_value_t = false)]
    pub deterministic: bool,

    #[arg(long)]
    pub session_dir: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub exit_on_finish: bool,

    #[arg(long)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone)]
struct LiveSettings {
    config: Option<HarnessConfig>,
    config_path: Option<PathBuf>,
    session_dir: PathBuf,
    workspace_root: PathBuf,
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
    launch_metadata: LaunchMetadata,
    launch_mode_label: Option<String>,
    toggles: TogglesConfig,
}

struct LiveBootstrap {
    store: Arc<dyn EventStore>,
    run_dir: PathBuf,
}

#[derive(Clone)]
struct LiveCoordinatorConfigWarmup {
    state: Arc<tokio::sync::Mutex<LiveCoordinatorConfigWarmupState>>,
}

enum LiveCoordinatorConfigWarmupState {
    Disabled,
    Pending(JoinHandle<Result<CoordinatorConfig, String>>),
    Ready(Box<CoordinatorConfig>),
}

impl LiveCoordinatorConfigWarmup {
    fn start(settings: &LiveSettings, demo_mode: bool) -> Self {
        profile_handoff(&format!(
            "warmup.start demo_mode={} has_config={}",
            demo_mode,
            settings.config.is_some()
        ));
        let state = if demo_mode {
            LiveCoordinatorConfigWarmupState::Disabled
        } else if let Some(mut config) = settings.config.clone() {
            let session_dir = settings.session_dir.clone();
            LiveCoordinatorConfigWarmupState::Pending(tokio::task::spawn_blocking(move || {
                profile_handoff("warmup.build.begin");
                config.apply_session_dir_override(Some(session_dir));
                let result = bootstrap::build_interactive_coordinator_config(&config);
                profile_handoff("warmup.build.end");
                result
            }))
        } else {
            LiveCoordinatorConfigWarmupState::Disabled
        };

        Self {
            state: Arc::new(tokio::sync::Mutex::new(state)),
        }
    }

    async fn coordinator_config(
        &self,
        settings: &LiveSettings,
        demo_mode: bool,
    ) -> Result<CoordinatorConfig, String> {
        if demo_mode {
            profile_handoff("warmup.use_demo_config");
            return Ok(demo_coordinator_config(settings));
        }

        let pending = {
            let mut state = self.state.lock().await;
            match &*state {
                LiveCoordinatorConfigWarmupState::Ready(config) => {
                    profile_handoff("warmup.cache_hit");
                    return Ok(config.as_ref().clone());
                }
                LiveCoordinatorConfigWarmupState::Disabled => {
                    profile_handoff("warmup.disabled_fallback");
                    None
                }
                LiveCoordinatorConfigWarmupState::Pending(_) => {
                    profile_handoff("warmup.await_pending");
                    match std::mem::replace(&mut *state, LiveCoordinatorConfigWarmupState::Disabled)
                    {
                        LiveCoordinatorConfigWarmupState::Pending(handle) => Some(handle),
                        LiveCoordinatorConfigWarmupState::Ready(config) => {
                            let resolved = config.as_ref().clone();
                            profile_handoff("warmup.ready_race");
                            *state = LiveCoordinatorConfigWarmupState::Ready(config);
                            return Ok(resolved);
                        }
                        LiveCoordinatorConfigWarmupState::Disabled => None,
                    }
                }
            }
        };

        if let Some(handle) = pending {
            let config = handle
                .await
                .map_err(|err| format!("live coordinator warmup task failed: {err}"))??;
            profile_handoff("warmup.pending_resolved");
            let mut state = self.state.lock().await;
            *state = LiveCoordinatorConfigWarmupState::Ready(Box::new(config.clone()));
            return Ok(config);
        }

        profile_handoff("warmup.rebuild_fallback");
        interactive_coordinator_config(settings)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InteractiveWorkflow {
    Startup,
    NewSession,
    Continue { run_id: String, run_dir: PathBuf },
    Replay { run_dir: PathBuf },
    Quit,
}

type SelectedWorkflow = Arc<Mutex<Option<InteractiveWorkflow>>>;
type UiIntentSink = Arc<dyn Fn(UiIntent) + Send + Sync>;
type LaunchSelection = Arc<Mutex<LaunchMetadata>>;
type LiveAgentTargetState = Arc<Mutex<LiveAgentTarget>>;

fn recover_mutex_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug, Clone)]
struct LiveAgentTarget {
    agent_id: Option<String>,
    profile: String,
    last_request_id: Option<String>,
}

enum ResolvedTuiMode {
    Replay {
        run_dir: PathBuf,
    },
    Continue {
        settings: LiveSettings,
        run_dir: PathBuf,
    },
    Interactive {
        settings: LiveSettings,
    },
    Mock {
        settings: LiveSettings,
    },
    Scenario {
        settings: LiveSettings,
        scenario: ScenarioName,
    },
}

pub fn execute(
    cmd: TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let mut stderr = std::io::stderr();
    let config_context = harness_core::config::ConfigLoadContext::from_env();
    execute_with_io(
        cmd,
        config_path,
        global_session_dir,
        config_context,
        &mut stderr,
    )
}

pub(crate) fn execute_with_io(
    cmd: TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    config_context: harness_core::config::ConfigLoadContext,
    stderr: &mut dyn Write,
) -> ExitCode {
    let workspace_root = config_context.discovery.current_dir.clone();
    let mode = match resolve_tui_mode(
        &cmd,
        config_path,
        global_session_dir,
        workspace_root,
        &config_context,
    ) {
        Ok(mode) => mode,
        Err(err) => {
            let _ = writeln!(stderr, "tui setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    if let ResolvedTuiMode::Replay { run_dir } = &mode {
        return execute_replay_mode(run_dir, cmd.exit_on_finish, stderr);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(stderr, "failed to build async runtime: {err}");
            return ExitCode::from(1);
        }
    };

    let run_result = match mode {
        ResolvedTuiMode::Replay { .. } => Ok(()),
        ResolvedTuiMode::Continue { settings, run_dir } => {
            runtime.block_on(run_direct_continue_mode(&cmd, &settings, false, run_dir))
        }
        ResolvedTuiMode::Interactive { settings } => {
            runtime.block_on(run_interactive_mode(&cmd, &settings, false))
        }
        ResolvedTuiMode::Mock { settings } => {
            runtime.block_on(run_interactive_mode(&cmd, &settings, true))
        }
        ResolvedTuiMode::Scenario { settings, scenario } => {
            runtime.block_on(run_live_mode(&cmd, &settings, scenario))
        }
    };

    match run_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(stderr, "tui failed: {err}");
            ExitCode::from(1)
        }
    }
}

fn execute_replay_mode(run_dir: &Path, exit_on_finish: bool, stderr: &mut dyn Write) -> ExitCode {
    let events = match load_events_from_run_dir(run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(stderr, "replay setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    if exit_on_finish && has_terminal_event(&events) {
        return ExitCode::SUCCESS;
    }

    set_pending_replay_launch_metadata(Some(replay_launch_metadata_for_run(run_dir, &events)));

    if let Err(err) = run_tui_with_options(TuiOptions {
        mode: TuiMode::Replay {
            run_dir: run_dir.to_path_buf(),
            events,
        },
        exit_on_finish,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    }) {
        let _ = writeln!(stderr, "TUI error: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn resolve_tui_mode(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
) -> Result<ResolvedTuiMode, String> {
    if let Some(run_dir) = &cmd.replay {
        return Ok(ResolvedTuiMode::Replay {
            run_dir: run_dir.clone(),
        });
    }

    let settings = resolve_live_settings(
        cmd,
        config_path,
        global_session_dir,
        workspace_root,
        config_context,
    )?;

    if let Some(run_dir) = &cmd.continue_session {
        return Ok(ResolvedTuiMode::Continue {
            settings,
            run_dir: run_dir.clone(),
        });
    }

    if let Some(scenario) = cmd.scenario {
        return Ok(ResolvedTuiMode::Scenario { settings, scenario });
    }

    if cmd.mock {
        return Ok(ResolvedTuiMode::Mock { settings });
    }

    Ok(ResolvedTuiMode::Interactive { settings })
}

fn resolve_live_settings(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
) -> Result<LiveSettings, String> {
    let mut shell_allowlist = ShellAllowlist::default();
    let mut config_session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut config_deterministic = false;
    let mut config_seed = 0;
    let mut config_digest = "none".to_string();
    let mut config_default_profile = DEFAULT_MOCK_PROFILE.to_string();
    let mut live_config: Option<HarnessConfig> = None;
    let mut agent_profiles = golden_path_profiles();

    let loaded = if cmd.mock || cmd.scenario.is_some() {
        None
    } else {
        load_optional_config_with_digest_context(config_path.as_deref(), config_context)?
    };
    let project_config_loaded = loaded.is_some();

    let mut connected_provider_ids = Vec::new();
    let mut no_provider_connected = false;
    if cmd.scenario.is_none() && !cmd.mock {
        let credential_store = harness_core::auth::CredentialStore::from_env();
        let runtime_catalog = crate::runtime_catalog::resolve_runtime_catalog(
            loaded.as_ref().map(|loaded| loaded.config.clone()),
            loaded.as_ref().map(|loaded| loaded.digest.clone()),
            None,
            credential_store.as_ref(),
            &|name| std::env::var(name).ok(),
        )?;
        let config = runtime_catalog.config;
        config_digest = runtime_catalog.config_digest;
        connected_provider_ids = runtime_catalog.connected_provider_ids;
        no_provider_connected = runtime_catalog.no_provider_connected;
        config_default_profile = bootstrap::interactive_profile_name(&config);
        agent_profiles = bootstrap::interactive_agent_profiles(&config)?;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        live_config = Some(config);
    } else if let Some(loaded) = loaded {
        let config = loaded.config;
        config_digest = loaded.digest;
        config_default_profile = bootstrap::interactive_profile_name(&config);
        agent_profiles = bootstrap::interactive_agent_profiles(&config)?;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        live_config = Some(config);
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic || Determinism::enabled(config_deterministic);
    let default_profile = cmd.profile.clone().unwrap_or(config_default_profile);
    let launch_mode_label = if live_config.is_some() {
        None
    } else {
        Some("Demo".to_string())
    };
    let mut launch_metadata =
        interactive_launch_metadata(live_config.as_ref(), &agent_profiles, &default_profile)?;
    if !project_config_loaded {
        launch_metadata = launch_metadata_for_connected_providers(
            launch_metadata,
            &connected_provider_ids,
            no_provider_connected,
        );
    }
    let launch_metadata = if live_config.is_some() && !no_provider_connected {
        apply_persisted_model_selection(launch_metadata)
    } else {
        launch_metadata
    };
    let toggles = runtime_toggles_config(live_config.as_ref(), &workspace_root);

    Ok(LiveSettings {
        config: live_config,
        config_path,
        session_dir,
        workspace_root,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
        launch_metadata,
        launch_mode_label,
        toggles,
    })
}

fn runtime_toggles_config(config: Option<&HarnessConfig>, workspace_root: &Path) -> TogglesConfig {
    let mut toggles = TogglesConfig::default();
    let Some(config) = config else {
        return toggles;
    };

    let skill_catalog_entries = discover_skill_catalog(workspace_root)
        .ok()
        .map(|catalog| catalog.entries)
        .unwrap_or_default();

    for (name, profile) in &config.agents {
        if profile.hidden {
            continue;
        }
        if !matches!(profile.mode, AgentMode::Subagent) {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::Agent { name: name.clone() },
                label: name.clone(),
                description: profile.description.clone(),
                enabled: true,
            });
        }
        if !matches!(profile.mode, AgentMode::Primary) {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::Subagent { name: name.clone() },
                label: name.clone(),
                description: profile.description.clone(),
                enabled: true,
            });
        }
        for tool in &profile.tools {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::AgentTool {
                    agent: name.clone(),
                    tool: tool.clone(),
                },
                label: format!("{name}: {tool}"),
                description: format!("Configured tool `{tool}` for `{name}`"),
                enabled: true,
            });
        }
        if profile.tools.iter().any(|tool| tool == "skill") {
            let skill_entries = if skill_catalog_entries.is_empty() {
                fallback_skill_toggle_entries(config)
            } else {
                skill_catalog_entries
                    .iter()
                    .map(skill_catalog_toggle_entry)
                    .collect()
            };
            for skill in skill_entries {
                toggles.entries.push(ToggleEntryConfig {
                    kind: ToggleEntryKind::AgentSkill {
                        agent: name.clone(),
                        skill: skill.id,
                    },
                    label: format!("{name}: {}", skill.label),
                    description: skill.description,
                    enabled: skill.enabled,
                });
            }
        }
    }

    for (index, hook) in config.hooks.lifecycle.iter().enumerate() {
        let id = hook
            .id
            .clone()
            .unwrap_or_else(|| format!("{} #{index}", hook.event.as_str()));
        toggles.entries.push(ToggleEntryConfig {
            kind: ToggleEntryKind::Hook { id: id.clone() },
            label: id,
            description: format!("{} lifecycle hook", hook.event.as_str()),
            enabled: true,
        });
    }

    for (name, server) in &config.integrations.mcp.servers {
        toggles.entries.push(ToggleEntryConfig {
            kind: ToggleEntryKind::McpServer { name: name.clone() },
            label: name.clone(),
            description: "Configured MCP server state".to_string(),
            enabled: server.enabled(),
        });
    }

    toggles
}

struct SkillToggleEntry {
    id: String,
    label: String,
    description: String,
    enabled: bool,
}

fn skill_catalog_toggle_entry(entry: &SkillCatalogEntry) -> SkillToggleEntry {
    let mut description = format!(
        "{} skill `{}` from {} root {}",
        entry.status.as_str(),
        entry.name,
        entry.source_scope,
        entry.root_path.display()
    );
    if let Some(reason) = entry.reason.as_deref() {
        description.push_str(&format!(" ({reason})"));
    } else if !entry.description.is_empty() {
        description.push_str(&format!(": {}", entry.description));
    }

    SkillToggleEntry {
        id: entry.stable_id.clone(),
        label: entry.name.clone(),
        description,
        enabled: entry.loadable,
    }
}

fn fallback_skill_toggle_entries(config: &HarnessConfig) -> Vec<SkillToggleEntry> {
    if config.skills.permissions.is_empty() {
        return vec![SkillToggleEntry {
            id: "skill-loading".to_string(),
            label: "skill loading".to_string(),
            description: "Configured skill loading surface".to_string(),
            enabled: true,
        }];
    }

    config
        .skills
        .permissions
        .keys()
        .map(|pattern| SkillToggleEntry {
            id: format!("permission:{pattern}"),
            label: pattern.clone(),
            description: format!("Configured skill permission pattern `{pattern}`"),
            enabled: true,
        })
        .collect()
}

fn model_selection_state_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HARNESS_MODEL_SELECTION_STATE_FILE") {
        let path = PathBuf::from(path);
        return (!path.as_os_str().is_empty()).then_some(path);
    }

    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        let state_home = PathBuf::from(state_home);
        if !state_home.as_os_str().is_empty() {
            return Some(state_home.join("harness").join(MODEL_SELECTION_STATE_FILE));
        }
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| !home.as_os_str().is_empty())
        .map(|home| {
            home.join(".local")
                .join("state")
                .join("harness")
                .join(MODEL_SELECTION_STATE_FILE)
        })
}

fn load_persisted_model_selection() -> Option<PersistedModelSelection> {
    load_persisted_model_selection_from_path(&model_selection_state_path()?)
}

fn load_persisted_model_selection_from_path(path: &Path) -> Option<PersistedModelSelection> {
    let body = fs::read_to_string(path).ok()?;
    let selection = serde_json::from_str::<PersistedModelSelection>(&body).ok()?;
    persisted_model_selection_valid(&selection).then_some(selection)
}

fn persisted_model_selection_valid(selection: &PersistedModelSelection) -> bool {
    selection.schema_version == 1
        && model_selection_value_present(&selection.profile)
        && model_selection_value_present(&selection.provider)
        && model_selection_value_present(&selection.model)
        && selection
            .variant
            .as_deref()
            .is_none_or(model_selection_value_present)
}

fn model_selection_value_present(value: &str) -> bool {
    !value.trim().is_empty()
}

fn save_persisted_model_selection(launch_metadata: &LaunchMetadata) -> Result<(), String> {
    let Some(path) = model_selection_state_path() else {
        return Ok(());
    };
    save_persisted_model_selection_to_path(&path, launch_metadata)
}

fn save_persisted_model_selection_to_path(
    path: &Path,
    launch_metadata: &LaunchMetadata,
) -> Result<(), String> {
    let Some(selection) = persisted_model_selection_from_metadata(launch_metadata) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create model selection state dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let body = serde_json::to_vec_pretty(&selection)
        .map_err(|err| format!("failed to serialize model selection state: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write model selection state {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to replace model selection state {}: {err}",
            path.display()
        )
    })
}

fn persisted_model_selection_from_metadata(
    launch_metadata: &LaunchMetadata,
) -> Option<PersistedModelSelection> {
    Some(PersistedModelSelection {
        schema_version: 1,
        profile: launch_metadata.profile().to_string(),
        provider: launch_metadata.provider().to_string(),
        model: launch_metadata.model()?.to_string(),
        variant: launch_metadata.variant().map(str::to_string),
    })
}

fn apply_persisted_model_selection(launch_metadata: LaunchMetadata) -> LaunchMetadata {
    let Some(selection) = load_persisted_model_selection() else {
        return launch_metadata;
    };
    apply_model_selection_to_launch_metadata(launch_metadata, &selection)
}

fn apply_model_selection_to_launch_metadata(
    launch_metadata: LaunchMetadata,
    selection: &PersistedModelSelection,
) -> LaunchMetadata {
    let Some(option) = matching_persisted_model_option(&launch_metadata, selection) else {
        return launch_metadata;
    };
    LaunchMetadata::from_model_option(option)
        .with_available_models(launch_metadata.available_models().to_vec())
        .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec())
}

fn matching_persisted_model_option<'a>(
    launch_metadata: &'a LaunchMetadata,
    selection: &PersistedModelSelection,
) -> Option<&'a ModelOption> {
    if !persisted_model_selection_valid(selection) {
        return None;
    }

    let active_profile = launch_metadata.profile();
    launch_metadata.available_models().iter().find(|option| {
        option.profile == active_profile
            && option.provider == selection.provider
            && option.model == selection.model
            && option.variant() == selection.variant.as_deref()
    })
}

fn persist_launch_selection_for_exit(launch_metadata: &LaunchMetadata) {
    if let Err(err) = save_persisted_model_selection(launch_metadata) {
        profile_handoff(&format!("model_selection.persist_failed {err}"));
    }
}

fn interactive_launch_metadata(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    profile: &str,
) -> Result<LaunchMetadata, String> {
    let Some(selected_profile) = agent_profiles.get(profile) else {
        return Err(format!(
            "interactive mode requires a configured profile named `{profile}`"
        ));
    };

    let available_models = model_options_for_profiles(config, agent_profiles, profile);
    let launch_metadata = config
        .and_then(|config| resolve_profile_model_metadata(config, profile).ok())
        .map(|metadata| {
            LaunchMetadata::from_model_option(&ModelOption {
                profile: metadata.profile,
                provider: metadata.provider,
                provider_display_label: Some(metadata.provider_display_label),
                provider_backend_label: metadata.provider_backend_label,
                model: metadata.model,
                model_display_label: Some(metadata.model_display_label),
                variant: metadata.variant,
                variant_display_label: metadata.variant_display_label,
                display_label: Some(metadata.display_label),
                token_window_label: metadata.token_window_label,
                context_window_tokens: metadata.context_window_tokens,
                max_input_tokens: metadata.max_input_tokens,
                max_output_tokens: metadata.max_output_tokens,
                description: metadata.description,
                profile_description: metadata.profile_description,
                reasoning_effort: metadata.reasoning_effort,
                text_verbosity: metadata.text_verbosity,
                recommended_for: metadata.recommended_for,
            })
        })
        .unwrap_or_else(|| {
            LaunchMetadata::from_model_ref(
                selected_profile.name.clone(),
                &selected_profile.model_ref,
            )
        });

    Ok(launch_metadata
        .with_available_models(available_models)
        .with_switchable_profiles(switchable_profile_names(config, agent_profiles, profile)))
}

fn switchable_profile_names(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<String> {
    let mut profiles = config
        .map(|config| {
            config
                .agents
                .iter()
                .filter(|(name, profile)| {
                    agent_profiles.contains_key(name.as_str())
                        && !profile.hidden
                        && !profile.mode.is_subagent_only()
                        && name.as_str() != harness_core::session_title::TITLE_AGENT_NAME
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if profiles.is_empty() {
        profiles = ["build", "plan"]
            .into_iter()
            .filter(|profile| agent_profiles.contains_key(*profile))
            .map(str::to_string)
            .collect();
    }

    if profiles.is_empty() && agent_profiles.contains_key(selected_profile) {
        profiles.push(selected_profile.to_string());
    }

    let mut ordered = Vec::new();
    if let Some(index) = profiles
        .iter()
        .position(|profile| profile == selected_profile)
    {
        ordered.push(profiles.remove(index));
    }
    for profile in profiles {
        if !ordered.contains(&profile) {
            ordered.push(profile);
        }
    }
    ordered
}

fn model_options_for_profiles(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<ModelOption> {
    config
        .map(|config| configured_profile_model_options(config, agent_profiles, selected_profile))
        .unwrap_or_else(|| model_options_from_profiles(agent_profiles))
}

fn configured_profile_model_options(
    config: &HarnessConfig,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<ModelOption> {
    let catalog_entries = configured_model_catalog(config);
    let mut options = Vec::new();

    if agent_profiles.contains_key(selected_profile) {
        let profile_description = resolve_profile_model_metadata(config, selected_profile)
            .ok()
            .and_then(|metadata| metadata.profile_description);
        for entry in &catalog_entries {
            let option = ModelOption {
                profile: selected_profile.to_string(),
                provider: entry.provider.clone(),
                provider_display_label: Some(entry.provider_display_label.clone()),
                provider_backend_label: entry.provider_backend_label.clone(),
                model: entry.model.clone(),
                model_display_label: Some(entry.model_display_label.clone()),
                variant: entry.variant.clone(),
                variant_display_label: entry.variant_display_label.clone(),
                display_label: Some(entry.display_label.clone()),
                token_window_label: entry.token_window_label.clone(),
                context_window_tokens: entry.context_window_tokens,
                max_input_tokens: entry.max_input_tokens,
                max_output_tokens: entry.max_output_tokens,
                description: entry.description.clone(),
                profile_description: profile_description.clone(),
                reasoning_effort: entry.reasoning_effort.clone(),
                text_verbosity: entry.text_verbosity.clone(),
                recommended_for: entry.recommended_for.clone(),
            };

            if !options.iter().any(|existing| existing == &option) {
                options.push(option);
            }
        }
    }

    for profile in agent_profiles
        .keys()
        .filter(|profile| profile.as_str() != harness_core::session_title::TITLE_AGENT_NAME)
    {
        if let Ok(metadata) = resolve_profile_model_metadata(config, profile) {
            let configured_provider = metadata.provider.clone();
            let configured_model = metadata.model.clone();
            let profile_description = metadata.profile_description.clone();

            for entry in catalog_entries.iter().filter(|entry| {
                entry.provider == configured_provider && entry.model == configured_model
            }) {
                let option = ModelOption {
                    profile: profile.clone(),
                    provider: entry.provider.clone(),
                    provider_display_label: Some(entry.provider_display_label.clone()),
                    provider_backend_label: entry.provider_backend_label.clone(),
                    model: entry.model.clone(),
                    model_display_label: Some(entry.model_display_label.clone()),
                    variant: entry.variant.clone(),
                    variant_display_label: entry.variant_display_label.clone(),
                    display_label: Some(entry.display_label.clone()),
                    token_window_label: entry.token_window_label.clone(),
                    context_window_tokens: entry.context_window_tokens,
                    max_input_tokens: entry.max_input_tokens,
                    max_output_tokens: entry.max_output_tokens,
                    description: entry.description.clone(),
                    profile_description: profile_description.clone(),
                    reasoning_effort: entry.reasoning_effort.clone(),
                    text_verbosity: entry.text_verbosity.clone(),
                    recommended_for: entry.recommended_for.clone(),
                };

                if !options.iter().any(|existing| existing == &option) {
                    options.push(option);
                }
            }

            let preferred = ModelOption {
                profile: profile.clone(),
                provider: metadata.provider,
                provider_display_label: Some(metadata.provider_display_label),
                provider_backend_label: metadata.provider_backend_label,
                model: metadata.model,
                model_display_label: Some(metadata.model_display_label),
                variant: metadata.variant,
                variant_display_label: metadata.variant_display_label,
                display_label: Some(metadata.display_label),
                token_window_label: metadata.token_window_label,
                context_window_tokens: metadata.context_window_tokens,
                max_input_tokens: metadata.max_input_tokens,
                max_output_tokens: metadata.max_output_tokens,
                description: metadata.description,
                profile_description: metadata.profile_description,
                reasoning_effort: metadata.reasoning_effort,
                text_verbosity: metadata.text_verbosity,
                recommended_for: metadata.recommended_for,
            };

            if !options.iter().any(|option| option == &preferred) {
                options.push(preferred);
            }
        }
    }

    options
}

fn model_options_from_profiles(
    agent_profiles: &BTreeMap<String, AgentProfile>,
) -> Vec<ModelOption> {
    agent_profiles
        .values()
        .filter(|profile| profile.name != harness_core::session_title::TITLE_AGENT_NAME)
        .map(|profile| ModelOption::from_model_ref(profile.name.clone(), &profile.model_ref))
        .collect()
}

fn launch_metadata_for_connected_providers(
    launch_metadata: LaunchMetadata,
    connected_provider_ids: &[String],
    no_provider_connected: bool,
) -> LaunchMetadata {
    if no_provider_connected {
        return LaunchMetadata::new(launch_metadata.profile().to_string(), "local", None)
            .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec());
    }
    if connected_provider_ids.is_empty() {
        return launch_metadata;
    }

    let connected = connected_provider_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let available = launch_metadata
        .available_models()
        .iter()
        .filter(|option| connected.contains(option.provider.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let current_connected = connected.contains(launch_metadata.provider());
    let mut selected = if current_connected {
        launch_metadata.clone()
    } else if let Some(first) = available
        .iter()
        .find(|option| option.profile == launch_metadata.profile())
        .or_else(|| available.first())
    {
        LaunchMetadata::from_model_option(first)
    } else {
        LaunchMetadata::new(launch_metadata.profile().to_string(), "local", None)
    };
    selected = selected
        .with_available_models(available)
        .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec());
    selected
}

fn launch_metadata_model_ref(launch_metadata: &LaunchMetadata) -> Option<String> {
    Some(format!(
        "{}:{}",
        launch_metadata.provider(),
        launch_metadata.model()?
    ))
}

fn launch_metadata_model_settings(launch_metadata: &LaunchMetadata) -> AgentModelSettings {
    AgentModelSettings {
        variant: launch_metadata.variant().map(str::to_string),
        reasoning_effort: launch_metadata.reasoning_effort().map(str::to_string),
        text_verbosity: launch_metadata.text_verbosity().map(str::to_string),
        reasoning_summary: launch_metadata
            .reasoning_effort()
            .map(|_| "auto".to_string()),
    }
}

fn launch_metadata_for_mode(
    settings: &LiveSettings,
    selection: &LaunchSelection,
) -> LaunchMetadata {
    let launch_metadata = recover_mutex_lock(selection).clone();
    if let Some(mode_label) = settings.launch_mode_label.as_deref() {
        launch_metadata.with_mode_label(mode_label)
    } else {
        launch_metadata
    }
}

fn demo_coordinator_config(settings: &LiveSettings) -> CoordinatorConfig {
    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config
}

fn interactive_coordinator_config(settings: &LiveSettings) -> Result<CoordinatorConfig, String> {
    let mut config = settings
        .config
        .clone()
        .ok_or_else(bootstrap::interactive_config_guidance)?;
    config.apply_session_dir_override(Some(settings.session_dir.clone()));
    bootstrap::build_interactive_coordinator_config(&config)
}

fn prepare_new_live_workspace(
    settings: &LiveSettings,
    demo_mode: bool,
    run_id_override: &str,
) -> Result<PathBuf, String> {
    if demo_mode {
        return create_workspace(
            &settings.session_dir,
            ScenarioName::GoldenPathInteractive,
            Some(run_id_override),
        );
    }

    Ok(settings.workspace_root.clone())
}

fn record_launch_selection(selection: &LaunchSelection, launch_metadata: &LaunchMetadata) {
    let launch_metadata = launch_metadata.clone().without_mode_label();
    *recover_mutex_lock(selection) = launch_metadata.clone();
}

fn handle_model_switch_intent(
    intent: &UiIntent,
    launch_selection: &LaunchSelection,
    persist_model_selection: bool,
) -> bool {
    let UiIntent::SwitchModel {
        launch_metadata, ..
    } = intent
    else {
        return false;
    };

    record_launch_selection(launch_selection, launch_metadata);
    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(launch_selection));
    }
    true
}

fn scenario_launch_metadata() -> LaunchMetadata {
    LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo")
}

fn onboarding_required_for_runtime(config: Option<&HarnessConfig>, demo_mode: bool) -> bool {
    if demo_mode {
        return false;
    }
    let credential_store = harness_core::auth::CredentialStore::from_env();
    auth_onboarding_required_for_config(config, credential_store.as_ref())
}

#[cfg(not(test))]
fn auth_onboarding_required_for_config(
    config: Option<&HarnessConfig>,
    credential_store: Option<&harness_core::auth::CredentialStore>,
) -> bool {
    crate::auth_cmd::onboarding_required_for_config(
        config,
        &|name| {
            std::env::var(name)
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
        },
        credential_store,
    )
}

#[cfg(test)]
fn auth_onboarding_required_for_config(
    _config: Option<&HarnessConfig>,
    _credential_store: Option<&harness_core::auth::CredentialStore>,
) -> bool {
    false
}

async fn run_interactive_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
) -> Result<(), String> {
    profile_handoff("interactive_mode.begin");
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let launch_selection = Arc::new(Mutex::new(
        settings.launch_metadata.clone().without_mode_label(),
    ));
    let persist_model_selection = settings.config.is_some() && !demo_mode;
    let onboarding_required = onboarding_required_for_runtime(settings.config.as_ref(), demo_mode);
    let coordinator_config_warmup = LiveCoordinatorConfigWarmup::start(settings, demo_mode);
    profile_handoff("interactive_mode.warmup_started");

    let result = run_interactive_workflow_loop(
        InteractiveWorkflow::Startup,
        {
            let launch_selection = Arc::clone(&launch_selection);
            move || {
                set_pending_live_launch_metadata(launch_metadata_for_mode(
                    settings,
                    &launch_selection,
                ));
                load_startup_session_history_entries(&settings.session_dir)
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            move |session_history_entries| {
                run_startup_launcher(
                    cmd.exit_on_finish,
                    session_history_entries,
                    Arc::clone(&launch_selection),
                    persist_model_selection,
                    Some(prompt_history_path_for_session_dir(&settings.session_dir)),
                    onboarding_required,
                    TuiAuthBackendContext::from_settings(settings),
                )
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            let coordinator_config_warmup = coordinator_config_warmup.clone();
            move || {
                run_new_live_session(
                    cmd,
                    settings,
                    demo_mode,
                    Arc::clone(&launch_selection),
                    coordinator_config_warmup.clone(),
                )
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            let coordinator_config_warmup = coordinator_config_warmup.clone();
            move |run_id, run_dir| {
                run_continue_session_bootstrap(
                    cmd,
                    settings,
                    demo_mode,
                    run_id,
                    run_dir,
                    Arc::clone(&launch_selection),
                    coordinator_config_warmup.clone(),
                )
            }
        },
        |run_dir| async move { run_replay_tui(run_dir, cmd.exit_on_finish).await },
    )
    .await;

    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(&launch_selection));
    }
    close_preserved_terminal_session().map_err(|err| err.to_string())?;
    result
}

async fn run_interactive_workflow_loop<
    LoadStartupEntries,
    StartupRunner,
    NewSessionRunner,
    ContinueRunner,
    ReplayRunner,
    StartupFuture,
    NewSessionFuture,
    ContinueFuture,
    ReplayFuture,
>(
    initial_workflow: InteractiveWorkflow,
    mut load_startup_entries: LoadStartupEntries,
    mut run_startup: StartupRunner,
    mut run_new_session: NewSessionRunner,
    mut run_continue: ContinueRunner,
    mut run_replay: ReplayRunner,
) -> Result<(), String>
where
    LoadStartupEntries: FnMut() -> Result<Vec<SessionHistoryEntry>, String>,
    StartupRunner: FnMut(Vec<SessionHistoryEntry>) -> StartupFuture,
    StartupFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    NewSessionRunner: FnMut() -> NewSessionFuture,
    NewSessionFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    ContinueRunner: FnMut(String, PathBuf) -> ContinueFuture,
    ContinueFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    ReplayRunner: FnMut(PathBuf) -> ReplayFuture,
    ReplayFuture: Future<Output = Result<InteractiveWorkflow, String>>,
{
    let mut workflow = initial_workflow;
    loop {
        workflow = match workflow {
            InteractiveWorkflow::Startup => run_startup(load_startup_entries()?).await?,
            InteractiveWorkflow::NewSession => run_new_session().await?,
            InteractiveWorkflow::Continue { run_id, run_dir } => {
                run_continue(run_id, run_dir).await?
            }
            InteractiveWorkflow::Replay { run_dir } => run_replay(run_dir).await?,
            InteractiveWorkflow::Quit => return Ok(()),
        };
    }
}

async fn run_direct_continue_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
    run_dir: PathBuf,
) -> Result<(), String> {
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let launch_selection = Arc::new(Mutex::new(
        settings.launch_metadata.clone().without_mode_label(),
    ));
    let persist_model_selection = settings.config.is_some() && !demo_mode;
    let onboarding_required = onboarding_required_for_runtime(settings.config.as_ref(), demo_mode);
    let coordinator_config_warmup = LiveCoordinatorConfigWarmup::start(settings, demo_mode);
    let _ = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("direct_continue_mode.warmup_ready");
    let run_id = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "continue session requires a run directory path, got {}",
                run_dir.display()
            )
        })?
        .to_string();

    let result = run_interactive_workflow_loop(
        InteractiveWorkflow::Continue { run_id, run_dir },
        {
            let launch_selection = Arc::clone(&launch_selection);
            move || {
                set_pending_live_launch_metadata(launch_metadata_for_mode(
                    settings,
                    &launch_selection,
                ));
                load_startup_session_history_entries(&settings.session_dir)
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            move |session_history_entries| {
                run_startup_launcher(
                    cmd.exit_on_finish,
                    session_history_entries,
                    Arc::clone(&launch_selection),
                    persist_model_selection,
                    Some(prompt_history_path_for_session_dir(&settings.session_dir)),
                    onboarding_required,
                    TuiAuthBackendContext::from_settings(settings),
                )
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            let coordinator_config_warmup = coordinator_config_warmup.clone();
            move || {
                run_new_live_session(
                    cmd,
                    settings,
                    demo_mode,
                    Arc::clone(&launch_selection),
                    coordinator_config_warmup.clone(),
                )
            }
        },
        {
            let launch_selection = Arc::clone(&launch_selection);
            let coordinator_config_warmup = coordinator_config_warmup.clone();
            move |run_id, run_dir| {
                run_continue_session_bootstrap(
                    cmd,
                    settings,
                    demo_mode,
                    run_id,
                    run_dir,
                    Arc::clone(&launch_selection),
                    coordinator_config_warmup.clone(),
                )
            }
        },
        |run_dir| async move { run_replay_tui(run_dir, cmd.exit_on_finish).await },
    )
    .await;

    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(&launch_selection));
    }
    close_preserved_terminal_session().map_err(|err| err.to_string())?;
    result
}

fn load_startup_session_history_entries(
    session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    inspect_session_catalog(session_dir).map(|entries| {
        entries
            .into_iter()
            .filter(crate::replay::SessionInspectionEntry::is_visible_in_operator_history)
            .map(|entry| SessionHistoryEntry {
                run_dir: entry.run_dir,
                catalog: entry.catalog,
            })
            .collect()
    })
}

fn load_live_session_history_entries(
    run_dir: &Path,
    fallback_session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    let session_dir = run_dir.parent().unwrap_or(fallback_session_dir);
    load_startup_session_history_entries(session_dir)
}

async fn run_startup_launcher(
    exit_on_finish: bool,
    session_history_entries: Vec<SessionHistoryEntry>,
    launch_selection: LaunchSelection,
    persist_model_selection: bool,
    prompt_history_path: Option<PathBuf>,
    onboarding_required: bool,
    auth_backend: TuiAuthBackendContext,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("startup_launcher.begin");
    let selected_intent = Arc::new(Mutex::new(None::<UiIntent>));
    let selected_intent_sink = Arc::clone(&selected_intent);
    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let auth_update_tx = live_update_tx.clone();
    let startup_auth_backend = auth_backend.clone();
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if handle_model_switch_intent(&intent, &launch_selection, persist_model_selection) {
            return;
        }

        if let UiIntent::OpenAuthManager { args, stdin } = intent {
            spawn_tui_auth_backend_task(
                args,
                stdin,
                startup_auth_backend.config_path.clone(),
                startup_auth_backend.session_dir.clone(),
                startup_auth_backend.workspace_root.clone(),
                auth_update_tx.clone(),
            );
            return;
        }

        if !matches!(
            intent,
            UiIntent::NewSession
                | UiIntent::ReplaySession { .. }
                | UiIntent::ContinueSession { .. }
                | UiIntent::SubmitPrompt { .. }
                | UiIntent::CompactSession
                | UiIntent::InterruptSession { .. }
                | UiIntent::QuitRequested
        ) {
            return;
        }
        profile_handoff(&format!("startup_launcher.intent {intent:?}"));
        let mut slot = recover_mutex_lock(&selected_intent_sink);
        if slot.is_none() {
            *slot = Some(intent);
        }
    });

    let tui_result = tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Startup {
                session_history_entries,
                prompt_history_path,
                onboarding_required,
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings: None,
            toggles: None,
            preserve_terminal_on_exit: true,
        })
    })
    .await
    .map_err(|err| format!("startup launcher task failed: {err}"))?;

    if let Err(err) = tui_result {
        return Err(format!("startup launcher error: {err}"));
    }

    let selected_intent = recover_mutex_lock(&selected_intent).clone();
    profile_handoff(&format!("startup_launcher.end intent={selected_intent:?}"));

    Ok(map_startup_intent_to_workflow(selected_intent))
}

fn map_startup_intent_to_workflow(intent: Option<UiIntent>) -> InteractiveWorkflow {
    match intent {
        Some(UiIntent::NewSession) => InteractiveWorkflow::NewSession,
        Some(UiIntent::ReplaySession { run_dir, .. }) => InteractiveWorkflow::Replay { run_dir },
        Some(UiIntent::ContinueSession { run_id, run_dir }) => {
            InteractiveWorkflow::Continue { run_id, run_dir }
        }
        Some(UiIntent::SubmitPrompt { text, .. }) => {
            set_pending_live_prompt_auto_submit(Some(text));
            InteractiveWorkflow::NewSession
        }
        Some(UiIntent::QuitRequested)
        | None
        | Some(UiIntent::ResolvePermission { .. })
        | Some(UiIntent::OpenAuthManager { .. })
        | Some(UiIntent::CompactSession)
        | Some(UiIntent::InterruptSession { .. })
        | Some(UiIntent::ForkSession { .. })
        | Some(UiIntent::CloneSession { .. })
        | Some(UiIntent::SwitchModel { .. }) => InteractiveWorkflow::Quit,
    }
}

async fn run_replay_tui(
    run_dir: PathBuf,
    exit_on_finish: bool,
) -> Result<InteractiveWorkflow, String> {
    let events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    set_pending_replay_launch_metadata(Some(replay_launch_metadata_for_run(&run_dir, &events)));
    let selected_workflow = Arc::new(Mutex::new(None::<InteractiveWorkflow>));
    let selected_workflow_sink = Arc::clone(&selected_workflow);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if let Some(workflow) = live_workflow_from_intent(&intent) {
            capture_first_workflow(&selected_workflow_sink, workflow);
        }
    });

    tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Replay { run_dir, events },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings: None,
            toggles: None,
            preserve_terminal_on_exit: true,
        })
    })
    .await
    .map_err(|err| format!("replay tui task failed: {err}"))?
    .map_err(|err| format!("replay tui error: {err}"))?;

    take_selected_workflow_or(&selected_workflow, InteractiveWorkflow::Startup)
}

async fn run_continue_session_bootstrap(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
    run_id: String,
    run_dir: PathBuf,
    launch_selection: LaunchSelection,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff(&format!("continue_bootstrap.begin run_id={run_id}"));
    let resume_plan = inspect_resume_plan(&run_dir);
    if !resume_plan.is_resumable {
        let reason = resume_plan
            .resume_disabled_reason
            .unwrap_or_else(|| RESUME_UNAVAILABLE_FALLBACK_REASON.to_string());
        return Err(format!(
            "continue session is disabled for {run_id}: {reason}"
        ));
    }

    let historical_events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    let resume_agent_id = select_resume_agent_id(&resume_plan, &historical_events, &run_id)?;
    let run_name = latest_run_name(&historical_events)
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_RUN_NAME.to_string());

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("continue_bootstrap.coordinator_ready");
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );
    profile_handoff("continue_bootstrap.coordinator_spawned");

    let run = coordinator
        .resume_run(run_id.clone(), run_name)
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("continue_bootstrap.resume_run_done");
    if let Some(config) = settings.config.as_ref() {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }
    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("continue_bootstrap.event_store_done");

    let preloaded_last_seq = historical_events.last().map(|event| event.seq).unwrap_or(0);
    let recorded_runtime_context = load_recorded_runtime_context(&run_dir);
    let resume_profile = resume_plan
        .known_agents
        .get(&resume_agent_id)
        .map(String::as_str);
    let continue_metadata = continue_launch_metadata(
        &run.run_id,
        recorded_runtime_context.as_ref(),
        &historical_events,
        &resume_agent_id,
        resume_profile,
    )
    .with_available_models(
        recover_mutex_lock(&launch_selection)
            .available_models()
            .to_vec(),
    );
    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let intent_live_update_tx = live_update_tx.clone();

    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(resume_agent_id.clone()),
        profile: continue_metadata.profile().to_string(),
        last_request_id: latest_request_id_for_agent(&historical_events, &resume_agent_id),
    }));
    let forwarder_live_agent_target = Arc::clone(&live_agent_target);
    let event_forwarder_task = tokio::spawn(async move {
        forward_events_to_tui(
            store,
            live_update_tx,
            preloaded_last_seq.saturating_add(1),
            Some(forwarder_live_agent_target),
            false,
        )
        .await
    });

    let intent_coordinator = coordinator.clone();
    let intent_live_agent_target = Arc::clone(&live_agent_target);
    let auth_backend = TuiAuthBackendContext::from_settings(settings);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_live_agent_target),
            intent_live_update_tx,
            auth_backend,
        )
        .await
    });

    let (selected_workflow, ui_intent_sender) = build_live_ui_intent_router(
        intent_tx.clone(),
        Arc::clone(&launch_selection),
        settings.config.is_some() && !demo_mode,
    );

    let exit_on_finish = cmd.exit_on_finish;
    let toggles = Some(settings.toggles.clone());
    set_pending_live_launch_metadata(continue_metadata);
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    let session_history_entries =
        load_live_session_history_entries(&run.run_dir, &settings.session_dir)?;

    let tui_result = tokio::task::spawn_blocking(move || {
        run_tui_with_options(continue_live_tui_options(
            run.run_dir,
            historical_events,
            session_history_entries,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            true,
            prompt_history_path,
            toggles,
        ))
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;

    if let Err(err) = tui_result {
        event_forwarder_task.abort();
        ui_intent_task.abort();
        return Err(format!("TUI error: {err}"));
    }

    drop(intent_tx);

    let selected_workflow = take_selected_workflow(&selected_workflow)?;
    let stop_result = stop_live_source_run(&coordinator).await;
    event_forwarder_task.abort();
    ui_intent_task.abort();

    stop_result?;

    Ok(selected_workflow)
}

#[expect(
    clippy::too_many_arguments,
    reason = "continue live TUI options mirror the live handoff state explicitly"
)]
fn continue_live_tui_options(
    run_dir: PathBuf,
    historical_events: Vec<EventEnvelopeV1>,
    session_history_entries: Vec<SessionHistoryEntry>,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
    prompt_history_path: Option<PathBuf>,
    toggles: Option<TogglesConfig>,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events,
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        toggles,
        preserve_terminal_on_exit: true,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "new live TUI options mirror the live handoff state explicitly"
)]
fn new_live_tui_options(
    run_dir: PathBuf,
    session_history_entries: Vec<SessionHistoryEntry>,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
    prompt_history_path: Option<PathBuf>,
    toggles: Option<TogglesConfig>,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events: Vec::new(),
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        toggles,
        preserve_terminal_on_exit: true,
    }
}

fn load_recorded_runtime_context(run_dir: &Path) -> Option<RecordedRuntimeContext> {
    load_run_metadata(run_dir).and_then(|metadata| metadata.recorded_runtime_context)
}

fn launch_metadata_from_recorded_runtime_context(
    recorded_runtime_context: &RecordedRuntimeContext,
) -> LaunchMetadata {
    LaunchMetadata::from_model_option(&ModelOption {
        profile: recorded_runtime_context.profile.clone(),
        provider: recorded_runtime_context.provider.clone(),
        provider_display_label: recorded_runtime_context.provider_display_label.clone(),
        provider_backend_label: recorded_runtime_context.provider_backend_label.clone(),
        model: recorded_runtime_context.model.clone(),
        model_display_label: recorded_runtime_context.model_display_label.clone(),
        variant: recorded_runtime_context.variant.clone(),
        variant_display_label: recorded_runtime_context.variant_display_label.clone(),
        display_label: Some(recorded_runtime_context.display_label.clone())
            .filter(|value| model_selection_value_present(value)),
        token_window_label: recorded_runtime_context.token_window_label.clone(),
        context_window_tokens: recorded_runtime_context.context_window_tokens,
        max_input_tokens: recorded_runtime_context.max_input_tokens,
        max_output_tokens: recorded_runtime_context.max_output_tokens,
        description: recorded_runtime_context.description.clone(),
        profile_description: recorded_runtime_context.profile_description.clone(),
        reasoning_effort: recorded_runtime_context.reasoning_effort.clone(),
        text_verbosity: recorded_runtime_context.text_verbosity.clone(),
        recommended_for: recorded_runtime_context.recommended_for.clone(),
    })
}

fn replay_launch_metadata_for_run(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    let recorded_runtime_context = load_recorded_runtime_context(run_dir);
    replay_launch_metadata(recorded_runtime_context.as_ref(), historical_events)
}

fn replay_launch_metadata(
    recorded_runtime_context: Option<&RecordedRuntimeContext>,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    let fallback = LaunchMetadata::default().with_mode_label("Replay");
    if let Some(recorded_runtime_context) = recorded_runtime_context {
        return launch_metadata_from_recorded_runtime_context(recorded_runtime_context)
            .with_mode_label("Replay");
    }
    if historical_events.is_empty() {
        return fallback;
    }

    let profile = historical_events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.clone()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.profile().to_string());
    let (provider, model) = historical_events
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

    LaunchMetadata::new(profile, provider, model).with_mode_label("Replay")
}

fn build_live_ui_intent_router(
    intent_tx: mpsc::UnboundedSender<UiIntent>,
    launch_selection: LaunchSelection,
    persist_model_selection: bool,
) -> (SelectedWorkflow, UiIntentSink) {
    let selected_workflow = Arc::new(Mutex::new(None::<InteractiveWorkflow>));
    let selected_workflow_sink = Arc::clone(&selected_workflow);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        handle_model_switch_intent(&intent, &launch_selection, persist_model_selection);
        if let Some(workflow) = live_workflow_from_intent(&intent) {
            capture_first_workflow(&selected_workflow_sink, workflow);
        }
        if forward_intent_to_live_run(&intent) {
            let _ = intent_tx.send(intent);
        }
    });

    (selected_workflow, on_ui_intent)
}

fn live_workflow_from_intent(intent: &UiIntent) -> Option<InteractiveWorkflow> {
    match intent {
        UiIntent::NewSession => Some(InteractiveWorkflow::NewSession),
        UiIntent::ReplaySession { run_dir, .. } => Some(InteractiveWorkflow::Replay {
            run_dir: run_dir.clone(),
        }),
        UiIntent::ContinueSession { run_id, run_dir } => Some(InteractiveWorkflow::Continue {
            run_id: run_id.clone(),
            run_dir: run_dir.clone(),
        }),
        UiIntent::QuitRequested => Some(InteractiveWorkflow::Quit),
        UiIntent::ResolvePermission { .. }
        | UiIntent::SubmitPrompt { .. }
        | UiIntent::OpenAuthManager { .. }
        | UiIntent::CompactSession
        | UiIntent::InterruptSession { .. }
        | UiIntent::ForkSession { .. }
        | UiIntent::CloneSession { .. }
        | UiIntent::SwitchModel { .. } => None,
    }
}

fn forward_intent_to_live_run(intent: &UiIntent) -> bool {
    matches!(
        intent,
        UiIntent::ResolvePermission { .. }
            | UiIntent::SubmitPrompt { .. }
            | UiIntent::OpenAuthManager { .. }
            | UiIntent::CompactSession
            | UiIntent::InterruptSession { .. }
            | UiIntent::ForkSession { .. }
            | UiIntent::CloneSession { .. }
            | UiIntent::SwitchModel { .. }
            | UiIntent::QuitRequested
    )
}

fn capture_first_workflow(selected_workflow: &SelectedWorkflow, workflow: InteractiveWorkflow) {
    if let Ok(mut slot) = selected_workflow.lock() {
        if slot.is_none() {
            *slot = Some(workflow);
        }
    }
}

fn take_selected_workflow(
    selected_workflow: &SelectedWorkflow,
) -> Result<InteractiveWorkflow, String> {
    take_selected_workflow_or(selected_workflow, InteractiveWorkflow::Quit)
}

fn take_selected_workflow_or(
    selected_workflow: &SelectedWorkflow,
    default: InteractiveWorkflow,
) -> Result<InteractiveWorkflow, String> {
    selected_workflow
        .lock()
        .map_err(|_| "live workflow selection lock poisoned".to_string())
        .map(|mut slot| slot.take().unwrap_or(default))
}

async fn stop_live_source_run(coordinator: &CoordinatorHandle) -> Result<(), String> {
    let stop_result = coordinator.stop_run().await;
    if let Err(err) = stop_result {
        if !matches!(err, CoordinatorError::RunNotStarted) {
            return Err(err.to_string());
        }
    }
    Ok(())
}

fn latest_request_id_for_agent(
    historical_events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        (event.actor.kind == ActorKind::Worker && event.actor.agent_id.as_deref() == Some(agent_id))
            .then(|| event.correlation_id.clone())
            .flatten()
    })
}

fn continue_launch_metadata(
    run_id: &str,
    recorded_runtime_context: Option<&RecordedRuntimeContext>,
    historical_events: &[EventEnvelopeV1],
    resume_agent_id: &str,
    resume_profile: Option<&str>,
) -> LaunchMetadata {
    let fallback =
        LaunchMetadata::from_model_ref("unknown", "unknown:unknown").with_mode_label("Continued");
    if let Some(recorded_runtime_context) = recorded_runtime_context {
        return launch_metadata_from_recorded_runtime_context(recorded_runtime_context)
            .with_mode_label("Continued");
    }
    if historical_events.is_empty() {
        return fallback;
    }

    let profile = resume_profile.map(str::to_string).or_else(|| {
        historical_events.iter().rev().find_map(|event| {
            let EventV1::AgentSpawned(data) = &event.payload else {
                return None;
            };
            (data.agent_id == resume_agent_id).then(|| data.profile.clone())
        })
    });
    let provider_started = historical_events.iter().rev().find_map(|event| {
        let EventV1::ProviderRequestStarted(data) = &event.payload else {
            return None;
        };
        if event.actor.kind != ActorKind::Worker
            || event.actor.agent_id.as_deref() != Some(resume_agent_id)
        {
            return None;
        }
        Some((data.provider_id.clone(), data.model_id.clone()))
    });

    let (provider, model) =
        provider_started.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

    LaunchMetadata::new(
        profile.unwrap_or_else(|| format!("resumed:{run_id}")),
        provider,
        Some(model),
    )
    .with_mode_label("Continued")
}

async fn run_new_live_session(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
    launch_selection: LaunchSelection,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("new_live.begin");
    let run_id_override = if settings.deterministic {
        deterministic_run_id(settings.seed, ScenarioName::GoldenPathInteractive)
    } else {
        unique_interactive_run_id()
    };

    if settings.deterministic {
        let run_id = &run_id_override;
        let stale_run_dir = settings.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    let launch_metadata = launch_metadata_for_mode(settings, &launch_selection);
    let run_dir = settings.session_dir.join(&run_id_override);

    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let mut shutdown_tx = Some(shutdown_tx);

    let runtime_task = tokio::spawn(run_new_live_runtime(
        settings.clone(),
        demo_mode,
        run_id_override,
        launch_metadata.clone(),
        coordinator_config_warmup,
        live_update_tx,
        intent_rx,
        shutdown_rx,
    ));

    let (selected_workflow, ui_intent_sender) = build_live_ui_intent_router(
        intent_tx.clone(),
        Arc::clone(&launch_selection),
        settings.config.is_some() && !demo_mode,
    );

    let exit_on_finish = cmd.exit_on_finish;
    let toggles = Some(settings.toggles.clone());
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    set_pending_live_launch_metadata(launch_metadata);

    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        run_tui_with_options(new_live_tui_options(
            run_dir,
            Vec::new(),
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            true,
            prompt_history_path,
            toggles,
        ))
    })
    .await;
    profile_handoff("new_live.live_tui_end");

    let tui_result = match tui_result {
        Ok(result) => result,
        Err(err) => {
            if let Some(shutdown_tx) = shutdown_tx.take() {
                let _ = shutdown_tx.send(());
            }
            let _ = await_task("new live runtime", runtime_task).await;
            return Err(format!("TUI task failed: {err}"));
        }
    };

    if let Err(err) = tui_result {
        if let Some(shutdown_tx) = shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        let _ = await_task("new live runtime", runtime_task).await;
        return Err(format!("TUI error: {err}"));
    }

    drop(intent_tx);
    if let Some(shutdown_tx) = shutdown_tx.take() {
        let _ = shutdown_tx.send(());
    }

    let selected_workflow = take_selected_workflow(&selected_workflow);
    await_task("new live runtime", runtime_task).await?;

    selected_workflow
}

#[expect(
    clippy::too_many_arguments,
    reason = "new live runtime task wiring passes explicit runtime dependencies"
)]
async fn run_new_live_runtime(
    settings: LiveSettings,
    demo_mode: bool,
    run_id_override: String,
    launch_metadata: LaunchMetadata,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let _ = live_update_tx.send(LiveUpdate::Status("starting new session".to_string()));
    let bootstrap_error_tx = live_update_tx.clone();
    let session_history_update_tx = live_update_tx.clone();

    match bootstrap_new_live_runtime(
        &settings,
        demo_mode,
        run_id_override,
        launch_metadata,
        coordinator_config_warmup,
        live_update_tx,
        intent_rx,
    )
    .await
    {
        Ok(runtime) => {
            let session_history_task = spawn_session_history_refresh(
                settings.session_dir.clone(),
                session_history_update_tx,
            );
            runtime
                .wait_for_shutdown(shutdown_rx, session_history_task)
                .await
        }
        Err(err) => {
            let _ = bootstrap_error_tx.send(LiveUpdate::OperatorNotice {
                message: format!("new session failed: {err}"),
                level: OperatorNoticeLevel::Error,
            });
            let _ = runtime_bootstrap_error_notice(&err, shutdown_rx).await;
            Err(err)
        }
    }
}

struct NewLiveRuntime {
    coordinator: CoordinatorHandle,
    event_forwarder_task: JoinHandle<Result<(), String>>,
    ui_intent_task: JoinHandle<Result<(), String>>,
}

impl NewLiveRuntime {
    async fn wait_for_shutdown(
        self,
        shutdown_rx: oneshot::Receiver<()>,
        session_history_task: JoinHandle<Result<(), String>>,
    ) -> Result<(), String> {
        let _ = shutdown_rx.await;
        let stop_result = stop_live_source_run(&self.coordinator).await;
        self.event_forwarder_task.abort();
        self.ui_intent_task.abort();
        await_task("session history refresh", session_history_task).await?;
        stop_result
    }
}

fn spawn_session_history_refresh(
    session_dir: PathBuf,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
) -> JoinHandle<Result<(), String>> {
    tokio::task::spawn_blocking(move || {
        match load_startup_session_history_entries(&session_dir) {
            Ok(entries) => {
                let _ = live_update_tx.send(LiveUpdate::SessionHistory(entries));
            }
            Err(err) => {
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                    message: format!("session history refresh failed: {err}"),
                    level: OperatorNoticeLevel::Error,
                });
            }
        }
        Ok(())
    })
}

async fn runtime_bootstrap_error_notice(
    err: &str,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let _ = shutdown_rx.await;
    Err(err.to_string())
}

async fn bootstrap_new_live_runtime(
    settings: &LiveSettings,
    demo_mode: bool,
    run_id_override: String,
    launch_metadata: LaunchMetadata,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    intent_rx: mpsc::UnboundedReceiver<UiIntent>,
) -> Result<NewLiveRuntime, String> {
    let workspace = prepare_new_live_workspace(settings, demo_mode, run_id_override.as_str())?;
    profile_handoff("new_live.workspace_ready");

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("new_live.coordinator_ready");
    coordinator_config.run_id_override = Some(run_id_override);
    coordinator_config.session_mode_source = Some(if demo_mode {
        SessionModeSource::InteractiveMock
    } else {
        SessionModeSource::InteractiveLive
    });
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    let run_name = create_default_title(clock.as_ref(), false);
    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );
    profile_handoff("new_live.coordinator_spawned");

    let run = coordinator
        .start_run(run_name, &workspace)
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.start_run_done");
    if let Some(config) = settings.config.as_ref() {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }
    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.event_store_done");

    let agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            launch_metadata.profile().to_string(),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.spawn_agent_idle_done");

    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: launch_metadata.profile().to_string(),
        last_request_id: None,
    }));
    let event_forwarder_task = tokio::spawn({
        let live_update_tx = live_update_tx.clone();
        let forwarder_live_agent_target = Arc::clone(&live_agent_target);
        async move {
            forward_events_to_tui(
                store,
                live_update_tx,
                1,
                Some(forwarder_live_agent_target),
                false,
            )
            .await
        }
    });

    let intent_coordinator = coordinator.clone();
    let intent_live_agent_target = Arc::clone(&live_agent_target);
    let auth_backend = TuiAuthBackendContext::from_settings(settings);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_live_agent_target),
            live_update_tx,
            auth_backend,
        )
        .await
    });

    Ok(NewLiveRuntime {
        coordinator,
        event_forwarder_task,
        ui_intent_task,
    })
}

async fn run_live_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    scenario: ScenarioName,
) -> Result<(), String> {
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let deterministic_run_id = settings
        .deterministic
        .then(|| deterministic_run_id(settings.seed, scenario));

    if let Some(run_id) = &deterministic_run_id {
        let stale_run_dir = settings.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    let workspace = create_workspace(
        &settings.session_dir,
        scenario,
        deterministic_run_id.as_deref(),
    )?;

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config.run_id_override = deterministic_run_id;
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let (bootstrap_tx, bootstrap_rx) = oneshot::channel::<LiveBootstrap>();
    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let intent_live_update_tx = live_update_tx.clone();

    let scenario_coordinator = coordinator.clone();
    let scenario_task = tokio::spawn(async move {
        run_scenario_runner(scenario_coordinator, scenario, workspace, bootstrap_tx).await
    });

    let bootstrap = bootstrap_rx
        .await
        .map_err(|_| "scenario runner exited before live TUI bootstrap was ready".to_string())?;

    let LiveBootstrap { store, run_dir } = bootstrap;
    if let Some(config) = settings.config.as_ref() {
        let _ = logging::init_logging(config, &run_dir)?;
    }

    let stop_forwarder_after_terminal_event = cmd.exit_on_finish;
    let event_forwarder_task = tokio::spawn(async move {
        forward_events_to_tui(
            store,
            live_update_tx,
            1,
            None,
            stop_forwarder_after_terminal_event,
        )
        .await
    });

    let intent_coordinator = coordinator.clone();
    let auth_backend = TuiAuthBackendContext::from_settings(settings);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            None,
            intent_live_update_tx,
            auth_backend,
        )
        .await
    });

    let ui_intent_sender = {
        let intent_tx = intent_tx.clone();
        Arc::new(move |intent: UiIntent| {
            let _ = intent_tx.send(intent);
        })
    };

    let exit_on_finish = cmd.exit_on_finish;
    let toggles = Some(settings.toggles.clone());
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    set_pending_live_launch_metadata(scenario_launch_metadata());
    let session_history_entries =
        load_live_session_history_entries(&run_dir, &settings.session_dir)?;

    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        run_tui_with_options(new_live_tui_options(
            run_dir,
            session_history_entries,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            false,
            prompt_history_path,
            toggles,
        ))
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;
    profile_handoff("new_live.live_tui_end");

    if let Err(err) = tui_result {
        scenario_task.abort();
        event_forwarder_task.abort();
        ui_intent_task.abort();
        return Err(format!("TUI error: {err}"));
    }

    drop(intent_tx);

    if cmd.exit_on_finish {
        await_task("scenario runner", scenario_task).await?;
        await_task("event forwarder", event_forwarder_task).await?;
        await_task("ui intent handler", ui_intent_task).await?;
    } else {
        let stop_result = coordinator.stop_run().await;
        scenario_task.abort();
        event_forwarder_task.abort();
        ui_intent_task.abort();

        if let Err(err) = stop_result {
            if !matches!(err, CoordinatorError::RunNotStarted) {
                return Err(err.to_string());
            }
        }
    }

    Ok(())
}

async fn run_scenario_runner(
    coordinator: CoordinatorHandle,
    scenario: ScenarioName,
    workspace: PathBuf,
    bootstrap_tx: oneshot::Sender<LiveBootstrap>,
) -> Result<(), String> {
    let run = coordinator
        .start_run(scenario.as_str(), &workspace)
        .await
        .map_err(|err| err.to_string())?;

    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;
    let _ = bootstrap_tx.send(LiveBootstrap {
        store,
        run_dir: run.run_dir.clone(),
    });

    coordinator
        .spawn_agent(supervisor_actor(), "planner", None)
        .await
        .map_err(|err| err.to_string())?;

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .map_err(|err| err.to_string())?;

    let tool_call_id = coordinator
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit",
            golden_path_edit_args(),
        )
        .await
        .map_err(|err| err.to_string())?;

    if !scenario.interactive_permissions() {
        let permission_id =
            wait_for_permission_id(&run.events_path, &tool_call_id, DEFAULT_EVENT_WAIT_TIMEOUT)
                .await?;
        coordinator
            .resolve_permission(permission_id, PermissionDecision::Allow, None)
            .await
            .map_err(|err| err.to_string())?;
    }

    let tool_status = wait_for_tool_finished(
        &run.events_path,
        &tool_call_id,
        if scenario.interactive_permissions() {
            None
        } else {
            Some(DEFAULT_EVENT_WAIT_TIMEOUT)
        },
        ToolFinishTerminalEvents::Error,
    )
    .await?;

    if tool_status != ToolCallStatus::Succeeded {
        return Err(format!("tool call did not succeed: {tool_status:?}"));
    }

    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    Ok(())
}

async fn forward_events_to_tui(
    store: Arc<dyn EventStore>,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    start_from_seq: u64,
    live_agent_target: Option<LiveAgentTargetState>,
    stop_after_terminal_event: bool,
) -> Result<(), String> {
    let mut from_seq = start_from_seq.max(1);
    let mut last_seq_seen = from_seq.saturating_sub(1);

    loop {
        let mut stream = store.subscribe(from_seq).map_err(|err| err.to_string())?;
        let mut should_resubscribe = false;

        while let Some(next) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match next {
                Ok(event) => {
                    if event.seq <= last_seq_seen {
                        continue;
                    }

                    let terminal_event = is_terminal_event(&event.payload);
                    last_seq_seen = event.seq;
                    from_seq = last_seq_seen.saturating_add(1);
                    maybe_update_live_agent_target_for_plan_handoff(
                        &event,
                        live_agent_target.as_ref(),
                    );
                    if live_update_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .is_err()
                    {
                        return Ok(());
                    }
                    if stop_after_terminal_event && terminal_event {
                        return Ok(());
                    }
                }
                Err(EventStoreError::SubscriberLagged(skipped)) => {
                    let _ = live_update_tx.send(LiveUpdate::Status(format!(
                        "live stream lagged by {skipped}; replaying from seq {}",
                        last_seq_seen.saturating_add(1)
                    )));

                    let mut replay = store
                        .replay(last_seq_seen.saturating_add(1))
                        .map_err(|err| err.to_string())?;
                    while let Some(replayed) =
                        std::future::poll_fn(|cx| replay.as_mut().poll_next(cx)).await
                    {
                        let replayed_event = replayed.map_err(|err| err.to_string())?;
                        if replayed_event.seq <= last_seq_seen {
                            continue;
                        }

                        let terminal_event = is_terminal_event(&replayed_event.payload);
                        last_seq_seen = replayed_event.seq;
                        from_seq = last_seq_seen.saturating_add(1);
                        maybe_update_live_agent_target_for_plan_handoff(
                            &replayed_event,
                            live_agent_target.as_ref(),
                        );
                        if live_update_tx
                            .send(LiveUpdate::Event(Box::new(replayed_event)))
                            .is_err()
                        {
                            return Ok(());
                        }
                        if stop_after_terminal_event && terminal_event {
                            return Ok(());
                        }
                    }

                    should_resubscribe = true;
                    break;
                }
                Err(err) => {
                    return Err(format!("live stream error: {err}"));
                }
            }
        }

        if should_resubscribe {
            continue;
        }

        break;
    }

    Ok(())
}

fn manual_compaction_success_message(
    checkpoint_id: &str,
    tokens_before_estimate: Option<u32>,
    tokens_after_estimate: Option<u32>,
) -> String {
    let prefix = format!("manual compaction checkpoint written: {checkpoint_id}");
    match (tokens_before_estimate, tokens_after_estimate) {
        (Some(before), Some(after)) if before != after => format!(
            "{prefix} · active ctx {} → {} est",
            compact_token_estimate(before),
            compact_token_estimate(after)
        ),
        (Some(_), Some(_)) => format!("{prefix} · active ctx estimate unchanged"),
        _ => prefix,
    }
}

fn maybe_update_live_agent_target_for_plan_handoff(
    event: &EventEnvelopeV1,
    live_agent_target: Option<&LiveAgentTargetState>,
) {
    let Some(live_agent_target) = live_agent_target else {
        return;
    };
    let EventV1::AgentSpawned(payload) = &event.payload else {
        return;
    };
    if payload.profile != harness_core::plan::BUILD_AGENT_NAME {
        return;
    }

    let mut target = recover_mutex_lock(live_agent_target);
    if target.profile != harness_core::plan::PLAN_AGENT_NAME {
        return;
    }
    if payload.parent_agent_id.as_deref() != target.agent_id.as_deref() {
        return;
    }

    target.agent_id = Some(payload.agent_id.clone());
    target.profile = payload.profile.clone();
    target.last_request_id = None;
}

fn compact_token_estimate(value: u32) -> String {
    if value >= 1_000_000 {
        return format!("{:.1}M", f64::from(value) / 1_000_000.0);
    }
    if value >= 1_000 {
        return format!("{:.1}K", f64::from(value) / 1_000.0);
    }
    value.to_string()
}

fn spawn_tui_auth_backend_task(
    args: Vec<String>,
    stdin: Option<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
) {
    let normalized_args = normalize_tui_auth_args(args.clone());
    let display = display_tui_auth_args(&normalized_args);
    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
        message: format!("auth backend running: harness auth {display}"),
        level: OperatorNoticeLevel::Info,
    });
    std::thread::spawn(move || {
        let (message, level, success) = run_tui_auth_backend_once(
            args,
            config_path.clone(),
            session_dir.clone(),
            workspace_root.clone(),
            stdin.unwrap_or_default(),
            Some(live_update_tx.clone()),
        );
        let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
        let _ = live_update_tx.send(LiveUpdate::AuthBackendResult { success });
        if success {
            match refreshed_launch_metadata_after_auth(
                normalized_args.first().map(String::as_str),
                config_path,
                session_dir,
                workspace_root,
            ) {
                Ok(Some(launch_metadata)) => {
                    let _ = live_update_tx.send(LiveUpdate::AuthProviderCatalogRefreshed {
                        launch_metadata: Box::new(launch_metadata),
                    });
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: "provider catalog refreshed; choose a model with /model"
                            .to_string(),
                        level: OperatorNoticeLevel::Info,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: format!("provider catalog refresh skipped: {err}"),
                        level: OperatorNoticeLevel::Error,
                    });
                }
            }
        }
    });
}

fn refreshed_launch_metadata_after_auth(
    command: Option<&str>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
) -> Result<Option<LaunchMetadata>, String> {
    if command != Some("login") {
        return Ok(None);
    }
    let settings = resolve_live_settings(
        &TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: None,
        },
        config_path,
        session_dir,
        workspace_root.clone(),
        &harness_core::config::ConfigLoadContext::from_env().with_current_dir(workspace_root),
    )?;
    Ok(Some(settings.launch_metadata))
}

fn run_tui_auth_backend_once(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    stdin: String,
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
) -> (String, OperatorNoticeLevel, bool) {
    let deps = harness::CliDeps::real().with_current_dir(workspace_root);
    run_tui_auth_backend_streaming_with_deps(
        args,
        config_path,
        session_dir,
        &stdin,
        &deps,
        live_update_tx,
    )
}

#[cfg(test)]
fn run_tui_auth_backend_once_with_deps(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    deps: &harness::CliDeps,
) -> (String, OperatorNoticeLevel) {
    let (message, level, _) =
        run_tui_auth_backend_streaming_with_deps(args, config_path, session_dir, "", deps, None);
    (message, level)
}

fn run_tui_auth_backend_streaming_with_deps(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdin: &str,
    deps: &harness::CliDeps,
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
) -> (String, OperatorNoticeLevel, bool) {
    let args = normalize_tui_auth_args(args);
    let mut stdin = std::io::Cursor::new(stdin.as_bytes().to_vec());
    let mut stdout = TuiAuthNoticeWriter::new(
        live_update_tx.clone(),
        OperatorNoticeLevel::Info,
        "auth backend output",
    );
    let mut stderr = TuiAuthNoticeWriter::new(
        live_update_tx,
        OperatorNoticeLevel::Error,
        "auth backend error",
    );
    let mut io = harness::CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let code =
        harness::execute_auth_backend_args_with_io(&args, config_path, session_dir, &mut io, deps);
    stdout.flush_pending();
    stderr.flush_pending();
    let output = harness::AuthBackendOutput {
        code,
        stdout: stdout.captured(),
        stderr: stderr.captured(),
    };
    let level = if output.code == 0 {
        OperatorNoticeLevel::Info
    } else {
        OperatorNoticeLevel::Error
    };
    (
        format_tui_auth_backend_output(&args, &output),
        level,
        output.code == 0,
    )
}

struct TuiAuthNoticeWriter {
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
    level: OperatorNoticeLevel,
    prefix: &'static str,
    redactor: DefaultRedactor,
    pending: String,
    captured: String,
}

impl TuiAuthNoticeWriter {
    fn new(
        live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
        level: OperatorNoticeLevel,
        prefix: &'static str,
    ) -> Self {
        Self {
            live_update_tx,
            level,
            prefix,
            redactor: DefaultRedactor::default(),
            pending: String::new(),
            captured: String::new(),
        }
    }

    fn captured(&self) -> String {
        self.captured.clone()
    }

    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.pending);
        self.emit_line(&line);
    }

    fn emit_line(&mut self, raw_line: &str) {
        let redacted = compact_auth_backend_text(&self.redactor.redact_text(raw_line));
        if redacted.is_empty() {
            return;
        }
        self.captured.push_str(&redacted);
        self.captured.push('\n');
        if let Some(tx) = &self.live_update_tx {
            let _ = tx.send(LiveUpdate::OperatorNotice {
                message: format!("{}: {redacted}", self.prefix),
                level: self.level,
            });
        }
    }
}

impl Write for TuiAuthNoticeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.pending.push_str(&text);
        while let Some(newline_index) = self.pending.find('\n') {
            let line = self.pending[..newline_index]
                .trim_end_matches('\r')
                .to_string();
            self.pending.drain(..=newline_index);
            self.emit_line(&line);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_pending();
        Ok(())
    }
}

fn normalize_tui_auth_args(args: Vec<String>) -> Vec<String> {
    if args.is_empty() {
        vec!["list".to_string()]
    } else {
        args
    }
}

fn display_tui_auth_args(args: &[String]) -> String {
    let mut display = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            display.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        if tui_auth_arg_redacts_next(arg) {
            display.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_tui_auth_arg_value(arg) {
            display.push(redacted);
            continue;
        }
        display.push(arg.clone());
    }
    display.join(" ")
}

fn tui_auth_arg_redacts_next(arg: &str) -> bool {
    matches!(
        arg,
        "--mock-token" | "--mock-refresh-token" | "--enterprise-url"
    )
}

fn redact_tui_auth_arg_value(arg: &str) -> Option<String> {
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

fn format_tui_auth_backend_output(args: &[String], output: &harness::AuthBackendOutput) -> String {
    let command = display_tui_auth_args(args);
    let stdout = compact_auth_backend_text(&output.stdout);
    let stderr = compact_auth_backend_text(&output.stderr);
    match (output.code, stdout.is_empty(), stderr.is_empty()) {
        (0, false, true) => format!("auth backend completed: harness auth {command}\n{stdout}"),
        (0, true, false) => format!("auth backend completed: harness auth {command}\n{stderr}"),
        (0, false, false) => {
            format!("auth backend completed: harness auth {command}\n{stdout}\n{stderr}")
        }
        (0, true, true) => format!("auth backend completed: harness auth {command}"),
        (_, false, true) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stdout}",
                output.code
            )
        }
        (_, true, false) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stderr}",
                output.code
            )
        }
        (_, false, false) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stdout}\n{stderr}",
                output.code
            )
        }
        (_, true, true) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}",
                output.code
            )
        }
    }
}

fn compact_auth_backend_text(text: &str) -> String {
    const MAX_AUTH_NOTICE_CHARS: usize = 1600;
    let compact = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if compact.chars().count() <= MAX_AUTH_NOTICE_CHARS {
        compact
    } else {
        let mut truncated = compact
            .chars()
            .take(MAX_AUTH_NOTICE_CHARS)
            .collect::<String>();
        truncated.push_str("\n… truncated");
        truncated
    }
}

#[derive(Clone)]
struct TuiAuthBackendContext {
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
}

impl TuiAuthBackendContext {
    fn from_settings(settings: &LiveSettings) -> Self {
        Self {
            config_path: settings.config_path.clone(),
            session_dir: Some(settings.session_dir.clone()),
            workspace_root: settings.workspace_root.clone(),
        }
    }
}

async fn handle_ui_intents(
    coordinator: CoordinatorHandle,
    mut intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    user_actor: EventActor,
    live_agent_target: Option<LiveAgentTargetState>,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    auth_backend: TuiAuthBackendContext,
) -> Result<(), String> {
    while let Some(intent) = intent_rx.recv().await {
        match intent {
            UiIntent::ResolvePermission {
                permission_id,
                decision,
                reason,
                grant_scope,
            } => {
                coordinator
                    .resolve_permission_with_grant_scope(
                        permission_id,
                        decision,
                        reason,
                        grant_scope,
                    )
                    .await
                    .map_err(|err| err.to_string())?;
            }
            UiIntent::SubmitPrompt {
                text,
                selected_file_tags,
                selected_agent_tags,
                selected_resource_tags,
                launch_metadata,
            } => {
                let agent_id = live_agent_target.as_ref().and_then(|target| {
                    target
                        .lock()
                        .ok()
                        .and_then(|target| target.agent_id.clone())
                });

                if let Some(agent_id) = agent_id {
                    let request_id = coordinator
                        .request_agent_turn_with_model_and_selected_tags(
                            user_actor.clone(),
                            agent_id,
                            text,
                            harness_core::file_tag::SelectedPromptTags {
                                files: selected_file_tags,
                                agents: selected_agent_tags,
                                resources: selected_resource_tags,
                            },
                            launch_metadata_model_ref(&launch_metadata),
                            Some(launch_metadata_model_settings(&launch_metadata)),
                        )
                        .await
                        .map_err(|err| err.to_string())?;
                    if let Some(live_agent_target) = live_agent_target.as_ref() {
                        let mut target = live_agent_target
                            .lock()
                            .map_err(|_| "live agent target lock poisoned".to_string())?;
                        target.last_request_id = Some(request_id);
                    }
                }
            }
            UiIntent::CompactSession => {
                let Some(live_agent_target) = live_agent_target.as_ref() else {
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: "manual compaction unavailable: no live agent target".to_string(),
                        level: OperatorNoticeLevel::Error,
                    });
                    continue;
                };

                let (agent_id, through_request_id) = live_agent_target
                    .lock()
                    .map_err(|_| "live agent target lock poisoned".to_string())
                    .map(|target| (target.agent_id.clone(), target.last_request_id.clone()))?;

                let Some(agent_id) = agent_id else {
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: "manual compaction unavailable: no active live agent".to_string(),
                        level: OperatorNoticeLevel::Error,
                    });
                    continue;
                };

                let (message, level) = match coordinator
                    .compact_agent_context(agent_id, through_request_id, "manual")
                    .await
                {
                    Ok(ManualCompactionOutcome::CheckpointWritten {
                        checkpoint_id,
                        tokens_before_estimate,
                        tokens_after_estimate,
                    }) => (
                        manual_compaction_success_message(
                            &checkpoint_id,
                            tokens_before_estimate,
                            tokens_after_estimate,
                        ),
                        OperatorNoticeLevel::Info,
                    ),
                    Ok(ManualCompactionOutcome::NoOp) => (
                        "manual compaction skipped: need at least two completed turns".to_string(),
                        OperatorNoticeLevel::Info,
                    ),
                    Err(err) => (
                        format!("manual compaction failed: {err}"),
                        OperatorNoticeLevel::Error,
                    ),
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::OpenAuthManager { args, stdin } => {
                spawn_tui_auth_backend_task(
                    args,
                    stdin,
                    auth_backend.config_path.clone(),
                    auth_backend.session_dir.clone(),
                    auth_backend.workspace_root.clone(),
                    live_update_tx.clone(),
                );
            }
            UiIntent::InterruptSession { task_ids } => {
                for task_id in task_ids {
                    if let Err(err) = coordinator.cancel_task(task_id, "interrupted").await {
                        let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                            message: format!("interrupt failed: {err}"),
                            level: OperatorNoticeLevel::Error,
                        });
                    }
                }
            }
            UiIntent::ForkSession {
                source_run_dir,
                events,
                stable_prefix,
                prompt_text,
            } => {
                let notice =
                    materialize_tui_fork_child(source_run_dir, events, stable_prefix, prompt_text);
                let _ = live_update_tx.send(notice);
            }
            UiIntent::CloneSession {
                source_run_dir,
                events,
                stable_prefix,
            } => {
                let notice =
                    materialize_tui_lineage_child("clone", source_run_dir, events, stable_prefix);
                let _ = live_update_tx.send(notice);
            }
            UiIntent::SwitchModel { profile, .. } => {
                let Some(live_agent_target) = live_agent_target.as_ref() else {
                    continue;
                };

                let already_selected = live_agent_target
                    .lock()
                    .map_err(|_| "live agent target lock poisoned".to_string())?
                    .profile
                    == profile;
                if already_selected {
                    continue;
                }

                let agent_id = coordinator
                    .spawn_agent_idle(supervisor_actor(), profile.clone(), None)
                    .await
                    .map_err(|err| err.to_string())?;
                let mut target = live_agent_target
                    .lock()
                    .map_err(|_| "live agent target lock poisoned".to_string())?;
                target.agent_id = Some(agent_id);
                target.profile = profile;
                target.last_request_id = None;
            }
            UiIntent::NewSession
            | UiIntent::ReplaySession { .. }
            | UiIntent::ContinueSession { .. } => {}
            UiIntent::QuitRequested => {
                let stop_result = coordinator.stop_run().await;
                if let Err(err) = stop_result {
                    if !matches!(err, CoordinatorError::RunNotStarted) {
                        return Err(err.to_string());
                    }
                }
                break;
            }
        }
    }
    Ok(())
}

fn materialize_tui_lineage_child(
    operation: &'static str,
    source_run_dir: PathBuf,
    events: Vec<EventEnvelopeV1>,
    stable_prefix: StableSessionPrefix,
) -> LiveUpdate {
    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    });

    match result {
        Ok(result) => LiveUpdate::OperatorNotice {
            message: tui_lineage_success_message(operation, &result),
            level: OperatorNoticeLevel::Info,
        },
        Err(err) => LiveUpdate::OperatorNotice {
            message: format!("Harness session {operation} blocked: {err}"),
            level: OperatorNoticeLevel::Error,
        },
    }
}

fn materialize_tui_fork_child(
    source_run_dir: PathBuf,
    events: Vec<EventEnvelopeV1>,
    stable_prefix: StableSessionPrefix,
    prompt_text: String,
) -> LiveUpdate {
    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    });

    match result {
        Ok(result) => LiveUpdate::ContinueSession {
            run_id: result.child_run_id,
            run_dir: result.child_run_dir,
            prompt_draft: prompt_text,
        },
        Err(err) => LiveUpdate::OperatorNotice {
            message: format!("Harness session fork blocked: {err}"),
            level: OperatorNoticeLevel::Error,
        },
    }
}

fn tui_lineage_success_message(
    operation: &str,
    result: &ChildSessionMaterializationResult,
) -> String {
    format!(
        "Harness session {operation} created {} from seq {} ({} events, {} artifacts)",
        result.child_run_id, result.source_cutoff_seq, result.event_count, result.artifact_count
    )
}

fn user_actor() -> EventActor {
    EventActor::new(ActorKind::User, Some("interactive-user".to_string()))
}

async fn await_task(name: &str, handle: JoinHandle<Result<(), String>>) -> Result<(), String> {
    match handle.await {
        Ok(result) => result.map_err(|err| format!("{name} task failed: {err}")),
        Err(err) => Err(format!("{name} task join failed: {err}")),
    }
}

fn has_terminal_event(events: &[EventEnvelopeV1]) -> bool {
    events.iter().any(|event| is_terminal_event(&event.payload))
}

fn is_terminal_event(payload: &EventV1) -> bool {
    matches!(payload, EventV1::RunFinished(_) | EventV1::RunFailed(_))
}

fn unique_interactive_run_id() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let namespace = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("interactive:{}:{}", std::process::id(), stamp).as_bytes(),
    );
    format!("run_{}", namespace.simple())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn replay_launch_metadata_for_test(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    replay_launch_metadata_for_run(run_dir, historical_events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::most_recent_conversational_agent_id;
    use harness_core::auth::{
        AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
    };
    use harness_core::config::load_config_from_str;
    use harness_core::event::{
        AgentSpawnedEvent, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
        RunFinishedEvent, RunStartedEvent, SCHEMA_VERSION,
    };
    use harness_core::store::{EventEnvelopeWithoutSeqV1, InMemoryEventStore};
    use harness_tui::app::{set_pending_live_prompt_draft, AppState};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    fn mock_mode_cwd_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn live_tui_command() -> TuiCommand {
        TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: None,
        }
    }

    fn with_harness_data_home<T>(data_home: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = mock_mode_cwd_test_lock().lock().expect("env lock poisoned");
        let previous = std::env::var_os("HARNESS_DATA_HOME");
        std::env::set_var("HARNESS_DATA_HOME", data_home);
        let outcome = f();
        match previous {
            Some(value) => std::env::set_var("HARNESS_DATA_HOME", value),
            None => std::env::remove_var("HARNESS_DATA_HOME"),
        }
        outcome
    }

    fn lineage_test_event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_tui_lineage_{seq:04}"),
            seq,
            run_id: "run_tui_lineage_source".to_string(),
            mono_ms: seq,
            ts: Some(format!("2026-05-03T00:00:{seq:02}Z")),
            actor: EventActor::new(ActorKind::System, Some("tui-lineage-test".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_tui_lineage_source".to_string()),
            payload,
        }
    }

    #[test]
    fn plan_handoff_updates_live_agent_target_to_spawned_build_agent() {
        let target = Arc::new(Mutex::new(LiveAgentTarget {
            agent_id: Some("agent_plan".to_string()),
            profile: "plan".to_string(),
            last_request_id: Some("req_plan".to_string()),
        }));
        let event = lineage_test_event(
            1,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_build".to_string(),
                profile: "build".to_string(),
                parent_agent_id: Some("agent_plan".to_string()),
            }),
        );

        maybe_update_live_agent_target_for_plan_handoff(&event, Some(&target));

        let target = target.lock().expect("target lock");
        assert_eq!(target.agent_id.as_deref(), Some("agent_build"));
        assert_eq!(target.profile, "build");
        assert_eq!(target.last_request_id, None);
    }

    fn stable_lineage_test_events() -> Vec<EventEnvelopeV1> {
        vec![
            lineage_test_event(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "tui lineage source".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            lineage_test_event(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "stable".to_string(),
                }),
            ),
        ]
    }

    fn active_stable_lineage_test_events() -> Vec<EventEnvelopeV1> {
        vec![
            lineage_test_event(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "tui active lineage source".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            lineage_test_event(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_tui_lineage".to_string(),
                    parent_agent_id: None,
                    profile: "build".to_string(),
                }),
            ),
            lineage_test_event(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.5".to_string(),
                    prompt_summary: "first turn".to_string(),
                    request_digest: "digest-tui-lineage".to_string(),
                    metadata: None,
                }),
            ),
            lineage_test_event(
                4,
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-output".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
        ]
    }

    fn first_prompt_lineage_test_events() -> Vec<EventEnvelopeV1> {
        vec![
            lineage_test_event(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "tui first prompt lineage source".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            lineage_test_event(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_tui_lineage".to_string(),
                    parent_agent_id: None,
                    profile: "build".to_string(),
                }),
            ),
        ]
    }

    fn write_recorded_runtime_context_meta(run_dir: &Path) {
        let meta = serde_json::json!({
            "run_id": "run_tui_lineage_source",
            "run_name": "tui lineage source",
            "workspace_root": "/workspace",
            "created_at": "2026-05-04T00:00:00Z",
            "config_digest": "digest-config",
            "harness_version": env!("CARGO_PKG_VERSION"),
            "recorded_runtime_context": {
                "profile": "build",
                "provider": "default",
                "model": "gpt-5.5",
                "variant": null,
                "display_label": "gpt-5.5",
                "token_window_label": null,
                "context_window_tokens": null,
                "max_input_tokens": null,
                "max_output_tokens": null,
                "description": null,
                "recommended_for": null,
                "reasoning_effort": null,
                "text_verbosity": null
            }
        });
        std::fs::write(
            run_dir.join("meta.json"),
            serde_json::to_vec_pretty(&meta).expect("serialize meta"),
        )
        .expect("write source meta");
    }

    fn catalog_event(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_{run_id}_{seq:04}"),
            seq,
            run_id: run_id.to_string(),
            mono_ms: seq,
            ts: Some(format!("2026-05-03T00:01:{seq:02}Z")),
            actor: EventActor::new(ActorKind::System, Some("tui-catalog-test".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        }
    }

    fn forwarder_event_draft(
        run_id: &str,
        marker: &str,
        payload: EventV1,
    ) -> EventEnvelopeWithoutSeqV1 {
        EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_forwarder_{marker}"),
            run_id: run_id.to_string(),
            mono_ms: 0,
            ts: Some("2026-05-03T00:02:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("tui-forwarder-test".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        }
    }

    fn catalog_events(run_id: &str) -> Vec<EventEnvelopeV1> {
        vec![
            catalog_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: run_id.replace('_', " "),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            catalog_event(
                run_id,
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "stable".to_string(),
                }),
            ),
        ]
    }

    fn write_catalog_run(run_dir: &Path, events: &[EventEnvelopeV1]) {
        std::fs::create_dir_all(run_dir).expect("create catalog run dir");
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize event"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n"))
            .expect("write catalog events");
    }

    #[test]
    fn tui_startup_new_session_bootstraps_live_after_intent() {
        assert_eq!(
            map_startup_intent_to_workflow(Some(UiIntent::NewSession)),
            InteractiveWorkflow::NewSession
        );
    }

    #[test]
    fn tui_startup_replay_session_uses_replay_mode() {
        let run_dir = PathBuf::from("/tmp/sessions/run_replay");
        assert_eq!(
            map_startup_intent_to_workflow(Some(UiIntent::ReplaySession {
                run_id: "run_replay".to_string(),
                run_dir: run_dir.clone(),
            })),
            InteractiveWorkflow::Replay { run_dir }
        );
    }

    #[test]
    fn tui_startup_continue_session_uses_continue_workflow() {
        let run_dir = PathBuf::from("/tmp/sessions/run_continue");
        assert_eq!(
            map_startup_intent_to_workflow(Some(UiIntent::ContinueSession {
                run_id: "run_continue".to_string(),
                run_dir: run_dir.clone(),
            })),
            InteractiveWorkflow::Continue {
                run_id: "run_continue".to_string(),
                run_dir,
            }
        );
    }

    #[test]
    fn tui_startup_carries_unsent_draft_into_new_live_session() {
        set_pending_live_prompt_draft(Some("draft to keep".to_string()));

        let live = AppState::new_live(None, false, None);
        assert_eq!(live.prompt_buffer, "draft to keep");
    }

    #[test]
    fn workflow_managed_live_tuis_preserve_terminal_between_handoffs() {
        let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
        let sink: UiIntentSink = Arc::new(|_| {});

        let fresh = new_live_tui_options(
            PathBuf::from("/tmp/run-new"),
            Vec::new(),
            rx,
            false,
            Arc::clone(&sink),
            true,
            None,
            None,
        );
        assert!(fresh.preserve_terminal_on_exit);
        assert!(matches!(
            fresh.mode,
            TuiMode::Live {
                compact_session_supported: true,
                ..
            }
        ));

        let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
        let resumed = continue_live_tui_options(
            PathBuf::from("/tmp/run-continue"),
            Vec::new(),
            Vec::new(),
            rx,
            false,
            sink,
            true,
            None,
            None,
        );
        assert!(resumed.preserve_terminal_on_exit);
        assert!(matches!(
            resumed.mode,
            TuiMode::Live {
                compact_session_supported: true,
                ..
            }
        ));
    }

    #[test]
    fn new_live_tui_options_allow_pre_bootstrap_run_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let run_dir = temp_dir.path().join("run_projected_new_session");
        let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
        let sink: UiIntentSink = Arc::new(|_| {});

        let options = new_live_tui_options(
            run_dir.clone(),
            Vec::new(),
            rx,
            false,
            sink,
            true,
            None,
            None,
        );

        let TuiMode::Live {
            run_dir: configured_run_dir,
            historical_events,
            ..
        } = options.mode
        else {
            panic!("expected live TUI mode");
        };
        assert_eq!(configured_run_dir, run_dir);
        assert!(historical_events.is_empty());
        assert!(options.preserve_terminal_on_exit);
    }

    #[tokio::test]
    async fn session_history_refresh_sends_bootstrapped_catalog() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let run_dir = temp_dir.path().join("run_projected_new_session");
        write_catalog_run(&run_dir, &catalog_events("run_projected_new_session"));
        let (tx, rx) = std_mpsc::channel::<LiveUpdate>();

        await_task(
            "session history refresh",
            spawn_session_history_refresh(temp_dir.path().to_path_buf(), tx),
        )
        .await
        .expect("refresh session history");

        let update = rx.try_recv().expect("session history update");
        let LiveUpdate::SessionHistory(entries) = update else {
            panic!("expected session history update");
        };
        assert!(entries
            .iter()
            .any(|entry| entry.catalog.run_id == "run_projected_new_session"));
    }

    #[test]
    fn resumed_live_tui_options_carry_normalized_lineage_history() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root_dir = temp_dir.path().join("root_session");
        let child_dir = temp_dir.path().join("child_session");
        write_catalog_run(&root_dir, &catalog_events("root_session"));
        write_catalog_run(&child_dir, &catalog_events("child_session"));
        std::fs::write(
            child_dir.join("meta.json"),
            r#"{"harness_lineage":{"harness_source_run_id":"root_session"}}"#,
        )
        .expect("write child lineage metadata");

        let entries = load_live_session_history_entries(&child_dir, temp_dir.path())
            .expect("load live lineage entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.catalog.run_id == "child_session")
                .and_then(|entry| entry.catalog.parent_session_id.as_deref()),
            Some("root_session")
        );

        let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
        let sink: UiIntentSink = Arc::new(|_| {});
        let options = continue_live_tui_options(
            child_dir,
            Vec::new(),
            entries,
            rx,
            false,
            sink,
            true,
            None,
            None,
        );

        let TuiMode::Live {
            session_history_entries,
            ..
        } = options.mode
        else {
            panic!("expected live TUI mode");
        };
        assert_eq!(session_history_entries.len(), 2);
        assert!(session_history_entries.iter().any(|entry| {
            entry.catalog.run_id == "child_session"
                && entry.catalog.parent_session_id.as_deref() == Some("root_session")
        }));
    }

    #[tokio::test]
    async fn compact_intent_reports_noop_status_for_idle_live_agent() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
        config.deterministic_store = true;
        config.agent_profiles = golden_path_profiles();

        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator
            .start_run("compact_status", temp_dir.path())
            .await
            .expect("start run");
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "planner", None)
            .await
            .expect("spawn agent");

        let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
            agent_id: Some(agent_id),
            profile: "planner".to_string(),
            last_request_id: None,
        }));
        let (intent_tx, intent_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = std_mpsc::channel();

        let handle = tokio::spawn(handle_ui_intents(
            coordinator.clone(),
            intent_rx,
            user_actor(),
            Some(live_agent_target),
            status_tx,
            TuiAuthBackendContext {
                config_path: None,
                session_dir: Some(temp_dir.path().to_path_buf()),
                workspace_root: temp_dir.path().to_path_buf(),
            },
        ));

        intent_tx
            .send(UiIntent::CompactSession)
            .expect("send compact intent");
        drop(intent_tx);

        handle
            .await
            .expect("ui intent task join")
            .expect("ui intent task ok");
        let status = status_rx.recv().expect("status update");
        assert!(matches!(
            status,
            LiveUpdate::OperatorNotice {
                message,
                level: OperatorNoticeLevel::Info,
            } if message == "manual compaction skipped: need at least two completed turns"
        ));

        coordinator.stop_run().await.expect("stop run");
    }

    #[test]
    fn manual_compaction_success_message_reports_active_context_delta() {
        assert_eq!(
            manual_compaction_success_message("checkpoint_000123", Some(18_200), Some(4_100)),
            "manual compaction checkpoint written: checkpoint_000123 · active ctx 18.2K → 4.1K est"
        );
        assert_eq!(
            manual_compaction_success_message("checkpoint_000124", Some(4_100), Some(4_100)),
            "manual compaction checkpoint written: checkpoint_000124 · active ctx estimate unchanged"
        );
    }

    #[test]
    fn tui_auth_backend_runs_same_auth_command_and_redacts_output() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");
        let config_path = temp.path().join("harness.jsonc");
        std::fs::write(
            &config_path,
            r#"
            {
              provider: {
                codex_route: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  authProvider: "codex",
                  models: {
                    "gpt-5.4-mini": { name: "GPT-5.4 mini" },
                  },
                },
              },
              model: "codex_route/gpt-5.4-mini",
              permission: "ask",
            }
            "#,
        )
        .expect("write config");
        let deps = harness::CliDeps::real()
            .with_current_dir(temp.path().to_path_buf())
            .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());

        let secret = "tui-auth-backend-secret-value";
        let (message, level) = run_tui_auth_backend_once_with_deps(
            vec![
                "login".to_string(),
                "codex".to_string(),
                "--mock-token".to_string(),
                secret.to_string(),
            ],
            Some(config_path.clone()),
            Some(temp.path().join("sessions")),
            &deps,
        );

        assert_eq!(level, OperatorNoticeLevel::Info);
        assert!(message.contains("auth backend completed: harness auth login codex"));
        assert!(!message.contains(secret), "TUI notice leaked auth secret");
        assert!(
            data_home.join("harness/credentials/codex.json").is_file(),
            "TUI auth route must write through the same credential backend as CLI auth"
        );

        let (list_message, list_level) = run_tui_auth_backend_once_with_deps(
            vec!["list".to_string()],
            Some(config_path),
            Some(temp.path().join("sessions")),
            &deps,
        );
        assert_eq!(list_level, OperatorNoticeLevel::Info);
        assert!(list_message.contains("presence=stored"));
        assert!(
            !list_message.contains(secret),
            "TUI auth list leaked auth secret"
        );
    }

    #[test]
    fn tui_auth_backend_streams_output_and_accepts_hidden_stdin() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");
        let config_path = temp.path().join("harness.jsonc");
        std::fs::write(
            &config_path,
            r#"
            {
              provider: {
                codex_route: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  authProvider: "codex",
                  models: {
                    "gpt-5.4-mini": { name: "GPT-5.4 mini" },
                  },
                },
              },
              model: "codex_route/gpt-5.4-mini",
              permission: "ask",
            }
            "#,
        )
        .expect("write config");
        let deps = harness::CliDeps::real()
            .with_current_dir(temp.path().to_path_buf())
            .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
        let secret = "sk-tui-streamed-stdin-secret";
        let (tx, rx) = std_mpsc::channel();

        let (message, level, success) = run_tui_auth_backend_streaming_with_deps(
            vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string(),
            ],
            Some(config_path),
            Some(temp.path().join("sessions")),
            secret,
            &deps,
            Some(tx),
        );

        assert!(success);
        assert_eq!(level, OperatorNoticeLevel::Info);
        assert!(
            !message.contains(secret),
            "final notice leaked stdin secret"
        );
        let notices = rx.try_iter().collect::<Vec<_>>();
        assert!(
            notices.iter().any(|update| matches!(
                update,
                LiveUpdate::OperatorNotice { message, level: OperatorNoticeLevel::Info }
                    if message.contains("stored api_key credential for codex")
            )),
            "expected streamed auth output before the final completion notice"
        );
        assert!(
            notices.iter().all(|update| match update {
                LiveUpdate::OperatorNotice { message, .. } => !message.contains(secret),
                _ => true,
            }),
            "streamed auth notice leaked stdin secret"
        );
        assert!(
            data_home.join("harness/credentials/codex.json").is_file(),
            "streamed TUI auth should store the API key through the CLI backend"
        );
    }

    #[test]
    fn tui_lineage_clone_materializes_child_from_memory_snapshot() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
        std::fs::create_dir(&source_run_dir).expect("create source run dir");
        std::fs::write(source_run_dir.join(".writer.lock"), "locked").expect("write source lock");
        let events = stable_lineage_test_events();
        let stable_prefix = harness_core::session_lineage::latest_clone_stable_prefix(&events)
            .expect("stable clone prefix");

        let notice =
            materialize_tui_lineage_child("clone", source_run_dir.clone(), events, stable_prefix);

        let LiveUpdate::OperatorNotice { message, level } = notice else {
            panic!("expected lineage operator notice");
        };
        assert_eq!(level, OperatorNoticeLevel::Info);
        assert!(
            message.starts_with("Harness session clone created run_harness_child"),
            "unexpected success message: {message}"
        );
        assert!(
            temp_dir
                .path()
                .read_dir()
                .expect("read session dir")
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("run_harness_child")),
            "expected published child run beside source"
        );
    }

    #[test]
    fn tui_lineage_fork_continues_child_with_prompt_draft() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
        std::fs::create_dir(&source_run_dir).expect("create source run dir");
        std::fs::write(source_run_dir.join(".writer.lock"), "locked").expect("write source lock");
        let events = active_stable_lineage_test_events();
        let stable_prefix = harness_core::session_lineage::validate_tui_fork_stable_prefix(
            &events,
            events.len() as u64,
        )
        .expect("stable fork prefix");

        let update = materialize_tui_fork_child(
            source_run_dir,
            events,
            stable_prefix,
            "repeat this prompt".to_string(),
        );

        let LiveUpdate::ContinueSession {
            run_id,
            run_dir,
            prompt_draft,
        } = update
        else {
            panic!("expected fork continuation update");
        };
        assert_eq!(prompt_draft, "repeat this prompt");
        assert_eq!(run_id, run_dir.file_name().unwrap().to_string_lossy());
        let child_events = load_events_from_run_dir(&run_dir).expect("load child events");
        assert!(matches!(
            child_events.last().map(|event| &event.payload),
            Some(EventV1::RunFinished(_))
        ));
        let resume_plan = inspect_resume_plan(&run_dir);
        assert!(
            resume_plan.is_resumable,
            "child should be resumable: {:?}",
            resume_plan.resume_disabled_reason
        );
    }

    #[test]
    fn tui_lineage_fork_first_prompt_uses_recorded_runtime_context_for_resume() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
        std::fs::create_dir(&source_run_dir).expect("create source run dir");
        std::fs::write(source_run_dir.join(".writer.lock"), "locked").expect("write source lock");
        write_recorded_runtime_context_meta(&source_run_dir);
        let events = first_prompt_lineage_test_events();
        let stable_prefix = harness_core::session_lineage::validate_tui_fork_stable_prefix(
            &events,
            events.len() as u64,
        )
        .expect("stable first prompt fork prefix");

        let update = materialize_tui_fork_child(
            source_run_dir,
            events,
            stable_prefix,
            "first prompt".to_string(),
        );

        let LiveUpdate::ContinueSession { run_dir, .. } = update else {
            panic!("expected fork continuation update");
        };
        let resume_plan = inspect_resume_plan(&run_dir);
        assert_eq!(
            resume_plan.provider_model.as_deref(),
            Some("default/gpt-5.5")
        );
        assert!(
            resume_plan.is_resumable,
            "child should be resumable from copied metadata: {:?}",
            resume_plan.resume_disabled_reason
        );
    }

    #[tokio::test]
    async fn event_forwarder_stops_after_terminal_event_when_requested() {
        // arrange
        let store = Arc::new(InMemoryEventStore::new());
        store
            .append(forwarder_event_draft(
                "run_forwarder_terminal",
                "started",
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "forwarder terminal".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ))
            .expect("append started event");
        store
            .append(forwarder_event_draft(
                "run_forwarder_terminal",
                "finished",
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ))
            .expect("append finished event");
        let (tx, rx) = std_mpsc::channel();

        // act
        tokio::time::timeout(
            Duration::from_millis(500),
            forward_events_to_tui(store, tx, 1, None, true),
        )
        .await
        .expect("forwarder should stop after forwarding terminal event")
        .expect("forwarder succeeds");

        // assert
        let updates = rx.try_iter().collect::<Vec<_>>();
        assert_eq!(updates.len(), 2);
        assert!(matches!(updates[0], LiveUpdate::Event(_)));
        assert!(
            matches!(updates[1], LiveUpdate::Event(ref event) if is_terminal_event(&event.payload))
        );
    }

    #[tokio::test]
    async fn compact_intent_reports_unavailable_when_no_live_agent_target_exists() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
        config.deterministic_store = true;
        config.agent_profiles = golden_path_profiles();

        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator
            .start_run("compact_status", temp_dir.path())
            .await
            .expect("start run");

        let (intent_tx, intent_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = std_mpsc::channel();

        let handle = tokio::spawn(handle_ui_intents(
            coordinator.clone(),
            intent_rx,
            user_actor(),
            None,
            status_tx,
            TuiAuthBackendContext {
                config_path: None,
                session_dir: Some(temp_dir.path().to_path_buf()),
                workspace_root: temp_dir.path().to_path_buf(),
            },
        ));

        intent_tx
            .send(UiIntent::CompactSession)
            .expect("send compact intent");
        drop(intent_tx);

        handle
            .await
            .expect("ui intent task join")
            .expect("ui intent task ok");
        let status = status_rx.recv().expect("status update");
        assert!(matches!(
            status,
            LiveUpdate::OperatorNotice {
                message,
                level: OperatorNoticeLevel::Error,
            } if message == "manual compaction unavailable: no live agent target"
        ));

        coordinator.stop_run().await.expect("stop run");
    }

    #[test]
    fn live_ui_router_forwards_compact_intent_without_switching_workflow() {
        let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
        let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
        let (selected_workflow, sink) =
            build_live_ui_intent_router(intent_tx, Arc::clone(&launch_selection), false);

        sink(UiIntent::CompactSession);

        assert!(recover_mutex_lock(&selected_workflow).is_none());
        assert_eq!(intent_rx.try_recv().ok(), Some(UiIntent::CompactSession));
    }

    #[test]
    fn live_ui_router_forwards_interrupt_intent_without_switching_workflow() {
        let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
        let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
        let (selected_workflow, sink) =
            build_live_ui_intent_router(intent_tx, Arc::clone(&launch_selection), false);

        sink(UiIntent::InterruptSession {
            task_ids: vec!["task_active".to_string()],
        });

        assert!(recover_mutex_lock(&selected_workflow).is_none());
        assert_eq!(
            intent_rx.try_recv().ok(),
            Some(UiIntent::InterruptSession {
                task_ids: vec!["task_active".to_string()],
            })
        );
    }

    #[test]
    fn live_ui_router_records_model_switch_without_switching_workflow() {
        let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
        let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
        let (selected_workflow, sink) =
            build_live_ui_intent_router(intent_tx, Arc::clone(&launch_selection), false);
        let launch_metadata =
            LaunchMetadata::from_model_ref("ops", "anthropic:claude-3.7").with_mode_label("Live");

        sink(UiIntent::SwitchModel {
            profile: "ops".to_string(),
            launch_metadata: launch_metadata.clone(),
        });

        assert!(recover_mutex_lock(&selected_workflow).is_none());
        assert_eq!(
            intent_rx.try_recv().ok(),
            Some(UiIntent::SwitchModel {
                profile: "ops".to_string(),
                launch_metadata,
            })
        );
        let recorded = recover_mutex_lock(&launch_selection).clone();
        assert_eq!(recorded.profile(), "ops");
        assert_eq!(recorded.provider(), "anthropic");
        assert_eq!(recorded.model(), Some("claude-3.7"));
        assert_eq!(recorded.mode_label(), None);
    }

    #[test]
    fn no_config_tui_without_credentials_enters_connect_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");

        let settings = with_harness_data_home(&data_home, || {
            resolve_live_settings(
                &live_tui_command(),
                None,
                None,
                temp.path().to_path_buf(),
                &harness_core::config::ConfigLoadContext::from_env()
                    .with_current_dir(temp.path().to_path_buf()),
            )
        })
        .expect("no-config live settings should resolve");

        assert!(settings.config.is_some());
        assert_eq!(settings.launch_metadata.provider(), "local");
        assert_eq!(settings.launch_metadata.model(), None);
        assert!(settings.launch_metadata.available_models().is_empty());
    }

    #[test]
    fn no_config_tui_with_stored_codex_launches_connected_catalog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");
        CredentialStore::new(data_home.join("harness"))
            .save(&StoredCredential::api_key(
                AuthProviderId::Codex,
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .expect("save credential");

        let settings = with_harness_data_home(&data_home, || {
            resolve_live_settings(
                &live_tui_command(),
                None,
                None,
                temp.path().to_path_buf(),
                &harness_core::config::ConfigLoadContext::from_env()
                    .with_current_dir(temp.path().to_path_buf()),
            )
        })
        .expect("stored Codex credential should resolve live settings");

        assert_eq!(settings.launch_metadata.provider(), "openai-codex");
        assert!(settings.launch_metadata.model().is_some());
        assert!(settings
            .launch_metadata
            .available_models()
            .iter()
            .all(|option| option.provider == "openai-codex"));
    }

    #[test]
    fn auth_refresh_reloads_no_config_builtin_catalog_after_login() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");
        CredentialStore::new(data_home.join("harness"))
            .save(&StoredCredential::api_key(
                AuthProviderId::GithubCopilot,
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .expect("save credential");

        let launch_metadata = with_harness_data_home(&data_home, || {
            refreshed_launch_metadata_after_auth(
                Some("login"),
                None,
                None,
                temp.path().to_path_buf(),
            )
        })
        .expect("refresh should resolve")
        .expect("launch metadata should refresh after login");

        assert_eq!(launch_metadata.provider(), "github-copilot");
        assert!(launch_metadata.model().is_some());
        assert!(launch_metadata
            .available_models()
            .iter()
            .all(|option| option.provider == "github-copilot"));
    }

    #[test]
    fn no_config_tui_restores_recent_builtin_model_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_home = temp.path().join("data");
        let state_path = temp.path().join("model.json");
        CredentialStore::new(data_home.join("harness"))
            .save(&StoredCredential::api_key(
                AuthProviderId::Codex,
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .expect("save credential");
        std::fs::write(
            &state_path,
            r#"{"schema_version":1,"profile":"build","provider":"openai-codex","model":"gpt-5.5"}"#,
        )
        .expect("write model state");

        let settings = with_harness_data_home(&data_home, || {
            let previous = std::env::var_os("HARNESS_MODEL_SELECTION_STATE_FILE");
            std::env::set_var("HARNESS_MODEL_SELECTION_STATE_FILE", &state_path);
            let result = resolve_live_settings(
                &live_tui_command(),
                None,
                None,
                temp.path().to_path_buf(),
                &harness_core::config::ConfigLoadContext::from_env()
                    .with_current_dir(temp.path().to_path_buf()),
            );
            match previous {
                Some(value) => std::env::set_var("HARNESS_MODEL_SELECTION_STATE_FILE", value),
                None => std::env::remove_var("HARNESS_MODEL_SELECTION_STATE_FILE"),
            }
            result
        })
        .expect("stored Codex credential should resolve live settings");

        assert_eq!(settings.launch_metadata.provider(), "openai-codex");
        assert_eq!(settings.launch_metadata.model(), Some("gpt-5.5"));
    }

    #[test]
    fn project_config_tui_restores_recent_model_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("harness.jsonc");
        let state_path = temp.path().join("model.json");
        std::fs::write(
            &config_path,
            r#"{
              provider: {
                "openai-codex": {
                  type: "openai_compatible",
                  options: {
                    authProvider: "codex",
                    baseURL: "https://api.openai.com/v1",
                    apiKeyEnv: ["OPENAI_API_KEY"],
                  },
                  models: {
                    "gpt-5.4-mini": { name: "GPT 5.4 Mini" },
                    "gpt-5.5": { name: "GPT 5.5" },
                  },
                },
              },
              model: "openai-codex/gpt-5.4-mini",
              agent: {
                build: { enable: true, model: "openai-codex/gpt-5.4-mini" },
                plan: { enable: true, model: "openai-codex/gpt-5.4-mini" },
              },
              default_agent: "build",
              permission: "ask",
            }"#,
        )
        .expect("write project config");
        std::fs::write(
            &state_path,
            r#"{"schema_version":1,"profile":"build","provider":"openai-codex","model":"gpt-5.5"}"#,
        )
        .expect("write model state");

        let previous_state = std::env::var_os("HARNESS_MODEL_SELECTION_STATE_FILE");
        let previous_key = std::env::var_os("OPENAI_API_KEY");
        std::env::set_var("HARNESS_MODEL_SELECTION_STATE_FILE", &state_path);
        std::env::set_var("OPENAI_API_KEY", "test-token");
        let result = resolve_live_settings(
            &live_tui_command(),
            Some(config_path),
            None,
            temp.path().to_path_buf(),
            &harness_core::config::ConfigLoadContext::from_env()
                .with_current_dir(temp.path().to_path_buf()),
        );
        match previous_state {
            Some(value) => std::env::set_var("HARNESS_MODEL_SELECTION_STATE_FILE", value),
            None => std::env::remove_var("HARNESS_MODEL_SELECTION_STATE_FILE"),
        }
        match previous_key {
            Some(value) => std::env::set_var("OPENAI_API_KEY", value),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }

        let settings = result.expect("project config live settings should resolve");
        assert_eq!(settings.launch_metadata.provider(), "openai-codex");
        assert_eq!(settings.launch_metadata.model(), Some("gpt-5.5"));
    }

    #[test]
    fn mock_mode_ignores_discovered_cwd_config() {
        let _guard = mock_mode_cwd_test_lock()
            .lock()
            .expect("mock mode cwd lock poisoned");
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("harness.jsonc"),
            r#"{
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {
                    "gpt-5.4-mini": {
                      display_name: "GPT-5.4 Mini"
                    }
                  }
                }
              },
              agents: {
                build: {
                  description: "Implementation",
                  system_prompt: "Implement carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  tools: []
                }
              },
              default_agent: "build",
              permissions: {
                defaults: {
                  edit: "allow",
                  shell: "allow",
                  network: "allow"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions"
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }"#,
        )
        .expect("write discovered cwd config");

        let result = resolve_live_settings(
            &TuiCommand {
                replay: None,
                continue_session: None,
                scenario: None,
                mock: true,
                deterministic: false,
                session_dir: None,
                exit_on_finish: false,
                profile: None,
            },
            None,
            None,
            temp.path().to_path_buf(),
            &harness_core::config::ConfigLoadContext::from_env()
                .with_current_dir(temp.path().to_path_buf()),
        );

        let settings = result.expect("mock mode settings should resolve");
        assert!(settings.config.is_none());
        assert_eq!(settings.launch_mode_label.as_deref(), Some("Demo"));
        assert_eq!(settings.launch_metadata.profile(), "worker");
        assert_eq!(settings.launch_metadata.provider(), "mock");
        assert_eq!(settings.launch_metadata.model(), Some("model-1"));
    }

    #[test]
    fn live_new_session_uses_current_workspace_instead_of_seeded_demo_workspace() {
        let _guard = mock_mode_cwd_test_lock()
            .lock()
            .expect("mock mode cwd lock poisoned");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("harness.jsonc");
        std::fs::write(
            &config_path,
            r#"{
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {
                    "gpt-5.4-mini": {
                      display_name: "GPT-5.4 Mini"
                    }
                  }
                }
              },
              agents: {
                build: {
                  description: "Implementation",
                  system_prompt: "Implement carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  tools: []
                }
              },
              default_agent: "build",
              permissions: {
                defaults: {
                  edit: "allow",
                  shell: "allow",
                  network: "allow"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions"
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }"#,
        )
        .expect("write live config");

        let result = resolve_live_settings(
            &TuiCommand {
                replay: None,
                continue_session: None,
                scenario: None,
                mock: false,
                deterministic: false,
                session_dir: None,
                exit_on_finish: false,
                profile: None,
            },
            Some(config_path.clone()),
            None,
            temp.path().to_path_buf(),
            &harness_core::config::ConfigLoadContext::from_env()
                .with_current_dir(temp.path().to_path_buf()),
        );

        let settings = result.expect("live mode settings should resolve");
        let workspace = prepare_new_live_workspace(&settings, false, "run_test")
            .expect("live workspace should resolve");

        assert_eq!(settings.launch_mode_label, None);
        assert_eq!(workspace, temp.path());
        assert!(!workspace.join("demo.txt").exists());
        assert!(!settings
            .session_dir
            .join("workspaces")
            .join("golden_path_interactive-run_test")
            .exists());
    }

    #[test]
    fn continue_selects_most_recent_conversational_agent_not_first_key() {
        let mut known_agents = BTreeMap::new();
        known_agents.insert("agent_000001".to_string(), "alpha".to_string());
        known_agents.insert("agent_000002".to_string(), "beta".to_string());

        let historical_events = vec![
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0001".to_string(),
                seq: 1,
                run_id: "run_fixture".to_string(),
                mono_ms: 1,
                ts: None,
                actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                correlation_id: None,
                causation_id: None,
                stream_key: Some("run:run_fixture".to_string()),
                payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0002".to_string(),
                seq: 2,
                run_id: "run_fixture".to_string(),
                mono_ms: 2,
                ts: None,
                actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                correlation_id: None,
                causation_id: None,
                stream_key: Some("run:run_fixture".to_string()),
                payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "beta".to_string(),
                    parent_agent_id: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0003".to_string(),
                seq: 3,
                run_id: "run_fixture".to_string(),
                mono_ms: 3,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                correlation_id: Some("req_000010".to_string()),
                causation_id: None,
                stream_key: Some("agent:agent_000001".to_string()),
                payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000010".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-a".to_string(),
                    prompt_summary: "first".to_string(),
                    request_digest: "digest-a".to_string(),
                    metadata: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0004".to_string(),
                seq: 4,
                run_id: "run_fixture".to_string(),
                mono_ms: 4,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
                correlation_id: Some("req_000011".to_string()),
                causation_id: None,
                stream_key: Some("agent:agent_000002".to_string()),
                payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000011".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-b".to_string(),
                    prompt_summary: "second".to_string(),
                    request_digest: "digest-b".to_string(),
                    metadata: None,
                }),
            },
        ];

        let selected = most_recent_conversational_agent_id(&historical_events, &known_agents);
        assert_eq!(selected.as_deref(), Some("agent_000002"));
    }

    #[test]
    fn continue_metadata_uses_selected_agent_history_in_multi_agent_session() {
        let historical_events = vec![
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0001".to_string(),
                seq: 1,
                run_id: "run_fixture".to_string(),
                mono_ms: 1,
                ts: None,
                actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                correlation_id: None,
                causation_id: None,
                stream_key: Some("run:run_fixture".to_string()),
                payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0002".to_string(),
                seq: 2,
                run_id: "run_fixture".to_string(),
                mono_ms: 2,
                ts: None,
                actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                correlation_id: None,
                causation_id: None,
                stream_key: Some("run:run_fixture".to_string()),
                payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "beta".to_string(),
                    parent_agent_id: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0003".to_string(),
                seq: 3,
                run_id: "run_fixture".to_string(),
                mono_ms: 3,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                correlation_id: Some("req_000010".to_string()),
                causation_id: None,
                stream_key: Some("agent:agent_000001".to_string()),
                payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000010".to_string(),
                    provider_id: "provider-alpha".to_string(),
                    model_id: "model-alpha".to_string(),
                    prompt_summary: "alpha turn".to_string(),
                    request_digest: "digest-alpha".to_string(),
                    metadata: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0004".to_string(),
                seq: 4,
                run_id: "run_fixture".to_string(),
                mono_ms: 4,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
                correlation_id: Some("req_000011".to_string()),
                causation_id: None,
                stream_key: Some("agent:agent_000002".to_string()),
                payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000011".to_string(),
                    provider_id: "provider-beta".to_string(),
                    model_id: "model-beta".to_string(),
                    prompt_summary: "beta turn".to_string(),
                    request_digest: "digest-beta".to_string(),
                    metadata: None,
                }),
            },
        ];

        let metadata = continue_launch_metadata(
            "run_fixture",
            None,
            &historical_events,
            "agent_000001",
            Some("alpha"),
        );

        assert_eq!(metadata.profile(), "alpha");
        assert_eq!(metadata.provider(), "provider-alpha");
        assert_eq!(metadata.model(), Some("model-alpha"));
        assert_eq!(metadata.mode_label(), Some("Continued"));
    }

    #[test]
    fn interactive_launch_metadata_exposes_catalog_and_cross_profile_switch_options() {
        let config = load_config_from_str(
            r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {
                    "gpt-5.4-mini": {
                      display_name: "GPT-5.4 Mini",
                      variants: {
                        low: {
                          display_name: "Low"
                        },
                        medium: {
                          display_name: "Medium"
                        },
                        high: {
                          display_name: "High"
                        },
                        xhigh: {
                          display_name: "XHigh"
                        }
                      }
                    },
                    "gpt-5.4": {
                      display_name: "GPT-5.4"
                    }
                  }
                }
              },
              agents: {
                build: {
                  description: "Implementation",
                  system_prompt: "Implement carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  tools: []
                },
                plan: {
                  description: "Planning",
                  system_prompt: "Plan carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  variant: "low",
                  tools: []
                },
                ops: {
                  description: "Operations",
                  system_prompt: "Operate carefully.",
                  model_ref: "default:gpt-5.4",
                  tools: []
                }
              },
              default_agent: "build",
              permissions: {
                defaults: {
                  edit: "allow",
                  shell: "allow",
                  network: "allow"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions"
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }
            "#,
        )
        .expect("config should parse");

        let agent_profiles = bootstrap::interactive_agent_profiles(&config)
            .expect("interactive agent profiles should build");
        let metadata = interactive_launch_metadata(Some(&config), &agent_profiles, "build")
            .expect("launch metadata should build");

        assert!(metadata
            .available_models()
            .iter()
            .any(|option| option.profile == "build"));
        assert!(metadata
            .available_models()
            .iter()
            .any(|option| option.profile == "ops" && option.model == "gpt-5.4"));
        assert!(metadata
            .available_models()
            .iter()
            .any(|option| option.profile == "build" && option.model == "gpt-5.4"));
        let mut mini_variants = metadata
            .available_models()
            .iter()
            .filter(|option| option.profile == "build" && option.model == "gpt-5.4-mini")
            .filter_map(|option| option.variant.as_deref())
            .collect::<Vec<_>>();
        mini_variants.sort_unstable();
        assert_eq!(mini_variants, vec!["high", "low", "medium", "xhigh"]);
    }

    #[test]
    fn shipped_example_config_preserves_configured_model_variant() {
        let config_path = crate::cli_config::shipped_example_config_path();
        let config = harness_core::config::load_config_from_file(&config_path)
            .expect("shipped example config should parse with discovered prompts");

        let agent_profiles = bootstrap::interactive_agent_profiles(&config)
            .expect("interactive agent profiles should build");
        let metadata = interactive_launch_metadata(Some(&config), &agent_profiles, "build")
            .expect("launch metadata should build");

        assert_eq!(metadata.profile(), "build");
        assert_eq!(metadata.variant(), Some("high"));
    }

    #[test]
    fn persisted_model_selection_restores_valid_variant_for_active_profile() {
        let base = LaunchMetadata::from_model_option(&ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("Default".to_string()),
            provider_backend_label: None,
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT-5.4 Mini".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        })
        .with_available_models(vec![
            ModelOption {
                profile: "build".to_string(),
                provider: "default".to_string(),
                provider_display_label: Some("Default".to_string()),
                provider_backend_label: None,
                model: "gpt-5.4-mini".to_string(),
                model_display_label: Some("GPT-5.4 Mini".to_string()),
                variant: None,
                variant_display_label: None,
                display_label: Some("GPT-5.4 Mini".to_string()),
                token_window_label: None,
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
                description: None,
                profile_description: None,
                reasoning_effort: None,
                text_verbosity: None,
                recommended_for: None,
            },
            ModelOption {
                profile: "build".to_string(),
                provider: "default".to_string(),
                provider_display_label: Some("Default".to_string()),
                provider_backend_label: None,
                model: "gpt-5.4-mini".to_string(),
                model_display_label: Some("GPT-5.4 Mini".to_string()),
                variant: Some("high".to_string()),
                variant_display_label: Some("High".to_string()),
                display_label: Some("GPT-5.4 Mini High".to_string()),
                token_window_label: None,
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
                description: None,
                profile_description: None,
                reasoning_effort: Some("high".to_string()),
                text_verbosity: None,
                recommended_for: None,
            },
        ]);

        let restored = apply_model_selection_to_launch_metadata(
            base,
            &PersistedModelSelection {
                schema_version: 1,
                profile: "build".to_string(),
                provider: "default".to_string(),
                model: "gpt-5.4-mini".to_string(),
                variant: Some("high".to_string()),
            },
        );

        assert_eq!(restored.profile(), "build");
        assert_eq!(restored.provider(), "default");
        assert_eq!(restored.model(), Some("gpt-5.4-mini"));
        assert_eq!(restored.variant(), Some("high"));
        assert_eq!(restored.reasoning_effort(), Some("high"));
    }

    #[test]
    fn persisted_model_selection_preserves_switchable_profiles() {
        let base = LaunchMetadata::from_model_ref("ops", "default:gpt-5.4")
            .with_available_models(vec![ModelOption::from_model_ref("ops", "default:gpt-5.4")])
            .with_switchable_profiles(vec![
                "ops".to_string(),
                "build".to_string(),
                "plan".to_string(),
            ]);

        let restored = apply_model_selection_to_launch_metadata(
            base,
            &PersistedModelSelection {
                schema_version: 1,
                profile: "ops".to_string(),
                provider: "default".to_string(),
                model: "gpt-5.4".to_string(),
                variant: None,
            },
        );

        assert_eq!(restored.profile(), "ops");
        assert_eq!(restored.switchable_profiles(), ["ops", "build", "plan"]);
    }

    #[test]
    fn persisted_model_selection_ignores_unconfigured_variant() {
        let base =
            LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
                vec![ModelOption::from_model_ref("build", "default:gpt-5.4-mini")],
            );
        let restored = apply_model_selection_to_launch_metadata(
            base.clone(),
            &PersistedModelSelection {
                schema_version: 1,
                profile: "build".to_string(),
                provider: "default".to_string(),
                model: "gpt-5.4-mini".to_string(),
                variant: Some("stale".to_string()),
            },
        );

        assert_eq!(restored, base);
    }

    #[test]
    fn persisted_model_selection_round_trips_model_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("model.json");
        let metadata = LaunchMetadata::from_model_option(&ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: None,
            provider_backend_label: None,
            model: "gpt-5.4-mini".to_string(),
            model_display_label: None,
            variant: Some("xhigh".to_string()),
            variant_display_label: None,
            display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        });

        save_persisted_model_selection_to_path(&path, &metadata).expect("persist model selection");
        let selection =
            load_persisted_model_selection_from_path(&path).expect("load model selection");

        assert_eq!(selection.schema_version, 1);
        assert_eq!(selection.profile, "build");
        assert_eq!(selection.provider, "default");
        assert_eq!(selection.model, "gpt-5.4-mini");
        assert_eq!(selection.variant.as_deref(), Some("xhigh"));
    }

    #[test]
    fn runtime_toggles_report_compact_skill_catalog_states() {
        // arrange
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join(".git")).expect("git dir");
        for (name, body) in [
            ("ready-skill", "READY SKILL BODY SENTINEL"),
            ("disabled-skill", "DISABLED SKILL BODY SENTINEL"),
        ] {
            let skill_dir = workspace.path().join(".agent-harness/skills").join(name);
            fs::create_dir_all(&skill_dir).expect("skill dir");
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name} description\n---\n\n{body}\n"),
            )
            .expect("skill file");
        }
        let config = load_config_from_str(
            r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: { "gpt-5.4-mini": { display_name: "GPT-5.4 Mini" } }
                }
              },
              agents: {
                build: {
                  description: "Implementation",
                  system_prompt: "Implement carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  tools: ["skill"]
                }
              },
              default_agent: "build",
              permissions: { defaults: { edit: "allow", shell: "allow", network: "allow" } },
              runtime: { session_dir: ".agent-harness/sessions" },
              integrations: { remote_search: { endpoint: "https://mcp.exa.ai/mcp" } },
              skills: {
                project_roots: [".agent-harness/skills"],
                global_roots: [],
                disabled: ["disabled-skill"]
              }
            }
            "#,
        )
        .expect("config should parse");

        let toggles = runtime_toggles_config(Some(&config), workspace.path());
        let ready = toggles
            .entries
            .iter()
            // act
            .find(|entry| {
                // assert
                matches!(&entry.kind, ToggleEntryKind::AgentSkill { agent, skill }
                    if agent == "build" && skill == "skill:project:ready-skill")
            })
            .expect("ready skill toggle");
        assert_eq!(ready.label, "build: ready-skill");
        assert!(ready.description.contains("loadable skill `ready-skill`"));
        assert!(ready.description.contains("project root"));
        assert!(ready.enabled);

        let disabled = toggles
            .entries
            .iter()
            .find(|entry| {
                matches!(&entry.kind, ToggleEntryKind::AgentSkill { agent, skill }
                    if agent == "build" && skill == "skill:project:disabled-skill")
            })
            .expect("disabled skill toggle");
        assert_eq!(disabled.label, "build: disabled-skill");
        assert!(disabled
            .description
            .contains("disabled skill `disabled-skill`"));
        assert!(disabled.description.contains("disabled by skills.disabled"));
        assert!(!disabled.enabled);

        let rendered = format!("{toggles:?}");
        assert!(!rendered.contains("READY SKILL BODY SENTINEL"));
        assert!(!rendered.contains("DISABLED SKILL BODY SENTINEL"));
    }

    #[test]
    fn live_coordinator_config_warmup_reuses_interactive_config() {
        let config = load_config_from_str(
            r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {
                    "gpt-5.4-mini": {
                      display_name: "GPT-5.4 Mini"
                    }
                  }
                }
              },
              agents: {
                build: {
                  description: "Implementation",
                  system_prompt: "Implement carefully.",
                  model_ref: "default:gpt-5.4-mini",
                  tools: ["read"]
                }
              },
              default_agent: "build",
              permissions: {
                defaults: {
                  edit: "allow",
                  shell: "allow",
                  network: "allow"
                },
                shell_allowlist: {
                  executables: ["bash"],
                  cwd_roots: ["."]
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions"
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }
            "#,
        )
        .expect("config should parse");
        let session_dir = PathBuf::from("/tmp/warmed-session-dir");
        let agent_profiles = bootstrap::interactive_agent_profiles(&config)
            .expect("interactive agent profiles should build");
        let settings = LiveSettings {
            config: Some(config),
            config_path: None,
            session_dir: session_dir.clone(),
            workspace_root: PathBuf::from("/tmp/warmed-workspace"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: false,
            seed: 0,
            config_digest: "digest".to_string(),
            launch_metadata: interactive_launch_metadata(None, &agent_profiles, "build")
                .expect("launch metadata should build"),
            launch_mode_label: None,
            toggles: TogglesConfig::default(),
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        runtime.block_on(async {
            let warmup = LiveCoordinatorConfigWarmup::start(&settings, false);
            let first = warmup
                .coordinator_config(&settings, false)
                .await
                .expect("warmup should build interactive coordinator config");
            let second = warmup
                .coordinator_config(&settings, false)
                .await
                .expect("warmup should reuse cached coordinator config");

            assert_eq!(first.session_dir, session_dir);
            assert_eq!(second.session_dir, session_dir);
            assert!(first.agent_profiles.contains_key("build"));
            assert!(second.tool_registry.get("read").is_some());
        });
    }

    #[test]
    fn continue_launch_metadata_preserves_cross_profile_switch_options() {
        let continue_metadata = LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(vec![ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )])
            .with_mode_label("Continued");
        let continue_profile = continue_metadata.profile().to_string();

        assert_eq!(continue_profile, "build");
        assert!(continue_metadata
            .available_models()
            .iter()
            .any(|option| option.profile == "build"));
    }

    #[test]
    fn continue_metadata_prefers_recorded_runtime_context_before_event_inference() {
        let historical_events = vec![EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0001".to_string(),
            seq: 1,
            run_id: "run_fixture".to_string(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000001".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "heuristic-provider".to_string(),
                model_id: "heuristic-model".to_string(),
                prompt_summary: "turn".to_string(),
                request_digest: "digest".to_string(),
                metadata: None,
            }),
        }];
        let recorded_runtime_context = RecordedRuntimeContext {
            profile: "recorded-profile".to_string(),
            profile_description: Some("Recorded agent".to_string()),
            provider: "recorded-provider".to_string(),
            provider_display_label: Some("Recorded Provider".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "recorded-model".to_string(),
            variant: Some("recorded-variant".to_string()),
            display_label: "Recorded Model".to_string(),
            model_display_label: Some("Recorded Model".to_string()),
            variant_display_label: Some("Recorded Variant".to_string()),
            token_window_label: Some("128k ctx".to_string()),
            context_window_tokens: Some(128_000),
            max_input_tokens: Some(64_000),
            max_output_tokens: Some(8_000),
            description: Some("recorded description".to_string()),
            recommended_for: Some("deep work".to_string()),
            reasoning_effort: Some("high".to_string()),
            text_verbosity: Some("medium".to_string()),
        };

        let metadata = continue_launch_metadata(
            "run_fixture",
            Some(&recorded_runtime_context),
            &historical_events,
            "agent_000001",
            Some("heuristic-profile"),
        );

        assert_eq!(metadata.profile(), "recorded-profile");
        assert_eq!(metadata.provider(), "recorded-provider");
        assert_eq!(metadata.model(), Some("recorded-model"));
        assert_eq!(metadata.variant(), Some("recorded-variant"));
        assert_eq!(metadata.display_label(), Some("Recorded Model"));
        assert_eq!(metadata.mode_label(), Some("Continued"));
    }

    #[test]
    fn replay_launch_metadata_prefers_recorded_runtime_context_before_event_inference() {
        let historical_events = vec![EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0001".to_string(),
            seq: 1,
            run_id: "run_fixture".to_string(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000001".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "heuristic-provider".to_string(),
                model_id: "heuristic-model".to_string(),
                prompt_summary: "turn".to_string(),
                request_digest: "digest".to_string(),
                metadata: None,
            }),
        }];
        let recorded_runtime_context = RecordedRuntimeContext {
            profile: "recorded-profile".to_string(),
            profile_description: Some("Recorded agent".to_string()),
            provider: "recorded-provider".to_string(),
            provider_display_label: Some("Recorded Provider".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "recorded-model".to_string(),
            variant: None,
            display_label: "Recorded Replay Model".to_string(),
            model_display_label: Some("Recorded Replay Model".to_string()),
            variant_display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            recommended_for: None,
            reasoning_effort: None,
            text_verbosity: None,
        };

        let metadata = replay_launch_metadata(Some(&recorded_runtime_context), &historical_events);

        assert_eq!(metadata.profile(), "recorded-profile");
        assert_eq!(metadata.provider(), "recorded-provider");
        assert_eq!(metadata.model(), Some("recorded-model"));
        assert_eq!(metadata.display_label(), Some("Recorded Replay Model"));
        assert_eq!(metadata.mode_label(), Some("Replay"));
    }

    #[test]
    fn replay_bootstrap_falls_back_when_recorded_runtime_context_missing() {
        let historical_events = vec![
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0001".to_string(),
                seq: 1,
                run_id: "run_fixture".to_string(),
                mono_ms: 1,
                ts: None,
                actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                correlation_id: None,
                causation_id: None,
                stream_key: Some("run:run_fixture".to_string()),
                payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "legacy-profile".to_string(),
                    parent_agent_id: None,
                }),
            },
            EventEnvelopeV1 {
                schema_version: 1,
                event_id: "evt-0002".to_string(),
                seq: 2,
                run_id: "run_fixture".to_string(),
                mono_ms: 2,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                correlation_id: Some("req_000001".to_string()),
                causation_id: None,
                stream_key: Some("agent:agent_000001".to_string()),
                payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "legacy-provider".to_string(),
                    model_id: "legacy-model".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            },
        ];

        let metadata = replay_launch_metadata(None, &historical_events);

        assert_eq!(metadata.profile(), "legacy-profile");
        assert_eq!(metadata.provider(), "legacy-provider");
        assert_eq!(metadata.model(), Some("legacy-model"));
        assert_eq!(metadata.mode_label(), Some("Replay"));
    }
}
