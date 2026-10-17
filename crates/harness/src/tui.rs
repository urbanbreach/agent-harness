// allow: SIZE_OK — CLI TUI handoff (launch + setup + config)
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
#[cfg(test)]
use harness_core::config::ShellAllowlist;
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::ToolCallStatus;
#[cfg(test)]
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
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
use harness_tui::live_update_channel;
use harness_tui::{
    close_preserved_terminal_session, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions,
    UiIntent,
};
#[cfg(test)]
use harness_tui::{CompactionCommandKind, CompactionCommandOutcome, OperatorNoticeLevel};
use tokio::sync::{mpsc, oneshot};

use crate::cli_config::apply_runtime_metadata;
use crate::live::{
    await_task, connect_provider_options, recover_mutex_lock,
    set_pending_prompt_commands_from_settings, stop_live_source_run, user_actor,
};

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

#[cfg(test)]
use crate::live::auth_backend::{
    run_tui_auth_backend_once_with_deps, run_tui_auth_backend_streaming_with_deps,
};
use crate::live::auth_backend::{spawn_tui_auth_backend_task, TuiAuthBackendContext};
use crate::live::coordinator_warmup::LiveCoordinatorConfigWarmup;
use crate::live::launch_metadata::continue_launch_metadata;
#[cfg(test)]
use crate::live::launch_metadata::interactive_launch_metadata;
#[cfg(test)]
use crate::live::lineage::{materialize_tui_fork_child, materialize_tui_lineage_child};
use crate::live::live_events::{forward_events_to_tui, latest_request_id_for_agent};
#[cfg(test)]
use crate::live::live_intents::foreground_background_success_message;
use crate::live::live_intents::{handle_ui_intents, LiveAgentTarget};
use crate::live::live_options::{continue_live_tui_options, new_live_tui_options};
#[cfg(test)]
use crate::live::live_settings::prepare_new_live_workspace;
use crate::live::live_settings::{
    launch_metadata_for_mode, resolve_tui_mode, scenario_launch_metadata, LiveSettings,
    ResolvedTuiMode,
};
#[cfg(test)]
use crate::live::live_settings::{
    resolve_live_settings, resolve_live_settings_for_test, LiveSettingsDeps,
};
#[cfg(test)]
use crate::live::new_live::spawn_session_history_refresh;
use crate::live::new_live::{run_new_live_session, run_new_worktree_live_session};
use crate::live::profile_log::profile_handoff;
#[cfg(test)]
use crate::live::replay::is_terminal_event;
use crate::live::replay::{execute_replay_mode, run_replay_tui};
#[cfg(test)]
use crate::live::runtime_toggles::runtime_toggles_config;
use crate::live::session_history::{
    load_live_session_history_entries, load_recorded_runtime_context,
    load_startup_session_history_entries,
};
#[cfg(test)]
use crate::live::workflow::UiIntentSink;
use crate::live::workflow::{
    build_live_ui_intent_router, handle_model_switch_intent, map_startup_intent_to_workflow,
    persist_launch_selection_for_exit, run_interactive_workflow_loop, take_selected_workflow,
    InteractiveWorkflow, LaunchSelection, LaunchSelectionState,
};

#[cfg(test)]
use crate::live::model_selection::{
    apply_model_selection_to_launch_metadata, apply_persisted_model_selection_from_path,
    load_persisted_model_selection_from_path, save_persisted_model_selection_to_path,
    PersistedModelSelection,
};

#[cfg(test)]
use crate::live::launch_metadata::replay_launch_metadata;

use harness_core::auth::plugin::AuthPluginRegistry;
#[cfg(test)]
use harness_core::proj::RecordedRuntimeContext;
use harness_tui::app::ConnectProviderOption;
#[cfg(test)]
use harness_tui::app::ToggleEntryKind;

fn set_pending_connect_providers_from_config(
    config: Option<&harness_core::config::HarnessConfig>,
    demo_mode: bool,
) {
    harness_tui::app::set_pending_connect_providers(connect_provider_options_for_mode(
        config,
        demo_mode,
        || {
            std::thread::spawn(harness_core::provider_catalog::ProviderCatalog::from_env)
                .join()
                .ok()
                .and_then(Result::ok)
        },
    ));
}

