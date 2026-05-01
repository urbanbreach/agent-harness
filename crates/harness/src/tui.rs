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
use std::time::{Duration, Instant};

use clap::Args;
use harness_core::agent::{AgentModelSettings, AgentProfile};
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{
    configured_model_catalog, load_resolved_config, resolve_profile_model_metadata, HarnessConfig,
    ShellAllowlist,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
    ManualCompactionOutcome,
};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionDecision;
use harness_core::proj::{
    inspect_resume_plan, RecordedRuntimeContext, ResumePlan, RunMetadata, SessionModeSource,
};
use harness_core::redact::DefaultRedactor;
use harness_core::store::{EventStore, EventStoreError};
use harness_tools::coordinator_registry;
use harness_tui::app::{
    set_pending_live_launch_metadata, set_pending_live_prompt_auto_submit, LaunchMetadata,
    ModelOption, SessionHistoryEntry,
};
use harness_tui::{
    close_preserved_terminal_session, load_events_from_run_dir, run_tui_with_options,
    set_pending_replay_launch_metadata, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
    UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::bootstrap;
use crate::logging;
use crate::replay::inspect_session_catalog;
use crate::scenarios::{
    create_workspace, default_permission_policy, golden_path_patch, golden_path_profiles,
    golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";
const DEFAULT_MOCK_PROFILE: &str = "worker";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const EVENT_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

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

#[derive(Debug)]
struct LiveSettings {
    config: Option<HarnessConfig>,
    session_dir: PathBuf,
    workspace_root: PathBuf,
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
    launch_metadata: LaunchMetadata,
    launch_mode_label: Option<String>,
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
    let mode = match resolve_tui_mode(&cmd, config_path, global_session_dir) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("tui setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    if let ResolvedTuiMode::Replay { run_dir } = &mode {
        return execute_replay_mode(run_dir, cmd.exit_on_finish);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to build async runtime: {err}");
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
            eprintln!("tui failed: {err}");
            ExitCode::from(1)
        }
    }
}

fn execute_replay_mode(run_dir: &Path, exit_on_finish: bool) -> ExitCode {
    let events = match load_events_from_run_dir(run_dir) {
        Ok(events) => events,
        Err(err) => {
            eprintln!("replay setup failed: {err}");
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
        preserve_terminal_on_exit: false,
    }) {
        eprintln!("TUI error: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn resolve_tui_mode(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<ResolvedTuiMode, String> {
    if let Some(run_dir) = &cmd.replay {
        return Ok(ResolvedTuiMode::Replay {
            run_dir: run_dir.clone(),
        });
    }

    let settings = resolve_live_settings(cmd, config_path, global_session_dir)?;

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
) -> Result<LiveSettings, String> {
    let workspace_root = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
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
        load_resolved_config(config_path.as_deref()).map_err(|err| err.to_string())?
    };

    if let Some(loaded) = loaded {
        let config = loaded.config;
        config_digest = config_digest_for_paths(&loaded.paths)?;
        config_default_profile = bootstrap::interactive_profile_name(&config);
        agent_profiles = bootstrap::interactive_agent_profiles(&config)?;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        live_config = Some(config);
    } else if cmd.scenario.is_none() && !cmd.mock {
        return Err(bootstrap::interactive_config_guidance());
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic
        || config_deterministic
        || matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));
    let default_profile = cmd.profile.clone().unwrap_or(config_default_profile);
    let launch_mode_label = if live_config.is_some() {
        None
    } else {
        Some("Demo".to_string())
    };
    let launch_metadata =
        interactive_launch_metadata(live_config.as_ref(), &agent_profiles, &default_profile)?;

    Ok(LiveSettings {
        config: live_config,
        session_dir,
        workspace_root,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
        launch_metadata,
        launch_mode_label,
    })
}

fn config_digest_for_paths(paths: &[PathBuf]) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        let config_bytes = fs::read(path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        hasher.update(&config_bytes);
        hasher.update(&[0xff]);
    }
    Ok(hasher.finalize().to_hex().to_string())
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

    Ok(launch_metadata.with_available_models(available_models))
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

    for profile in agent_profiles.keys() {
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
        .map(|profile| ModelOption::from_model_ref(profile.name.clone(), &profile.model_ref))
        .collect()
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
    *recover_mutex_lock(selection) = launch_metadata.clone().without_mode_label();
}

fn scenario_launch_metadata() -> LaunchMetadata {
    LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo")
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
    let coordinator_config_warmup = LiveCoordinatorConfigWarmup::start(settings, demo_mode);
    let _ = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("interactive_mode.warmup_ready");

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
        |run_dir| async move {
            run_replay_tui(run_dir, cmd.exit_on_finish).await?;
            Ok(InteractiveWorkflow::Startup)
        },
    )
    .await;

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
        |run_dir| async move {
            run_replay_tui(run_dir, cmd.exit_on_finish).await?;
            Ok(InteractiveWorkflow::Startup)
        },
    )
    .await;

    close_preserved_terminal_session().map_err(|err| err.to_string())?;
    result
}

