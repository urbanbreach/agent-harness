use super::*;

pub(in crate::coord) async fn start_tool_call_execution<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    args: ToolCallExecutionArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallExecutionArgs {
        tool_call_id,
        tool_id,
        args_json,
        actor,
        category,
        hook_executions,
        tool_registry,
        request_correlation_id,
        respond_to,
    } = args;
    let mut respond_to = respond_to;
    let tool_metadata = tool_identity_metadata(&tool_id, &args_json);

    let Some(tool) = tool_registry.get(&tool_id) else {
        append_payload_event(
            clock,
            redactor,
            run_state,
            actor,
            Some(format!("tool_call:{tool_call_id}")),
            EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                policy: "unknown_tool_id".to_string(),
                detail: format!("tool `{tool_id}` is not registered"),
            }),
        )?;

        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            "unknown tool",
            request_correlation_id.as_deref(),
            requested_tool_call_metadata(&tool_id, &args_json),
            &[],
        )?;
        return Err(CoordinatorError::PolicyViolation(format!(
            "tool `{tool_id}` is not registered"
        )));
    };

    let actor_kind = actor.kind;
    if !tool_registry.capability_allowed(actor_kind, tool.capability()) {
        append_payload_event(
            clock,
            redactor,
            run_state,
            actor,
            Some(format!("tool_call:{tool_call_id}")),
            EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                policy: "tool_capability_forbidden".to_string(),
                detail: format!(
                    "actor {:?} cannot call {} requiring {:?}",
                    actor_kind,
                    tool_id,
                    tool.capability()
                ),
            }),
        )?;

        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            "capability forbidden",
            request_correlation_id.as_deref(),
            requested_tool_call_metadata(&tool_id, &args_json),
            &[],
        )?;
        return Err(CoordinatorError::PolicyViolation(
            "tool capability forbidden for actor".to_string(),
        ));
    }

    let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &tool_call_id);

    append_tool_call_started_event(
        clock,
        redactor,
        run_state,
        &tool_call_id,
        request_correlation_id.as_deref(),
    )?;

    if let Some(metadata) = hashline_edit.as_ref() {
        append_edit_proposed_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            metadata,
            request_correlation_id.as_deref(),
        )?;
    }

    let started_hook_batch = hooks::run_lifecycle_hooks(
        clock,
        hook_command_executor.as_ref(),
        &hook_runtime_config,
        HookInvocationContext {
            event: HookLifecycleEvent::ToolCallStarted,
            run_id: run_state.info.run_id.clone(),
            workspace_root: run_state.info.workspace_root.clone(),
            artifacts_dir: run_state.info.artifacts_dir.clone(),
            actor: Some(actor.clone()),
            agent_id: actor.agent_id.clone(),
            request_id: request_correlation_id.clone(),
            permission_id: None,
            task_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_id: Some(tool_id.clone()),
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            category: category.clone(),
            outcome: Some("started".to_string()),
            output_summary: None,
            failure_reason: None,
        },
    )
    .await;
    let mut initial_hook_executions = hook_executions;
    initial_hook_executions.extend(started_hook_batch.hook_executions.clone());
    if let Some(reason) = started_hook_batch.critical_failure.clone() {
        append_failed_tool_call_finished_event(
            clock,
            redactor,
            run_state,
            &tool_call_id,
            &reason,
            request_correlation_id.as_deref(),
            tool_call_metadata(
                tool_metadata.as_ref(),
                None,
                Vec::new(),
                None,
                initial_hook_executions.clone(),
            ),
            &initial_hook_executions,
        )?;
        if let Some(respond_to) = respond_to.take() {
            let _ = respond_to.send(Err(reason.clone()));
        }
        return Err(CoordinatorError::LifecycleHookFailed(reason.to_string()));
    }

    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let queue_key = ConcurrencyKey::Tool {
        tool_id: tool_id.clone(),
    };

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        actor.clone(),
        Some(format!("task:{task_id}")),
        request_correlation_id.clone(),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.clone(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.queue_key()),
        }),
    )?;

    let cancellation_token = run_state.shutdown_token.child_token();
    let tool_state = run_state.tool_state.clone();
    let run_id = run_state.info.run_id.clone();
    let workspace_root = run_state.info.workspace_root.clone();
    let artifacts_dir = run_state.info.artifacts_dir.clone();
    let coordinator = CoordinatorHandle { tx: job_tx.clone() };
    let current_model = actor.agent_id.as_deref().and_then(|agent_id| {
        run_state
            .running_agent_turns
            .values()
            .find(|turn| turn.agent_id == agent_id)
            .map(|turn| (turn.model_ref.clone(), turn.model_settings.clone()))
    });
    run_state.tasks.insert(
        task_id.clone(),
        TaskState {
            tool_call_id: tool_call_id.clone(),
            tool_metadata,
            owner_actor: actor.clone(),
            request_correlation_id,
            queue_key,
            state: TaskExecutionState::Running,
            cancellation_token: cancellation_token.clone(),
            started_mono_ms: clock.mono_ms(),
            last_progress_mono_ms: clock.mono_ms(),
            last_progress_kind: JobProgressKind::Heartbeat,
            hashline_edit,
            respond_to,
        },
    );
    run_state.task_hook_state.insert(
        task_id.clone(),
        TaskHookState {
            tool_id: tool_id.clone(),
            category: category.clone(),
            hook_executions: initial_hook_executions,
        },
    );

    tokio::spawn(async move {
        let _ = job_tx
            .send(Command::JobProgress {
                task_id: task_id.clone(),
                kind: JobProgressKind::Heartbeat,
            })
            .await;

        let context = ToolContext {
            run_id,
            workspace_root,
            artifacts_dir,
            actor,
            category,
            tool_call_id: tool_call_id.clone(),
            current_model_ref: current_model
                .as_ref()
                .map(|(model_ref, _)| model_ref.clone()),
            current_model_settings: current_model.as_ref().map(|(_, settings)| settings.clone()),
            tool_state,
            coordinator,
        };

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                let _ = job_tx
                    .send(Command::JobFinished {
                        task_id,
                        outcome: JobOutcome::Cancelled {
                            reason: "job cancelled".to_string(),
                        },
                    })
                    .await;
            }
            result = tool.call(context, args_json) => {
                let outcome = match result {
                    Ok(result) => JobOutcome::Succeeded { result },
                    Err(err) => JobOutcome::Failed {
                        error: err.to_string(),
                    },
                };

                let _ = job_tx
                    .send(Command::JobFinished {
                        task_id,
                        outcome,
                    })
                    .await;
            }
        }
    });

    Ok(())
}
