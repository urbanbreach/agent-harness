// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::fs;
use std::path::PathBuf;
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
use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, LiveUpdateSender, OperatorNoticeLevel,
    UiIntent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::cli_config::apply_runtime_metadata;
use crate::logging;
use crate::scenarios::{supervisor_actor, ScenarioName};

use super::auth_backend::TuiAuthBackendContext;
use super::coordinator_warmup::LiveCoordinatorConfigWarmup;
use super::live_events::forward_events_to_tui;
use super::live_intents::{handle_ui_intents, LiveAgentTarget};
use super::live_options::new_live_tui_options;
use super::live_runtime::bootstrap_new_live_runtime;
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
    name: Option<String>,
    worktree_path: Option<PathBuf>,
    launch_selection: LaunchSelection,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
) -> Result<InteractiveWorkflow, String> {
    profile_handoff("new_worktree_live.begin");
    let worktree_settings = if let Some(path) = worktree_path {
        let entries = harness_core::worktree::list_session_worktrees(
            &settings.workspace_root,
            None,
            &settings.data_dir,
        )
        .map_err(|error| error.to_string())?;
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        if !entries
            .iter()
            .any(|entry| entry.path.canonicalize().ok().as_ref() == Some(&path))
        {
            return Err("Selected checkout is not a worktree of this repository".to_string());
        }
        let mut selected = settings.clone();
        selected.workspace_root = path;
        selected
    } else {
        prepare_worktree_live_settings(settings, name.as_deref())?
    };
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

pub(super) fn prepare_worktree_live_settings(
    settings: &LiveSettings,
    name: Option<&str>,
) -> Result<LiveSettings, String> {
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
        data_dir: &settings.data_dir,
        worktree_parent: None,
        slug: name,
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
    let run_id_override = unique_interactive_run_id();

    let launch_metadata = launch_metadata_for_mode(settings, &launch_selection);
    let run_dir = settings.session_dir.join(&run_id_override);

    let (live_update_tx, live_update_rx) = live_update_channel();
    if let Some(notice) =
        TuiAuthBackendContext::from_settings(settings).model_prompt_notice(&launch_metadata)
    {
        let _ = live_update_tx.send(notice);
    }
    let (intent_tx, intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let mut shutdown_tx = Some(shutdown_tx);

    let runtime_task = tokio::spawn(run_new_live_runtime(
        settings.clone(),
        demo_mode,
        run_id_override,
        launch_metadata.clone(),
        coordinator_config_warmup,
        Arc::clone(&launch_selection),
        live_update_tx.clone(),
        intent_rx,
        shutdown_rx,
    ));

    let (selected_workflow, ui_intent_sender) = build_live_ui_intent_router(
        intent_tx.clone(),
        live_update_tx,
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
    let prompt_history_path = Some(prompt_history_path_for_session_dir(&settings.session_dir));
    super::set_pending_prompt_commands_from_settings(settings);
    set_pending_live_launch_metadata(launch_metadata);
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

    let data_dir = settings.data_dir.clone();
    let tui_result = tokio::task::spawn_blocking(move || {
        profile_handoff("new_live.live_tui_begin");
        let mut options = new_live_tui_options(
            run_dir,
            Vec::new(),
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
        let _auth_cleanup = super::auth_backend::TuiAuthCleanup;
        run_tui_with_options(options)
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
    launch_selection: LaunchSelection,
    live_update_tx: LiveUpdateSender,
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
        launch_selection,
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

pub(super) struct NewLiveRuntime {
    pub(super) coordinator: CoordinatorHandle,
    pub(super) event_forwarder_task: JoinHandle<Result<(), String>>,
    pub(super) ui_intent_task: JoinHandle<Result<(), String>>,
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
    live_update_tx: LiveUpdateSender,
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