fn load_startup_session_history_entries(
    session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    inspect_session_catalog(session_dir).map(|entries| {
        entries
            .into_iter()
            .filter(startup_session_history_entry_visible)
            .map(|entry| SessionHistoryEntry {
                run_dir: entry.run_dir,
                catalog: entry.catalog,
            })
            .collect()
    })
}

fn startup_session_history_entry_visible(entry: &crate::replay::SessionInspectionEntry) -> bool {
    !matches!(
        entry.catalog.mode_source,
        SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
    )
}

async fn run_startup_launcher(
    exit_on_finish: bool,
    session_history_entries: Vec<SessionHistoryEntry>,
    launch_selection: LaunchSelection,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("startup_launcher.begin");
    let selected_intent = Arc::new(Mutex::new(None::<UiIntent>));
    let selected_intent_sink = Arc::clone(&selected_intent);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if let UiIntent::SwitchModel {
            launch_metadata, ..
        } = &intent
        {
            record_launch_selection(&launch_selection, launch_metadata);
            return;
        }

        if !matches!(
            intent,
            UiIntent::NewSession
                | UiIntent::ReplaySession { .. }
                | UiIntent::ContinueSession { .. }
                | UiIntent::SubmitPrompt { .. }
                | UiIntent::CompactSession
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
            },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings: None,
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
        | Some(UiIntent::CompactSession)
        | Some(UiIntent::SwitchModel { .. }) => InteractiveWorkflow::Quit,
    }
}

async fn run_replay_tui(run_dir: PathBuf, exit_on_finish: bool) -> Result<(), String> {
    let events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    set_pending_replay_launch_metadata(Some(replay_launch_metadata_for_run(&run_dir, &events)));
    tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Replay { run_dir, events },
            exit_on_finish,
            on_ui_intent: None,
            keybindings: None,
            preserve_terminal_on_exit: false,
        })
    })
    .await
    .map_err(|err| format!("replay tui task failed: {err}"))?
    .map_err(|err| format!("replay tui error: {err}"))
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
            .unwrap_or_else(|| "resume unavailable without reason".to_string());
        return Err(format!(
            "continue session is disabled for {run_id}: {reason}"
        ));
    }

    let historical_events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    let resume_agent_id = select_resume_agent_id(&resume_plan, &historical_events, &run_id)?;
    let run_name = latest_run_name(&historical_events).unwrap_or_else(|| "interactive".to_string());

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("continue_bootstrap.coordinator_ready");
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = settings.deterministic;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

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

    let event_forwarder_task = tokio::spawn(async move {
        forward_events_to_tui(store, live_update_tx, preloaded_last_seq.saturating_add(1)).await
    });

    let intent_coordinator = coordinator.clone();
    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(resume_agent_id.clone()),
        profile: continue_metadata.profile().to_string(),
        last_request_id: latest_request_id_for_agent(&historical_events, &resume_agent_id),
    }));
    let intent_live_agent_target = Arc::clone(&live_agent_target);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_live_agent_target),
            intent_live_update_tx,
        )
        .await
    });

    let (selected_workflow, ui_intent_sender) =
        build_live_ui_intent_router(intent_tx.clone(), Arc::clone(&launch_selection));

    let exit_on_finish = cmd.exit_on_finish;
    set_pending_live_launch_metadata(continue_metadata);

    let tui_result = tokio::task::spawn_blocking(move || {
        run_tui_with_options(continue_live_tui_options(
            run.run_dir,
            historical_events,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            true,
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

fn continue_live_tui_options(
    run_dir: PathBuf,
    historical_events: Vec<EventEnvelopeV1>,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events,
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        preserve_terminal_on_exit: true,
    }
}

fn new_live_tui_options(
    run_dir: PathBuf,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events: Vec::new(),
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        preserve_terminal_on_exit: true,
    }
}

fn load_run_metadata(run_dir: &Path) -> Option<RunMetadata> {
    let meta_path = run_dir.join("meta.json");
    let body = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&body).ok()
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
            .filter(|value| !value.trim().is_empty()),
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
) -> (SelectedWorkflow, UiIntentSink) {
    let selected_workflow = Arc::new(Mutex::new(None::<InteractiveWorkflow>));
    let selected_workflow_sink = Arc::clone(&selected_workflow);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if let UiIntent::SwitchModel {
            launch_metadata, ..
        } = &intent
        {
            record_launch_selection(&launch_selection, launch_metadata);
        }
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
        | UiIntent::CompactSession
        | UiIntent::SwitchModel { .. } => None,
    }
}

