use super::*;

impl Coordinator {
    pub(in crate::coord) async fn compact_agent_context_internal(
        &mut self,
        task_id: Option<&str>,
        agent_id: &str,
        through_request_id: Option<String>,
        trigger_reason: &str,
        usage: Option<harness_providers::CompletionUsage>,
    ) -> Result<CompactAgentContextResult, CoordinatorError> {
        let (existing_context, trigger, hook_context) = {
            let Some(run_state) = self.run_state.as_ref() else {
                return Err(CoordinatorError::RunNotStarted);
            };

            let existing_context = run_state
                .provider_context_by_agent
                .get(agent_id)
                .cloned()
                .unwrap_or_default();
            let manual_tokens_before = (trigger_reason == "manual")
                .then(|| approximate_provider_context_tokens(&existing_context));

            let running_turn = task_id
                .and_then(|task_id| run_state.running_agent_turns.get(task_id))
                .or_else(|| {
                    run_state.running_agent_turns.values().find(|running| {
                        running.agent_id == agent_id
                            && through_request_id
                                .as_deref()
                                .is_none_or(|request_id| running.request_id == request_id)
                    })
                });

            let trigger = if let Some(running) = running_turn {
                let prompt_tokens_estimate = (trigger_reason == "pre_prompt")
                    .then(|| approximate_text_tokens(&running.request_prompt));
                ProviderCompactionTrigger {
                    agent_id: agent_id.to_string(),
                    profile_name: running.profile_name.clone(),
                    model_ref: running.model_ref.clone(),
                    provider_id: running.latest_provider_id.clone(),
                    model_id: running.latest_model_id.clone(),
                    through_request_id,
                    trigger_reason: trigger_reason.to_string(),
                    tokens_before: usage
                        .as_ref()
                        .map(|usage| usage.prompt_tokens)
                        .or(manual_tokens_before),
                    prompt_tokens_estimate,
                    estimate_source: None,
                }
            } else {
                let profile = run_state
                    .agents
                    .get(agent_id)
                    .cloned()
                    .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.to_string()))?;
                ProviderCompactionTrigger {
                    agent_id: agent_id.to_string(),
                    profile_name: profile.name,
                    model_ref: profile.model_ref,
                    provider_id: None,
                    model_id: None,
                    through_request_id,
                    trigger_reason: trigger_reason.to_string(),
                    tokens_before: usage
                        .as_ref()
                        .map(|usage| usage.prompt_tokens)
                        .or(manual_tokens_before),
                    prompt_tokens_estimate: None,
                    estimate_source: None,
                }
            };

            let hook_context = HookInvocationContext {
                event: HookLifecycleEvent::CompactionRequested,
                run_id: run_state.info.run_id.clone(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(agent_id)),
                agent_id: Some(agent_id.to_string()),
                request_id: trigger.through_request_id.clone(),
                permission_id: None,
                task_id: task_id.map(str::to_string),
                tool_call_id: None,
                tool_id: None,
                provider_id: trigger.provider_id.clone(),
                model_id: trigger.model_id.clone(),
                parent_agent_id: run_state.subagent_parent_by_id.get(agent_id).cloned(),
                category: Some(trigger.profile_name.clone()),
                outcome: Some(trigger.trigger_reason.clone()),
                output_summary: trigger.tokens_before.map(|tokens| tokens.to_string()),
                failure_reason: None,
            };

