use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{load_config_from_file, resolve_config_path, ShellAllowlist};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_core::store::{EventStore, EventStoreError};
use harness_tools::coordinator_registry;
use harness_tui::{
    load_events_from_run_dir, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::scenarios::{
    create_workspace, default_permission_policy, golden_path_patch, golden_path_profiles,
    golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";
const DEFAULT_INTERACTIVE_PROFILE: &str = "worker";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Args, Clone)]
pub struct TuiCommand {
    #[arg(long, conflicts_with = "scenario")]
    pub replay: Option<PathBuf>,

    #[arg(long, value_enum, conflicts_with = "replay")]
    pub scenario: Option<ScenarioName>,

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
    session_dir: PathBuf,
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
    default_profile: String,
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
    if let Some(run_dir) = &cmd.replay {
        return execute_replay_mode(run_dir, cmd.exit_on_finish);
    }

    let settings = match resolve_live_settings(&cmd, config_path, global_session_dir) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("tui setup failed: {err}");
            return ExitCode::from(2);
        }
    };

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

    let run_result = if let Some(scenario) = cmd.scenario {
        runtime.block_on(run_live_mode(&cmd, &settings, scenario))
    } else {
        runtime.block_on(run_interactive_mode(&cmd, &settings))
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
    let mut config_default_profile = DEFAULT_INTERACTIVE_PROFILE.to_string();

    if let Some(path) = explicit_config {
        let config =
            load_config_from_file(&path).map_err(|err| format!("{} ({})", err, path.display()))?;
        let config_bytes = fs::read(&path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        config_digest = blake3::hash(&config_bytes).to_hex().to_string();
        config_default_profile = read_default_profile_from_config_bytes(&config_bytes)
            .unwrap_or_else(|| DEFAULT_INTERACTIVE_PROFILE.to_string());
        shell_allowlist = config.permissions.shell_allowlist;
        config_session_dir = config.paths.session_dir;
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic
        || config_deterministic
        || matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));
    let default_profile = cmd
        .profile
        .clone()
        .unwrap_or(config_default_profile);

    Ok(LiveSettings {
        session_dir,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
        default_profile,
    })
}

async fn run_interactive_mode(cmd: &TuiCommand, settings: &LiveSettings) -> Result<(), String> {
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

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

    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
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
        .spawn_agent(supervisor_actor(), settings.default_profile.clone(), None)
        .await
        .map_err(|err| err.to_string())?;

    let (live_update_tx, live_update_rx) = std_mpsc::channel::<LiveUpdate>();
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();

    let event_forwarder_task =
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx).await });

    let intent_coordinator = coordinator.clone();
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(agent_id),
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
        tokio::spawn(async move { forward_events_to_tui(store, live_update_tx).await });

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
) -> Result<(), String> {
    let mut from_seq = 1_u64;
    let mut last_seq_seen = 0_u64;

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

fn read_default_profile_from_config_bytes(config_bytes: &[u8]) -> Option<String> {
    let raw = std::str::from_utf8(config_bytes).ok()?;
    let parsed: serde_json::Value = json5::from_str(raw).ok()?;
    parsed
        .get("ui")
        .and_then(|ui| ui.as_object())
        .and_then(|ui| ui.get("default_profile").or_else(|| ui.get("defaultProfile")))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
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
