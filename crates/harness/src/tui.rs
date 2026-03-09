use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{
    load_config_from_file, resolve_config_path, HarnessConfig, ShellAllowlist,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionDecision;
use harness_core::proj::{inspect_resume_plan, ResumePlan, SessionModeSource};
use harness_core::redact::DefaultRedactor;
use harness_core::store::{EventStore, EventStoreError};
use harness_tools::coordinator_registry;
use harness_tui::app::{
    set_pending_live_launch_metadata, set_pending_live_prompt_auto_submit, LaunchMetadata,
    SessionHistoryEntry,
};
use harness_tui::{
    load_events_from_run_dir, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::bootstrap;
use crate::replay::inspect_session_catalog;
use crate::scenarios::{
    create_workspace, default_permission_policy, golden_path_patch, golden_path_profiles,
    golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";
const DEFAULT_MOCK_PROFILE: &str = "worker";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Args, Clone)]
pub struct TuiCommand {
    #[arg(long, conflicts_with = "scenario")]
    pub replay: Option<PathBuf>,

    #[arg(long, value_enum, conflicts_with = "replay")]
    pub scenario: Option<ScenarioName>,

    #[arg(long, default_value_t = false, conflicts_with_all = ["replay", "scenario"])]
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
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
    default_profile: String,
    launch_metadata: LaunchMetadata,
}

struct LiveBootstrap {
    store: Arc<dyn EventStore>,
    run_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LauncherSelection {
    NewSession,
    ReplaySession { run_dir: PathBuf },
    ContinueSession { run_id: String, run_dir: PathBuf },
    Quit,
}

enum ResolvedTuiMode {
    Replay {
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

    if let Err(err) = run_tui_with_options(TuiOptions {
        mode: TuiMode::Replay {
            run_dir: run_dir.to_path_buf(),
            events,
        },
        exit_on_finish,
        on_ui_intent: None,
        keybindings: None,
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
    let explicit_config = resolve_config_path(config_path.as_deref());
    let mut shell_allowlist = ShellAllowlist::default();
    let mut config_session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut config_deterministic = false;
    let mut config_seed = 0;
    let mut config_digest = "none".to_string();
    let mut config_default_profile = DEFAULT_MOCK_PROFILE.to_string();
    let mut live_config: Option<HarnessConfig> = None;

    if let Some(path) = explicit_config {
        let config =
            load_config_from_file(&path).map_err(|err| format!("{} ({})", err, path.display()))?;
        let config_bytes = fs::read(&path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        config_digest = blake3::hash(&config_bytes).to_hex().to_string();
        config_default_profile = bootstrap::interactive_profile_name(&config);
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
    let launch_metadata = interactive_launch_metadata(live_config.as_ref(), &default_profile);

    Ok(LiveSettings {
        config: live_config,
        session_dir,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
        default_profile,
        launch_metadata,
    })
}

fn interactive_launch_metadata(config: Option<&HarnessConfig>, profile: &str) -> LaunchMetadata {
    config
        .and_then(|cfg| cfg.categories.get(profile))
        .map(|category| LaunchMetadata::from_model_ref(profile.to_string(), &category.model_ref))
        .unwrap_or_else(|| LaunchMetadata::from_model_ref(profile.to_string(), "mock:model-1"))
        .with_mode_label(if config.is_some() { "Live" } else { "Demo" })
}

fn scenario_launch_metadata() -> LaunchMetadata {
    LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo")
}

async fn run_interactive_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
) -> Result<(), String> {
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let session_history_entries = load_startup_session_history_entries(&settings.session_dir)?;
    set_pending_live_launch_metadata(settings.launch_metadata.clone());
    let selection = run_startup_launcher(cmd.exit_on_finish, session_history_entries).await?;

    match selection {
        LauncherSelection::NewSession => run_new_live_session(cmd, settings, demo_mode).await,
        LauncherSelection::ReplaySession { run_dir } => {
            run_replay_tui(run_dir, cmd.exit_on_finish).await
        }
        LauncherSelection::ContinueSession { run_id, run_dir } => {
            run_continue_session_bootstrap(cmd, settings, demo_mode, run_id, run_dir).await
        }
        LauncherSelection::Quit => Ok(()),
    }
}

fn load_startup_session_history_entries(
    session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    inspect_session_catalog(session_dir).map(|entries| {
        entries
            .into_iter()
            .filter(|entry| {
                !matches!(
                    entry.catalog.mode_source,
                    SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
                )
            })
            .map(|entry| SessionHistoryEntry {
                run_dir: entry.run_dir,
                catalog: entry.catalog,
            })
            .collect()
    })
}

async fn run_startup_launcher(
    exit_on_finish: bool,
    session_history_entries: Vec<SessionHistoryEntry>,
) -> Result<LauncherSelection, String> {
    let selected_intent = Arc::new(Mutex::new(None::<UiIntent>));
    let selected_intent_sink = Arc::clone(&selected_intent);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if !matches!(
            intent,
            UiIntent::NewSession
                | UiIntent::ReplaySession { .. }
                | UiIntent::ContinueSession { .. }
                | UiIntent::SubmitPrompt { .. }
                | UiIntent::QuitRequested
        ) {
            return;
        }
        let mut slot = selected_intent_sink
            .lock()
            .expect("startup launcher intent lock poisoned");
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
        })
    })
    .await
    .map_err(|err| format!("startup launcher task failed: {err}"))?;

    if let Err(err) = tui_result {
        return Err(format!("startup launcher error: {err}"));
    }

    let selected_intent = selected_intent
        .lock()
        .map_err(|_| "startup launcher intent lock poisoned".to_string())?
        .clone();

    Ok(map_launcher_intent(selected_intent))
}

fn map_launcher_intent(intent: Option<UiIntent>) -> LauncherSelection {
    match intent {
        Some(UiIntent::NewSession) => LauncherSelection::NewSession,
        Some(UiIntent::ReplaySession { run_dir, .. }) => {
            LauncherSelection::ReplaySession { run_dir }
        }
        Some(UiIntent::ContinueSession { run_id, run_dir }) => {
            LauncherSelection::ContinueSession { run_id, run_dir }
        }
        Some(UiIntent::SubmitPrompt { text }) => {
            set_pending_live_prompt_auto_submit(Some(text));
            LauncherSelection::NewSession
        }
        Some(UiIntent::QuitRequested) | None | Some(UiIntent::ResolvePermission { .. }) => {
            LauncherSelection::Quit
        }
    }
}

async fn run_replay_tui(run_dir: PathBuf, exit_on_finish: bool) -> Result<(), String> {
    let events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Replay { run_dir, events },
            exit_on_finish,
            on_ui_intent: None,
            keybindings: None,
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
) -> Result<(), String> {
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

    let mut coordinator_config = if demo_mode {
        let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
        coordinator_config.permission_policy = default_permission_policy();
        coordinator_config.tool_registry =
            Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
        coordinator_config.provider = Arc::new(golden_path_provider());
        coordinator_config.agent_profiles = golden_path_profiles();
        coordinator_config
    } else {
        let mut config = settings
            .config
            .clone()
            .ok_or_else(bootstrap::interactive_config_guidance)?;
        config.apply_session_dir_override(Some(settings.session_dir.clone()));
        bootstrap::build_interactive_coordinator_config(&config)?
    };
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .resume_run(run_id.clone(), run_name)
        .await
        .map_err(|err| err.to_string())?;
    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let preloaded_last_seq = historical_events.last().map(|event| event.seq).unwrap_or(0);
    let resume_profile = resume_plan
        .known_agents
        .get(&resume_agent_id)
        .map(String::as_str);
    let continue_metadata = continue_launch_metadata(
        &run.run_id,
        &historical_events,
        &resume_agent_id,
        resume_profile,
    );

    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    for event in &historical_events {
        let _ = live_update_tx.send(LiveUpdate::Event(Box::new(event.clone())));
    }
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();

    let event_forwarder_task = tokio::spawn(async move {
        forward_events_to_tui(store, live_update_tx, preloaded_last_seq.saturating_add(1)).await
    });

    let intent_coordinator = coordinator.clone();
    let intent_resume_agent_id = resume_agent_id.clone();
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_resume_agent_id),
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
    set_pending_live_launch_metadata(continue_metadata);

    let tui_result = tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Live {
                run_dir: run.run_dir,
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(ui_intent_sender),
            keybindings: None,
        })
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;

    if let Err(err) = tui_result {
        event_forwarder_task.abort();
        ui_intent_task.abort();
        return Err(format!("TUI error: {err}"));
    }

    drop(intent_tx);

    let stop_result = coordinator.stop_run().await;
    event_forwarder_task.abort();
    ui_intent_task.abort();

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

fn continue_launch_metadata(
    run_id: &str,
    historical_events: &[EventEnvelopeV1],
    resume_agent_id: &str,
    resume_profile: Option<&str>,
) -> LaunchMetadata {
    let fallback =
        LaunchMetadata::from_model_ref("unknown", "unknown:unknown").with_mode_label("Continued");
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
) -> Result<(), String> {
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

    let workspace = create_workspace(
        &settings.session_dir,
        ScenarioName::GoldenPathInteractive,
        Some(run_id_override.as_str()),
    )?;

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = if demo_mode {
        let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
        coordinator_config.permission_policy = default_permission_policy();
        coordinator_config.tool_registry =
            Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
        coordinator_config.provider = Arc::new(golden_path_provider());
        coordinator_config.agent_profiles = golden_path_profiles();
        coordinator_config
    } else {
        let mut config = settings
            .config
            .clone()
            .ok_or_else(bootstrap::interactive_config_guidance)?;
        config.apply_session_dir_override(Some(settings.session_dir.clone()));
        bootstrap::build_interactive_coordinator_config(&config)?
    };
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.run_id_override = Some(run_id_override);
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .map_err(|err| err.to_string())?;
    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), settings.default_profile.clone(), None)
        .await
        .map_err(|err| err.to_string())?;

    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();

    let event_forwarder_task =
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx, 1).await });

    let intent_coordinator = coordinator.clone();
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(intent_coordinator, intent_rx, user_actor(), Some(agent_id)).await
    });

    let ui_intent_sender = {
        let intent_tx = intent_tx.clone();
        Arc::new(move |intent: UiIntent| {
            let _ = intent_tx.send(intent);
        })
    };

    let exit_on_finish = cmd.exit_on_finish;
    set_pending_live_launch_metadata(settings.launch_metadata.clone());

    let tui_result = tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Live {
                run_dir: run.run_dir,
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(ui_intent_sender),
            keybindings: None,
        })
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;

    if let Err(err) = tui_result {
        event_forwarder_task.abort();
        ui_intent_task.abort();
        return Err(format!("TUI error: {err}"));
    }

    drop(intent_tx);

    let stop_result = coordinator.stop_run().await;
    event_forwarder_task.abort();
    ui_intent_task.abort();

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
    coordinator_config.deterministic_store = settings.deterministic;
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

    let scenario_coordinator = coordinator.clone();
    let scenario_task = tokio::spawn(async move {
        run_scenario_runner(scenario_coordinator, scenario, workspace, bootstrap_tx).await
    });

    let bootstrap = bootstrap_rx
        .await
        .map_err(|_| "scenario runner exited before live TUI bootstrap was ready".to_string())?;

    let LiveBootstrap { store, run_dir } = bootstrap;

    let event_forwarder_task =
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx, 1).await });

    let intent_coordinator = coordinator.clone();
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(intent_coordinator, intent_rx, user_actor(), None).await
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
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Live {
                run_dir,
                update_rx: live_update_rx,
            },
            exit_on_finish,
            on_ui_intent: Some(ui_intent_sender),
            keybindings: None,
        })
    })
    .await
    .map_err(|err| format!("TUI task failed: {err}"))?;

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
            .resolve_permission(permission_id, PermissionDecision::Allow)
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

