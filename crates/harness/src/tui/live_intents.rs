// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::sync::{Arc, Mutex};

use harness_core::coord::{CoordinatorHandle, ManualCompactionOutcome};
use harness_core::event::{EventActor, EventEnvelopeV1, EventV1};
use harness_tui::{
    CompactionCommandKind, CompactionCommandOutcome, LiveUpdate, LiveUpdateSender,
    OperatorNoticeLevel, UiIntent,
};
use tokio::sync::mpsc;

use super::auth_backend::{spawn_tui_auth_backend_task, TuiAuthBackendContext};
use super::launch_metadata::{
    launch_metadata_model_ref, launch_metadata_model_settings, launch_metadata_model_target,
};
use super::lineage::{materialize_tui_fork_child, materialize_tui_lineage_child};
use super::recover_mutex_lock;
use crate::scenarios::supervisor_actor;

pub(super) type LiveAgentTargetState = Arc<Mutex<LiveAgentTarget>>;

#[derive(Debug, Clone)]
pub(super) struct LiveAgentTarget {
    pub(super) agent_id: Option<String>,
    pub(super) profile: String,
    pub(super) last_request_id: Option<String>,
}

async fn load_rewind_points(
    coordinator: &CoordinatorHandle,
    cancel_task_ids: Vec<String>,
) -> Result<Vec<harness_core::conversation_rewind::RewindPoint>, String> {
    for task_id in cancel_task_ids {
        coordinator
            .cancel_task(task_id, "cancel turn before rewind")
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Some(points) = coordinator
                .rewind_points()
                .await
                .map_err(|error| error.to_string())?
            {
                return Ok(points);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| "The running turn did not stop.".to_string())?
}

async fn send_rewind_points(
    coordinator: CoordinatorHandle,
    updates: LiveUpdateSender,
    generation: u64,
    cancel_task_ids: Vec<String>,
) {
    let result = load_rewind_points(&coordinator, cancel_task_ids).await;
    let _ = updates.send(LiveUpdate::RewindPoints { generation, result });
}

async fn stream_command_output(
    coordinator: CoordinatorHandle,
    actor: EventActor,
    updates: LiveUpdateSender,
    id: String,
) {
    let mut subscription = match coordinator.subscribe_command(actor, id).await {
        Ok(Some(subscription)) => subscription,
        result => {
            let message = match result {
                Err(error) => format!("Command output unavailable: {error}"),
                _ => "Command output is no longer available.".into(),
            };
            let _ = updates.send(LiveUpdate::OperatorNotice {
                message,
                level: OperatorNoticeLevel::Error,
            });
            return;
        }
    };
    let mut snapshot = subscription.snapshot;
    let mut cadence = tokio::time::interval(std::time::Duration::from_millis(33));
    loop {
        let terminal = snapshot.is_terminal();
        if updates
            .send(LiveUpdate::CommandOutput(Box::new(snapshot)))
            .is_err()
            || terminal
        {
            return;
        }
        if subscription.updates.changed().await.is_err() {
            return;
        }
        cadence.tick().await;
        snapshot = subscription.updates.borrow_and_update().clone();
    }
}

async fn change_command_view(
    views: &mut tokio::task::JoinSet<()>,
    coordinator: &CoordinatorHandle,
    actor: &EventActor,
    updates: &LiveUpdateSender,
    task_id: Option<String>,
) {
    views.shutdown().await;
    if let Some(id) = task_id {
        views.spawn(stream_command_output(
            coordinator.clone(),
            actor.clone(),
            updates.clone(),
            id,
        ));
    }
}

async fn request_selected_turn(
    coordinator: &CoordinatorHandle,
    user_actor: &EventActor,
    agent_id: String,
    text: String,
    selected_tags: harness_core::file_tag::SelectedPromptTags,
    attachments: &[harness_tui::composer_integration::SubmissionAttachment],
    launch_metadata: &harness_tui::app::LaunchMetadata,
) -> Result<String, String> {
    let attachment_metadata = prompt_attachment_metadata(attachments)?;
    let result = match launch_metadata_model_target(launch_metadata) {
        Some(target) => {
            coordinator
                .request_agent_turn_with_model_target_and_selected_tags_and_attachments(
                    user_actor.clone(),
                    agent_id,
                    text,
                    selected_tags,
                    attachment_metadata,
                    target,
                )
                .await
        }
        None => {
            coordinator
                .request_agent_turn_with_model_and_selected_tags_and_attachments(
                    user_actor.clone(),
                    agent_id,
                    text,
                    selected_tags,
                    attachment_metadata,
                    launch_metadata_model_ref(launch_metadata),
                    Some(launch_metadata_model_settings(launch_metadata)),
                )
                .await
        }
    };
    result.map_err(|error| error.to_string())
}

/// Steers the agent's running turn, or submits a normal prompt when the coordinator
/// reports the agent idle. Returns the turn the text joined or started. Steering is
/// text-only, so an image-bearing steer goes through the queued-turn submission, which
/// waits behind a running turn and keeps its images.
async fn steer_or_submit(
    coordinator: &CoordinatorHandle,
    user_actor: &EventActor,
    agent_id: String,
    text: String,
    attachments: &[harness_tui::composer_integration::SubmissionAttachment],
    launch_metadata: &harness_tui::app::LaunchMetadata,
) -> Result<String, String> {
    if attachments.is_empty() {
        let outcome = coordinator
            .steer_agent_turn(user_actor.clone(), agent_id.clone(), text.clone())
            .await
            .map_err(|error| error.to_string())?;
        if let harness_core::coord::SteerOutcome::Steering { turn_id, .. } = outcome {
            return Ok(turn_id);
        }
    }
    request_selected_turn(
        coordinator,
        user_actor,
        agent_id,
        text,
        harness_core::file_tag::SelectedPromptTags {
            files: Vec::new(),
            agents: Vec::new(),
            resources: Vec::new(),
        },
        attachments,
        launch_metadata,
    )
    .await
}

/// Starts or steers a turn for a submitted draft and records it for the live agent. A
/// prompt the run refuses goes back into the editor with the reason, instead of ending
/// intent handling and silently dropping every later prompt, interrupt and answer.
async fn send_prompt(
    coordinator: &CoordinatorHandle,
    user_actor: &EventActor,
    target: Option<&LiveAgentTargetState>,
    updates: &LiveUpdateSender,
    intent: UiIntent,
) -> Result<(), String> {
    let (receipt, intent) = match intent {
        UiIntent::QueuedPrompt { id, prompt } => (Some(id), *prompt),
        intent => (None, intent),
    };
    let Some(target) = target else {
        if let Some(id) = receipt {
            let _ = updates.send(LiveUpdate::QueuedPromptReceipt {
                id,
                result: Err("live agent target is unavailable".to_owned()),
            });
        }
        return Ok(());
    };
    let Some(agent_id) = target.lock().ok().and_then(|state| state.agent_id.clone()) else {
        if let Some(id) = receipt {
            let _ = updates.send(LiveUpdate::QueuedPromptReceipt {
                id,
                result: Err("live agent target is unavailable".to_owned()),
            });
        }
        return Ok(());
    };
    let (text, sent) = match intent {
        UiIntent::SubmitPrompt {
            text,
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            attachments,
            launch_metadata,
        }
        | UiIntent::FollowUpPrompt {
            text,
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            attachments,
            launch_metadata,
        } => {
            let selected_tags = harness_core::file_tag::SelectedPromptTags {
                files: selected_file_tags,
                agents: selected_agent_tags,
                resources: selected_resource_tags,
            };
            let sent = request_selected_turn(
                coordinator,
                user_actor,
                agent_id,
                text.clone(),
                selected_tags,
                &attachments,
                &launch_metadata,
            )
            .await;
            (text, sent)
        }
        UiIntent::SteerPrompt {
            text,
            attachments,
            launch_metadata,
        } => {
            let sent = steer_or_submit(
                coordinator,
                user_actor,
                agent_id,
                text.clone(),
                &attachments,
                &launch_metadata,
            )
            .await;
            (text, sent)
        }
        command @ (UiIntent::CompactSession { .. }
        | UiIntent::Handoff { .. }
        | UiIntent::Shake { .. })
            if receipt.is_some() =>
        {
            let result = match manual_compaction_outcome(coordinator, Some(target), command).await {
                Ok((kind, outcome)) => {
                    let result = command_receipt(&outcome);
                    let _ = updates.send(LiveUpdate::CompactionCommand { kind, outcome });
                    result
                }
                Err(message) => Err(message),
            };
            (String::new(), result)
        }
        UiIntent::UpdateSessionTitle { title } if receipt.is_some() => {
            let result = coordinator
                .update_session_title(title)
                .await
                .map(|_| String::new())
                .map_err(|error| error.to_string());
            (String::new(), result)
        }
        _ => {
            if let Some(id) = receipt {
                let _ = updates.send(LiveUpdate::QueuedPromptReceipt {
                    id,
                    result: Err("unsupported queued runtime intent".to_owned()),
                });
            }
            return Ok(());
        }
    };
    if let Some(id) = receipt {
        let result = match sent {
            Ok(request_id) => match target.lock() {
                Ok(mut target) => {
                    if !request_id.is_empty() {
                        target.last_request_id = Some(request_id);
                    }
                    Ok(())
                }
                Err(_) => Err("live agent target lock poisoned".to_owned()),
            },
            Err(error) => Err(error),
        };
        let _ = updates.send(LiveUpdate::QueuedPromptReceipt { id, result });
        return Ok(());
    }
    match sent {
        Ok(request_id) => {
            target
                .lock()
                .map_err(|_| "live agent target lock poisoned".to_string())?
                .last_request_id = Some(request_id);
        }
        Err(error) => {
            let _ = updates.send(LiveUpdate::RestoreQueuedInput(vec![text]));
            let _ = updates.send(LiveUpdate::OperatorNotice {
                message: format!("Message not sent: {error}"),
                level: OperatorNoticeLevel::Error,
            });
        }
    }
    Ok(())
}

pub(super) async fn handle_ui_intents(
    coordinator: CoordinatorHandle,
    mut intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    user_actor: EventActor,
    live_agent_target: Option<LiveAgentTargetState>,
    live_update_tx: LiveUpdateSender,
    auth_backend: TuiAuthBackendContext,
) -> Result<(), String> {
    let mut command_views = tokio::task::JoinSet::new();
    while let Some(intent) = intent_rx.recv().await {
        match intent {
            UiIntent::InspectCommand { task_id } => {
                change_command_view(
                    &mut command_views,
                    &coordinator,
                    &user_actor,
                    &live_update_tx,
                    task_id,
                )
                .await;
            }
            UiIntent::LoadRewindPoints {
                generation,
                cancel_task_ids,
            } => {
                tokio::spawn(send_rewind_points(
                    coordinator.clone(),
                    live_update_tx.clone(),
                    generation,
                    cancel_task_ids,
                ));
            }
            UiIntent::RewindConversation {
                generation,
                request_id,
            } => {
                let result = coordinator
                    .rewind_conversation(request_id)
                    .await
                    .map_err(|error| error.to_string());
                let _ = live_update_tx.send(LiveUpdate::RewindComplete { generation, result });
            }
            UiIntent::SetYoloMode { enabled } => {
                set_yolo_mode(&coordinator, &live_update_tx, enabled).await?;
            }
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
            intent @ (UiIntent::SubmitPrompt { .. }
            | UiIntent::SteerPrompt { .. }
            | UiIntent::FollowUpPrompt { .. }
            | UiIntent::QueuedPrompt { .. }) => {
                send_prompt(
                    &coordinator,
                    &user_actor,
                    live_agent_target.as_ref(),
                    &live_update_tx,
                    intent,
                )
                .await?;
            }
            intent @ (UiIntent::CompactSession { .. }
            | UiIntent::Handoff { .. }
            | UiIntent::Shake { .. }
            | UiIntent::IdleCompaction) => {
                let coordinator = coordinator.clone();
                let target = live_agent_target.clone();
                let updates = live_update_tx.clone();
                tokio::spawn(async move {
                    match manual_compaction_outcome(&coordinator, target.as_ref(), intent).await {
                        Ok((kind, outcome)) => {
                            let _ = updates.send(LiveUpdate::CompactionCommand { kind, outcome });
                        }
                        Err(message) => {
                            let _ = updates.send(LiveUpdate::OperatorNotice {
                                message,
                                level: OperatorNoticeLevel::Error,
                            });
                        }
                    }
                });
            }
            UiIntent::CancelCompaction { agent_id } => {
                coordinator
                    .cancel_compaction(agent_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            UiIntent::BackgroundForegroundSubagents => {
                let (message, level) = background_foreground_notice(&coordinator).await;
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::DemoteForegroundChildTask { handle_id } => {
                let (message, level) =
                    demote_foreground_child_notice(&coordinator, handle_id).await;
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::AuthBackendInput { line } => {
                super::auth_backend::send_tui_auth_backend_input(line)
            }
            UiIntent::OpenAuthManager {
                attempt,
                args,
                stdin,
            } => {
                spawn_tui_auth_backend_task(
                    attempt,
                    Some(coordinator.clone()),
                    args,
                    stdin,
                    auth_backend.clone(),
                    live_update_tx.clone(),
                );
            }
            UiIntent::InterruptSession { task_ids, reason } => {
                interrupt_tasks(
                    &coordinator,
                    &live_update_tx,
                    live_agent_target.as_ref(),
                    task_ids,
                    reason,
                )
                .await;
            }
            intent @ (UiIntent::WithdrawQueuedInput | UiIntent::WithdrawAcceptedInput) => {
                return_queued_input(
                    &coordinator,
                    &live_update_tx,
                    live_agent_target.as_ref(),
                    Withdrawal::for_intent(&intent),
                )
                .await;
            }
            UiIntent::CancelSubagent { session_id } => {
                cancel_child(&coordinator, &user_actor, &live_update_tx, session_id).await;
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
            UiIntent::SwitchModel {
                profile,
                launch_metadata,
            } => {
                let result = select_live_model(
                    &coordinator,
                    live_agent_target.as_ref(),
                    profile,
                    &launch_metadata,
                )
                .await;
                notify_model_switch(result, &auth_backend, &launch_metadata, &live_update_tx);
            }
            UiIntent::NewSession
            | UiIntent::NewWorktreeSession { .. }
            | UiIntent::SwitchWorktree { .. }
            | UiIntent::ReplaySession { .. }
            | UiIntent::ContinueSession { .. } => {}
            UiIntent::QuitRequested => {
                super::stop_live_source_run(&coordinator).await?;
                break;
            }
            UiIntent::UpdateSessionTitle { title } => {
                let (message, level) = update_session_title_notice(&coordinator, title).await;
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::RevertWorkspace {
                snapshot_request_id,
            } => {
                let (message, level) =
                    revert_workspace_notice(&coordinator, snapshot_request_id).await;
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::ExportSession => {}
            UiIntent::ImportForeignSession {
                source_path,
                dest_session_dir,
            } => {
                let (message, level) =
                    import_foreign_session_notice(&source_path, &dest_session_dir);
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::DeleteSession { run_id, run_dir } => {
                let (message, level) = delete_session_notice(&run_id, &run_dir);
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::RunShellCommand { command } => {
                let (message, level) = shell_command_notice(&coordinator, &command).await;
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
        }
    }
    Ok(())
}

async fn cancel_child(
    coordinator: &CoordinatorHandle,
    actor: &EventActor,
    updates: &LiveUpdateSender,
    session_id: String,
) {
    if let Err(error) = coordinator
        .request_subagent_cancel(
            actor.clone(),
            harness_core::subagent::SubagentCommandRequest::ChildSessionCancel {
                session_id,
                descendants: Vec::new(),
            },
        )
        .await
    {
        let _ = updates.send(LiveUpdate::OperatorNotice {
            message: format!("Subagent cancellation failed: {error}"),
            level: OperatorNoticeLevel::Error,
        });
    }
}

async fn update_session_title_notice(
    coordinator: &CoordinatorHandle,
    title: String,
) -> (String, OperatorNoticeLevel) {
    match coordinator.update_session_title(title).await {
        Ok(_) => (
            "session title updated".to_string(),
            OperatorNoticeLevel::Info,
        ),
        Err(err) => (
            format!("failed to update session title: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

async fn shell_command_notice(
    coordinator: &CoordinatorHandle,
    command: &str,
) -> (String, OperatorNoticeLevel) {
    let actor = EventActor::new(harness_core::event::ActorKind::User, None);
    match coordinator
        .request_tool_call(
            actor,
            None,
            "bash",
            serde_json::json!({ "command": command }),
        )
        .await
    {
        Ok(_) => (
            format!("shell command queued: {command}"),
            OperatorNoticeLevel::Info,
        ),
        Err(err) => (
            format!("shell command failed: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

async fn set_yolo_mode(
    coordinator: &CoordinatorHandle,
    live_update_tx: &LiveUpdateSender,
    enabled: bool,
) -> Result<(), String> {
    match coordinator.set_yolo_mode(enabled).await {
        Ok(()) => live_update_tx
            .send(LiveUpdate::YoloModeChanged { enabled })
            .map_err(|err| err.to_string())?,
        Err(error) => {
            live_update_tx
                .send(LiveUpdate::YoloModeChangeFailed)
                .map_err(|err| err.to_string())?;
            let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                message: format!("Failed to change YOLO mode: {error}"),
                level: OperatorNoticeLevel::Error,
            });
        }
    }
    Ok(())
}

fn import_foreign_session_notice(
    source_path: &std::path::Path,
    dest_session_dir: &std::path::Path,
) -> (String, OperatorNoticeLevel) {
    match harness_core::foreign_session::import_foreign_session_as_replay(
        source_path,
        dest_session_dir,
    ) {
        Ok(result) => (
            format!(
                "imported {} events from {}",
                result.event_count,
                result.source_path.display()
            ),
            OperatorNoticeLevel::Info,
        ),
        Err(err) => (
            format!("foreign import failed: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

fn delete_session_notice(run_id: &str, run_dir: &std::path::Path) -> (String, OperatorNoticeLevel) {
    match delete_session_dir(run_dir) {
        Ok(trash_dir) => (
            format!("session {} moved to {}", run_id, trash_dir.display()),
            OperatorNoticeLevel::Info,
        ),
        Err(err) => (
            format!("failed to delete session {run_id}: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

/// What to take back from the agent's queue.
#[derive(Clone, Copy)]
enum Withdrawal {
    /// Undelivered steering and plain queued follow-ups; `report_empty` says so when nothing
    /// was waiting.
    Plain { report_empty: bool },
    /// Everything the user queued, prompts with tags or attachments included: the rollback of
    /// a failed delivery, silent when nothing was accepted.
    Everything,
}

impl Withdrawal {
    fn for_intent(intent: &UiIntent) -> Self {
        match intent {
            UiIntent::WithdrawAcceptedInput => Self::Everything,
            _ => Self::Plain { report_empty: true },
        }
    }
}

/// Puts the agent's queued user input back in the editor, in the order it was sent.
async fn return_queued_input(
    coordinator: &CoordinatorHandle,
    live_update_tx: &LiveUpdateSender,
    live_agent_target: Option<&LiveAgentTargetState>,
    withdrawal: Withdrawal,
) {
    let Some(agent) = live_agent_target.and_then(|target| {
        target
            .lock()
            .ok()
            .and_then(|target| target.agent_id.clone())
    }) else {
        return;
    };
    let (result, report_empty) = match withdrawal {
        Withdrawal::Plain { report_empty } => {
            (coordinator.withdraw_queued_input(agent).await, report_empty)
        }
        Withdrawal::Everything => (coordinator.withdraw_accepted_input(agent).await, false),
    };
    let notice = match result {
        Ok(texts) if texts.is_empty() => report_empty.then(|| {
            (
                "Nothing is waiting to be sent".to_string(),
                OperatorNoticeLevel::Info,
            )
        }),
        Ok(texts) => {
            let _ = live_update_tx.send(LiveUpdate::RestoreQueuedInput(texts));
            None
        }
        Err(err) => Some((
            format!("could not return queued messages: {err}"),
            OperatorNoticeLevel::Error,
        )),
    };
    if let Some((message, level)) = notice {
        let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
    }
}

async fn interrupt_tasks(
    coordinator: &CoordinatorHandle,
    live_update_tx: &LiveUpdateSender,
    live_agent_target: Option<&LiveAgentTargetState>,
    task_ids: Vec<String>,
    reason: harness_tui::app::InterruptReason,
) {
    // A user interrupt returns queued steering and follow-ups to the editor before the
    // turn stops, so none of them starts a turn on its own.
    if reason == harness_tui::app::InterruptReason::User {
        return_queued_input(
            coordinator,
            live_update_tx,
            live_agent_target,
            Withdrawal::Plain {
                report_empty: false,
            },
        )
        .await;
    }
    for task_id in task_ids {
        if let Err(err) = coordinator
            .cancel_task(task_id, reason.coordinator_reason())
            .await
        {
            let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                message: format!("interrupt failed: {err}"),
                level: OperatorNoticeLevel::Error,
            });
        }
    }
}

fn notify_model_switch(
    result: Result<(), String>,
    auth_backend: &TuiAuthBackendContext,
    launch_metadata: &harness_tui::app::LaunchMetadata,
    live_update_tx: &LiveUpdateSender,
) {
    let notice = match result {
        Ok(()) => auth_backend.model_prompt_notice(launch_metadata),
        Err(err) => Some(LiveUpdate::OperatorNotice {
            message: format!("Failed to change model: {err}"),
            level: OperatorNoticeLevel::Error,
        }),
    };
    if let Some(notice) = notice {
        let _ = live_update_tx.send(notice);
    }
}

async fn select_live_model(
    coordinator: &CoordinatorHandle,
    live_agent_target: Option<&LiveAgentTargetState>,
    profile: String,
    launch_metadata: &harness_tui::app::LaunchMetadata,
) -> Result<(), String> {
    switch_live_model(coordinator, live_agent_target, profile).await?;
    announce_selection(coordinator, live_agent_target, launch_metadata).await;
    Ok(())
}

/// Model and reasoning selections fire at the pick, not at the next turn.
async fn announce_selection(
    coordinator: &CoordinatorHandle,
    live_agent_target: Option<&LiveAgentTargetState>,
    launch_metadata: &harness_tui::app::LaunchMetadata,
) {
    let Some(agent) = live_agent_target.and_then(|target| {
        target
            .lock()
            .ok()
            .and_then(|target| target.agent_id.clone())
    }) else {
        return;
    };
    let selection = match launch_metadata_model_target(launch_metadata) {
        Some(target) => Some((target.model_ref.clone(), (&target).into())),
        None => launch_metadata_model_ref(launch_metadata)
            .map(|model_ref| (model_ref, launch_metadata_model_settings(launch_metadata))),
    };
    if let Some((model_ref, settings)) = selection {
        let _ = coordinator
            .announce_model_selection(agent, model_ref, settings)
            .await;
    }
}

async fn switch_live_model(
    coordinator: &CoordinatorHandle,
    live_agent_target: Option<&LiveAgentTargetState>,
    profile: String,
) -> Result<(), String> {
    let Some(live_agent_target) = live_agent_target else {
        return Err("no live agent target".to_string());
    };
    if live_agent_target
        .lock()
        .map_err(|_| "live agent target lock poisoned".to_string())?
        .profile
        == profile
    {
        return Ok(());
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
    Ok(())
}

/// Runs one manual maintenance command and types how it ended. A cancellation or failure
/// is never folded into an info notice: the interface presents both as errors.
async fn manual_compaction_outcome(
    coordinator: &CoordinatorHandle,
    live_agent_target: Option<&LiveAgentTargetState>,
    intent: UiIntent,
) -> Result<(CompactionCommandKind, CompactionCommandOutcome), String> {
    let kind = match &intent {
        UiIntent::Handoff { .. } => CompactionCommandKind::Handoff,
        UiIntent::Shake { .. } => CompactionCommandKind::Shake,
        UiIntent::IdleCompaction => CompactionCommandKind::Idle,
        _ => CompactionCommandKind::Compact,
    };
    let Some(live_agent_target) = live_agent_target else {
        return Ok((
            kind,
            CompactionCommandOutcome::Failed("no live agent target".into()),
        ));
    };
    let (agent_id, through_request_id) = live_agent_target
        .lock()
        .map_err(|_| "live agent target lock poisoned".to_string())
        .map(|target| (target.agent_id.clone(), target.last_request_id.clone()))?;
    let Some(agent_id) = agent_id else {
        return Ok((
            kind,
            CompactionCommandOutcome::Failed("no active live agent".into()),
        ));
    };

    let result = match intent {
        UiIntent::CompactSession { instructions, mode } => {
            coordinator
                .compact_agent_context_with_options(
                    agent_id,
                    harness_core::coord::ManualCompactionOptions {
                        through_request_id,
                        reason: "manual".into(),
                        instructions,
                        internal_guidance: None,
                        mode,
                        suppress_continuation: false,
                    },
                )
                .await
        }
        UiIntent::Handoff { focus } => coordinator.handoff_agent_context(agent_id, focus).await,
        UiIntent::IdleCompaction => coordinator.idle_compact_agent_context(agent_id).await,
        UiIntent::Shake { mode } => {
            return Ok((
                kind,
                match coordinator.shake_agent_context(agent_id, mode).await {
                    Ok(outcome) => shake_outcome(&outcome),
                    Err(error) => CompactionCommandOutcome::Failed(error.to_string()),
                },
            ));
        }
        _ => return Err("Unsupported maintenance intent".into()),
    };
    Ok((
        kind,
        match result {
            Ok(ManualCompactionOutcome::Compacted {
                tokens_after,
                saved_path,
                ..
            }) => CompactionCommandOutcome::Done {
                message: None,
                tokens_after: Some(tokens_after),
                saved_path,
                rebuild: true,
            },
            Ok(ManualCompactionOutcome::NoOp { reason }) => {
                CompactionCommandOutcome::Warning(reason)
            }
            Err(harness_core::coord::CoordinatorError::CompactionRejected(rejection)) => {
                CompactionCommandOutcome::Warning(rejection.to_string())
            }
            Err(harness_core::coord::CoordinatorError::CompactionCancelled { .. }) => {
                CompactionCommandOutcome::Cancelled
            }
            Err(error) => CompactionCommandOutcome::Failed(error.to_string()),
        },
    ))
}

/// `/shake` reports what it dropped; a pass that dropped nothing says so, whichever mode
/// asked, and does not rebuild the transcript.
pub(super) fn shake_outcome(
    outcome: &harness_core::coord::ShakeOutcome,
) -> CompactionCommandOutcome {
    let nothing = outcome.tokens_freed == 0
        && (outcome.summary.starts_with("No images found")
            || outcome.summary.starts_with("No thinking blocks found")
            || outcome.summary == "Nothing to shake.");
    CompactionCommandOutcome::Done {
        message: Some(if nothing {
            "Nothing to shake.".into()
        } else {
            outcome.summary.clone()
        }),
        tokens_after: None,
        saved_path: None,
        rebuild: !nothing,
    }
}

/// The queue's receipt of a command: it only fails when the command did not run to a
/// result the user can act on.
fn command_receipt(outcome: &CompactionCommandOutcome) -> Result<String, String> {
    match outcome {
        CompactionCommandOutcome::Done { .. } | CompactionCommandOutcome::Warning(_) => {
            Ok(String::new())
        }
        CompactionCommandOutcome::Cancelled => Err("cancelled".into()),
        CompactionCommandOutcome::Failed(error) => Err(error.clone()),
    }
}

async fn background_foreground_notice(
    coordinator: &CoordinatorHandle,
) -> (String, OperatorNoticeLevel) {
    match coordinator.demote_all_foreground_child_tasks().await {
        Ok(results) => {
            let summary = harness_core::foreground_demote::summarize_demote_outcomes(&results);
            if summary.demoted == 0 {
                (
                    "no foreground subagent is currently blocking this session".to_string(),
                    OperatorNoticeLevel::Error,
                )
            } else {
                (
                    format!(
                        "{}; {}",
                        foreground_background_success_message(summary.demoted),
                        summary.one_line()
                    ),
                    OperatorNoticeLevel::Info,
                )
            }
        }
        Err(err) => (
            format!("foreground subagent backgrounding failed: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

async fn demote_foreground_child_notice(
    coordinator: &CoordinatorHandle,
    handle_id: String,
) -> (String, OperatorNoticeLevel) {
    match coordinator.demote_foreground_child_task(handle_id).await {
        Ok(result) => match result {
            harness_core::foreground_demote::DemoteToBackgroundResult::Demoted {
                background_id,
                ..
            } => (
                format!("foreground subagent demoted to background ({background_id})"),
                OperatorNoticeLevel::Info,
            ),
            harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                reason, ..
            } => (
                format!("foreground subagent demote rejected: {reason}"),
                OperatorNoticeLevel::Error,
            ),
            harness_core::foreground_demote::DemoteToBackgroundResult::Unavailable {
                reason,
                ..
            } => (
                format!("foreground subagent demote unavailable: {reason}"),
                OperatorNoticeLevel::Error,
            ),
        },
        Err(err) => (
            format!("foreground subagent demote failed: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

async fn revert_workspace_notice(
    coordinator: &CoordinatorHandle,
    snapshot_request_id: String,
) -> (String, OperatorNoticeLevel) {
    match coordinator.revert_workspace(snapshot_request_id).await {
        Ok(summary) => {
            let mut parts = Vec::new();
            if !summary.restored_paths.is_empty() {
                parts.push(format!("{} restored", summary.restored_paths.len()));
            }
            if !summary.removed_paths.is_empty() {
                parts.push(format!("{} removed", summary.removed_paths.len()));
            }
            let message = if parts.is_empty() {
                "workspace reverted (no paths affected)".to_string()
            } else {
                format!("workspace reverted: {}", parts.join(", "))
            };
            let level = if summary.failed_paths.is_empty() {
                OperatorNoticeLevel::Info
            } else {
                OperatorNoticeLevel::Error
            };
            (message, level)
        }
        Err(err) => (
            format!("workspace revert failed: {err}"),
            OperatorNoticeLevel::Error,
        ),
    }
}

fn prompt_attachment_metadata(
    attachments: &[harness_tui::composer_integration::SubmissionAttachment],
) -> Result<Vec<harness_core::attachment_transport::AttachmentMetadata>, String> {
    let payloads = attachments
        .iter()
        .map(|attachment| {
            let metadata = harness_core::attachment_transport::AttachmentMetadata::from_bytes(
                attachment.id.get().to_string(),
                attachment.mime.as_str(),
                None,
                &attachment.bytes,
                None,
            );
            harness_providers::attachment_protocol::AttachmentPayload::new(
                harness_providers::attachment_protocol::AttachmentMetadata::new(
                    metadata.id.clone(),
                    metadata.mime.clone(),
                    metadata.size,
                    None,
                    metadata.content_ref.as_str(),
                ),
                attachment.bytes.clone(),
            )
        })
        .collect::<Vec<_>>();
    harness_providers::attachment_protocol::serialize_attachments(
        &harness_providers::attachment_protocol::AttachmentProtocol::openai(),
        &payloads,
    )
    .map_err(|error| format!("attachment serialization failed: {error}"))?;
    Ok(payloads
        .into_iter()
        .map(|payload| {
            harness_core::attachment_transport::AttachmentMetadata::from_bytes(
                payload.metadata.id,
                payload.metadata.mime,
                None,
                &payload.bytes,
                None,
            )
        })
        .collect())
}

pub(super) fn foreground_background_success_message(count: usize) -> String {
    if count == 1 {
        "foreground subagent moved to background".to_string()
    } else {
        format!("{count} foreground subagents moved to background")
    }
}

/// Move a session run directory to a sibling `trash/` folder.
///
/// This reuses session path safety: the run_dir must be a valid directory,
/// and the trash folder is created as a sibling of the session root.
fn delete_session_dir(run_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let parent = run_dir.parent().ok_or_else(|| {
        format!(
            "cannot determine parent of session dir {}",
            run_dir.display()
        )
    })?;

    let trash_dir = parent.join("trash");
    std::fs::create_dir_all(&trash_dir)
        .map_err(|err| format!("failed to create trash dir {}: {err}", trash_dir.display()))?;

    let run_name = run_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            format!(
                "cannot determine session dir name from {}",
                run_dir.display()
            )
        })?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = trash_dir.join(format!("{run_name}-{timestamp}"));

    std::fs::rename(run_dir, &dest).map_err(|err| {
        format!(
            "failed to move session dir {} to {}: {err}",
            run_dir.display(),
            dest.display()
        )
    })?;

    Ok(dest)
}
