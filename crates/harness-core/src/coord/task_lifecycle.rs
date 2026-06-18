use super::*;

impl Coordinator {
    pub(in crate::coord) fn job_progress_internal(
        &mut self,
        task_id: String,
        kind: JobProgressKind,
    ) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };

        let Some(task) = run_state.tasks.get_mut(&task_id) else {
            return;
        };

        task.last_progress_mono_ms = self.clock.mono_ms();
        task.last_progress_kind = kind;
    }

    pub(in crate::coord) async fn background_request_projection_internal(
        &mut self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        let events = self.replay_current_run_events().await?;
        let request_ref = resolve_background_request_ref(
            events.iter(),
            &actor,
            request_id.as_deref(),
            selector_hint.as_deref(),
        )
        .map_err(background_projection_error_to_coordinator_error)?;
        project_background_request(events.iter(), &request_ref)
            .map_err(background_projection_error_to_coordinator_error)
    }

    pub(in crate::coord) async fn cancel_background_request_internal(
        &mut self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        reason: String,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        let projection = self
            .background_request_projection_internal(
                actor.clone(),
                request_id.clone(),
                selector_hint.clone(),
            )
            .await?;
        if projection.terminal {
            return Ok(projection);
        }

        let scheduler_task_id = projection.scheduler_task_id.clone().ok_or_else(|| {
            CoordinatorError::UnknownTask(format!(
                "background request `{}` has no scheduler task id",
                projection.request_id
            ))
        })?;
        self.cancel_task_internal(scheduler_task_id, reason).await?;
        self.background_request_projection_internal(actor, request_id, selector_hint)
            .await
    }

    pub(in crate::coord) async fn replay_current_run_events(
        &self,
    ) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
        let store = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?
            .event_store
            .clone();
        let mut stream = store.replay(1)?;
        let mut events = Vec::new();
        while let Some(next) = stream.next().await {
            events.push(next?);
        }
        Ok(events)
    }

    pub(in crate::coord) async fn cancel_task_internal(
        &mut self,
        task_id: String,
        reason: String,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };

        if let Some(queued) = run_state.queued_agent_turns.remove(&task_id) {
            if queued.scheduler_queued {
                let _ = run_state.scheduler.cancel_queued(&task_id);
            }
            let agent_id = queued.agent_id.clone();
            let should_promote_next = !run_state
                .running_agent_turns
                .values()
                .any(|running| running.agent_id == agent_id);
            let terminal_event = append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&queued.agent_id),
                Some(format!("task:{task_id}")),
                Some(queued.request_id),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id,
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
                self.config.provider_retry,
                self.config.provider.clone(),
                self.config.tool_registry.clone(),
                queued.child_task,
                &terminal_event,
                background_notification_status_for_cancel_reason(&terminal_event_summary(
                    &terminal_event,
                )),
                &terminal_event_summary(&terminal_event),
            )
            .await?;
            if should_promote_next {
                self.promote_next_agent_blocked_turn(&agent_id).await?;
            }
            return Ok(());
        }

        if let Some(running) = run_state.running_agent_turns.get(&task_id).cloned() {
            running.cancellation_token.cancel();
            run_state.cancelled_running_tasks.insert(task_id.clone());
            if let Some(memory) = cancelled_failure_memory_from_running(&running, &reason) {
                push_incomplete_provider_turn(run_state, &running, &running.request_id, memory);
            }
            let child_tool_task_ids = run_state
                .tasks
                .iter()
                .filter(|(_, child_task)| {
                    child_task.request_correlation_id.as_deref()
                        == Some(running.request_id.as_str())
                })
                .map(|(child_task_id, _)| child_task_id.clone())
                .collect::<Vec<_>>();
            for child_task_id in child_tool_task_ids {
                if let Some(child_task) = run_state.tasks.get(&child_task_id) {
                    child_task.cancellation_token.cancel();
                }
                run_state.cancelled_running_tasks.insert(child_task_id);
            }
            let terminal_event = append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                agent_actor(&running.agent_id),
                Some(format!("task:{task_id}")),
                Some(running.request_id.clone()),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id,
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
                self.config.provider_retry,
                self.config.provider.clone(),
                self.config.tool_registry.clone(),
                running.child_task,
                &terminal_event,
                background_notification_status_for_cancel_reason(&terminal_event_summary(
                    &terminal_event,
                )),
                &terminal_event_summary(&terminal_event),
            )
            .await?;
            return Ok(());
        }

        let Some(task) = run_state.tasks.get(&task_id) else {
            return Ok(());
        };
        let owner_actor = task.owner_actor.clone();
        let request_correlation_id = task.request_correlation_id.clone();

        task.cancellation_token.cancel();
        run_state.cancelled_running_tasks.insert(task_id.clone());

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            owner_actor,
            Some(format!("task:{task_id}")),
            request_correlation_id,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id,
                reason,
                task_scope: Some(TaskTerminalScope::ToolCall),
            }),
        )?;

        Ok(())
    }

    pub(in crate::coord) fn watchdog_tick_internal(&mut self) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let now = self.clock.mono_ms();
        let snapshots = run_state
            .tasks
            .iter()
            .filter_map(|(task_id, task)| {
                if task.state != TaskExecutionState::Running {
                    return None;
                }

                Some(TaskProgressSnapshot {
                    task_id: task_id.clone(),
                    key: task.queue_key.clone(),
                    last_progress_mono_ms: task.last_progress_mono_ms,
                })
            })
            .collect::<Vec<_>>();

        let stale = run_state
            .scheduler
            .detect_stale(now, self.config.stale_timeout_ms, &snapshots);

        for stale_task in stale {
            let task_id = stale_task.task_id;
            let stale_for_ms = stale_task.stale_for_ms;
            let (actor, request_correlation_id) = run_state
                .tasks
                .get(&task_id)
                .map(|task| {
                    (
                        task.owner_actor.clone(),
                        task.request_correlation_id.clone(),
                    )
                })
                .unwrap_or_else(|| (system_actor(), None));

            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor,
                Some(format!("task:{task_id}")),
                request_correlation_id,
                EventV1::StaleDetected(StaleDetectedEvent {
                    task_id: task_id.clone(),
                    stale_for_ms,
                }),
            )?;

            if let Some(task) = run_state.tasks.get(&task_id) {
                task.cancellation_token.cancel();
            }
            run_state.cancelled_running_tasks.insert(task_id);
        }

        Ok(())
    }

    #[cfg(test)]
    pub(in crate::coord) fn job_finished_internal(
        &mut self,
        task_id: String,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        block_on_coordinator_future(self.job_finished_internal_async(task_id, outcome))
    }

    pub(in crate::coord) async fn job_finished_internal_async(
        &mut self,
        task_id: String,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(task) = run_state.tasks.remove(&task_id) else {
            return Ok(());
        };
        let task_hook_state = run_state
            .task_hook_state
            .remove(&task_id)
            .unwrap_or_else(|| TaskHookState {
                tool_id: match &task.queue_key {
                    ConcurrencyKey::Tool { tool_id } => tool_id.clone(),
                    _ => String::new(),
                },
                category: None,
                hook_executions: Vec::new(),
            });

        if run_state.cancelled_running_tasks.remove(&task_id) {
            let _ = run_state.scheduler.complete(&task.queue_key);
            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                task.owner_actor,
                Some(format!("task:{task_id}")),
                task.request_correlation_id,
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id,
                    result_digest: digest12(format!("{:?}", outcome).as_bytes()),
                }),
            )?;
            return Ok(());
        }

        let _ = run_state.scheduler.complete(&task.queue_key);
        let request_correlation_id = task.request_correlation_id.clone();
        let finished_mono_ms = self.clock.mono_ms();
        let timing = execution_timing_metadata(task.started_mono_ms, finished_mono_ms);

        match outcome {
            JobOutcome::Succeeded { result } => {
                let result_for_response = result.clone();
                let applied_edits = applied_tool_edit_metadata(
                    &task_hook_state.tool_id,
                    &result_for_response,
                    task.hashline_edit.as_ref(),
                );
                for applied_edit in &applied_edits {
                    let AppliedToolEditMetadata {
                        metadata,
                        diff_rel_path,
                        diff_digest,
                        deleted,
                    } = applied_edit;
                    let new_file_digest = if *deleted {
                        digest12(b"")
                    } else {
                        match workspace_file_digest(&run_state.info.workspace_root, &metadata.path)
                        {
                            Ok(new_file_digest) => new_file_digest,
                            Err(reason) => {
                                append_edit_rejected_event(
                                    self.clock.as_ref(),
                                    self.redactor.as_ref(),
                                    run_state,
                                    &task.tool_call_id,
                                    metadata,
                                    format!("failed to compute file digest: {reason}"),
                                    request_correlation_id.as_deref(),
                                )?;
                                continue;
                            }
                        }
                    };
                    append_edit_applied_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        EditAppliedEventArgs {
                            tool_call_id: &task.tool_call_id,
                            metadata,
                            new_file_digest,
                            diff_rel_path: diff_rel_path.clone(),
                            diff_digest: diff_digest.clone(),
                            request_correlation_id: request_correlation_id.as_deref(),
                        },
                    )?;
                }

                let mut formatter_warnings = Vec::new();
                let mut formatted_paths = std::collections::BTreeSet::new();
                let caching_discovery =
                    formatter::CachingFormatterDiscovery::new(formatter::RealFormatterDiscovery);
                for applied_edit in &applied_edits {
                    if applied_edit.deleted {
                        continue;
                    }
                    let path = &applied_edit.metadata.path;
                    if !formatted_paths.insert(path.clone()) {
                        continue;
                    }
                    if let Err(warning) = formatter::run_formatter_for_path_with_discovery(
                        &self.config.formatter,
                        &run_state.info.workspace_root,
                        path,
                        &caching_discovery,
                    )
                    .await
                    {
                        formatter_warnings.push(format!("{path}: {warning}"));
                    }
                }

                let mut result_summary = result.display_text.clone();
                if !formatter_warnings.is_empty() {
                    result_summary.push_str("\n\nFormatter warnings:\n");
                    for warning in formatter_warnings {
                        result_summary.push_str(&warning);
                        result_summary.push('\n');
                    }
                }
                let artifact_refs = event_artifact_refs(&result.artifacts);
                let lineage = tool_task_lineage_metadata(
                    &task.tool_call_id,
                    task.request_correlation_id.as_deref(),
                    result_for_response.structured_json.as_ref(),
                );
                let mut hook_executions = task_hook_state.hook_executions.clone();
                hook_executions.extend(extract_hook_execution_metadata(
                    result_for_response.structured_json.as_ref(),
                ));
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("succeeded".to_string()),
                        output_summary: Some(result_summary.clone()),
                        failure_reason: None,
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                for artifact in &result.artifacts {
                    append_artifact_written_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        artifact,
                        request_correlation_id.as_deref(),
                        task.tool_metadata.as_ref(),
                    )?;
                }
                if let Some(reason) = finish_hook_batch.critical_failure.clone() {
                    append_payload_event_with_correlation(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        task.owner_actor.clone(),
                        Some(format!("task:{task_id}")),
                        request_correlation_id.clone(),
                        EventV1::TaskCancelled(TaskCancelledEvent {
                            task_id,
                            reason: reason.clone(),
                            task_scope: Some(TaskTerminalScope::ToolCall),
                        }),
                    )?;
                    append_failed_tool_call_finished_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        &reason,
                        request_correlation_id.as_deref(),
                        tool_call_metadata(
                            task.tool_metadata.as_ref(),
                            Some(lineage),
                            artifact_refs,
                            Some(timing.clone()),
                            hook_executions.clone(),
                        ),
                        &hook_executions,
                    )?;
                    if let Some(respond_to) = task.respond_to {
                        let _ = respond_to.send(Err(reason.clone()));
                    }
                    return Ok(());
                }
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCompleted(TaskCompletedEvent {
                        task_id,
                        result_digest: digest12(result_summary.as_bytes()),
                        result_summary: result_summary.clone(),
                        metadata: Some(TaskCompletionMetadata {
                            lineage: Some(lineage.clone()),
                            task_scope: Some(TaskTerminalScope::ToolCall),
                            timing: Some(timing.clone()),
                            hook_executions: hook_executions.clone(),
                        }),
                    }),
                )?;

                let output_json = Some(stable_tool_output_json(
                    result_for_response.structured_json.clone(),
                    &result_summary,
                    &artifact_refs,
                    &lineage,
                    &timing,
                    &hook_executions,
                ));

                append_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    ToolCallFinishedEventArgs {
                        tool_call_id: &task.tool_call_id,
                        status: ToolCallStatus::Succeeded,
                        output_summary: Some(result_summary),
                        output_json,
                        metadata: tool_call_metadata(
                            task.tool_metadata.as_ref(),
                            Some(lineage),
                            artifact_refs,
                            Some(timing.clone()),
                            hook_executions,
                        ),
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Ok(result_for_response));
                }
            }
            JobOutcome::Failed { error } => {
                let mut final_error = error.clone();
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        error.clone(),
                        request_correlation_id.as_deref(),
                    )?;
                }

                let mut hook_executions = task_hook_state.hook_executions.clone();
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("failed".to_string()),
                        output_summary: None,
                        failure_reason: Some(final_error.clone()),
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                if let Some(hook_reason) = finish_hook_batch.critical_failure {
                    final_error =
                        format!("{final_error}; critical lifecycle hook failed: {hook_reason}");
                }

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason: final_error.clone(),
                        task_scope: Some(TaskTerminalScope::ToolCall),
                    }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    &final_error,
                    request_correlation_id.as_deref(),
                    tool_call_metadata(
                        task.tool_metadata.as_ref(),
                        Some(tool_task_lineage_metadata(
                            &task.tool_call_id,
                            task.request_correlation_id.as_deref(),
                            None,
                        )),
                        Vec::new(),
                        Some(timing.clone()),
                        hook_executions.clone(),
                    ),
                    &hook_executions,
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Err(format!("tool execution failed: {final_error}")));
                }
            }
            JobOutcome::Cancelled { reason } => {
                let mut final_reason = reason.clone();
                if let Some(metadata) = task.hashline_edit.as_ref() {
                    append_edit_rejected_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &task.tool_call_id,
                        metadata,
                        reason.clone(),
                        request_correlation_id.as_deref(),
                    )?;
                }

                let mut hook_executions = task_hook_state.hook_executions.clone();
                let finish_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::ToolCallFinished,
                        run_id: run_state.info.run_id.clone(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(task.owner_actor.clone()),
                        agent_id: task.owner_actor.agent_id.clone(),
                        request_id: request_correlation_id.clone(),
                        permission_id: None,
                        task_id: Some(task_id.clone()),
                        tool_call_id: Some(task.tool_call_id.clone()),
                        tool_id: Some(task_hook_state.tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: task_hook_state.category.clone(),
                        outcome: Some("cancelled".to_string()),
                        output_summary: None,
                        failure_reason: Some(final_reason.clone()),
                    },
                )
                .await;
                hook_executions.extend(finish_hook_batch.hook_executions.clone());
                if let Some(hook_reason) = finish_hook_batch.critical_failure {
                    final_reason =
                        format!("{final_reason}; critical lifecycle hook failed: {hook_reason}");
                }

                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    task.owner_actor.clone(),
                    Some(format!("task:{task_id}")),
                    request_correlation_id.clone(),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id,
                        reason: final_reason.clone(),
                        task_scope: Some(TaskTerminalScope::ToolCall),
                    }),
                )?;

                append_failed_tool_call_finished_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &task.tool_call_id,
                    &final_reason,
                    request_correlation_id.as_deref(),
                    tool_call_metadata(
                        task.tool_metadata.as_ref(),
                        Some(tool_task_lineage_metadata(
                            &task.tool_call_id,
                            task.request_correlation_id.as_deref(),
                            None,
                        )),
                        Vec::new(),
                        Some(timing),
                        hook_executions.clone(),
                    ),
                    &hook_executions,
                )?;
                if let Some(respond_to) = task.respond_to {
                    let _ = respond_to.send(Err(format!("tool call cancelled: {final_reason}")));
                }
            }
        }

        Ok(())
    }
}