fn connect_provider_options_for_mode(
    config: Option<&harness_core::config::HarnessConfig>,
    demo_mode: bool,
    load_live: impl FnOnce() -> Option<harness_core::provider_catalog::ProviderCatalog>,
) -> Vec<ConnectProviderOption> {
    let catalog = if demo_mode {
        harness_core::provider_catalog::ProviderCatalog::from_embedded().ok()
    } else {
        load_live()
    };
    connect_provider_options(
        config,
        &AuthPluginRegistry::with_builtins(),
        catalog.as_ref(),
    )
}

#[derive(Debug, Args, Clone)]
pub struct TuiCommand {
    #[arg(long, conflicts_with = "scenario")]
    pub replay: Option<PathBuf>,

    #[arg(
        long = "continue",
        alias = "continue-session",
        value_name = "SESSION",
        conflicts_with_all = ["replay", "scenario"]
    )]
    pub continue_session: Option<PathBuf>,

    #[arg(long, value_enum, conflicts_with = "replay")]
    pub scenario: Option<ScenarioName>,

    #[arg(long, default_value_t = false, conflicts_with_all = ["replay", "scenario"])]
    pub mock: bool,

    /// Automatically approve ordinary tool requests; remember this mode for the session.
    #[arg(long, conflicts_with_all = ["replay", "scenario"])]
    pub yolo: bool,

    #[arg(long, default_value_t = false)]
    pub deterministic: bool,

    #[arg(long)]
    pub session_dir: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub exit_on_finish: bool,

    #[arg(long, conflicts_with_all = ["replay", "scenario", "continue_session"])]
    pub profile: Option<String>,

    /// Run inline instead of using the terminal alternate screen.
    #[arg(long, default_value_t = false)]
    pub no_alt_screen: bool,
}

struct LiveBootstrap {
    store: Arc<dyn EventStore>,
    run_dir: PathBuf,
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

