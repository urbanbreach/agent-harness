// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::*;

impl Coordinator {
    pub(in crate::coord) async fn background_foreground_child_tasks_internal(
        &mut self,
    ) -> Result<usize, CoordinatorError> {
        let detachments = {
            let Some(run_state) = self.run_state.as_mut() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            let parent_session_id = run_state.info.run_id.to_string();
            let foreground_children = foreground_child_tasks(run_state, &parent_session_id);
            let mut detachments = Vec::new();

            for child in foreground_children {
                let Some(parent_task_id) = parent_task_id_for_child(run_state, &child) else {
                    continue;
                };
                mark_child_task_backgrounded(run_state, &child.child_request_id);
                detachments.push((parent_task_id, child));
            }

            detachments
        };

        if detachments.is_empty() {
            return Err(CoordinatorError::UnknownTask(
                "no foreground subagent is currently blocking this session".to_string(),
            ));
        }

        let count = detachments.len();
        for (parent_task_id, child) in detachments {
            self.job_finished_internal_async(
                parent_task_id,
                JobOutcome::Succeeded {
                    result: backgrounded_child_tool_result(&child),
                },
            )
            .await?;
        }

        Ok(count)
    }

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
        let store = Arc::clone(
            &self
                .run_state
                .as_ref()
                .ok_or(CoordinatorError::RunNotStarted)?
                .event_store,
        );
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
                    task_id: task_id.into(),
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
                    task_id: task_id.into(),
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
                task_id: task_id.into(),
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
                    task_id: task_id.clone().into(),
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
                .get(task_id.as_str())
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

            if let Some(task) = run_state.tasks.get(task_id.as_str()) {
                task.cancellation_token.cancel();
            }
            run_state
                .cancelled_running_tasks
                .insert(task_id.to_string());
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
                    task_id: task_id.into(),
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
                let mut applied_edits = applied_tool_edit_metadata(
                    &task_hook_state.tool_id,
                    &result_for_response,
                    task.hashline_edit.as_ref(),
                );

                // Run formatters BEFORE computing digests so that the stored
                // digest and EditApplied event reflect the post-format file
                // content, re-reading the file
                // after formatting.
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

                // Regenerate diff artifacts to reflect post-format content,
                // re-reading the file after
                // formatting and regenerating the diff from the original
                // pre-edit content vs the formatted file.
                for applied_edit in &mut applied_edits {
                    if applied_edit.deleted {
                        continue;
                    }
                    let Some(before_rel_path) = &applied_edit.before_rel_path else {
                        continue;
                    };
                    let Some(diff_rel_path) = &applied_edit.diff_rel_path else {
                        continue;
                    };

                    let before_name = Path::new(before_rel_path)
                        .strip_prefix(crate::session_paths::ARTIFACTS_DIR_NAME)
                        .unwrap_or(Path::new(before_rel_path));
                    let before_full_path = run_state.info.artifacts_dir.join(before_name);
                    let before_content = match tokio::fs::read_to_string(&before_full_path).await {
                        Ok(content) => content,
                        Err(_) => continue,
                    };

                    let file_path = if Path::new(&applied_edit.metadata.path).is_absolute() {
                        PathBuf::from(&applied_edit.metadata.path)
                    } else {
                        run_state
                            .info
                            .workspace_root
                            .join(&applied_edit.metadata.path)
                    };
                    let formatted_content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(content) => content,
                        Err(_) => continue,
                    };

                    let before_normalized = normalize_for_diff(&before_content);
                    let formatted_normalized = normalize_for_diff(&formatted_content);

                    if before_normalized == formatted_normalized {
                        continue;
                    }

                    let raw_diff =
                        similar::TextDiff::from_lines(&before_normalized, &formatted_normalized)
                            .unified_diff()
                            .to_string();
                    let new_diff = trim_diff(&raw_diff);

