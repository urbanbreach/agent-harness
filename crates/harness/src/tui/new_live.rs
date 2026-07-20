// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};

use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::coord::{spawn_coordinator, CoordinatorHandle};
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::session_title::create_default_title;
use harness_tui::app::{
    prompt_history_path_for_session_dir, set_pending_live_launch_metadata,
    set_pending_settings_project_config, LaunchMetadata,
};
use harness_tui::{run_tui_with_options, LiveUpdate, OperatorNoticeLevel, UiIntent};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::cli_config::apply_runtime_metadata;
use crate::logging;
use crate::scenarios::{deterministic_run_id, supervisor_actor, ScenarioName};

use super::auth_backend::TuiAuthBackendContext;
use super::coordinator_warmup::LiveCoordinatorConfigWarmup;
use super::live_events::forward_events_to_tui;
use super::live_intents::{handle_ui_intents, LiveAgentTarget};
use super::live_options::new_live_tui_options;
use super::live_settings::{launch_metadata_for_mode, prepare_new_live_workspace, LiveSettings};
use super::profile_log::profile_handoff;
use super::session_history::load_startup_session_history_entries;
use super::workflow::{
    build_live_ui_intent_router, take_selected_workflow, InteractiveWorkflow, LaunchSelection,
};
use super::{await_task, stop_live_source_run, unique_interactive_run_id, user_actor, TuiCommand};

pub(super) async fn run_new_worktree_live_session(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
    launch_selection: LaunchSelection,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("new_worktree_live.begin");
    let worktree_settings = prepare_worktree_live_settings(settings)?;
    profile_handoff(&format!(
        "new_worktree_live.created {}",
        worktree_settings.workspace_root.display()
    ));
    run_new_live_session(
        cmd,
        &worktree_settings,
        demo_mode,
        launch_selection,
        coordinator_config_warmup,
    )
    .await
}

fn prepare_worktree_live_settings(settings: &LiveSettings) -> Result<LiveSettings, String> {
    use harness_core::cow_worktree::apply_cow_worktree_fastpath;
    use harness_core::workspace::WorkspaceEnvironment;
    use harness_core::worktree::{create_session_worktree, CreateWorktreeOptions};

    let environment = WorkspaceEnvironment::discover(settings.workspace_root.clone());
    if !environment.is_git_repository {
        return Err(format!(
            "New worktree requires a git repository (workspace: {})",
            settings.workspace_root.display()
        ));
    }

    let created = create_session_worktree(CreateWorktreeOptions {
        repository_root: &environment.workspace_root,
        worktree_parent: None,
        slug: None,
        start_point: None,
    })
    .map_err(|err| format!("failed to create worktree: {err}"))?;

    let overlay_candidates = [
        "harness.jsonc",
        "harness.json",
        "tui.jsonc",
        "tui.json",
        ".harness-cow-overlay",
    ];
    let relative_paths: Vec<&str> = overlay_candidates
        .iter()
        .copied()
        .filter(|rel| {
            let src = environment.workspace_root.join(rel);
            let dst = created.path.join(rel);
            src.is_file() && !dst.exists()
        })
        .collect();
    let _cow_report =
        apply_cow_worktree_fastpath(&environment.workspace_root, &created.path, &relative_paths);

    let mut worktree_settings = settings.clone();
    worktree_settings.workspace_root = created.path;
    Ok(worktree_settings)
}

pub(super) async fn run_new_live_session(
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
        settings.config_digest.clone(),
    );

    let exit_on_finish = cmd.exit_on_finish;
    let toggles = Some(settings.toggles.clone());
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    set_pending_live_launch_metadata(launch_metadata);
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

    let workspace_root = settings.workspace_root.clone();
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
            workspace_root,
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

pub(super) fn spawn_session_history_refresh(
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
