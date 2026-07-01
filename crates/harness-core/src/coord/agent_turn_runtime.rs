use super::*;

impl Coordinator {
    #[expect(
        clippy::too_many_arguments,
        reason = "agent turn requests pass explicit actor, target, prompt, tags, overrides, and child task metadata"
    )]
    pub(in crate::coord) async fn request_agent_turn_internal(
        &mut self,
        actor: EventActor,
        agent_id: String,
        prompt: String,
        selected_tags: crate::file_tag::SelectedPromptTags,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
        child_task_metadata: Option<ChildTaskRequestMetadata>,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        if !matches!(actor.kind, ActorKind::Supervisor | ActorKind::User) {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor,
                Some(format!("agent:{agent_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "request_agent_turn_requires_user_or_supervisor".to_string(),
                    detail: "only user/supervisor may request agent turns".to_string(),
                }),
            )?;
            return Err(CoordinatorError::PolicyViolation(
                "only user/supervisor may request agent turns".to_string(),
            ));
        }

        let profile = run_state
            .agents
            .get(&agent_id)
            .cloned()
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent_id.clone()))?;

        let request_id = allocate_provider_request_id(run_state);
        if run_state.child_session_mirrors.contains_key(&agent_id) {
            run_state
                .child_request_session_by_id
                .insert(request_id.clone(), agent_id.clone());
        }
        let child_task = child_task_metadata.map(|metadata| ChildTaskTurnState {
            parent_tool_call_id: metadata.parent_tool_call_id,
            parent_session_id: metadata.parent_session_id,
            parent_agent_id: metadata.parent_agent_id,
            child_session_id: metadata.child_session_id,
            child_request_id: request_id.clone(),
            task_id: metadata.task_id,
            description: metadata.description,
            run_in_background: metadata.run_in_background,
        });

        let prompt = if profile.name == crate::plan::PLAN_AGENT_NAME {
            Self::plan_mode_prompt(
                &run_state.info.run_id,
                &run_state.info.workspace_root,
                &prompt,
            )
        } else {
            prompt
        };

        let prompt_context = crate::file_tag::materialize_prompt_part_context(
            &run_state.info.workspace_root,
            &prompt,
            &selected_tags.files,
            &selected_tags.agents,
            &selected_tags.resources,
        );

        let request = AgentRequest {
            agent_id,
            prompt,
            prompt_context,
            selected_file_tags: selected_tags.files,
            selected_agent_tags: selected_tags.agents,
            selected_resource_tags: selected_tags.resources,
            model_ref: model_ref_override.unwrap_or_else(|| profile.model_ref.clone()),
            model_settings: model_settings_override
                .unwrap_or_else(|| default_model_settings_for_profile(&profile.name)),
        };

        if child_task.is_none()
            && actor.kind == ActorKind::User
            && run_state.next_provider_request_id == 2
        {
            run_state.recorded_runtime_context = Some(RecordedRuntimeContext::from_profile_model(
                &profile.name,
                &request.model_ref,
            ));
            write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
        }

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("agent:{}", request.agent_id)),
            Some(request_id.clone()),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone(),
                text: request.prompt.clone(),
            }),
        )?;

        if actor.kind == ActorKind::User {
            self.ensure_harness_session_title(&request.prompt).await;
        }

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        schedule_agent_turn(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            self.config.hook_command_executor.clone(),
            self.job_tx.clone(),
            run_state,
            self.config.hook_runtime_config.clone(),
            self.config.compaction.clone(),
            self.config.provider_retry,
            ScheduleAgentTurnArgs {
                provider: self.config.provider.clone(),
                tool_registry: self.config.tool_registry.clone(),
                profile,
                request,
                request_id: request_id.clone(),
                child_task,
            },
        )
        .await?;

        Ok(request_id)
    }

    async fn ensure_harness_session_title(&mut self, prompt: &str) {
        let Some(run_state) = self.run_state.as_ref() else {
            return;
        };
        if !is_parent_default_title(&run_state.info.run_name)
            || run_state.next_provider_request_id != 2
        {
            return;
        }

        let Some(profile) = self.config.agent_profiles.get(TITLE_AGENT_NAME).cloned() else {
            return;
        };
        let provider = self.config.provider.clone();

        let title = match generate_harness_session_title(provider, profile, prompt).await {
            Ok(Some(title)) => title,
            Ok(None) => return,
            Err(reason) => {
                tracing::warn!(reason, "failed to generate session title");
                return;
            }
        };

        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };
        if !is_parent_default_title(&run_state.info.run_name)
            || run_state.next_provider_request_id != 2
        {
            return;
        }

        let run_stream_key = format!("run:{}", run_state.info.run_id);
        let title_event = EventV1::SessionTitleUpdated(crate::event::SessionTitleUpdatedEvent {
            title: title.clone(),
        });
        let persist_result = append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(run_stream_key),
            title_event,
        )
        .map(|_| {
            run_state.info.run_name = title;
        })
        .and_then(|_| write_run_metadata(run_state, &self.config, self.clock.as_ref()));
        if let Err(err) = persist_result {
            tracing::warn!(error = %err, "failed to persist generated session title");
        }
    }

    pub(in crate::coord) fn allocate_provider_request_id_internal(
        &mut self,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        Ok(allocate_provider_request_id(run_state))
    }

    fn plan_mode_prompt(run_id: &str, workspace_root: &Path, prompt: &str) -> String {
        let plan_path = crate::plan::plan_file_relative_path(run_id);
        let plan_file = plan_path.to_string_lossy();
        let plan_file_status = if workspace_root.join(&plan_path).is_file() {
            format!(
                "An active plan file already exists at {plan_file}. Read it first, then make incremental edits only to that file."
            )
        } else {
            format!(
                "No plan file exists yet. Create your final plan at {plan_file}. This workspace-relative path is the only writable target during Plan mode."
            )
        };

        format!(
            "{prompt}\n\n<system-reminder>\nPlan mode is active. The user does not want execution yet. You MUST NOT make edits except to the active plan file at {plan_file}, run non-readonly tools, change configs, or make commits. This supersedes all other instructions. Harness enforces this with runtime permissions; do not rely on prompt text alone.\n\n## Plan File Info\n{plan_file_status}\nBuild the plan incrementally by writing or editing only {plan_file}. The plan file should contain your final recommended approach, not an exhaustive transcript of alternatives considered. Keep it concise enough to scan and detailed enough to execute.\n\n## Plan Workflow\n### Phase 1: Initial Understanding\nUse read-only tools to understand the request, relevant code paths, constraints, and existing tests. Native read/search/LSP tools are allowed when exposed. Bash, when exposed by the active profile, is permission-gated and additionally restricted by runtime policy to a small read-only inspection subset; never use bash to modify files, configs, git state, or the environment.\n\n### Phase 2: Parallel Exploration\nLaunch zero to three `explore` subagents only when useful for read-only codebase research. Use one agent for isolated or known-file work; use multiple agents when scope is uncertain, several modules are involved, or separate searches for implementation, call sites, and tests would improve the plan. Runtime policy only permits the read-only `explore` profile in Plan mode; do not launch `general`, `build`, or user-defined write-capable agents.\n\n### Phase 3: Synthesis\nSynthesize the findings into one recommended implementation approach. Ask a clarifying question only when read-only exploration cannot resolve a requirement, tradeoff, or safety concern.\n\n### Phase 4: Final Plan\nUpdate {plan_file} with the recommended approach, critical files to modify, key risks or constraints, and a verification section describing focused tests or end-to-end checks.\n\n### Phase 5: Terminal Action\nAt the end of the turn, either ask a necessary clarifying question or call `plan_exit` to request approval to switch to Build. Do NOT ask whether the plan is okay with the question tool; use `plan_exit` for plan approval.\n</system-reminder>"
        )
    }
}

