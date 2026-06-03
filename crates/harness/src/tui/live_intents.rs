use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};

use harness_core::coord::{CoordinatorError, CoordinatorHandle, ManualCompactionOutcome};
use harness_core::event::{EventActor, EventEnvelopeV1, EventV1};
use harness_tui::{LiveUpdate, OperatorNoticeLevel, UiIntent};
use tokio::sync::mpsc;

use super::auth_backend::{spawn_tui_auth_backend_task, TuiAuthBackendContext};
use super::launch_metadata::{launch_metadata_model_ref, launch_metadata_model_settings};
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

pub(super) fn maybe_update_live_agent_target_for_plan_handoff(
    event: &EventEnvelopeV1,
    live_agent_target: Option<&LiveAgentTargetState>,
) {
    let Some(live_agent_target) = live_agent_target else {
        return;
    };
    let EventV1::AgentSpawned(payload) = &event.payload else {
        return;
    };
    if payload.profile != harness_core::plan::BUILD_AGENT_NAME {
        return;
    }

    let mut target = recover_mutex_lock(live_agent_target);
    if target.profile != harness_core::plan::PLAN_AGENT_NAME {
        return;
    }
    if payload.parent_agent_id.as_deref() != target.agent_id.as_deref() {
        return;
    }

    target.agent_id = Some(payload.agent_id.clone());
    target.profile = payload.profile.clone();
    target.last_request_id = None;
}

pub(super) async fn handle_ui_intents(
    coordinator: CoordinatorHandle,
    mut intent_rx: mpsc::UnboundedReceiver<UiIntent>,
    user_actor: EventActor,
    live_agent_target: Option<LiveAgentTargetState>,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    auth_backend: TuiAuthBackendContext,
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
                selected_file_tags,
                selected_agent_tags,
                selected_resource_tags,
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
                        .request_agent_turn_with_model_and_selected_tags(
                            user_actor.clone(),
                            agent_id,
                            text,
                            harness_core::file_tag::SelectedPromptTags {
                                files: selected_file_tags,
                                agents: selected_agent_tags,
                                resources: selected_resource_tags,
                            },
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
            UiIntent::OpenAuthManager { args, stdin } => {
                spawn_tui_auth_backend_task(
                    args,
                    stdin,
                    auth_backend.config_path.clone(),
                    auth_backend.session_dir.clone(),
                    auth_backend.workspace_root.clone(),
                    live_update_tx.clone(),
                );
            }
            UiIntent::InterruptSession { task_ids } => {
                for task_id in task_ids {
                    if let Err(err) = coordinator.cancel_task(task_id, "interrupted").await {
                        let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                            message: format!("interrupt failed: {err}"),
                            level: OperatorNoticeLevel::Error,
                        });
                    }
                }
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

pub(super) fn manual_compaction_success_message(
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
