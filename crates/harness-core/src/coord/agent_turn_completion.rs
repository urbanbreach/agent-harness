// allow: SIZE_OK — coordinator turn completion state machine (lifecycle phases)
use super::compaction::{
    collect_entries_for_branch_summary, generate_branch_summary, GenerateBranchSummaryOptions,
};
use super::*;

impl Coordinator {
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

        let trigger_reason = request.trigger_reason.clone();
        self.start_compaction_generation(
            CompactAgentContextRequest {
                task_id: Some(request.task_id),
                agent_id: request.agent_id,
                through_request_id: Some(request.request_id),
                trigger_reason: trigger_reason.clone(),
                evidence: CompactionRequestEvidence::default(),
            },
            PendingCompactionResponse::Internal { trigger_reason },
        )
        .await;
    }

    pub(in crate::coord) async fn summarize_session_branch(
        &mut self,
        agent_id: &str,
        old_leaf_seq: u64,
        target_seq: u64,
    ) -> Result<BranchSummaryOutcome, CoordinatorError> {
        let (events, model_ref) = {
            let Some(run_state) = self.run_state.as_ref() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            let stream = run_state.event_store.replay(1)?;
            let mut events = Vec::new();
            let mut stream = std::pin::pin!(stream);
            while let Some(result) = stream.next().await {
                events.push(result?);
            }
            let model_ref = run_state
                .running_agent_turns
                .values()
                .find(|r| r.agent_id == agent_id)
                .map(|r| r.model_ref.clone())
                .or_else(|| run_state.agents.get(agent_id).map(|p| p.model_ref.clone()))
                .unwrap_or_else(|| "default:default".to_string());
            (events, model_ref)
        };

        let collected = collect_entries_for_branch_summary(
            &events,
            agent_id,
            Some(target_seq.min(old_leaf_seq)),
        );
        if collected.entries.is_empty() {
            return Ok(BranchSummaryOutcome::NoOp);
        }

        let model = crate::agent::AgentModelRef::parse(&model_ref);
        let Some(context_window) = self
            .run_state
            .as_ref()
            .and_then(|state| state.recorded_runtime_context.as_ref())
            .and_then(|context| context.model_limits.context_window_tokens())
        else {
            // Unknown limits cannot support an exact branch-summary token budget.
            return Ok(BranchSummaryOutcome::NoOp);
        };

        let options = GenerateBranchSummaryOptions {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            context_window,
            ..Default::default()
        };

        let result =
            generate_branch_summary(self.config.provider.as_ref(), &collected.entries, &options)
                .await;

        if result.aborted || result.summary.is_none() {
            if let Some(err) = result.error {
                tracing::warn!(%err, %agent_id, "branch summary generation failed");
            }
            return Ok(BranchSummaryOutcome::NoOp);
        }

        let summary = result.summary.unwrap_or_default();
        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(format!("branch-summary:{agent_id}")),
            EventV1::BranchSummary(crate::event::BranchSummaryEvent {
                agent_id: agent_id.to_string(),
                summary: summary.clone(),
                from_event_seq: old_leaf_seq,
                read_files: result.read_files.clone(),
                modified_files: result.modified_files.clone(),
                from_hook: false,
            }),
        )?;

        Ok(BranchSummaryOutcome::Generated {
            summary_preview: summary_preview(&summary),
            read_files: result.read_files,
            modified_files: result.modified_files,
        })
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
                        child_task: queued.child_task.as_ref(),
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(&blocked_task_id) else {
                    return Ok(());
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    Arc::clone(&self.config.hook_command_executor),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider_retry,
                    Arc::clone(&self.config.provider),
                    Arc::clone(&self.config.tool_registry),
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
                    run_id: run_state.info.run_id.to_string(),
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
                    profile: running.profile.clone(),
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
                        run_id: run_state.info.run_id.to_string(),
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
                        profile: running.profile.clone(),
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
                                    task_id: task_id.clone().into(),
                                    reason,
                                    task_scope: Some(TaskTerminalScope::AgentTurn),
                                }),
                            )?;
                            append_background_task_notification_and_schedule(
                                self.clock.as_ref(),
                                self.redactor.as_ref(),
                                Arc::clone(&self.config.hook_command_executor),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider_retry,
                                Arc::clone(&self.config.provider),
                                Arc::clone(&self.config.tool_registry),
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
                                    attachments: running.attachments.clone(),
                                    request_id: Some(request_id.clone().into()),
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
                                    task_id: task_id.clone().into(),
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
                                Arc::clone(&self.config.hook_command_executor),
                                self.job_tx.clone(),
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                self.config.compaction.clone(),
                                self.config.provider_retry,
                                Arc::clone(&self.config.provider),
                                Arc::clone(&self.config.tool_registry),
                                running.child_task.clone(),
                                &terminal_event,
                                BackgroundTaskNotificationStatus::Completed,
                                &terminal_event_summary(&terminal_event),
                            )
                            .await?;
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
                                task_id: task_id.clone().into(),
                                reason: reason.clone(),
                                task_scope: Some(TaskTerminalScope::AgentTurn),
                            }),
                        )?;
                        append_background_task_notification_and_schedule(
                            self.clock.as_ref(),
                            self.redactor.as_ref(),
                            Arc::clone(&self.config.hook_command_executor),
                            self.job_tx.clone(),
                            run_state,
                            self.config.hook_runtime_config.clone(),
                            self.config.compaction.clone(),
                            self.config.provider_retry,
                            Arc::clone(&self.config.provider),
                            Arc::clone(&self.config.tool_registry),
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
            if let Some(queued) = run_state
                .queued_agent_turns
                .get(task.task_id.as_str())
                .cloned()
            {
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
                        child_task: queued.child_task.as_ref(),
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(task.task_id.as_str())
                else {
                    continue;
                };
                start_agent_turn_execution(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    Arc::clone(&self.config.hook_command_executor),
                    self.job_tx.clone(),
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    self.config.compaction.clone(),
                    self.config.provider_retry,
                    Arc::clone(&self.config.provider),
                    Arc::clone(&self.config.tool_registry),
                    queued,
                )
                .await?;
            }
        }

        schedule_pending_agent_wakeups_for_idle_agent(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            Arc::clone(&self.config.hook_command_executor),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            self.config.compaction.clone(),
            self.config.provider_retry,
            Arc::clone(&self.config.provider),
            Arc::clone(&self.config.tool_registry),
            &finished_agent_id,
        )
        .await?;

        self.promote_next_agent_blocked_turn(&finished_agent_id)
            .await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct ProviderContextCompaction {
    pub(in crate::coord) updated_context: ProviderContext,
    checkpoint_id: String,
    tokens_before_estimate: Option<u32>,
    tokens_after_estimate: Option<u32>,
}