pub(in crate::coord) struct AgentTurnTaskScheduledEventArgs<'a> {
    pub(in crate::coord) task_id: &'a str,
    pub(in crate::coord) agent_id: &'a str,
    pub(in crate::coord) request_id: &'a str,
    pub(in crate::coord) queue_key: &'a ConcurrencyKey,
    pub(in crate::coord) state: TaskScheduleState,
}

pub(in crate::coord) struct ScheduleAgentTurnArgs {
    pub(in crate::coord) provider: Arc<dyn Provider>,
    pub(in crate::coord) tool_registry: Arc<ToolRegistry>,
    pub(in crate::coord) profile: AgentProfile,
    pub(in crate::coord) request: AgentRequest,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) child_task: Option<ChildTaskTurnState>,
}

struct TurnStartPhaseResult {
    cancellation_token: CancellationToken,
    critical_failure: Option<String>,
}

fn nested_provider_model_queue_key(
    run_state: &RunState,
    agent_id: &str,
    provider_id: String,
    model_id: String,
    base_queue_key: ConcurrencyKey,
) -> ConcurrencyKey {
    let Some(parent_agent_id) = run_state.subagent_parent_by_id.get(agent_id) else {
        return base_queue_key;
    };
    let base_queue_key_display = base_queue_key.queue_key();
    let parent_holds_same_model = run_state.running_agent_turns.values().any(|turn| {
        turn.agent_id == *parent_agent_id && turn.queue_key.queue_key() == base_queue_key_display
    });
    if !parent_holds_same_model {
        return base_queue_key;
    }

    ConcurrencyKey::NestedProviderModel {
        provider_id,
        model_id,
        parent_agent_id: parent_agent_id.clone(),
        agent_id: agent_id.to_string(),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "agent turn scheduling needs explicit coordinator dependencies"
)]
pub(in crate::coord) async fn schedule_agent_turn<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    provider_retry_config: ProviderRetryRuntimeConfig,
    args: ScheduleAgentTurnArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ScheduleAgentTurnArgs {
        provider,
        tool_registry,
        profile,
        request,
        request_id,
        child_task,
    } = args;
    let model = crate::agent::AgentModelRef::parse(&request.model_ref);
    let agent_id = request.agent_id.clone();
    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;
    let session_id = run_state.info.run_id.clone();

    let provider_id = model.provider_id.clone();
    let model_id = model.model_id.clone();
    let base_queue_key = ConcurrencyKey::ProviderModel {
        provider_id: model.provider_id,
        model_id: model.model_id,
    };
    let queue_key = nested_provider_model_queue_key(
        run_state,
        &agent_id,
        provider_id,
        model_id,
        base_queue_key,
    );

    if run_state.agent_has_active_or_queued_turn(&agent_id) {
        append_agent_turn_task_scheduled_event(
            clock,
            redactor,
            run_state,
            AgentTurnTaskScheduledEventArgs {
                task_id: &task_id,
                agent_id: &agent_id,
                request_id: &request_id,
                queue_key: &queue_key,
                state: TaskScheduleState::Queued,
            },
        )?;

        run_state.queue_agent_turn(QueuedAgentTurn {
            task_id,
            agent_id,
            session_id,
            request_id,
            profile,
            request,
            queue_key,
            scheduler_queued: false,
            child_task,
        });

        return Ok(());
    }

    match run_state
        .scheduler
        .schedule(task_id.clone(), queue_key.clone())
    {
        ScheduleDecision::Started(_) => {
            append_agent_turn_task_scheduled_event(
                clock,
                redactor,
                run_state,
                AgentTurnTaskScheduledEventArgs {
                    task_id: &task_id,
                    agent_id: &agent_id,
                    request_id: &request_id,
                    queue_key: &queue_key,
                    state: TaskScheduleState::Started,
                },
            )?;

            start_agent_turn_execution(
                clock,
                redactor,
                hook_command_executor,
                job_tx,
                run_state,
                hook_runtime_config,
                compaction_config,
                provider_retry_config,
                provider,
                tool_registry,
                QueuedAgentTurn {
                    task_id,
                    agent_id,
                    session_id,
                    request_id,
                    profile,
                    request,
                    queue_key,
                    scheduler_queued: false,
                    child_task,
                },
            )
            .await?;
        }
        ScheduleDecision::Queued(_) => {
            append_agent_turn_task_scheduled_event(
                clock,
                redactor,
                run_state,
                AgentTurnTaskScheduledEventArgs {
                    task_id: &task_id,
                    agent_id: &agent_id,
                    request_id: &request_id,
                    queue_key: &queue_key,
                    state: TaskScheduleState::Queued,
                },
            )?;

            run_state.queue_agent_turn(QueuedAgentTurn {
                task_id,
                agent_id,
                session_id,
                request_id,
                profile,
                request,
                queue_key,
                scheduler_queued: true,
                child_task,
            });
        }
    }

    Ok(())
}