                    let diff_name = Path::new(diff_rel_path)
                        .strip_prefix(crate::session_paths::ARTIFACTS_DIR_NAME)
                        .unwrap_or(Path::new(diff_rel_path));
                    let diff_full_path = run_state.info.artifacts_dir.join(diff_name);
                    if std::fs::write(&diff_full_path, new_diff.as_bytes()).is_ok() {
                        applied_edit.diff_digest =
                            Some(blake3::hash(new_diff.as_bytes()).to_hex().to_string());
                    }
                }

                // Compute digests and store EditApplied events AFTER formatting
                // so new_file_digest matches the actual on-disk file content.
                for applied_edit in &applied_edits {
                    let AppliedToolEditMetadata {
                        metadata,
                        diff_rel_path,
                        diff_digest,
                        before_rel_path: _,
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
                        run_id: run_state.info.run_id.to_string(),
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
                            task_id: task_id.into(),
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
                        task_id: task_id.into(),
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
                        run_id: run_state.info.run_id.to_string(),
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
                        task_id: task_id.into(),
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
                        run_id: run_state.info.run_id.to_string(),
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
                        task_id: task_id.into(),
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

fn foreground_child_tasks(
    run_state: &RunState,
    parent_session_id: &str,
) -> Vec<ChildTaskTurnState> {
    run_state
        .running_agent_turns
        .values()
        .filter_map(|running| running.child_task.as_ref())
        .chain(
            run_state
                .queued_agent_turns
                .values()
                .filter_map(|queued| queued.child_task.as_ref()),
        )
        .filter(|child| {
            !child.run_in_background && child.parent_session_id.as_str() == parent_session_id
        })
        .cloned()
        .collect()
}

fn parent_task_id_for_child(run_state: &RunState, child: &ChildTaskTurnState) -> Option<String> {
    run_state
        .tasks
        .iter()
        .find(|(_, task)| task.tool_call_id == child.parent_tool_call_id)
        .map(|(task_id, _)| task_id.clone())
}

fn mark_child_task_backgrounded(run_state: &mut RunState, child_request_id: &str) {
    for running in run_state.running_agent_turns.values_mut() {
        if let Some(child) = running.child_task.as_mut() {
            if child.child_request_id == child_request_id {
                child.run_in_background = true;
            }
        }
    }
    for queued in run_state.queued_agent_turns.values_mut() {
        if let Some(child) = queued.child_task.as_mut() {
            if child.child_request_id == child_request_id {
                child.run_in_background = true;
            }
        }
    }
}

fn backgrounded_child_tool_result(child: &ChildTaskTurnState) -> ToolResult {
    let display_text = format!(
        "task_id: {} (for resuming to continue this task if needed)\nrequest_id: {}\n\n<task_result>Foreground subagent moved to background.</task_result>",
        child.child_session_id, child.child_request_id
    );
    ToolResult::structured(
        display_text,
        json!({
            "description": child.description,
            "task_id": child.child_session_id,
            "session_id": child.child_session_id,
            "request_id": child.child_request_id,
            "child_session_id": child.child_session_id,
            "child_request_id": child.child_request_id,
            "background": true,
            "mode": "background",
            "status": "scheduled",
            "lineage": {
                "parent_tool_call_id": child.parent_tool_call_id,
                "parent_session_id": child.parent_session_id,
                "parent_agent_id": child.parent_agent_id,
                "child_session_id": child.child_session_id,
                "child_request_id": child.child_request_id,
            },
            "next_actions": [
                format!("background_output(request_id=\"{}\")", child.child_request_id),
                format!("task(task_id=\"{}\")", child.child_session_id),
            ],
        }),
    )
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn normalize_for_diff(text: &str) -> String {
    strip_bom(text).replace("\r\n", "\n")
}

fn trim_diff(diff: &str) -> String {
    let lines: Vec<&str> = diff.split('\n').collect();

    let is_content = |line: &&str| {
        (line.starts_with('+') && !line.starts_with("+++"))
            || (line.starts_with('-') && !line.starts_with("---"))
            || (line.starts_with(' ') && !line.is_empty())
    };

    let min_indent = lines
        .iter()
        .filter(|line| is_content(line))
        .filter_map(|line| {
            let content = &line[1..];
            if content.trim().is_empty() {
                return None;
            }
            Some(content.len() - content.trim_start().len())
        })
        .min()
        .unwrap_or(0);

    if min_indent == 0 {
        return diff.to_string();
    }

    lines
        .iter()
        .map(|line| {
            if is_content(line) {
                let prefix = &line[..1];
                let content = &line[1..];
                if content.len() >= min_indent {
                    format!("{prefix}{}", &content[min_indent..])
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod diff_helper_tests {
    use super::*;

    #[test]
    fn strip_bom_removes_bom_prefix() {
        assert_eq!(strip_bom("\u{feff}hello"), "hello");
        assert_eq!(strip_bom("hello"), "hello");
        assert_eq!(strip_bom(""), "");
    }

    #[test]
    fn normalize_for_diff_strips_bom_and_crlf() {
        assert_eq!(normalize_for_diff("\u{feff}\r\nhello\r\n"), "\nhello\n");
        assert_eq!(normalize_for_diff("hello\n"), "hello\n");
        assert_eq!(normalize_for_diff("\r\n\r\n"), "\n\n");
    }

    #[test]
    fn trim_diff_strips_common_indent() {
        let diff = "--- a\n+++ b\n@@ -1,2 +1,2 @@\n     line1\n-    old\n+    new\n";
        let trimmed = trim_diff(diff);
        assert!(
            trimmed.contains(" line1"),
            "context line should have 1 space prefix"
        );
        assert!(
            trimmed.contains("-old"),
            "removed line should have no indent"
        );
        assert!(trimmed.contains("+new"), "added line should have no indent");
    }

    #[test]
    fn trim_diff_preserves_no_indent_diffs() {
        let diff = "--- a\n+++ b\n@@ -1,2 +1,2 @@\n line1\n-old\n+new\n";
        assert_eq!(trim_diff(diff), diff);
    }

    #[test]
    fn trim_diff_skips_empty_content_lines_for_indent_calc() {
        let diff = "--- a\n+++ b\n@@ -1,3 +1,3 @@\n     line1\n     \n-    old\n+    new\n";
        let trimmed = trim_diff(diff);
        assert!(trimmed.contains("-old"));
        assert!(trimmed.contains("+new"));
    }
}