fn forward_intent_to_live_run(intent: &UiIntent) -> bool {
    matches!(
        intent,
        UiIntent::ResolvePermission { .. }
            | UiIntent::SubmitPrompt { .. }
            | UiIntent::CompactSession
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
    selected_workflow
        .lock()
        .map_err(|_| "live workflow selection lock poisoned".to_string())
        .map(|mut slot| slot.take().unwrap_or(InteractiveWorkflow::Quit))
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

fn latest_run_name(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if let EventV1::RunStarted(data) = &event.payload {
            Some(data.run_name.clone())
        } else {
            None
        }
    })
}

fn select_resume_agent_id(
    resume_plan: &ResumePlan,
    historical_events: &[EventEnvelopeV1],
    run_id: &str,
) -> Result<String, String> {
    if resume_plan.known_agents.is_empty() {
        return Err(format!(
            "continue session requires at least one agent binding for {run_id}"
        ));
    }

    most_recent_conversational_agent_id(historical_events, &resume_plan.known_agents)
        .or_else(|| most_recent_known_agent_spawn_id(historical_events, &resume_plan.known_agents))
        .ok_or_else(|| {
            format!(
                "continue session requires a deterministically targetable conversational agent for {run_id}"
            )
        })
}

fn most_recent_conversational_agent_id(
    historical_events: &[EventEnvelopeV1],
    known_agents: &BTreeMap<String, String>,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        let conversational_payload = matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                | EventV1::ProviderStreamDelta(_)
                | EventV1::ProviderRequestFinished(_)
                | EventV1::AssistantMessageFinished(_)
                | EventV1::TaskCompleted(_)
                | EventV1::TaskCancelled(_)
        );
        if !conversational_payload || event.actor.kind != ActorKind::Worker {
            return None;
        }

        event
            .actor
            .agent_id
            .as_ref()
            .filter(|agent_id| known_agents.contains_key(*agent_id))
            .cloned()
    })
}

