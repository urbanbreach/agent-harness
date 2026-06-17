use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use harness_tui::app::{set_pending_live_prompt_auto_submit, LaunchMetadata, SessionHistoryEntry};
use harness_tui::UiIntent;
use tokio::sync::mpsc;

use super::model_selection::save_persisted_model_selection;
use super::profile_log::profile_handoff;
use super::recover_mutex_lock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InteractiveWorkflow {
    Startup,
    NewSession,
    Continue { run_id: String, run_dir: PathBuf },
    Replay { run_dir: PathBuf },
    Quit,
}

pub(super) type SelectedWorkflow = Arc<Mutex<Option<InteractiveWorkflow>>>;
pub(super) type UiIntentSink = Arc<dyn Fn(UiIntent) + Send + Sync>;
pub(super) type LaunchSelection = Arc<Mutex<LaunchMetadata>>;

pub(super) fn persist_launch_selection_for_exit(launch_metadata: &LaunchMetadata) {
    if let Err(err) = save_persisted_model_selection(launch_metadata) {
        profile_handoff(&format!("model_selection.persist_failed {err}"));
    }
}

fn record_launch_selection(selection: &LaunchSelection, launch_metadata: &LaunchMetadata) {
    let launch_metadata = launch_metadata.clone().without_mode_label();
    *recover_mutex_lock(selection) = launch_metadata.clone();
}

pub(super) fn handle_model_switch_intent(
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

pub(super) async fn run_interactive_workflow_loop<
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

pub(super) fn map_startup_intent_to_workflow(intent: Option<UiIntent>) -> InteractiveWorkflow {
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
        | Some(UiIntent::SwitchModel { .. })
        | Some(UiIntent::UpdateSessionTitle { .. })
        | Some(UiIntent::RevertWorkspace { .. }) => InteractiveWorkflow::Quit,
    }
}

pub(super) fn build_live_ui_intent_router(
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

pub(super) fn live_workflow_from_intent(intent: &UiIntent) -> Option<InteractiveWorkflow> {
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
        | UiIntent::SwitchModel { .. }
        | UiIntent::UpdateSessionTitle { .. }
        | UiIntent::RevertWorkspace { .. } => None,
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
            | UiIntent::UpdateSessionTitle { .. }
            | UiIntent::RevertWorkspace { .. }
    )
}

pub(super) fn capture_first_workflow(
    selected_workflow: &SelectedWorkflow,
    workflow: InteractiveWorkflow,
) {
    if let Ok(mut slot) = selected_workflow.lock() {
        if slot.is_none() {
            *slot = Some(workflow);
        }
    }
}

pub(super) fn take_selected_workflow(
    selected_workflow: &SelectedWorkflow,
) -> Result<InteractiveWorkflow, String> {
    take_selected_workflow_or(selected_workflow, InteractiveWorkflow::Quit)
}

pub(super) fn take_selected_workflow_or(
    selected_workflow: &SelectedWorkflow,
    default: InteractiveWorkflow,
) -> Result<InteractiveWorkflow, String> {
    selected_workflow
        .lock()
        .map_err(|_| "live workflow selection lock poisoned".to_string())
        .map(|mut slot| slot.take().unwrap_or(default))
}