#[derive(Debug, Clone)]
pub(in crate::coord) enum CompactAgentContextResult {
    Compacted {
        context: ProviderContext,
        applied: AppliedCompaction,
    },
    NoOp {
        context: ProviderContext,
    },
}

impl CompactAgentContextResult {
    pub(in crate::coord) fn into_context(self) -> ProviderContext {
        match self {
            Self::Compacted { context, .. } | Self::NoOp { context } => context,
        }
    }

    pub(in crate::coord) fn into_manual_outcome(self) -> ManualCompactionOutcome {
        match self {
            Self::Compacted { applied, .. } => ManualCompactionOutcome::Compacted {
                tokens_before: applied.tokens_before,
                tokens_after: applied.tokens_after,
                summary_preview: summary_preview(&applied.summary),
            },
            Self::NoOp { .. } => ManualCompactionOutcome::NoOp,
        }
    }
}

fn summary_preview(summary: &str) -> String {
    const PREVIEW_MAX: usize = 200;
    if summary.len() <= PREVIEW_MAX {
        return summary.to_string();
    }
    let truncated = &summary[..PREVIEW_MAX];
    match truncated.rfind('\n') {
        Some(idx) if idx > PREVIEW_MAX / 2 => format!("{}…", &truncated[..idx]),
        _ => format!("{truncated}…"),
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