fn most_recent_known_agent_spawn_id(
    historical_events: &[EventEnvelopeV1],
    known_agents: &BTreeMap<String, String>,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        let EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        known_agents
            .contains_key(&data.agent_id)
            .then(|| data.agent_id.clone())
    })
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
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = settings.deterministic;
    coordinator_config.run_id_override = Some(run_id_override);
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );
    profile_handoff("new_live.coordinator_spawned");

    let run = coordinator
        .start_run("interactive", &workspace)
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

    let launch_metadata = launch_metadata_for_mode(settings, &launch_selection);

    let agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            launch_metadata.profile().to_string(),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.spawn_agent_idle_done");

    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let intent_live_update_tx = live_update_tx.clone();

    let event_forwarder_task =
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx, 1).await });

    let intent_coordinator = coordinator.clone();
    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: launch_metadata.profile().to_string(),
        last_request_id: None,
    }));
    let intent_live_agent_target = Arc::clone(&live_agent_target);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_live_agent_target),
            intent_live_update_tx,
        )
        .await
    });

    let (selected_workflow, ui_intent_sender) =
        build_live_ui_intent_router(intent_tx.clone(), Arc::clone(&launch_selection));

    let exit_on_finish = cmd.exit_on_finish;
    set_pending_live_launch_metadata(launch_metadata);

    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        run_tui_with_options(new_live_tui_options(
            run.run_dir,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            true,
        ))
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;
    profile_handoff("new_live.live_tui_end");

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
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = settings.deterministic;
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config.run_id_override = deterministic_run_id;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

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

    let event_forwarder_task =
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx, 1).await });

    let intent_coordinator = coordinator.clone();
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            None,
            intent_live_update_tx,
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
    set_pending_live_launch_metadata(scenario_launch_metadata());

    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        run_tui_with_options(new_live_tui_options(
            run_dir,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            false,
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
            "edit.hashline_apply",
            serde_json::to_value(golden_path_patch()).map_err(|err| err.to_string())?,
        )
        .await
        .map_err(|err| err.to_string())?;

    if !scenario.interactive_permissions() {
        let permission_id =
            wait_for_permission_id(&run.events_path, &tool_call_id, WAIT_TIMEOUT).await?;
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
            Some(WAIT_TIMEOUT)
        },
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

                    last_seq_seen = event.seq;
                    from_seq = last_seq_seen.saturating_add(1);
                    if live_update_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .is_err()
                    {
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

                        last_seq_seen = replayed_event.seq;
                        from_seq = last_seq_seen.saturating_add(1);
                        if live_update_tx
                            .send(LiveUpdate::Event(Box::new(replayed_event)))
                            .is_err()
                        {
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

fn compact_token_estimate(value: u32) -> String {
    if value >= 1_000_000 {
        return format!("{:.1}M", f64::from(value) / 1_000_000.0);
    }
    if value >= 1_000 {
        return format!("{:.1}K", f64::from(value) / 1_000.0);
    }
    value.to_string()
}

async fn handle_ui_intents(
    coordinator: CoordinatorHandle,
    mut intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    user_actor: EventActor,
    live_agent_target: Option<LiveAgentTargetState>,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
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
                        .request_agent_turn_with_model(
                            user_actor.clone(),
                            agent_id,
                            text,
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
    events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::RunFinished(_) | EventV1::RunFailed(_)
        )
    })
}

async fn wait_for_permission_id(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let events = load_events(events_path)?;
        if let Some(permission_id) = events.into_iter().find_map(|event| match event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(data.permission_id)
            }
            _ => None,
        }) {
            return Ok(permission_id);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for PermissionRequested for {tool_call_id}"
            ));
        }

        tokio::time::sleep(EVENT_WAIT_POLL_INTERVAL).await;
    }
}

async fn wait_for_tool_finished(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Option<Duration>,
) -> Result<ToolCallStatus, String> {
    let deadline = timeout.map(|wait| Instant::now() + wait);
    loop {
        let events = load_events(events_path)?;

        if let Some(status) = events.iter().find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => {
                Some(data.status)
            }
            _ => None,
        }) {
            return Ok(status);
        }

        if let Some(run_error) = events.iter().find_map(|event| match &event.payload {
            EventV1::RunFailed(data) => Some(data.error.clone()),
            _ => None,
        }) {
            return Err(format!(
                "run failed before ToolCallFinished for {tool_call_id}: {run_error}"
            ));
        }

        if events
            .iter()
            .any(|event| matches!(&event.payload, EventV1::RunFinished(_)))
        {
            return Err(format!(
                "run finished before ToolCallFinished for {tool_call_id}"
            ));
        }

        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for ToolCallFinished for {tool_call_id}"
                ));
            }
        }

        tokio::time::sleep(EVENT_WAIT_POLL_INTERVAL).await;
    }
}

fn load_events(path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read events file {}: {err}", path.display()))?;
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).map_err(|err| err.to_string()))
        .collect()
}