async fn handle_ui_intents(
    coordinator: CoordinatorHandle,
    mut intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    user_actor: EventActor,
    agent_id: Option<String>,
) -> Result<(), String> {
    while let Some(intent) = intent_rx.recv().await {
        match intent {
            UiIntent::ResolvePermission {
                permission_id,
                decision,
            } => {
                coordinator
                    .resolve_permission(permission_id, decision)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            UiIntent::SubmitPrompt { text } => {
                if let Some(agent_id) = agent_id.clone() {
                    coordinator
                        .request_agent_turn(user_actor.clone(), agent_id, text)
                        .await
                        .map_err(|err| err.to_string())?;
                }
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

        tokio::time::sleep(Duration::from_millis(10)).await;
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

        tokio::time::sleep(Duration::from_millis(10)).await;
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
mod tests {
    use super::*;
    use harness_core::event::{AgentSpawnedEvent, ProviderRequestStartedEvent};
    use harness_tui::app::AppState;

    #[test]
    fn tui_startup_new_session_bootstraps_live_after_intent() {
        assert_eq!(
            map_launcher_intent(Some(UiIntent::NewSession)),
            LauncherSelection::NewSession
        );
    }

    #[test]
    fn tui_startup_replay_session_uses_replay_mode() {
        let run_dir = PathBuf::from("/tmp/sessions/run_replay");
        assert_eq!(
            map_launcher_intent(Some(UiIntent::ReplaySession {
                run_id: "run_replay".to_string(),
                run_dir: run_dir.clone(),
            })),
            LauncherSelection::ReplaySession { run_dir }
        );
    }

    #[test]
    fn tui_startup_carries_unsent_draft_into_new_live_session() {
        set_pending_live_prompt_draft(Some("draft to keep".to_string()));

        let live = AppState::new_live(None, false, None);
        assert_eq!(live.prompt_buffer, "draft to keep");
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
                }),
            },
        ];

        let metadata = continue_launch_metadata(
            "run_fixture",
            &historical_events,
            "agent_000001",
            Some("alpha"),
        );

        assert_eq!(metadata.profile(), "alpha");
        assert_eq!(metadata.provider(), "provider-alpha");
        assert_eq!(metadata.model(), Some("model-alpha"));
        assert_eq!(metadata.mode_label(), Some("Continued"));
    }
}