pub(in crate::coord) fn append_agent_turn_task_scheduled_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: AgentTurnTaskScheduledEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let AgentTurnTaskScheduledEventArgs {
        task_id,
        agent_id,
        request_id,
        queue_key,
        state,
    } = args;

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        agent_actor(agent_id),
        Some(format!("task:{task_id}")),
        Some(request_id.to_string()),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.to_string(),
            state,
            queue_key: Some(queue_key.queue_key()),
        }),
    )
}

fn recompute_provider_context_for_agent(run_state: &RunState, agent_id: &str) -> ProviderContext {
    run_state
        .provider_context_by_agent
        .get(agent_id)
        .cloned()
        .unwrap_or_default()
}

pub(in crate::coord) fn provider_request_started_metadata(
    metadata: Option<ProviderRequestStartedMetadata>,
    turn_request_id: &str,
    provider_request_id: &str,
) -> Option<ProviderRequestStartedMetadata> {
    let mut metadata = metadata.unwrap_or_default();
    metadata
        .turn_id
        .get_or_insert_with(|| turn_request_id.to_string());
    metadata
        .provider_call_id
        .get_or_insert_with(|| provider_request_id.to_string());
    Some(metadata)
}

pub(in crate::coord) fn provider_request_finished_metadata(
    metadata: Option<ProviderRequestFinishedMetadata>,
    turn_request_id: &str,
    provider_request_id: &str,
) -> Option<ProviderRequestFinishedMetadata> {
    let mut metadata = metadata.unwrap_or_default();
    metadata
        .turn_id
        .get_or_insert_with(|| turn_request_id.to_string());
    metadata
        .provider_call_id
        .get_or_insert_with(|| provider_request_id.to_string());
    Some(metadata)
}

