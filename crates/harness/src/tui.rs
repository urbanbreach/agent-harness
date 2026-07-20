// allow: SIZE_OK — CLI TUI handoff (launch + setup + config)
use std::fs;
use std::io::Write;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::sync::Mutex;

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
#[cfg(test)]
use harness_core::config::ShellAllowlist;
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::{ActorKind, EventActor, ToolCallStatus};
#[cfg(test)]
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionDecision;
use harness_core::proj::inspect_resume_plan;
use harness_core::redact::DefaultRedactor;
use harness_core::store::EventStore;
use harness_tools::coordinator_registry;
#[cfg(test)]
use harness_tui::app::LaunchMetadata;
#[cfg(test)]
use harness_tui::app::TogglesConfig;
use harness_tui::app::{
    prompt_history_path_for_session_dir, set_pending_live_launch_metadata,
    set_pending_settings_project_config, SessionHistoryEntry,
};
#[cfg(test)]
use harness_tui::OperatorNoticeLevel;
use harness_tui::{
    close_preserved_terminal_session, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions,
    UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::cli_config::apply_runtime_metadata;

#[cfg(test)]
use crate::bootstrap;
use crate::cli_io::{
    load_events_from_run_dir, wait_for_permission_id, wait_for_tool_finished,
    ToolFinishTerminalEvents, DEFAULT_EVENT_WAIT_TIMEOUT,
};
use crate::defaults::{DEFAULT_INTERACTIVE_RUN_NAME, RESUME_UNAVAILABLE_FALLBACK_REASON};
use crate::logging;
use crate::recovery::{latest_run_name, select_resume_agent_id};
use crate::scenarios::{
    create_workspace, default_permission_policy, deterministic_run_id, golden_path_edit_args,
    golden_path_profiles, golden_path_provider, question_interactive_request_json,
    supervisor_actor, worker_actor, ScenarioName,
};

#[path = "tui/auth_backend.rs"]
mod auth_backend;
#[path = "tui/coordinator_warmup.rs"]
mod coordinator_warmup;
#[path = "tui/launch_metadata.rs"]
mod launch_metadata;
#[path = "tui/lineage.rs"]
mod lineage;
#[path = "tui/live_events.rs"]
mod live_events;
#[path = "tui/live_intents.rs"]
mod live_intents;
#[path = "tui/live_options.rs"]
mod live_options;
#[path = "tui/live_settings.rs"]
mod live_settings;
#[path = "tui/model_selection.rs"]
mod model_selection;
#[path = "tui/new_live.rs"]
mod new_live;
#[path = "tui/profile_log.rs"]
mod profile_log;
#[path = "tui/replay.rs"]
mod replay;
#[path = "tui/runtime_toggles.rs"]
mod runtime_toggles;
#[path = "tui/session_history.rs"]
mod session_history;
#[path = "tui/workflow.rs"]
mod workflow;

#[cfg(test)]
use self::auth_backend::{
    run_tui_auth_backend_once_with_deps, run_tui_auth_backend_streaming_with_deps,
};
use self::auth_backend::{spawn_tui_auth_backend_task, TuiAuthBackendContext};
use self::coordinator_warmup::LiveCoordinatorConfigWarmup;
use self::launch_metadata::continue_launch_metadata;
#[cfg(test)]
use self::launch_metadata::interactive_launch_metadata;
#[cfg(test)]
use self::lineage::{materialize_tui_fork_child, materialize_tui_lineage_child};
use self::live_events::{forward_events_to_tui, latest_request_id_for_agent};
#[cfg(test)]
use self::live_intents::{
    foreground_background_success_message, manual_compaction_success_message,
    maybe_update_live_agent_target_for_plan_handoff,
};
use self::live_intents::{handle_ui_intents, LiveAgentTarget};
use self::live_options::{continue_live_tui_options, new_live_tui_options};
#[cfg(test)]
use self::live_settings::prepare_new_live_workspace;
use self::live_settings::{
    launch_metadata_for_mode, resolve_tui_mode, scenario_launch_metadata, LiveSettings,
    ResolvedTuiMode,
};
#[cfg(test)]
use self::live_settings::{
    resolve_live_settings, resolve_live_settings_for_test, LiveSettingsDeps,
};
#[cfg(test)]
use self::new_live::spawn_session_history_refresh;
use self::new_live::{run_new_live_session, run_new_worktree_live_session};
use self::profile_log::profile_handoff;
#[cfg(test)]
use self::replay::is_terminal_event;
use self::replay::{execute_replay_mode, run_replay_tui};
#[cfg(test)]
use self::runtime_toggles::runtime_toggles_config;
use self::session_history::{
    load_live_session_history_entries, load_recorded_runtime_context,
    load_startup_session_history_entries,
};
#[cfg(test)]
use self::workflow::UiIntentSink;
use self::workflow::{
    build_live_ui_intent_router, handle_model_switch_intent, map_startup_intent_to_workflow,
    persist_launch_selection_for_exit, run_interactive_workflow_loop, take_selected_workflow,
    InteractiveWorkflow, LaunchSelection,
};

#[cfg(test)]
use self::model_selection::{
    apply_model_selection_to_launch_metadata, apply_persisted_model_selection_from_path,
    load_persisted_model_selection_from_path, save_persisted_model_selection_to_path,
    PersistedModelSelection,
};

#[cfg(test)]
use self::launch_metadata::replay_launch_metadata;

use harness_core::auth::plugin::AuthPluginRegistry;
#[cfg(test)]
use harness_core::proj::RecordedRuntimeContext;
use harness_tui::app::ConnectProviderOption;
#[cfg(test)]
use harness_tui::app::ToggleEntryKind;

fn set_pending_connect_providers_from_config(config: Option<&harness_core::config::HarnessConfig>) {
    use harness_tui::app::set_pending_connect_providers;

    let registry = AuthPluginRegistry::with_builtins();
    let catalog = std::thread::spawn(harness_core::provider_catalog::ProviderCatalog::from_env)
        .join()
        .ok()
        .and_then(Result::ok);
    set_pending_connect_providers(connect_provider_options(
        config,
        &registry,
        catalog.as_ref(),
    ));
}

fn connect_provider_options(
    config: Option<&harness_core::config::HarnessConfig>,
    registry: &AuthPluginRegistry,
    catalog: Option<&harness_core::provider_catalog::ProviderCatalog>,
) -> Vec<ConnectProviderOption> {
    use harness_core::auth::plugin::AuthMethodSpec;
    use harness_core::auth::ProviderId;
    use harness_core::config::ProviderConfig;
    use harness_tui::app::auth_dialog::catalog_providers;
    use std::collections::BTreeSet;

    let mut providers = catalog
        .map(|catalog| catalog_providers(catalog, registry))
        .unwrap_or_default();
    let mut provider_ids = providers
        .iter()
        .map(|provider| provider.id.to_string())
        .collect::<BTreeSet<_>>();

    let Some(config) = config else {
        return providers;
    };

    for (provider_id, provider_config) in &config.providers {
        if provider_ids.contains(provider_id) {
            continue;
        }
        let ProviderConfig::OpenAiCompatible(ref oc) = provider_config;
        let label = oc.name.as_deref().unwrap_or(provider_id).to_string();

        if let Some(auth_provider) = oc.auth_provider.clone() {
            if provider_ids.contains(auth_provider.as_str()) {
                continue;
            }
            if let Some(plugin) = registry.get(&auth_provider) {
                provider_ids.insert(auth_provider.to_string());
                providers.push(ConnectProviderOption {
                    id: auth_provider.clone(),
                    label,
                    description: plugin.description().to_string(),
                    methods: plugin.auth_methods().to_vec(),
                    models: Vec::new(),
                });
            }
        } else if !oc.api_key_env.is_empty() {
            let env_var = oc.api_key_env[0].clone();
            let already_set = std::env::var(&env_var)
                .ok()
                .is_some_and(|v| !v.trim().is_empty());
            if !already_set {
                if let Some(id) = ProviderId::parse(provider_id.as_str()) {
                    provider_ids.insert(provider_id.clone());
                    providers.push(ConnectProviderOption {
                        id,
                        label,
                        description: "API key".to_string(),
                        methods: vec![AuthMethodSpec::ApiKey {
                            label: "Manually enter API Key".to_string(),
                        }],
                        models: Vec::new(),
                    });
                }
            }
        }
    }

    providers
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

struct LiveBootstrap {
    store: Arc<dyn EventStore>,
    run_dir: PathBuf,
}

fn recover_mutex_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
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

async fn run_interactive_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
) -> Result<(), String> {
    profile_handoff("interactive_mode.begin");
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    set_pending_connect_providers_from_config(settings.config.as_ref());
    let launch_selection = Arc::new(Mutex::new(
        settings.launch_metadata.clone().without_mode_label(),
    ));
    let persist_model_selection = settings.config.is_some() && !demo_mode;
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
                    TuiAuthBackendContext::from_settings(settings),
                    settings.workspace_root.clone(),
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
            move || {
                run_new_worktree_live_session(
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
        persist_launch_selection_for_exit(
            &recover_mutex_lock(&launch_selection),
            &settings.config_digest,
        );
    }
    close_preserved_terminal_session().map_err(|err| err.to_string())?;
    result
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
                    TuiAuthBackendContext::from_settings(settings),
                    settings.workspace_root.clone(),
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
            move || {
                run_new_worktree_live_session(
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
        persist_launch_selection_for_exit(
            &recover_mutex_lock(&launch_selection),
            &settings.config_digest,
        );
    }
    close_preserved_terminal_session().map_err(|err| err.to_string())?;
    result
}

async fn run_startup_launcher(
    exit_on_finish: bool,
    session_history_entries: Vec<SessionHistoryEntry>,
    launch_selection: LaunchSelection,
    persist_model_selection: bool,
    prompt_history_path: Option<PathBuf>,
    auth_backend: TuiAuthBackendContext,
    workspace_root: PathBuf,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("startup_launcher.begin");
    let selected_intent = Arc::new(Mutex::new(None::<UiIntent>));
    let selected_intent_sink = Arc::clone(&selected_intent);
    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let auth_update_tx = live_update_tx.clone();
    let startup_auth_backend = auth_backend.clone();
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if handle_model_switch_intent(
            &intent,
            &launch_selection,
            persist_model_selection,
            &auth_backend.config_digest,
        ) {
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
                | UiIntent::NewWorktreeSession
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
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings: None,
            toggles: None,
            preserve_terminal_on_exit: true,
            workspace_root: Some(workspace_root),
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
        run.run_id.as_str(),
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
        settings.config_digest.clone(),
    );

    let exit_on_finish = cmd.exit_on_finish;
    let toggles = Some(settings.toggles.clone());
    set_pending_live_launch_metadata(continue_metadata);
    if let Some(config_path) = settings.config_path.clone() {
        let hashline_edit = settings
            .config
            .as_ref()
            .map(|config| config.hashline_edit)
            .unwrap_or(true);
        let compaction_enabled = settings
            .config
            .as_ref()
            .map(|config| config.runtime.compaction.enabled)
            .unwrap_or(true);
        let compaction_auto_retry_overflow = settings
            .config
            .as_ref()
            .map(|config| config.runtime.compaction.auto_retry_overflow)
            .unwrap_or(true);
        let compaction_structured_summary_contract = settings
            .config
            .as_ref()
            .map(|config| config.runtime.compaction.structured_summary_contract)
            .unwrap_or(true);
        let compaction_estimated_token_triggers = settings
            .config
            .as_ref()
            .map(|config| config.runtime.compaction.estimated_token_triggers)
            .unwrap_or(true);
        let deterministic_enabled = settings
            .config
            .as_ref()
            .map(|config| config.runtime.deterministic.enabled)
            .unwrap_or(false);
        set_pending_settings_project_config(
            config_path,
            hashline_edit,
            compaction_enabled,
            compaction_auto_retry_overflow,
            compaction_structured_summary_contract,
            compaction_estimated_token_triggers,
            deterministic_enabled,
        );
    }
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    let session_history_entries =
        load_live_session_history_entries(&run.run_dir, &settings.session_dir)?;

    let workspace_root = settings.workspace_root.clone();
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
            workspace_root,
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

async fn stop_live_source_run(coordinator: &CoordinatorHandle) -> Result<(), String> {
    let stop_result = coordinator.stop_run().await;
    if let Err(err) = stop_result {
        if !matches!(err, CoordinatorError::RunNotStarted) {
            return Err(err.to_string());
        }
    }
    Ok(())
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

    let workspace_root = settings.workspace_root.clone();
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
            workspace_root,
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

    if scenario.is_question() {
        let worker = worker_actor(worker_agent_id);
        if scenario.interactive_permissions() {
            let _ = coordinator
                .request_question(
                    worker,
                    "toolcall_question_interactive",
                    question_interactive_request_json(),
                )
                .await;
        } else {
            let question_handle = coordinator.clone();
            let question_task = tokio::spawn(async move {
                question_handle
                    .request_question(
                        worker,
                        "toolcall_question_interactive",
                        question_interactive_request_json(),
                    )
                    .await
                    .map_err(|err| err.to_string())
                    .map(|_| ())
            });
            let permission_id = wait_for_permission_id(
                &run.events_path,
                "toolcall_question_interactive",
                DEFAULT_EVENT_WAIT_TIMEOUT,
            )
            .await?;
            coordinator
                .resolve_permission(
                    permission_id,
                    PermissionDecision::Allow,
                    Some(r#"[["A"]]"#.to_string()),
                )
                .await
                .map_err(|err| err.to_string())?;
            await_task("question request", question_task).await?;
        }

        let _ = coordinator.stop_run().await;
        return Ok(());
    }

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

fn user_actor() -> EventActor {
    EventActor::new(ActorKind::User, Some("interactive-user".to_string()))
}

async fn await_task(name: &str, handle: JoinHandle<Result<(), String>>) -> Result<(), String> {
    match handle.await {
        Ok(result) => result.map_err(|err| format!("{name} task failed: {err}")),
        Err(err) => Err(format!("{name} task join failed: {err}")),
    }
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
pub(crate) fn replay_launch_metadata_for_test(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    replay::replay_launch_metadata_for_test(run_dir, historical_events)
}

#[cfg(test)]
#[path = "tui/tests.rs"]
pub(crate) mod tests;