    if let ResolvedTuiMode::Replay { run_dir, .. } = &mode {
        return execute_replay_mode(run_dir, cmd.exit_on_finish, cmd.no_alt_screen, stderr);
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
            runtime.block_on(run_direct_continue_mode(&cmd, &settings, cmd.mock, run_dir))
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

    let restored = close_preserved_terminal_session().map_err(|err| err.to_string());
    match run_result.and(restored) {
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

    set_pending_connect_providers_from_config(settings.config.as_ref(), demo_mode);
    let launch_selection = Arc::new(Mutex::new(LaunchSelectionState {
        metadata: settings.launch_metadata.clone().without_mode_label(),
        config_digest: settings.config_digest.clone(),
    }));
    let persist_model_selection = settings.config.is_some() && !demo_mode;
    let coordinator_config_warmup = LiveCoordinatorConfigWarmup::start(settings, demo_mode);
    profile_handoff("interactive_mode.warmup_started");

    let result = run_interactive_workflow_loop(
        InteractiveWorkflow::Startup,
        || {
            set_pending_prompt_commands_from_settings(settings);
            set_pending_live_launch_metadata(launch_metadata_for_mode(settings, &launch_selection));
            load_startup_session_history_entries(&settings.session_dir)
        },
        async |session_history_entries, startup_notice| {
            run_startup_launcher(
                cmd.exit_on_finish,
                session_history_entries,
                startup_notice,
                Arc::clone(&launch_selection),
                persist_model_selection,
                Some(prompt_history_path_for_session_dir(&settings.session_dir)),
                TuiAuthBackendContext::from_settings(settings).with_login_refresh(
                    coordinator_config_warmup.clone(),
                    Arc::clone(&launch_selection),
                ),
                settings
                    .config
                    .as_ref()
                    .map(|config| config.ui.keybindings.clone()),
                cmd.no_alt_screen,
                settings.yolo,
                settings.data_dir.clone(),
            )
            .await
        },
        async || {
            run_new_live_session(
                cmd,
                settings,
                demo_mode,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |name, worktree_path| {
            run_new_worktree_live_session(
                cmd,
                settings,
                demo_mode,
                name,
                worktree_path,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |run_id, run_dir| {
            run_continue_session_bootstrap(
                cmd,
                settings,
                demo_mode,
                run_id,
                run_dir,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |run_dir| run_replay_tui(run_dir, cmd.exit_on_finish, cmd.no_alt_screen).await,
    )
    .await;

    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(&launch_selection));
    }
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

    let launch_selection = Arc::new(Mutex::new(LaunchSelectionState {
        metadata: settings.launch_metadata.clone().without_mode_label(),
        config_digest: settings.config_digest.clone(),
    }));
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
        || {
            set_pending_prompt_commands_from_settings(settings);
            set_pending_live_launch_metadata(launch_metadata_for_mode(settings, &launch_selection));
            load_startup_session_history_entries(&settings.session_dir)
        },
        async |session_history_entries, startup_notice| {
            run_startup_launcher(
                cmd.exit_on_finish,
                session_history_entries,
                startup_notice,
                Arc::clone(&launch_selection),
                persist_model_selection,
                Some(prompt_history_path_for_session_dir(&settings.session_dir)),
                TuiAuthBackendContext::from_settings(settings).with_login_refresh(
                    coordinator_config_warmup.clone(),
                    Arc::clone(&launch_selection),
                ),
                settings
                    .config
                    .as_ref()
                    .map(|config| config.ui.keybindings.clone()),
                cmd.no_alt_screen,
                settings.yolo,
                settings.data_dir.clone(),
            )
            .await
        },
        async || {
            run_new_live_session(
                cmd,
                settings,
                demo_mode,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |name, worktree_path| {
            run_new_worktree_live_session(
                cmd,
                settings,
                demo_mode,
                name,
                worktree_path,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |run_id, run_dir| {
            run_continue_session_bootstrap(
                cmd,
                settings,
                demo_mode,
                run_id,
                run_dir,
                Arc::clone(&launch_selection),
                coordinator_config_warmup.clone(),
            )
            .await
        },
        async |run_dir| run_replay_tui(run_dir, cmd.exit_on_finish, cmd.no_alt_screen).await,
    )
    .await;

    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(&launch_selection));
    }
    result
}

async fn run_startup_launcher(
    exit_on_finish: bool,
    session_history_entries: Vec<SessionHistoryEntry>,
    startup_notice: Option<String>,
    launch_selection: LaunchSelection,
    persist_model_selection: bool,
    prompt_history_path: Option<PathBuf>,
    auth_backend: TuiAuthBackendContext,
    keybindings: Option<BTreeMap<String, String>>,
    skip_alternate_screen: bool,
    yolo: bool,
    data_dir: PathBuf,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("startup_launcher.begin");
    let selected_intent = Arc::new(Mutex::new(None::<UiIntent>));
    let selected_intent_sink = Arc::clone(&selected_intent);
    let (live_update_tx, live_update_rx) = live_update_channel();
    let _ = live_update_tx.send(LiveUpdate::YoloModeChanged { enabled: yolo });
    if let Some(message) = startup_notice {
        let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
            message,
            level: harness_tui::OperatorNoticeLevel::Error,
        });
    }
    let auth_update_tx = live_update_tx.clone();
    if let Some(notice) =
        auth_backend.model_prompt_notice(&recover_mutex_lock(&launch_selection).metadata)
    {
        let _ = live_update_tx.send(notice);
    }
    let startup_auth_backend = auth_backend.clone();
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if handle_model_switch_intent(&intent, &launch_selection, persist_model_selection) {
            if let UiIntent::SwitchModel {
                launch_metadata, ..
            } = &intent
                && let Some(notice) = auth_backend.model_prompt_notice(launch_metadata)
            {
                let _ = auth_update_tx.send(notice);
            }
            return;
        }

        if let UiIntent::AuthBackendInput { line } = intent {
            crate::live::auth_backend::send_tui_auth_backend_input(line);
            return;
        }
        if let UiIntent::OpenAuthManager {
            attempt,
            args,
            stdin,
        } = intent
        {
            spawn_tui_auth_backend_task(
                attempt,
                None,
                args,
                stdin,
                startup_auth_backend.clone(),
                auth_update_tx.clone(),
            );
            return;
        }

        if !matches!(
            intent,
            UiIntent::NewSession
                | UiIntent::NewWorktreeSession { .. }
                | UiIntent::SwitchWorktree { .. }
                | UiIntent::ReplaySession { .. }
                | UiIntent::ContinueSession { .. }
                | UiIntent::SubmitPrompt { .. }
                | UiIntent::SteerPrompt { .. }
                | UiIntent::FollowUpPrompt { .. }
                | UiIntent::CancelCompaction { .. }
                | UiIntent::CompactSession { .. }
                | UiIntent::Handoff { .. }
                | UiIntent::Shake { .. }
                | UiIntent::IdleCompaction
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
        let _auth_cleanup = crate::live::auth_backend::TuiAuthCleanup;
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Startup {
                session_history_entries,
                prompt_history_path,
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings,
            storage_data_dir: Some(data_dir),
            toggles: None,
            preserve_terminal_on_exit: true,
            skip_alternate_screen,
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
    let (live_update_tx, live_update_rx) = live_update_channel();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let receipt_updates = live_update_tx.clone();
    let (runtime, continue_metadata, historical_events) = crate::live::live_runtime::bootstrap(
        settings,
        demo_mode,
        run_id,
        run_dir,
        Arc::clone(&launch_selection),
        coordinator_config_warmup,
        live_update_tx,
        intent_rx,
    )
    .await?;
    let crate::live::new_live::NewLiveRuntime {
        coordinator,
        event_forwarder_task,
        ui_intent_task,
    } = runtime;
    let run = coordinator
        .run_info()
        .await
        .map_err(|err| err.to_string())?;

    let (selected_workflow, ui_intent_sender) = build_live_ui_intent_router(
        intent_tx.clone(),
        receipt_updates,
        Arc::clone(&launch_selection),
        settings.config.is_some() && !demo_mode,
    );

    let exit_on_finish = cmd.exit_on_finish;
    let no_alt_screen = cmd.no_alt_screen;
    let toggles = Some(settings.toggles.clone());
    let keybindings = settings
        .config
        .as_ref()
        .map(|config| config.ui.keybindings.clone());
    set_pending_prompt_commands_from_settings(settings);
    set_pending_live_launch_metadata(continue_metadata);
    if let Some(config_path) = settings.config_path.clone() {
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
        let compaction_mid_turn_enabled = settings
            .config
            .as_ref()
            .map(|config| config.runtime.compaction.mid_turn_enabled)
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
            compaction_enabled,
            compaction_auto_retry_overflow,
            compaction_mid_turn_enabled,
            compaction_estimated_token_triggers,
            deterministic_enabled,
        );
    }
    if let Some(config) = &settings.config {
        let compact = &config.runtime.compaction;
        harness_tui::app::set_pending_idle_compaction_settings(
            compact.idle_enabled,
            compact.idle_threshold_tokens,
            compact.idle_timeout_seconds,
        );
    }
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    let session_history_entries =
        load_live_session_history_entries(&run.run_dir, &settings.session_dir)?;

    let data_dir = settings.data_dir.clone();
    let tui_result = tokio::task::spawn_blocking(move || {
        let mut options = continue_live_tui_options(
            run.run_dir,
            historical_events,
            session_history_entries,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            true,
            prompt_history_path,
            keybindings,
            toggles,
            no_alt_screen,
        );
        options.storage_data_dir = Some(data_dir);
        let _auth_cleanup = crate::live::auth_backend::TuiAuthCleanup;
        run_tui_with_options(options)
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;

    if let Err(err) = tui_result {
        let stop_result = stop_live_source_run(&coordinator).await;
        event_forwarder_task.abort();
        ui_intent_task.abort();
        stop_result?;
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

    let fixture_lock =
        harness_core::store::open_private_append(&settings.session_dir.join(".scenario.lock"))
            .map_err(|err| err.to_string())?;
    fixture_lock.try_lock().map_err(|err| err.to_string())?;
    if let Some(run_id) = &deterministic_run_id {
        crate::scenarios::reset_fixture(&settings.session_dir, run_id)?;
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
    coordinator_config.data_dir.clone_from(&settings.data_dir);
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    if scenario.is_compaction() {
        crate::scenarios::configure_compaction_scenario(&mut coordinator_config, scenario);
    }
    coordinator_config.run_id_override = deterministic_run_id;
    coordinator_config.session_mode_source =
        Some(harness_core::proj::SessionModeSource::ScenarioFixture);
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
    let (live_update_tx, live_update_rx) = live_update_channel();
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
    let live_agent_target = if scenario.is_compaction() {
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "default", None)
            .await
            .map_err(|error| error.to_string())?;
        Some(Arc::new(std::sync::Mutex::new(
            crate::live::live_intents::LiveAgentTarget {
                agent_id: Some(agent_id),
                profile: "default".into(),
                last_request_id: None,
            },
        )))
    } else {
        None
    };

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
    let receipt_updates = intent_live_update_tx.clone();
    let auth_backend = TuiAuthBackendContext::from_settings(settings);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            live_agent_target,
            intent_live_update_tx,
            auth_backend,
        )
        .await
    });

    let ui_intent_sender = {
        let intent_tx = intent_tx.clone();
        Arc::new(move |intent: UiIntent| {
            if let Err(error) = intent_tx.send(intent)
                && let UiIntent::QueuedPrompt { id, .. } = error.0
            {
                let _ = receipt_updates.send(LiveUpdate::QueuedPromptReceipt {
                    id,
                    result: Err("live session input channel is closed".to_owned()),
                });
            }
        })
    };

    let exit_on_finish = cmd.exit_on_finish;
    let no_alt_screen = cmd.no_alt_screen;
    let toggles = Some(settings.toggles.clone());
    let keybindings = settings
        .config
        .as_ref()
        .map(|config| config.ui.keybindings.clone());
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    set_pending_prompt_commands_from_settings(settings);
    if scenario.is_compaction() {
        // The scenario has no config file; settings writes land beside the session store.
        set_pending_settings_project_config(
            settings.session_dir.with_extension("settings.jsonc"),
            true,
            true,
            true,
            true,
            false,
        );
    }
    let launch_metadata = if scenario.is_compaction() {
        let mut model = harness_tui::app::ModelOption::from_model_ref("default", "mock:default");
        model.model_limits = harness_core::config::ResolvedModelLimits::from_values(
            Some(64_000),
            Some(64_000),
            Some(8_192),
            Default::default(),
        );
        harness_tui::app::LaunchMetadata::from_model_option(&model).with_mode_label("Local fixture")
    } else {
        scenario_launch_metadata()
    };
    set_pending_live_launch_metadata(launch_metadata);
    let session_history_entries =
        load_live_session_history_entries(&run_dir, &settings.session_dir)?;

    let data_dir = settings.data_dir.clone();
    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        let mut options = new_live_tui_options(
            run_dir,
            session_history_entries,
            live_update_rx,
            exit_on_finish,
            ui_intent_sender,
            false,
            prompt_history_path,
            keybindings,
            toggles,
            no_alt_screen,
        );
        options.storage_data_dir = Some(data_dir);
        let _auth_cleanup = crate::live::auth_backend::TuiAuthCleanup;
        run_tui_with_options(options)
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;
    profile_handoff("new_live.live_tui_end");

    if let Err(err) = tui_result {
        let stop_result = stop_live_source_run(&coordinator).await;
        scenario_task.abort();
        event_forwarder_task.abort();
        ui_intent_task.abort();
        stop_result?;
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

        if let Err(err) = stop_result
            && !matches!(err, CoordinatorError::RunNotStarted)
        {
            return Err(err.to_string());
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
    if scenario.is_compaction() {
        return Ok(());
    }

    let agent_id = coordinator
        .spawn_agent(supervisor_actor(), "default", None)
        .await
        .map_err(|err| err.to_string())?;

    if scenario.is_question() {
        let worker = worker_actor(agent_id);
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
            worker_actor(agent_id),
            None,
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

#[cfg(test)]
pub(crate) fn replay_launch_metadata_for_test(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    crate::live::replay::replay_launch_metadata_for_test(run_dir, historical_events)
}

#[cfg(test)]
#[path = "tui/tests.rs"]
pub(crate) mod tests;
