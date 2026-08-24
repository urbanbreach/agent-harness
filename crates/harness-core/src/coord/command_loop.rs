// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::time::Duration;

use tokio::time::MissedTickBehavior;

use super::*;

impl Coordinator {
    pub(in crate::coord) async fn run(mut self) {
        let mut command_channel_closed = false;
        let mut watchdog =
            tokio::time::interval(Duration::from_millis(self.config.watchdog_tick_ms.max(1)));
        watchdog.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            if command_channel_closed {
                if self.run_state.is_some() {
                    let _ = self
                        .stop_run_internal("coordinator command channel closed".to_string())
                        .await;
                } else {
                    break;
                }
            }

            tokio::select! {
                command = self.command_rx.recv(), if !command_channel_closed => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => command_channel_closed = true,
                    }
                }
                command = self.job_rx.recv() => {
                    if let Some(command) = command {
                        self.handle_command(command).await;
                    }
                }
                _ = watchdog.tick(), if self.has_running_tasks() => {
                    if let Err(err) = self.watchdog_tick_internal() {
                        tracing::warn!(error = %err, "coordinator watchdog tick failed");
                    }
                }
            }
        }
    }

    fn has_running_tasks(&self) -> bool {
        self.run_state.as_ref().is_some_and(|run_state| {
            run_state
                .tasks
                .values()
                .any(|task| task.state == TaskExecutionState::Running)
        })
    }

    fn agent_runtime_info_internal(
        &self,
        agent_id: String,
    ) -> Result<AgentRuntimeInfo, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let profile = run_state
            .agents
            .get(&agent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.clone()))?;
        Ok(AgentRuntimeInfo {
            agent_id: agent_id.clone(),
            profile_name: profile.name.clone(),
            model_ref: profile.model_ref.clone(),
            model_ref_explicit: profile.model_ref_explicit,
            toolset: profile.toolset.clone(),
            parent_agent_id: run_state.subagent_parent_by_id.get(&agent_id).cloned(),
        })
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::StartRun {
                run_name,
                workspace_root,
                respond_to,
            } => {
                let result = self
                    .start_run_internal_async(run_name, workspace_root)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "start_run");
            }
            Command::ResumeRun {
                run_id,
                run_name,
                respond_to,
            } => {
                let result = self.resume_run_internal(run_id, run_name);
                warn_oneshot_send_failure(respond_to.send(result), "resume_run");
            }
            Command::StopRun { respond_to } => {
                let result = self.stop_run_internal("run stopped".to_string()).await;
                warn_oneshot_send_failure(respond_to.send(result), "stop_run");
            }
            Command::FailRun { error, respond_to } => {
                let result = self.fail_run_internal(error).await;
                warn_oneshot_send_failure(respond_to.send(result), "fail_run");
            }
            Command::GetEventStore { respond_to } => {
                let result = self.get_event_store_internal();
                warn_oneshot_send_failure(respond_to.send(result), "get_event_store");
            }
            Command::GetRunInfo { respond_to } => {
                let result = self.current_run_info_internal();
                warn_oneshot_send_failure(respond_to.send(result), "get_run_info");
            }
            Command::UpdateSessionTitle { title, respond_to } => {
                let result = self.update_session_title_internal(title);
                warn_oneshot_send_failure(respond_to.send(result), "update_session_title");
            }
            Command::GetAgentRuntimeInfo {
                agent_id,
                respond_to,
            } => {
                let result = self.agent_runtime_info_internal(agent_id);
                warn_oneshot_send_failure(respond_to.send(result), "get_agent_runtime_info");
            }
            Command::SpawnAgent {
                actor,
                profile,
                parent_agent_id,
                child_session_title,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(
                        actor,
                        profile,
                        parent_agent_id,
                        child_session_title,
                        true,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent");
            }
            Command::SpawnAgentIdle {
                actor,
                profile,
                parent_agent_id,
                child_session_title,
                respond_to,
            } => {
                let result = self
                    .spawn_agent_internal(
                        actor,
                        profile,
                        parent_agent_id,
                        child_session_title,
                        false,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "spawn_agent_idle");
            }
            Command::RequestAgentTurn {
                actor,
                agent_id,
                prompt,
                selected_file_tags,
                selected_agent_tags,
                selected_resource_tags,
                attachments,
                model_ref_override,
                model_settings_override,
                model_target_override,
                child_task_metadata,
                respond_to,
            } => {
                let result = self
                    .request_agent_turn_internal(
                        actor,
                        agent_id,
                        prompt,
                        crate::file_tag::SelectedPromptTags {
                            files: selected_file_tags,
                            agents: selected_agent_tags,
                            resources: selected_resource_tags,
                        },
                        attachments,
                        model_ref_override,
                        model_settings_override,
                        model_target_override.map(|target| *target),
                        child_task_metadata,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "request_agent_turn");
            }
            Command::RequestToolCall {
                actor,
                legacy_profile_hint,
                tool_id,
                args_json,
                respond_to,
            } => {
                let result = self
                    .request_tool_call_internal(
                        actor,
                        legacy_profile_hint,
                        tool_id,
                        args_json,
                        None,
                        None,
                    )
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "request_tool_call");
            }
            Command::ExecuteAgentToolCall {
                actor,
                legacy_profile_hint,
                tool_id,
                args_json,
                reserved_tool_call_id,
                respond_to,
            } => {
                let _ = self
                    .request_tool_call_internal(
                        actor,
                        legacy_profile_hint,
                        tool_id,
                        args_json,
                        reserved_tool_call_id,
                        Some(respond_to),
                    )
                    .await;
            }
            Command::RequestQuestion {
                actor,
                tool_call_id,
                request_json,
                respond_to,
            } => {
                let _ = self
                    .request_question_internal(actor, tool_call_id, request_json, respond_to)
                    .await;
            }
            Command::ResolvePermission {
                permission_id,
                decision,
                reason,
                grant_scope,
                respond_to,
            } => {
                let result = self
                    .resolve_permission_internal(permission_id, decision, reason, grant_scope)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "resolve_permission");
            }
            Command::PermissionTimedOut { permission_id } => {
                self.resolve_permission_timeout_internal(permission_id)
                    .await;
            }
            Command::JobProgress { task_id, kind } => {
                self.job_progress_internal(task_id, kind);
            }
            Command::CancelTask {
                task_id,
                reason,
                respond_to,
            } => {
                let result = self.cancel_task_internal(task_id, reason).await;
                warn_oneshot_send_failure(respond_to.send(result), "cancel_task");
            }
            Command::GetBackgroundRequestProjection {
                actor,
                request_id,
                selector_hint,
                respond_to,
            } => {
                let result = self
                    .background_request_projection_internal(actor, request_id, selector_hint)
                    .await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "get_background_request_projection",
                );
            }
            Command::CancelBackgroundRequest {
                actor,
                request_id,
                selector_hint,
                reason,
                respond_to,
            } => {
                let result = self
                    .cancel_background_request_internal(actor, request_id, selector_hint, reason)
                    .await;
                warn_oneshot_send_failure(respond_to.send(result), "cancel_background_request");
            }
            Command::BackgroundForegroundChildTasks { respond_to } => {
                let result = self.background_foreground_child_tasks_internal().await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "background_foreground_child_tasks",
                );
            }
            Command::DemoteForegroundChildTask {
                handle_id,
                respond_to,
            } => {
                let result = self.demote_foreground_child_task_internal(handle_id).await;
                warn_oneshot_send_failure(respond_to.send(result), "demote_foreground_child_task");
            }
            Command::DemoteAllForegroundChildTasks { respond_to } => {
                let result = self.demote_all_foreground_child_tasks_internal().await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "demote_all_foreground_child_tasks",
                );
            }
            Command::JobFinished { task_id, outcome } => {
                let _ = self.job_finished_internal_async(task_id, outcome).await;
            }
            Command::AgentProviderRequestStarted {
                task_id,
                agent_id,
                request_id,
                provider_id,
                model_id,
                prompt_summary,
                request_digest,
                metadata,
                model_target,
            } => {
                let _ = self
                    .agent_provider_request_started_internal(AgentProviderRequestStartedArgs {
                        task_id,
                        agent_id,
                        request_id,
                        provider_id,
                        model_id,
                        prompt_summary,
                        request_digest,
                        metadata,
                        model_target: *model_target,
                    })
                    .await;
            }
            Command::AgentProviderStreamDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            } => {
                let _ = self.agent_provider_live_event_internal(
                    task_id,
                    agent_id,
                    LiveEventV1::ProviderTextDelta {
                        request_id: request_id.into(),
                        delta,
                    },
                );
            }
            Command::AgentProviderReasoningDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            } => {
                let _ = self.agent_provider_live_event_internal(
                    task_id,
                    agent_id,
                    LiveEventV1::ProviderReasoningDelta {
                        request_id: request_id.into(),
                        delta,
                    },
                );
            }
            Command::AgentProviderToolInputDelta {
                task_id,
                agent_id,
                request_id,
                tool_call_id,
                delta,
            } => {
                let _ = self.agent_provider_live_event_internal(
                    task_id,
                    agent_id,
                    LiveEventV1::ProviderToolInputDelta {
                        request_id: request_id.into(),
                        tool_call_id,
                        delta,
                    },
                );
            }
            Command::AgentProviderRequestFinished {
                task_id,
                agent_id,
                request_id,
                finish_reason,
                output_digest,
                usage,
                metadata,
                respond_to,
            } => {
                let result = self
                    .agent_provider_request_finished_internal(AgentProviderRequestFinishedArgs {
                        task_id,
                        agent_id,
                        request_id,
                        finish_reason,
                        output_digest,
                        usage,
                        metadata,
                    })
                    .await;
                if let Some(respond_to) = respond_to {
                    warn_oneshot_send_failure(
                        respond_to.send(result),
                        "agent_provider_request_finished",
                    );
                }
            }
            Command::AgentAssistantMessageFinished {
                task_id,
                agent_id,
                response,
                respond_to,
            } => {
                let result = self
                    .agent_assistant_message_finished_internal(task_id, agent_id, response)
                    .await;
                warn_oneshot_send_failure(
                    respond_to.send(result),
                    "agent_assistant_message_finished",
                );
            }
            Command::AllocateProviderRequestId { respond_to } => {
                let result = self.allocate_provider_request_id_internal();
                warn_oneshot_send_failure(respond_to.send(result), "allocate_provider_request_id");
            }
            Command::CompactAgentContext {
                task_id,
                agent_id,
                request_id,
                trigger_reason,
                evidence,
                respond_to,
            } => {
                let result = self
                    .compact_agent_context_internal(CompactAgentContextRequest {
                        task_id: Some(&task_id),
                        agent_id: &agent_id,
                        through_request_id: Some(request_id),
                        trigger_reason: &trigger_reason,
                        evidence,
                    })
                    .await
                    .map(CompactAgentContextResult::into_context);
                warn_oneshot_send_failure(respond_to.send(result), "compact_agent_context");
            }
            Command::ManualCompactAgentContext {
                agent_id,
                through_request_id,
                trigger_reason,
                respond_to,
            } => {
                let result = self
                    .compact_agent_context_internal(CompactAgentContextRequest {
                        task_id: None,
                        agent_id: &agent_id,
                        through_request_id,
                        trigger_reason: &trigger_reason,
                        evidence: CompactionRequestEvidence::default(),
                    })
                    .await
                    .map(CompactAgentContextResult::into_manual_outcome);
                warn_oneshot_send_failure(respond_to.send(result), "manual_compact_agent_context");
            }
            Command::AgentTurnFinished {
                task_id,
                agent_id,
                request_id,
                outcome,
            } => {
                let _ = self
                    .agent_turn_finished_internal(task_id, agent_id, request_id, outcome)
                    .await;
            }
            Command::SnapshotWorkspace {
                request_id,
                respond_to,
            } => {
                let result = self.snapshot_workspace_internal(request_id).await;
                warn_oneshot_send_failure(respond_to.send(result), "snapshot_workspace");
            }
            Command::RevertWorkspace {
                snapshot_request_id,
                respond_to,
            } => {
                let result = self.revert_workspace_internal(snapshot_request_id).await;
                warn_oneshot_send_failure(respond_to.send(result), "revert_workspace");
            }
            Command::GetPluginLifecycleSummary { respond_to } => {
                let result = self
                    .run_state
                    .as_ref()
                    .map(|rs| rs.plugin_lifecycle.summary())
                    .ok_or(CoordinatorError::RunNotStarted);
                warn_oneshot_send_failure(respond_to.send(result), "get_plugin_lifecycle_summary");
            }
        }
    }
}
