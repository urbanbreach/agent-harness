// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
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
                attachments,
                launch_metadata,
            } => {
                let agent_id = live_agent_target.as_ref().and_then(|target| {
                    target
                        .lock()
                        .ok()
                        .and_then(|target| target.agent_id.clone())
                });

                if let Some(agent_id) = agent_id {
                    let attachment_metadata = prompt_attachment_metadata(&attachments)?;
                    let request_id = coordinator
                        .request_agent_turn_with_model_and_selected_tags_and_attachments(
                            user_actor.clone(),
                            agent_id,
                            text,
                            harness_core::file_tag::SelectedPromptTags {
                                files: selected_file_tags,
                                agents: selected_agent_tags,
                                resources: selected_resource_tags,
                            },
                            attachment_metadata,
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
                    Ok(ManualCompactionOutcome::Compacted {
                        tokens_before,
                        tokens_after,
                        summary_preview,
                    }) => (
                        manual_compaction_success_message(
                            &summary_preview,
                            tokens_before,
                            tokens_after,
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
            UiIntent::BackgroundForegroundSubagents => {
                let (message, level) = match coordinator.demote_all_foreground_child_tasks().await {
                    Ok(results) => {
                        let summary =
                            harness_core::foreground_demote::summarize_demote_outcomes(&results);
                        if summary.demoted == 0 {
                            (
                                "no foreground subagent is currently blocking this session"
                                    .to_string(),
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
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::DemoteForegroundChildTask { handle_id } => {
                let (message, level) = match coordinator
                    .demote_foreground_child_task(handle_id.clone())
                    .await
                {
                    Ok(result) => match result {
                        harness_core::foreground_demote::DemoteToBackgroundResult::Demoted {
                            background_id,
                            ..
                        } => (
                            format!(
                                "foreground subagent demoted to background ({background_id})"
                            ),
                            OperatorNoticeLevel::Info,
                        ),
                        harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
                            reason,
                            ..
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
            | UiIntent::NewWorktreeSession { .. }
            | UiIntent::SwitchWorktree { .. }
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
            UiIntent::UpdateSessionTitle { title } => {
                let (message, level) = match coordinator.update_session_title(title).await {
                    Ok(_) => (
                        "session title updated".to_string(),
                        OperatorNoticeLevel::Info,
                    ),
                    Err(err) => (
                        format!("failed to update session title: {err}"),
                        OperatorNoticeLevel::Error,
                    ),
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::RevertWorkspace {
                snapshot_request_id,
            } => {
                let (message, level) = match coordinator.revert_workspace(snapshot_request_id).await
                {
                    Ok(summary) => {
                        let mut parts = Vec::new();
                        if !summary.restored_paths.is_empty() {
                            parts.push(format!("{} restored", summary.restored_paths.len()));
                        }
                        if !summary.removed_paths.is_empty() {
                            parts.push(format!("{} removed", summary.removed_paths.len()));
                        }
                        let msg = if parts.is_empty() {
                            "workspace reverted (no paths affected)".to_string()
                        } else {
                            format!("workspace reverted: {}", parts.join(", "))
                        };
                        let lvl = if summary.failed_paths.is_empty() {
                            OperatorNoticeLevel::Info
                        } else {
                            OperatorNoticeLevel::Error
                        };
                        (msg, lvl)
                    }
                    Err(err) => (
                        format!("workspace revert failed: {err}"),
                        OperatorNoticeLevel::Error,
                    ),
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::ExportSession => {}
            UiIntent::ImportForeignSession {
                source_path,
                dest_session_dir,
            } => {
                let (message, level) =
                    match harness_core::foreign_session::import_foreign_session_as_replay(
                        &source_path,
                        &dest_session_dir,
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
                    };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::DeleteSession { run_id, run_dir } => {
                let (message, level) = match delete_session_dir(&run_dir) {
                    Ok(trash_dir) => (
                        format!("session {} moved to {}", run_id, trash_dir.display()),
                        OperatorNoticeLevel::Info,
                    ),
                    Err(err) => (
                        format!("failed to delete session {run_id}: {err}"),
                        OperatorNoticeLevel::Error,
                    ),
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
            UiIntent::RunShellCommand { command } => {
                let actor = harness_core::event::EventActor::new(
                    harness_core::event::ActorKind::User,
                    None,
                );
                let (message, level) = match coordinator
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
                };
                let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
            }
        }
    }
    Ok(())
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

pub(super) fn manual_compaction_success_message(
    summary_preview: &str,
    tokens_before: u32,
    tokens_after: u32,
) -> String {
    let prefix = "manual compaction applied".to_string();
    let token_info = if tokens_before != tokens_after {
        format!(
            " · ctx {} → {} est",
            compact_token_estimate(tokens_before),
            compact_token_estimate(tokens_after)
        )
    } else {
        " · ctx estimate unchanged".to_string()
    };
    let preview = if summary_preview.is_empty() {
        String::new()
    } else {
        format!(" · {summary_preview}")
    };
    format!("{prefix}{token_info}{preview}")
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