fn deterministic_run_id(seed: u64, scenario: ScenarioName) -> String {
    let namespace = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("harness-seed:{seed}").as_bytes(),
    );
    let run_uuid = Uuid::new_v5(&namespace, scenario.as_str().as_bytes());
    format!("run_{}", run_uuid.simple())
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
pub(crate) fn assert_startup_command_workflow_maps_model_and_session_intents_correctly() {
    let launch_selection = Arc::new(Mutex::new(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4").with_available_models(vec![
            ModelOption::from_model_ref("deep", "default:gpt-5.4"),
            ModelOption::from_model_ref("ops", "anthropic:claude-3.7"),
        ]),
    ));
    let switched_metadata = LaunchMetadata::from_model_ref("ops", "anthropic:claude-3.7")
        .with_available_models(
            recover_mutex_lock(&launch_selection)
                .available_models()
                .to_vec(),
        )
        .with_mode_label("Continued");

    record_launch_selection(&launch_selection, &switched_metadata);

    let recorded = recover_mutex_lock(&launch_selection).clone();
    assert_eq!(recorded.profile(), "ops");
    assert_eq!(recorded.provider(), "anthropic");
    assert_eq!(recorded.model(), Some("claude-3.7"));
    assert_eq!(recorded.mode_label(), None);
    assert_eq!(recorded.available_models().len(), 2);

    let continue_run_dir = PathBuf::from("/tmp/sessions/run_continue");
    assert_eq!(
        map_startup_intent_to_workflow(Some(UiIntent::ContinueSession {
            run_id: "run_continue".to_string(),
            run_dir: continue_run_dir.clone(),
        })),
        InteractiveWorkflow::Continue {
            run_id: "run_continue".to_string(),
            run_dir: continue_run_dir,
        }
    );

    let replay_run_dir = PathBuf::from("/tmp/sessions/run_replay");
    assert_eq!(
        live_workflow_from_intent(&UiIntent::ReplaySession {
            run_id: "run_replay".to_string(),
            run_dir: replay_run_dir.clone(),
        }),
        Some(InteractiveWorkflow::Replay {
            run_dir: replay_run_dir,
        })
    );

    assert!(forward_intent_to_live_run(&UiIntent::SwitchModel {
        profile: "ops".to_string(),
        launch_metadata: switched_metadata,
    }));
    assert!(forward_intent_to_live_run(&UiIntent::CompactSession));
    assert_eq!(live_workflow_from_intent(&UiIntent::CompactSession), None);
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
    use harness_core::config::load_config_from_str;
    use harness_core::event::{AgentSpawnedEvent, ProviderRequestStartedEvent};
    use harness_tui::app::{set_pending_live_prompt_draft, AppState};
    use std::sync::{Mutex, OnceLock};

    fn mock_mode_cwd_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
            rx,
            false,
            Arc::clone(&sink),
            true,
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
            rx,
            false,
            sink,
            true,
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
            build_live_ui_intent_router(intent_tx, Arc::clone(&launch_selection));

        sink(UiIntent::CompactSession);

        assert!(recover_mutex_lock(&selected_workflow).is_none());
        assert_eq!(intent_rx.try_recv().ok(), Some(UiIntent::CompactSession));
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

        let previous_dir = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(temp.path()).expect("enter temp dir");

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
        );

        std::env::set_current_dir(previous_dir).expect("restore current dir");

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

        let previous_dir = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(temp.path()).expect("enter temp dir");

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
        );

        std::env::set_current_dir(previous_dir).expect("restore current dir");

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
    fn shipped_example_config_does_not_synthesize_unconfigured_model_variant() {
        let config_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/harness.example.jsonc");
        let config = harness_core::config::load_config_from_file(&config_path)
            .expect("shipped example config should parse with discovered prompts");

        let agent_profiles = bootstrap::interactive_agent_profiles(&config)
            .expect("interactive agent profiles should build");
        let metadata = interactive_launch_metadata(Some(&config), &agent_profiles, "build")
            .expect("launch metadata should build");

        assert_eq!(metadata.profile(), "build");
        assert_eq!(metadata.variant(), None);
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
            session_dir: session_dir.clone(),
            workspace_root: PathBuf::from("/tmp/warmed-workspace"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: false,
            seed: 0,
            config_digest: "digest".to_string(),
            launch_metadata: interactive_launch_metadata(None, &agent_profiles, "build")
                .expect("launch metadata should build"),
            launch_mode_label: None,
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