async fn run_turn_start_phase<C>(
    clock: &C,
    hook_command_executor: &(dyn LifecycleHookCommandExecutor + Send + Sync),
    run_state: &mut RunState,
    hook_runtime_config: &HookRuntimeConfig,
    task: &QueuedAgentTurn,
) -> TurnStartPhaseResult
where
    C: Clock + ?Sized,
{
    let cancellation_token = run_state.shutdown_token.child_token();
    let mut hook_executions = run_state
        .agent_hook_state
        .remove(&task.agent_id)
        .unwrap_or_default();

    let started_hook_batch = hooks::run_lifecycle_hooks(
        clock,
        hook_command_executor,
        hook_runtime_config,
        HookInvocationContext {
            event: HookLifecycleEvent::AgentTurnStarted,
            run_id: run_state.info.run_id.clone(),
            workspace_root: run_state.info.workspace_root.clone(),
            artifacts_dir: run_state.info.artifacts_dir.clone(),
            actor: Some(agent_actor(&task.agent_id)),
            agent_id: Some(task.agent_id.clone()),
            request_id: Some(task.request_id.clone()),
            permission_id: None,
            task_id: Some(task.task_id.clone()),
            tool_call_id: None,
            tool_id: None,
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            category: Some(task.profile.category.clone()),
            outcome: Some("started".to_string()),
            output_summary: Some(task.request.prompt.clone()),
            failure_reason: None,
        },
    )
    .await;
    hook_executions.extend(started_hook_batch.hook_executions.clone());

    run_state.begin_running_agent_turn(clock, task, hook_executions, cancellation_token.clone());

    TurnStartPhaseResult {
        cancellation_token,
        critical_failure: started_hook_batch.critical_failure,
    }
}

async fn request_agent_context_compaction(
    job_tx: &mpsc::Sender<Command>,
    task: &QueuedAgentTurn,
    trigger_reason: &str,
    usage: Option<harness_providers::CompletionUsage>,
) -> Result<ProviderContext, CoordinatorError> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::CompactAgentContext {
            task_id: task.task_id.clone(),
            agent_id: task.agent_id.clone(),
            request_id: task.request_id.clone(),
            trigger_reason: trigger_reason.to_string(),
            usage,
            respond_to,
        })
        .await
        .map_err(|_| CoordinatorError::CommandChannelClosed)?;

    response_rx
        .await
        .map_err(|_| CoordinatorError::ResponseChannelClosed)?
}