            (existing_context, trigger, hook_context)
        };

        let requested_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            hook_context,
        )
        .await;

        if let Some(reason) = requested_hook_batch.critical_failure {
            let Some(run_state) = self.run_state.as_mut() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            append_compaction_failed_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                &trigger,
                &reason,
                None,
                None,
            )?;
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }
        let summary_override =
            compaction_summary_override_from_hooks(&requested_hook_batch.hook_executions);
        let summary_decision = if let Some(summary) = summary_override {
            CompactionSummaryDecision::hook(summary)
        } else if self.config.compaction.model_backed {
            match self.model_backed_compaction_summary(&trigger).await {
                Ok(summary) => CompactionSummaryDecision::model(
                    compaction_summary_model_ref(&self.config.compaction, &trigger),
                    summary.summary,
                    false,
                    summary.split_prefix_summary,
                ),
                Err(reason) => {
                    tracing::warn!(%reason, agent_id = %trigger.agent_id, "model-backed compaction summary fell back to deterministic summary");
                    CompactionSummaryDecision::model(
                        compaction_summary_model_ref(&self.config.compaction, &trigger),
                        String::new(),
                        true,
                        None,
                    )
                }
            }
        } else {
            CompactionSummaryDecision::deterministic(&trigger)
        };

        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        let updated_context = match compact_provider_context(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            &trigger,
            &self.config.compaction,
            &summary_decision,
        ) {
            Ok(Some(compaction)) => compaction,
            Ok(None) if trigger.trigger_reason == "overflow_retry" => {
                let reason = "overflow retry requested compaction, but no checkpoint reduced the active provider context"
                    .to_string();
                append_compaction_failed_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &trigger,
                    &reason,
                    None,
                    None,
                )?;
                return Err(CoordinatorError::CompactionFailed(reason));
            }
            Ok(None) => {
                return Ok(CompactAgentContextResult::NoOp {
                    context: existing_context,
                })
            }
            Err(err) => {
                let reason = err.to_string();
                let _ = append_compaction_failed_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &trigger,
                    &reason,
                    None,
                    None,
                );
                return Err(err);
            }
        };

        if let ("overflow_retry", Some(task_id), Some(request_id)) = (
            trigger.trigger_reason.as_str(),
            task_id,
            trigger.through_request_id.as_deref(),
        ) {
            run_state.record_overflow_retry_compacted_context(
                task_id,
                request_id,
                updated_context.updated_context.clone(),
            );
        }

        Ok(CompactAgentContextResult::CheckpointWritten {
            context: updated_context.updated_context,
            checkpoint_id: updated_context.checkpoint_id,
            tokens_before_estimate: updated_context.tokens_before_estimate,
            tokens_after_estimate: updated_context.tokens_after_estimate,
        })
    }

    pub(in crate::coord) async fn compact_failed_terminal_agent_context(
        &mut self,
        request: FailedTerminalCompactionRequest,
    ) {
        let should_attempt = {
            let Some(run_state) = self.run_state.as_mut() else {
                return;
            };
            run_state.failed_terminal_compaction_attempt_should_run(&request)
        };
        if !should_attempt {
            return;
        }

        match self
            .compact_agent_context_internal(
                Some(&request.task_id),
                &request.agent_id,
                Some(request.request_id.clone()),
                &request.trigger_reason,
                None,
            )
            .await
        {
            Ok(CompactAgentContextResult::CheckpointWritten { .. })
            | Ok(CompactAgentContextResult::NoOp { .. }) => {}
            Err(err) => {
                tracing::warn!(
                    task_id = %request.task_id,
                    agent_id = %request.agent_id,
                    request_id = %request.request_id,
                    trigger_reason = %request.trigger_reason,
                    error = %err,
                    "failed-terminal provider context compaction did not complete; preserving original task terminal outcome"
                );
            }
        }
    }

    pub(in crate::coord) async fn model_backed_compaction_summary(
        &self,
        trigger: &ProviderCompactionTrigger,
    ) -> Result<ModelBackedCompactionSummary, String> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or_else(|| "run is not started".to_string())?;
        model_backed_compaction_summary_for(
            self.config.provider.clone(),
            &self.config.compaction,
            run_state,
            trigger,
            self.redactor.as_ref(),
        )
        .await
    }

    pub(in crate::coord) async fn promote_next_agent_blocked_turn(
        &mut self,
        agent_id: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };
        if run_state.agent_has_running_turn(agent_id) {
            return Ok(());
        }

        let Some(blocked_task_id) = run_state.next_agent_blocked_turn_id(agent_id) else {
            return Ok(());
        };
        let Some(queued) = run_state.queued_agent_turns.get(&blocked_task_id).cloned() else {
            return Ok(());
        };

        match run_state
            .scheduler
            .schedule(blocked_task_id.clone(), queued.queue_key.clone())
        {
            ScheduleDecision::Started(_) => {
                append_agent_turn_task_scheduled_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    AgentTurnTaskScheduledEventArgs {
                        task_id: &queued.task_id,
                        agent_id: &queued.agent_id,
                        request_id: &queued.request_id,
                        queue_key: &queued.queue_key,
                        state: TaskScheduleState::Started,
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(&blocked_task_id) else {
                    return Ok(());
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.config.hook_command_executor.clone(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )
                .await?;
            }
            ScheduleDecision::Queued(_) => {
                run_state.mark_queued_agent_turn_scheduler_queued(&blocked_task_id);
            }
        }

        Ok(())
    }

    pub(in crate::coord) async fn agent_turn_finished_internal(
        &mut self,
        task_id: String,
        _agent_id: String,
        request_id: String,
        outcome: AgentTurnTaskOutcome,
    ) -> Result<(), CoordinatorError> {
        let (dequeued, terminal_compaction, finished_agent_id) = {
            let Some(run_state) = self.run_state.as_mut() else {
                return Ok(());
            };

            let Some(running) = run_state.running_agent_turns.remove(&task_id) else {
                return Ok(());
            };

            let finished_agent_id = running.agent_id.clone();
            let was_cancelled = run_state.cancelled_running_tasks.remove(&task_id);
            let dequeued = run_state.scheduler.complete(&running.queue_key);
            let finished_mono_ms = self.clock.mono_ms();
            let subagent_parent_id = run_state
                .subagent_parent_by_id
                .get(&running.agent_id)
                .cloned();
            let (hook_outcome, hook_output_summary, hook_failure_reason) = match &outcome {
                AgentTurnTaskOutcome::Succeeded { output, .. } => {
                    ("succeeded".to_string(), Some(output.clone()), None)
                }
                AgentTurnTaskOutcome::Failed { reason, .. } => {
                    ("failed".to_string(), None, Some(reason.clone()))
                }
            };
            let finished_hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::AgentTurnFinished,
                    run_id: run_state.info.run_id.clone(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(agent_actor(&running.agent_id)),
                    agent_id: Some(running.agent_id.clone()),
                    request_id: Some(request_id.clone()),
                    permission_id: None,
                    task_id: Some(task_id.clone()),
                    tool_call_id: None,
                    tool_id: None,
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    category: running.category.clone(),
                    outcome: Some(hook_outcome.clone()),
                    output_summary: hook_output_summary.clone(),
                    failure_reason: hook_failure_reason.clone(),
                },
            )
            .await;
            let mut hook_executions = running.hook_executions.clone();
            hook_executions.extend(finished_hook_batch.hook_executions.clone());
            let mut critical_hook_failure = finished_hook_batch.critical_failure.clone();

            if let Some(parent_agent_id) = subagent_parent_id {
                let subagent_finished_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::SubagentFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(agent_actor(&running.agent_id)),
                        agent_id: Some(running.agent_id.clone()),
                        request_id: Some(request_id.clone()),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: None,
                        tool_id: None,
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: Some(parent_agent_id),
                        category: running.category.clone(),
                        outcome: Some(hook_outcome),
                        output_summary: hook_output_summary,
                        failure_reason: hook_failure_reason,
                    },
                )
                .await;
                hook_executions.extend(subagent_finished_hook_batch.hook_executions.clone());
                if let Some(reason) = subagent_finished_hook_batch.critical_failure {
                    critical_hook_failure = Some(match critical_hook_failure {
                        Some(existing) => format!("{existing}; {reason}"),
                        None => reason,
                    });
                }
            }

            let mut terminal_compaction = None;

            if was_cancelled {
                let memory = match &outcome {
                    AgentTurnTaskOutcome::Failed { reason, memory } => memory
                        .clone()
                        .or_else(|| cancelled_failure_memory_from_running(&running, reason)),
                    AgentTurnTaskOutcome::Succeeded { .. } => {
                        cancelled_failure_memory_from_running(&running, "job cancelled")
                    }
                };
                let has_incomplete_memory = memory.is_some();
                if let Some(memory) = memory {
                    push_incomplete_provider_turn(run_state, &running, &request_id, memory);
                }
                if has_incomplete_memory {
                    terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                        task_id.clone(),
                        running.agent_id.clone(),
                        request_id.clone(),
                        "aborted_response",
                    ));
                }
            } else {
                match outcome {
                    AgentTurnTaskOutcome::Succeeded { output, messages } => {
                        if let Some(reason) = critical_hook_failure.clone() {
                            push_incomplete_provider_turn(
                                run_state,
                                &running,
                                &request_id,
                                AgentTurnFailureMemory::failed(
                                    "hook_failure",
                                    reason.clone(),
                                    output.clone(),
                                    running.latest_provider_request_id.clone(),
                                ),
                            );
                            let terminal_event = append_payload_event_with_correlation(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                agent_actor(&running.agent_id),
                                Some(format!("task:{task_id}")),
                                Some(request_id.clone()),
                                EventV1::TaskCancelled(TaskCancelledEvent {
                                    task_id: task_id.clone(),
                                    reason,
                                    task_scope: Some(TaskTerminalScope::AgentTurn),
                                }),
                            )?;
                            append_background_task_notification_and_schedule(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                self.config.hook_command_executor.clone(),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider.clone(),
                                self.config.tool_registry.clone(),
                                running.child_task.clone(),
                                &terminal_event,
                                BackgroundTaskNotificationStatus::Failed,
                                &terminal_event_summary(&terminal_event),
                            )
                            .await?;
                            terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                                task_id.clone(),
                                running.agent_id.clone(),
                                request_id.clone(),
                                "failed_response",
                            ));
                        } else {
                            let lineage =
                                agent_turn_child_lineage(run_state, &running, &request_id);
                            run_state
                                .provider_context_by_agent
                                .entry(running.agent_id.clone())
                                .or_default()
                                .push_turn(ProviderConversationTurn {
                                    user_prompt: running.request_prompt.clone(),
                                    assistant_response: output.clone(),
                                    request_id: Some(request_id.clone()),
                                    first_seq: None,
                                    last_seq: None,
                                    artifacts: Vec::new(),
                                    messages,
                                    ..ProviderConversationTurn::default()
                                });
                            let terminal_event = append_payload_event_with_correlation(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                agent_actor(&running.agent_id),
                                Some(format!("task:{task_id}")),
                                Some(request_id.clone()),
                                EventV1::TaskCompleted(TaskCompletedEvent {
                                    task_id,
                                    result_digest: digest12(output.as_bytes()),
                                    result_summary: output,
                                    metadata: Some(TaskCompletionMetadata {
                                        lineage,
                                        task_scope: Some(TaskTerminalScope::AgentTurn),
                                        timing: Some(execution_timing_metadata(
                                            running.started_mono_ms,
                                            finished_mono_ms,
                                        )),
                                        hook_executions,
                                    }),
                                }),
                            )?;
                            append_background_task_notification_and_schedule(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                self.config.hook_command_executor.clone(),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider.clone(),
                                self.config.tool_registry.clone(),
                                running.child_task.clone(),
                                &terminal_event,
                                BackgroundTaskNotificationStatus::Completed,
                                &terminal_event_summary(&terminal_event),
                            )
                            .await?;

                            let proactive_trigger = ProviderCompactionTrigger {
                                agent_id: running.agent_id.clone(),
                                profile_name: running.profile_name.clone(),
                                model_ref: running.model_ref.clone(),
                                provider_id: running.latest_provider_id.clone(),
                                model_id: running.latest_model_id.clone(),
                                through_request_id: Some(request_id.clone()),
                                trigger_reason: "proactive".to_string(),
                                tokens_before: running
                                    .latest_provider_usage
                                    .as_ref()
                                    .map(|usage| usage.prompt_tokens),
                                prompt_tokens_estimate: None,
                                estimate_source: None,
                            };
                            let summary_decision = if self.config.compaction.model_backed {
                                match model_backed_compaction_summary_for(
                                    self.config.provider.clone(),
                                    &self.config.compaction,
                                    run_state,
                                    &proactive_trigger,
                                    self.redactor.as_ref(),
                                )
                                .await
                                {
                                    Ok(summary) => CompactionSummaryDecision::model(
                                        compaction_summary_model_ref(
                                            &self.config.compaction,
                                            &proactive_trigger,
                                        ),
                                        summary.summary,
                                        false,
                                        summary.split_prefix_summary,
                                    ),
                                    Err(reason) => {
                                        tracing::warn!(%reason, agent_id = %running.agent_id, "model-backed proactive compaction summary fell back to deterministic summary");
                                        CompactionSummaryDecision::model(
                                            compaction_summary_model_ref(
                                                &self.config.compaction,
                                                &proactive_trigger,
                                            ),
                                            String::new(),
                                            true,
                                            None,
                                        )
                                    }
                                }
                            } else {
                                CompactionSummaryDecision::deterministic(&proactive_trigger)
                            };
                            if let Err(err) = compact_provider_context(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                run_state,
                                &proactive_trigger,
                                &self.config.compaction,
                                &summary_decision,
                            ) {
                                tracing::warn!(
                                    agent_id = %running.agent_id,
                                    error = %err,
                                    "provider context compaction failed after successful agent turn"
                                );
                            }
                        }
                    }
                    AgentTurnTaskOutcome::Failed { reason, memory } => {
                        let reason = match critical_hook_failure.clone() {
                            Some(hook_reason) => {
                                format!("{reason}; critical lifecycle hook failed: {hook_reason}")
                            }
                            None => reason,
                        };
                        let mut memory = memory.or_else(|| {
                            critical_hook_failure.clone().map(|_| {
                                AgentTurnFailureMemory::failed(
                                    "hook_failure",
                                    reason.clone(),
                                    "",
                                    running.latest_provider_request_id.clone(),
                                )
                            })
                        });
                        if let Some(memory) = &mut memory {
                            memory.failure_reason = reason.clone();
                        }
                        let terminal_trigger_reason = memory
                            .as_ref()
                            .filter(|memory| {
                                memory.status == ProviderConversationTurnStatus::Aborted
                            })
                            .map(|_| "aborted_response")
                            .unwrap_or("failed_response");
                        let has_incomplete_memory = memory.is_some();
                        if let Some(memory) = memory {
                            push_incomplete_provider_turn(run_state, &running, &request_id, memory);
                        }
                        let terminal_event = append_payload_event_with_correlation(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            run_state,
                            agent_actor(&running.agent_id),
                            Some(format!("task:{task_id}")),
                            Some(request_id.clone()),
                            EventV1::TaskCancelled(TaskCancelledEvent {
                                task_id: task_id.clone(),
                                reason: reason.clone(),
                                task_scope: Some(TaskTerminalScope::AgentTurn),
                            }),
                        )?;
                        append_background_task_notification_and_schedule(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            self.config.hook_command_executor.clone(),
                            self.job_tx.clone(),
                            run_state,
                            self.config.hook_runtime_config.clone(),
                            self.config.compaction.clone(),
                            self.config.provider.clone(),
                            self.config.tool_registry.clone(),
                            running.child_task.clone(),
                            &terminal_event,
                            background_notification_status_for_cancel_reason(&reason),
                            &reason,
                        )
                        .await?;
                        if has_incomplete_memory {
                            terminal_compaction = Some(FailedTerminalCompactionRequest::new(
                                task_id.clone(),
                                running.agent_id.clone(),
                                request_id.clone(),
                                terminal_trigger_reason,
                            ));
                        }
                    }
                }
            }

            (dequeued, terminal_compaction, finished_agent_id)
        };

        if let Some(request) = terminal_compaction {
            self.compact_failed_terminal_agent_context(request).await;
        }

        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        for task in dequeued {
            if let Some(queued) = run_state.queued_agent_turns.get(&task.task_id).cloned() {
                append_agent_turn_task_scheduled_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    AgentTurnTaskScheduledEventArgs {
                        task_id: &queued.task_id,
                        agent_id: &queued.agent_id,
                        request_id: &queued.request_id,
                        queue_key: &queued.queue_key,
                        state: TaskScheduleState::Started,
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(&task.task_id) else {
                    continue;
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    self.config.hook_command_executor.clone(),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider.clone(),
                    self.config.tool_registry.clone(),
                    queued,
                )
                .await?;
            }
        }

        schedule_pending_agent_wakeups_for_idle_agent(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.config.hook_command_executor.clone(),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            self.config.compaction.clone(),
            self.config.provider.clone(),
            self.config.tool_registry.clone(),
            &finished_agent_id,
        )
        .await?;

        self.promote_next_agent_blocked_turn(&finished_agent_id)
            .await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct AppliedCompaction {
    pub(in crate::coord) updated_context: ProviderContext,
    checkpoint_id: String,
    tokens_before_estimate: Option<u32>,
    tokens_after_estimate: Option<u32>,
}

#[derive(Debug, Clone)]
pub(in crate::coord) enum CompactAgentContextResult {
    CheckpointWritten {
        context: ProviderContext,
        checkpoint_id: String,
        tokens_before_estimate: Option<u32>,
        tokens_after_estimate: Option<u32>,
    },
    NoOp {
        context: ProviderContext,
    },
}

impl CompactAgentContextResult {
    pub(in crate::coord) fn into_context(self) -> ProviderContext {
        match self {
            Self::CheckpointWritten { context, .. } | Self::NoOp { context } => context,
        }
    }

    pub(in crate::coord) fn into_manual_outcome(self) -> ManualCompactionOutcome {
        match self {
            Self::CheckpointWritten {
                checkpoint_id,
                tokens_before_estimate,
                tokens_after_estimate,
                ..
            } => ManualCompactionOutcome::CheckpointWritten {
                checkpoint_id,
                tokens_before_estimate,
                tokens_after_estimate,
            },
            Self::NoOp { .. } => ManualCompactionOutcome::NoOp,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct FailedTerminalCompactionRequest {
    pub(in crate::coord) task_id: String,
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) trigger_reason: String,
}

impl FailedTerminalCompactionRequest {
    pub(in crate::coord) fn new(
        task_id: impl Into<String>,
        agent_id: impl Into<String>,
        request_id: impl Into<String>,
        trigger_reason: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            agent_id: agent_id.into(),
            request_id: request_id.into(),
            trigger_reason: trigger_reason.into(),
        }
    }

    pub(in crate::coord) fn attempt_key(&self) -> (String, String) {
        (self.task_id.clone(), self.request_id.clone())
    }
}

pub(in crate::coord) fn compact_provider_context<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
    summary_decision: &CompactionSummaryDecision,
) -> Result<Option<AppliedCompaction>, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let Some(decision) = ProviderContextCompactionRequest::new(
        run_state,
        trigger.clone(),
        compaction_config,
        summary_decision,
    )
    .plan(redactor) else {
        return Ok(None);
    };
    let trigger = decision.trigger;
    let checkpoint = decision.checkpoint;
    let checkpoint_id = checkpoint.metadata.checkpoint_id.clone();
    let updated_context = decision.updated_context;
    let tokens_before_estimate = decision.tokens_before_estimate;
    let updated_tokens = decision.tokens_after_estimate;

    if trigger.trigger_reason != "manual" && updated_tokens >= tokens_before_estimate {
        if matches!(
            trigger.trigger_reason.as_str(),
            "pre_prompt" | "failed_response"
        ) {
            let reason = if trigger.trigger_reason == "pre_prompt" {
                format!(
                    "pre-prompt compaction did not reduce estimated provider context: before={tokens_before_estimate}, after={updated_tokens}"
                )
            } else {
                format!(
                    "failed-response compaction did not reduce estimated provider context: before={tokens_before_estimate}, after={updated_tokens}"
                )
            };
            append_compaction_failed_event(
                clock,
                redactor,
                run_state,
                &trigger,
                &reason,
                Some(checkpoint.metadata.checkpoint_id.clone()),
                Some(checkpoint.metadata.through_seq),
            )?;
        }
        return Ok(None);
    }

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionRequested(CompactionRequestedEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: checkpoint.metadata.agent_id.clone(),
            trigger_reason: trigger.trigger_reason.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            provider_id: checkpoint.metadata.provider_id.clone(),
            model_id: checkpoint.metadata.model_id.clone(),
            tokens_before: checkpoint.metadata.tokens_before,
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            estimate_source: trigger.estimate_source.clone(),
        }),
    )?;

    let body =
        serialize_provider_context_checkpoint(&checkpoint, trigger.estimate_source.as_deref())?;
    let artifact_store = crate::tool::ArtifactStore::new(run_state.info.artifacts_dir.clone())
        .map_err(|err| CoordinatorError::ResumeRestoreFailed {
            run_id: run_state.info.run_id.clone(),
            reason: format!("failed to open compaction artifact store: {err}"),
        })?;
    let artifact_name = format!(
        "compactions/{}/{}.json",
        trigger.agent_id, checkpoint.metadata.checkpoint_id
    );
    let artifact = artifact_store
        .write_text(&artifact_name, &body)
        .map_err(|err| CoordinatorError::ResumeRestoreFailed {
            run_id: run_state.info.run_id.clone(),
            reason: format!("failed to write compaction checkpoint artifact: {err}"),
        })?;
    append_compaction_artifact_written_event(clock, redactor, run_state, &checkpoint, &artifact)?;
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: checkpoint.metadata.agent_id.clone(),
            artifact_path: artifact.path.clone(),
            artifact_digest: artifact.digest.clone(),
            artifact_bytes: body.len() as u64,
            trigger_reason: trigger.trigger_reason.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            provider_id: checkpoint.metadata.provider_id.clone(),
            model_id: checkpoint.metadata.model_id.clone(),
            tokens_before: checkpoint.metadata.tokens_before,
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
            summary_tokens_estimate: checkpoint.metadata.summary_tokens_estimate,
            compacted_turns: checkpoint.metadata.compacted_turns,
            reduction_tokens_estimate: checkpoint.metadata.reduction_tokens_estimate,
            reduction_percent_estimate: checkpoint.metadata.reduction_percent_estimate,
            estimate_source: trigger.estimate_source.clone(),
            summary_source: checkpoint.summary_source.clone(),
            preserved_turns: checkpoint.recent_turns.len() as u32,
        }),
    )?;

    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("compaction:{}", trigger.agent_id)),
        EventV1::CompactionApplied(CompactionAppliedEvent {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: trigger.agent_id.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
            tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
            summary_tokens_estimate: checkpoint.metadata.summary_tokens_estimate,
            compacted_turns: checkpoint.metadata.compacted_turns,
            preserved_turns: checkpoint.metadata.preserved_turns,
            reduction_tokens_estimate: checkpoint.metadata.reduction_tokens_estimate,
            reduction_percent_estimate: checkpoint.metadata.reduction_percent_estimate,
            estimate_source: trigger.estimate_source.clone(),
        }),
    )?;

    run_state
        .provider_context_by_agent
        .insert(trigger.agent_id.clone(), updated_context.clone());

    Ok(Some(AppliedCompaction {
        updated_context,
        checkpoint_id,
        tokens_before_estimate: checkpoint.metadata.tokens_before_estimate,
        tokens_after_estimate: checkpoint.metadata.tokens_after_estimate,
    }))
}
