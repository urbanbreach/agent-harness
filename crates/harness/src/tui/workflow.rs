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
    NewWorktreeSession { name: Option<String> },
    SwitchWorktreeSession { worktree_path: PathBuf },
    Continue { run_id: String, run_dir: PathBuf },
    Replay { run_dir: PathBuf },
    Quit,
}

pub(super) type SelectedWorkflow = Arc<Mutex<Option<InteractiveWorkflow>>>;
pub(super) type UiIntentSink = Arc<dyn Fn(UiIntent) + Send + Sync>;
pub(super) type LaunchSelection = Arc<Mutex<LaunchMetadata>>;

pub(super) fn persist_launch_selection_for_exit(
    launch_metadata: &LaunchMetadata,
    config_digest: &str,
) {
    if let Err(err) = save_persisted_model_selection(launch_metadata, config_digest) {
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
    config_digest: &str,
) -> bool {
    let UiIntent::SwitchModel {
        launch_metadata, ..
    } = intent
    else {
        return false;
    };

    record_launch_selection(launch_selection, launch_metadata);
    if persist_model_selection {
        persist_launch_selection_for_exit(&recover_mutex_lock(launch_selection), config_digest);
    }
    true
}

pub(super) async fn run_interactive_workflow_loop<
    LoadStartupEntries,
    StartupRunner,
    NewSessionRunner,
    NewWorktreeSessionRunner,
    ContinueRunner,
    ReplayRunner,
    StartupFuture,
    NewSessionFuture,
    NewWorktreeSessionFuture,
    ContinueFuture,
    ReplayFuture,
>(
    initial_workflow: InteractiveWorkflow,
    mut load_startup_entries: LoadStartupEntries,
    mut run_startup: StartupRunner,
    mut run_new_session: NewSessionRunner,
    mut run_new_worktree_session: NewWorktreeSessionRunner,
    mut run_continue: ContinueRunner,
    mut run_replay: ReplayRunner,
) -> Result<(), String>
where
    LoadStartupEntries: FnMut() -> Result<Vec<SessionHistoryEntry>, String>,
    StartupRunner: FnMut(Vec<SessionHistoryEntry>, Option<String>) -> StartupFuture,
    StartupFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    NewSessionRunner: FnMut() -> NewSessionFuture,
    NewSessionFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    NewWorktreeSessionRunner: FnMut(Option<String>, Option<PathBuf>) -> NewWorktreeSessionFuture,
    NewWorktreeSessionFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    ContinueRunner: FnMut(String, PathBuf) -> ContinueFuture,
    ContinueFuture: Future<Output = Result<InteractiveWorkflow, String>>,
    ReplayRunner: FnMut(PathBuf) -> ReplayFuture,
    ReplayFuture: Future<Output = Result<InteractiveWorkflow, String>>,
{
    let mut workflow = initial_workflow;
    let mut startup_notice = None;
    loop {
        workflow = match workflow {
            InteractiveWorkflow::Startup => {
                run_startup(load_startup_entries()?, startup_notice.take()).await?
            }
            InteractiveWorkflow::NewSession => run_new_session().await?,
            InteractiveWorkflow::NewWorktreeSession { name } => {
                match run_new_worktree_session(name, None).await {
                    Ok(next) => next,
                    Err(error) => {
                        startup_notice = Some(error);
                        InteractiveWorkflow::Startup
                    }
                }
            }
            InteractiveWorkflow::SwitchWorktreeSession { worktree_path } => {
                match run_new_worktree_session(None, Some(worktree_path)).await {
                    Ok(next) => next,
                    Err(error) => {
                        startup_notice = Some(error);
                        InteractiveWorkflow::Startup
                    }
                }
            }
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
        Some(UiIntent::NewWorktreeSession { name }) => {
            InteractiveWorkflow::NewWorktreeSession { name }
        }
        Some(UiIntent::SwitchWorktree { worktree_path }) => {
            InteractiveWorkflow::SwitchWorktreeSession { worktree_path }
        }
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
        | Some(UiIntent::SetAlwaysApproveMode { .. })
        | Some(UiIntent::ResolvePermission { .. })
        | Some(UiIntent::OpenAuthManager { .. })
        | Some(UiIntent::CancelCompaction { .. })
        | Some(UiIntent::CompactSession { .. })
        | Some(UiIntent::BackgroundForegroundSubagents)
        | Some(UiIntent::DemoteForegroundChildTask { .. })
        | Some(UiIntent::InterruptSession { .. })
        | Some(UiIntent::ForkSession { .. })
        | Some(UiIntent::CloneSession { .. })
        | Some(UiIntent::SwitchModel { .. })
        | Some(UiIntent::UpdateSessionTitle { .. })
        | Some(UiIntent::DeleteSession { .. })
        | Some(UiIntent::LoadRewindPoints { .. })
        | Some(UiIntent::RewindConversation { .. })
        | Some(UiIntent::RevertWorkspace { .. })
        | Some(UiIntent::ExportSession)
        | Some(UiIntent::ImportForeignSession { .. })
        | Some(UiIntent::RunShellCommand { .. }) => InteractiveWorkflow::Quit,
    }
}

pub(super) fn build_live_ui_intent_router(
    intent_tx: mpsc::UnboundedSender<UiIntent>,
    launch_selection: LaunchSelection,
    persist_model_selection: bool,
    config_digest: String,
) -> (SelectedWorkflow, UiIntentSink) {
    let selected_workflow = Arc::new(Mutex::new(None::<InteractiveWorkflow>));
    let selected_workflow_sink = Arc::clone(&selected_workflow);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        handle_model_switch_intent(
            &intent,
            &launch_selection,
            persist_model_selection,
            &config_digest,
        );
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
        UiIntent::NewWorktreeSession { name } => {
            Some(InteractiveWorkflow::NewWorktreeSession { name: name.clone() })
        }
        UiIntent::SwitchWorktree { worktree_path } => {
            Some(InteractiveWorkflow::SwitchWorktreeSession {
                worktree_path: worktree_path.clone(),
            })
        }
        UiIntent::ReplaySession { run_dir, .. } => Some(InteractiveWorkflow::Replay {
            run_dir: run_dir.clone(),
        }),
        UiIntent::ContinueSession { run_id, run_dir } => Some(InteractiveWorkflow::Continue {
            run_id: run_id.clone(),
            run_dir: run_dir.clone(),
        }),
        UiIntent::QuitRequested => Some(InteractiveWorkflow::Quit),
        UiIntent::SetAlwaysApproveMode { .. }
        | UiIntent::ResolvePermission { .. }
        | UiIntent::SubmitPrompt { .. }
        | UiIntent::OpenAuthManager { .. }
        | UiIntent::CancelCompaction { .. }
        | UiIntent::CompactSession { .. }
        | UiIntent::BackgroundForegroundSubagents
        | UiIntent::DemoteForegroundChildTask { .. }
        | UiIntent::InterruptSession { .. }
        | UiIntent::ForkSession { .. }
        | UiIntent::CloneSession { .. }
        | UiIntent::SwitchModel { .. }
        | UiIntent::UpdateSessionTitle { .. }
        | UiIntent::DeleteSession { .. }
        | UiIntent::LoadRewindPoints { .. }
        | UiIntent::RewindConversation { .. }
        | UiIntent::RevertWorkspace { .. }
        | UiIntent::ExportSession
        | UiIntent::ImportForeignSession { .. }
        | UiIntent::RunShellCommand { .. } => None,
    }
}

fn forward_intent_to_live_run(intent: &UiIntent) -> bool {
    matches!(
        intent,
        UiIntent::SetAlwaysApproveMode { .. }
            | UiIntent::ResolvePermission { .. }
            | UiIntent::SubmitPrompt { .. }
            | UiIntent::OpenAuthManager { .. }
            | UiIntent::CancelCompaction { .. }
            | UiIntent::CompactSession { .. }
            | UiIntent::BackgroundForegroundSubagents
            | UiIntent::DemoteForegroundChildTask { .. }
            | UiIntent::InterruptSession { .. }
            | UiIntent::ForkSession { .. }
            | UiIntent::CloneSession { .. }
            | UiIntent::SwitchModel { .. }
            | UiIntent::QuitRequested
            | UiIntent::UpdateSessionTitle { .. }
            | UiIntent::DeleteSession { .. }
            | UiIntent::LoadRewindPoints { .. }
            | UiIntent::RewindConversation { .. }
            | UiIntent::RevertWorkspace { .. }
            | UiIntent::ImportForeignSession { .. }
            | UiIntent::RunShellCommand { .. }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::ready;

    #[tokio::test]
    async fn worktree_errors_return_to_the_launcher_with_a_notice() -> Result<(), String> {
        let mut launches = 0;
        let mut attempts = 0;
        run_interactive_workflow_loop(
            InteractiveWorkflow::NewWorktreeSession {
                name: Some("collision".into()),
            },
            || Ok(Vec::new()),
            |_, notice| {
                assert_eq!(notice.as_deref(), Some("worktree unavailable"));
                launches += 1;
                ready(Ok(if launches == 1 {
                    InteractiveWorkflow::SwitchWorktreeSession {
                        worktree_path: PathBuf::from("checkout"),
                    }
                } else {
                    InteractiveWorkflow::Quit
                }))
            },
            || ready(Ok(InteractiveWorkflow::Quit)),
            |name, path| {
                attempts += 1;
                if attempts == 1 {
                    assert_eq!(name.as_deref(), Some("collision"));
                } else {
                    assert_eq!(path, Some(PathBuf::from("checkout")));
                }
                ready(Err("worktree unavailable".into()))
            },
            |_, _| ready(Ok(InteractiveWorkflow::Quit)),
            |_| ready(Ok(InteractiveWorkflow::Quit)),
        )
        .await?;
        assert_eq!((launches, attempts), (2, 2));
        Ok(())
    }
}