#[expect(
    clippy::too_many_arguments,
    reason = "coordinator launch wiring intentionally passes explicit runtime dependencies"
)]
pub(in crate::coord) async fn start_agent_turn_execution<C, R>(
    clock: &C,
    _redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionRuntimeConfig,
    provider_retry_config: ProviderRetryRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    task: QueuedAgentTurn,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let turn_start = run_turn_start_phase(
        clock,
        hook_command_executor.as_ref(),
        run_state,
        &hook_runtime_config,
        &task,
    )
    .await;
    let cancellation_token = turn_start.cancellation_token;

    if let Some(reason) = turn_start.critical_failure {
        warn_command_send_failure(
            job_tx
                .send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome: AgentTurnTaskOutcome::Failed {
                        reason,
                        memory: None,
                    },
                })
                .await,
            "agent_turn_finished_from_hook_failure",
        );
        return Ok(());
    }

    let provider_context = recompute_provider_context_for_agent(run_state, &task.agent_id);

    tokio::spawn(async move {
        let task_id = task.task_id.clone();
        let agent_id = task.agent_id.clone();
        let request_id = task.request_id.clone();

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                warn_command_send_failure(job_tx.send(Command::AgentTurnFinished {
                    task_id,
                    agent_id,
                    request_id,
                    outcome: AgentTurnTaskOutcome::Failed {
                        reason: "job cancelled".to_string(),
                        memory: Some(AgentTurnFailureMemory::aborted(
                            "cancelled",
                            "job cancelled",
                            "",
                            None,
                        )),
                    },
                }).await, "agent_turn_finished_from_cancellation");
            }
            outcome = async {
                let mut prior_context = provider_context;
                let mut overflow_retry_attempted = false;

                let pre_prompt_critical_failure = match request_agent_context_compaction(
                    &job_tx,
                    &task,
                    "pre_prompt",
                    None,
                )
                .await
                {
                    Ok(compacted_context) => {
                        prior_context = compacted_context;
                        None
                    }
                    Err(CoordinatorError::LifecycleHookFailed(reason)) => Some(format!(
                        "pre-prompt compaction critical lifecycle hook failed: {reason}"
                    )),
                    Err(err) => {
                        tracing::warn!(
                            agent_id = %task.agent_id,
                            request_id = %task.request_id,
                            error = %err,
                            "pre-prompt provider context compaction failed; continuing without checkpoint"
                        );
                        None
                    }
                };

                if let Some(reason) = pre_prompt_critical_failure {
                    AgentTurnOutcome::failed(reason)
                } else {
                    loop {
                    let outcome = run_agent_turn_phase_loop(AgentTurnPhaseLoopRequest {
                        provider: provider.clone(),
                        tool_registry: tool_registry.clone(),
                        task: &task,
                        prior_context: &prior_context,
                        job_tx: job_tx.clone(),
                        cancellation_token: cancellation_token.clone(),
                        provider_retry: provider_retry_config,
                    })
                    .await;

                    match &outcome {
                        AgentTurnOutcome::Failed { reason, memory }
                            if compaction_config.auto_retry_overflow
                                && !overflow_retry_attempted
                                && is_provider_context_overflow_reason(reason) =>
                        {
                            match request_agent_context_compaction(
                                &job_tx,
                                &task,
                                "overflow_retry",
                                None,
                            )
                            .await
                            {
                                Ok(compacted_context) => {
                                    overflow_retry_attempted = true;
                                    prior_context = compacted_context;
                                    continue;
                                }
                                Err(err) => {
                                    let reason = format!(
                                        "{reason}; overflow compaction failed: {err}"
                                    );
                                    let mut memory = memory.clone();
                                    if let Some(memory) = &mut memory {
                                        memory.reason = reason.clone();
                                    }
                                    break AgentTurnOutcome::Failed { reason, memory };
                                }
                            }
                        }
                        AgentTurnOutcome::Failed { reason, memory }
                            if overflow_retry_attempted
                                && is_provider_context_overflow_reason(reason) =>
                        {
                            let reason = format!(
                                "{reason}; overflow persisted after checkpoint compaction; likely the active prompt or latest preserved turn still exceeds the provider window"
                            );
                            let mut memory = memory.clone().unwrap_or_else(|| {
                                AgentTurnFailure::new(
                                    ProviderConversationTurnStatus::Failed,
                                    "overflow_retry_failed",
                                    reason.clone(),
                                    "",
                                    None,
                                )
                            });
                            memory.status = ProviderConversationTurnStatus::Failed;
                            memory.failure_stage = "overflow_retry_failed".to_string();
                            memory.reason = reason.clone();
                            break AgentTurnOutcome::Failed {
                                reason,
                                memory: Some(memory),
                            };
                        }
                        _ => break outcome,
                    }
                    }
                }
            } => {
                let outcome = match outcome {
                    AgentTurnOutcome::Succeeded {
                        output,
                        messages,
                    } => AgentTurnTaskOutcome::Succeeded {
                        output,
                        messages,
                    },
                    AgentTurnOutcome::Failed { reason, memory } => AgentTurnTaskOutcome::Failed {
                        reason,
                        memory: memory.map(AgentTurnFailureMemory::from),
                    },
                };
                warn_command_send_failure(job_tx.send(Command::AgentTurnFinished {
                    task_id: task.task_id,
                    agent_id: task.agent_id,
                    request_id: task.request_id,
                    outcome,
                }).await, "agent_turn_finished");
            }
        }
    });

    Ok(())
}
