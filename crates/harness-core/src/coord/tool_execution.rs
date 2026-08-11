// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::*;
use crate::UnwrapOrAbort;

impl Coordinator {
    pub(in crate::coord) async fn request_tool_call_internal(
        &mut self,
        actor: EventActor,
        _legacy_profile_hint: Option<String>,
        tool_id: String,
        args_json: Value,
        respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
    ) -> Result<String, CoordinatorError> {
        let clock = Arc::clone(&self.clock);
        let redactor = Arc::clone(&self.redactor);
        let job_tx = self.job_tx.clone();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        let tool_call_id = format!("toolcall_{:06}", run_state.next_tool_call_id);
        run_state.next_tool_call_id += 1;

        let request_correlation_id = tool_request_correlation_id(run_state, &actor);
        let tool_metadata = requested_tool_call_metadata(&tool_id, &args_json);

        append_tool_call_requested_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            ToolCallRequestedEventArgs {
                actor: actor.clone(),
                tool_call_id: &tool_call_id,
                tool_id: &tool_id,
                args_json: &args_json,
                tool_metadata,
                request_correlation_id: request_correlation_id.as_deref(),
            },
        )?;

        let Some(tool) = self.config.tool_registry.get(&tool_id) else {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("tool_call:{tool_call_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "unknown_tool_id".to_string(),
                    detail: format!("tool `{tool_id}` is not registered"),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(format!(
                "tool `{tool_id}` is not registered"
            )));
        };

        let capability = tool.capability();
        if !self
            .config
            .tool_registry
            .capability_allowed(actor.kind, capability)
        {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("tool_call:{tool_call_id}")),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "tool_capability_forbidden".to_string(),
                    detail: format!(
                        "actor {:?} cannot call {} requiring {:?}",
                        actor.kind, tool_id, capability
                    ),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(
                "tool capability forbidden for actor".to_string(),
            ));
        }

        let effective_profile_name = if actor.kind == ActorKind::Worker {
            let Some(worker_agent_id) = actor.agent_id.as_deref() else {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "unknown_worker_agent_id".to_string(),
                        detail: "worker tool call missing actor agent_id".to_string(),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(
                    "worker tool call missing agent_id".to_string(),
                ));
            };

            let Some(worker_profile) = run_state.agents.get(worker_agent_id) else {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "unknown_worker_agent_id".to_string(),
                        detail: format!("worker agent_id `{worker_agent_id}` is not registered"),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(format!(
                    "worker agent_id `{worker_agent_id}` is not registered"
                )));
            };

            if !worker_profile
                .toolset
                .iter()
                .any(|allowed| allowed == &tool_id)
            {
                append_payload_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    actor.clone(),
                    Some(format!("tool_call:{tool_call_id}")),
                    EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                        policy: "tool_not_in_toolset".to_string(),
                        detail: format!(
                            "tool `{tool_id}` is not in worker `{worker_agent_id}` toolset"
                        ),
                    }),
                )?;

                return Err(CoordinatorError::PolicyViolation(format!(
                    "tool `{tool_id}` is not in worker toolset"
                )));
            }

            Some(worker_profile.name.clone())
        } else {
            actor
                .agent_id
                .as_deref()
                .and_then(|agent_id| run_state.agents.get(agent_id))
                .map(|profile| profile.name.clone())
                .or_else(|| Some("default".to_string()))
        };

        let raw_permission_kind = permission_kind_for_tool_call(&tool_id, capability);
        let skip_outer_question_permission = raw_permission_kind == Some(PermissionKind::Question);
        let maybe_kind = if skip_outer_question_permission {
            None
        } else {
            raw_permission_kind
        };
        let rule_selectors = maybe_kind
            .map(|kind| {
                permission_rule_request_selectors(&run_state.info.workspace_root, kind, &args_json)
            })
            .unwrap_or_default();
        let effective_permission_ruleset = if actor.kind == ActorKind::Worker {
            actor
                .agent_id
                .as_deref()
                .and_then(|id| run_state.agents.get(id))
                .map(|profile| profile.permission_ruleset.as_slice())
        } else {
            effective_profile_name
                .as_deref()
                .and_then(|name| self.config.agent_profiles.get(name))
                .map(|profile| profile.permission_ruleset.as_slice())
        }
        .unwrap_or_default();
        let decision = maybe_kind.map(|kind| {
            evaluate_permission_rule_requests_with_ruleset(
                &self.config.permission_policy,
                effective_profile_name.as_deref(),
                kind,
                &rule_selectors,
                effective_permission_ruleset,
            )
        });
        let hashline_edit = hashline_edit_metadata(&tool_id, &args_json, &tool_call_id);

        let ruleset_denied = if actor.kind == ActorKind::Worker {
            actor
                .agent_id
                .as_deref()
                .and_then(|id| run_state.agents.get(id))
                .is_some_and(|profile| {
                    crate::perm::is_tool_call_disabled(
                        &tool_id,
                        capability,
                        &profile.permission_ruleset,
                    )
                })
        } else {
            effective_profile_name
                .as_deref()
                .and_then(|name| self.config.agent_profiles.get(name))
                .is_some_and(|profile| {
                    crate::perm::is_tool_call_disabled(
                        &tool_id,
                        capability,
                        &profile.permission_ruleset,
                    )
                })
        };
        if ruleset_denied {
            let reason =
                format!("tool `{tool_id}` is catch-all denied by profile permission ruleset");
            finalize_permission_denied(
                clock.as_ref(),
                redactor.as_ref(),
                self.config.hook_command_executor.as_ref(),
                &self.config.hook_runtime_config,
                run_state,
                PermissionDeniedArgs {
                    actor: actor.clone(),
                    profile: effective_profile_name.clone(),
                    tool_id: &tool_id,
                    args_json: &args_json,
                    tool_call_id: &tool_call_id,
                    hashline_edit: hashline_edit.as_ref(),
                    kind: maybe_kind.unwrap_or(PermissionKind::Task),
                    reason: &reason,
                    request_correlation_id: request_correlation_id.as_deref(),
                },
            )
            .await?;
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
            }
            return Err(CoordinatorError::PermissionDenied(tool_call_id));
        }

        match decision {
            Some(PolicyDecision::Deny) => {
                finalize_permission_denied(
                    clock.as_ref(),
                    redactor.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    run_state,
                    PermissionDeniedArgs {
                        actor: actor.clone(),
                        profile: effective_profile_name.clone(),
                        tool_id: &tool_id,
                        args_json: &args_json,
                        tool_call_id: &tool_call_id,
                        hashline_edit: hashline_edit.as_ref(),
                        kind: maybe_kind.unwrap_or_abort(),
                        reason: "policy denied request",
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )
                .await?;
                if let Some(respond_to) = respond_to {
                    let _ =
                        respond_to.send(Err("tool call denied: policy denied request".to_string()));
                }
                return Err(CoordinatorError::PermissionDenied(tool_call_id));
            }
            Some(PolicyDecision::Ask {
                timeout_ms,
                default_decision,
            }) => {
                let permission_id = format!("perm_{:06}", run_state.next_permission_id);
                run_state.next_permission_id += 1;

                let summary = permission_summary(self.redactor.as_ref(), &tool_id, &args_json);
                let digest = permission_request_digest(&tool_id, &args_json);
                let hook_request_id = request_correlation_id
                    .clone()
                    .or_else(|| Some(tool_call_id.clone()));
                let kind = maybe_kind.unwrap_or_abort();
                let grant_request = permission_grant_request(
                    &run_state.info.workspace_root,
                    kind,
                    &tool_id,
                    &args_json,
                    &digest,
                );

                if run_state.permission_grant_authorizes(&grant_request) {
                    gate_doom_loop_and_start(
                        clock.as_ref(),
                        redactor.as_ref(),
                        Arc::clone(&self.config.hook_command_executor),
                        job_tx,
                        run_state,
                        self.config.hook_runtime_config.clone(),
                        &self.config.permission_policy,
                        ToolCallExecutionArgs {
                            tool_call_id: tool_call_id.clone(),
                            tool_id,
                            args_json,
                            actor,
                            profile: effective_profile_name.clone(),
                            permission_ruleset: effective_permission_ruleset.to_vec(),
                            hook_executions: Vec::new(),
                            tool_registry: Arc::clone(&self.config.tool_registry),
                            request_correlation_id,
                            respond_to,
                            external_directory_allow_prefixes: Vec::new(),
                        },
                    )
                    .await?;
                    return Ok(tool_call_id);
                }

                append_permission_requested_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    PermissionRequestedEventArgs {
                        permission_id: &permission_id,
                        tool_call_id: &tool_call_id,
                        kind,
                        summary: summary.clone(),
                        request_digest: digest,
                        timeout_ms,
                        default_decision: event_permission_decision(default_decision),
                        request_correlation_id: request_correlation_id.as_deref(),
                    },
                )?;

                let requested_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::PermissionRequested,
                        run_id: run_state.info.run_id.to_string(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(actor.clone()),
                        agent_id: actor.agent_id.clone(),
                        request_id: hook_request_id.clone(),
                        permission_id: Some(permission_id.clone()),
                        task_id: None,
                        tool_call_id: Some(tool_call_id.clone()),
                        tool_id: Some(tool_id.clone()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        profile: effective_profile_name.clone(),
                        outcome: Some("requested".to_string()),
                        output_summary: Some(summary),
                        failure_reason: None,
                    },
                )
                .await;

                let mut pending = PendingPermissionState {
                    tool_call_id: tool_call_id.clone(),
                    request_correlation_id,
                    hook_executions: requested_hook_batch.hook_executions.clone(),
                    grant_request: Some(grant_request),
                    resolution: PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        profile: effective_profile_name.clone(),
                        respond_to,
                    },
                };

                if let Some(reason) = requested_hook_batch.critical_failure {
                    let mut final_reason = format!("critical lifecycle hook failed: {reason}");

                    append_permission_resolved_event(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        permission_id.clone(),
                        EventPermissionDecision::Deny,
                        Some(final_reason.clone()),
                    )?;

                    let resolved_hook_batch = hooks::run_lifecycle_hooks(
                        self.clock.as_ref(),
                        self.config.hook_command_executor.as_ref(),
                        &self.config.hook_runtime_config,
                        HookInvocationContext {
                            event: HookLifecycleEvent::PermissionResolved,
                            run_id: run_state.info.run_id.to_string(),
                            workspace_root: run_state.info.workspace_root.clone(),
                            artifacts_dir: run_state.info.artifacts_dir.clone(),
                            actor: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { actor, .. } => {
                                    Some(actor.clone())
                                }
                                PendingPermissionResolution::Question { .. } => {
                                    Some(system_actor())
                                }
                            },
                            agent_id: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { actor, .. } => {
                                    actor.agent_id.clone()
                                }
                                PendingPermissionResolution::Question { .. } => None,
                            },
                            request_id: hook_request_id,
                            permission_id: Some(permission_id),
                            task_id: None,
                            tool_call_id: Some(pending.tool_call_id.clone()),
                            tool_id: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { tool_id, .. } => {
                                    Some(tool_id.clone())
                                }
                                PendingPermissionResolution::Question { .. } => {
                                    Some("question".to_string())
                                }
                            },
                            provider_id: None,
                            model_id: None,
                            parent_agent_id: None,
                            profile: match &pending.resolution {
                                PendingPermissionResolution::ToolCall { profile, .. } => {
                                    profile.clone()
                                }
                                PendingPermissionResolution::Question { .. } => None,
                            },
                            outcome: Some("deny".to_string()),
                            output_summary: Some(final_reason.clone()),
                            failure_reason: Some(final_reason.clone()),
                        },
                    )
                    .await;
                    pending
                        .hook_executions
                        .extend(resolved_hook_batch.hook_executions.clone());
                    if let Some(resolved_reason) = resolved_hook_batch.critical_failure {
                        final_reason = format!(
                            "{final_reason}; critical lifecycle hook failed: {resolved_reason}"
                        );
                    }

                    let response_message = format!("tool call denied: {final_reason}");
                    let pending_hook_executions = pending.hook_executions.clone();
                    reject_pending_permission(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &final_reason,
                        &response_message,
                        pending,
                        &pending_hook_executions,
                    )?;
                    return Err(CoordinatorError::LifecycleHookFailed(final_reason));
                }

                run_state.insert_pending_permission(permission_id.clone(), pending);

                if timeout_ms > 0 {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                        let _ = job_tx
                            .send(Command::PermissionTimedOut { permission_id })
                            .await;
                    });
                }
            }
            Some(PolicyDecision::Allow) | None => {
                gate_doom_loop_and_start(
                    clock.as_ref(),
                    redactor.as_ref(),
                    Arc::clone(&self.config.hook_command_executor),
                    job_tx,
                    run_state,
                    self.config.hook_runtime_config.clone(),
                    &self.config.permission_policy,
                    ToolCallExecutionArgs {
                        tool_call_id: tool_call_id.clone(),
                        tool_id,
                        args_json,
                        actor,
                        profile: effective_profile_name.clone(),
                        permission_ruleset: effective_permission_ruleset.to_vec(),
                        hook_executions: Vec::new(),
                        tool_registry: Arc::clone(&self.config.tool_registry),
                        request_correlation_id,
                        respond_to,
                        external_directory_allow_prefixes: Vec::new(),
                    },
                )
                .await?;
            }
        }

        Ok(tool_call_id)
    }
}

// Third consecutive identical call (tool id + permission_request_digest) → DoomLoop.
pub(in crate::coord) const DOOM_LOOP_STREAK_THRESHOLD: u32 = 3;

pub(in crate::coord) async fn gate_doom_loop_and_start<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    permission_policy: &PermissionPolicy,
    mut args: ToolCallExecutionArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let digest = permission_request_digest(&args.tool_id, &args.args_json);
    let streak = run_state.note_identical_tool_call(&args.tool_id, &digest);

    if run_state.doom_loop_always_granted || streak < DOOM_LOOP_STREAK_THRESHOLD {
        return gate_external_directory_and_start(
            clock,
            redactor,
            hook_command_executor,
            job_tx,
            run_state,
            hook_runtime_config,
            permission_policy,
            args,
        )
        .await;
    }

    let decision = evaluate_permission_rule_requests_with_ruleset(
        permission_policy,
        args.profile.as_deref(),
        PermissionKind::DoomLoop,
        &[],
        &args.permission_ruleset,
    );
    let grant_request = permission_grant_request(
        &run_state.info.workspace_root,
        PermissionKind::DoomLoop,
        &args.tool_id,
        &args.args_json,
        &digest,
    );

    if run_state.permission_grant_authorizes(&grant_request) {
        return gate_external_directory_and_start(
            clock,
            redactor,
            hook_command_executor,
            job_tx,
            run_state,
            hook_runtime_config,
            permission_policy,
            args,
        )
        .await;
    }

    match decision {
        PolicyDecision::Allow => {
            gate_external_directory_and_start(
                clock,
                redactor,
                hook_command_executor,
                job_tx,
                run_state,
                hook_runtime_config,
                permission_policy,
                args,
            )
            .await
        }
        PolicyDecision::Deny => {
            let reason = "policy denied doom_loop request";
            let respond_to = args.respond_to.take();
            finalize_permission_denied(
                clock,
                redactor,
                hook_command_executor.as_ref(),
                &hook_runtime_config,
                run_state,
                PermissionDeniedArgs {
                    actor: args.actor.clone(),
                    profile: args.profile.clone(),
                    tool_id: &args.tool_id,
                    args_json: &args.args_json,
                    tool_call_id: &args.tool_call_id,
                    hashline_edit: None,
                    kind: PermissionKind::DoomLoop,
                    reason,
                    request_correlation_id: args.request_correlation_id.as_deref(),
                },
            )
            .await?;
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
            }
            Err(CoordinatorError::PermissionDenied(args.tool_call_id))
        }
        PolicyDecision::Ask {
            timeout_ms,
            default_decision,
        } => {
            let permission_id = format!("perm_{:06}", run_state.next_permission_id);
            run_state.next_permission_id += 1;
            let summary = format!(
                "doom_loop: identical tool call streak={streak} tool={}",
                args.tool_id
            );
            let hook_request_id = args
                .request_correlation_id
                .clone()
                .or_else(|| Some(args.tool_call_id.clone()));

            append_permission_requested_event(
                clock,
                redactor,
                run_state,
                PermissionRequestedEventArgs {
                    permission_id: &permission_id,
                    tool_call_id: &args.tool_call_id,
                    kind: PermissionKind::DoomLoop,
                    summary: summary.clone(),
                    request_digest: digest,
                    timeout_ms,
                    default_decision: event_permission_decision(default_decision),
                    request_correlation_id: args.request_correlation_id.as_deref(),
                },
            )?;

            let requested_hook_batch = hooks::run_lifecycle_hooks(
                clock,
                hook_command_executor.as_ref(),
                &hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::PermissionRequested,
                    run_id: run_state.info.run_id.to_string(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(args.actor.clone()),
                    agent_id: args.actor.agent_id.clone(),
                    request_id: hook_request_id,
                    permission_id: Some(permission_id.clone()),
                    task_id: None,
                    tool_call_id: Some(args.tool_call_id.clone()),
                    tool_id: Some(args.tool_id.clone()),
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    profile: args.profile.clone(),
                    outcome: Some("requested".to_string()),
                    output_summary: Some(summary),
                    failure_reason: None,
                },
            )
            .await;

            let pending = PendingPermissionState {
                tool_call_id: args.tool_call_id.clone(),
                request_correlation_id: args.request_correlation_id.clone(),
                hook_executions: requested_hook_batch.hook_executions.clone(),
                grant_request: Some(grant_request),
                resolution: PendingPermissionResolution::ToolCall {
                    tool_id: args.tool_id,
                    args_json: args.args_json,
                    actor: args.actor,
                    profile: args.profile,
                    respond_to: args.respond_to,
                },
            };

            if let Some(reason) = requested_hook_batch.critical_failure {
                let final_reason = format!("critical lifecycle hook failed: {reason}");
                append_permission_resolved_event(
                    clock,
                    redactor,
                    run_state,
                    permission_id,
                    EventPermissionDecision::Deny,
                    Some(final_reason.clone()),
                )?;
                let response_message = format!("tool call denied: {final_reason}");
                let pending_hook_executions = pending.hook_executions.clone();
                reject_pending_permission(
                    clock,
                    redactor,
                    run_state,
                    &final_reason,
                    &response_message,
                    pending,
                    &pending_hook_executions,
                )?;
                return Err(CoordinatorError::LifecycleHookFailed(final_reason));
            }

            run_state.insert_pending_permission(permission_id.clone(), pending);

            if timeout_ms > 0 {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = job_tx
                        .send(Command::PermissionTimedOut { permission_id })
                        .await;
                });
            }
            Ok(())
        }
    }
}

pub(in crate::coord) async fn gate_external_directory_and_start<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    permission_policy: &PermissionPolicy,
    mut args: ToolCallExecutionArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let collection = collect_external_directory_paths(
        &run_state.info.workspace_root,
        &args.tool_id,
        &args.args_json,
    );

    if let Some(reason) = collection.hard_deny {
        let respond_to = args.respond_to.take();
        finalize_permission_denied(
            clock,
            redactor,
            hook_command_executor.as_ref(),
            &hook_runtime_config,
            run_state,
            PermissionDeniedArgs {
                actor: args.actor.clone(),
                profile: args.profile.clone(),
                tool_id: &args.tool_id,
                args_json: &args.args_json,
                tool_call_id: &args.tool_call_id,
                hashline_edit: None,
                kind: PermissionKind::ExternalDirectory,
                reason: &reason,
                request_correlation_id: args.request_correlation_id.as_deref(),
            },
        )
        .await?;
        if let Some(respond_to) = respond_to {
            let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
        }
        return Err(CoordinatorError::PermissionDenied(args.tool_call_id));
    }

    if collection.paths.is_empty() {
        args.external_directory_allow_prefixes = Vec::new();
        return start_tool_call_execution(
            clock,
            redactor,
            hook_command_executor,
            job_tx,
            run_state,
            hook_runtime_config,
            args,
        )
        .await;
    }

    let selectors = collection
        .paths
        .iter()
        .map(|path| PermissionRuleRequest::WorkspacePath(path.display().to_string()))
        .collect::<Vec<_>>();
    let decision = evaluate_permission_rule_requests_with_ruleset(
        permission_policy,
        args.profile.as_deref(),
        PermissionKind::ExternalDirectory,
        &selectors,
        &args.permission_ruleset,
    );
    let digest = permission_request_digest(&args.tool_id, &args.args_json);
    let authorized = external_directory_grants_authorize(
        run_state,
        &run_state.info.workspace_root,
        &args.tool_id,
        &args.args_json,
        &collection.paths,
        &digest,
    );

    match decision {
        PolicyDecision::Deny => {
            let reason = "policy denied external_directory request";
            let respond_to = args.respond_to.take();
            finalize_permission_denied(
                clock,
                redactor,
                hook_command_executor.as_ref(),
                &hook_runtime_config,
                run_state,
                PermissionDeniedArgs {
                    actor: args.actor.clone(),
                    profile: args.profile.clone(),
                    tool_id: &args.tool_id,
                    args_json: &args.args_json,
                    tool_call_id: &args.tool_call_id,
                    hashline_edit: None,
                    kind: PermissionKind::ExternalDirectory,
                    reason,
                    request_correlation_id: args.request_correlation_id.as_deref(),
                },
            )
            .await?;
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(format!("tool call denied: {reason}")));
            }
            Err(CoordinatorError::PermissionDenied(args.tool_call_id))
        }
        PolicyDecision::Ask {
            timeout_ms,
            default_decision,
        } if !authorized => {
            let permission_id = format!("perm_{:06}", run_state.next_permission_id);
            run_state.next_permission_id += 1;
            let summary = external_directory_summary(&collection.paths);
            let grant_request = permission_grant_request(
                &run_state.info.workspace_root,
                PermissionKind::ExternalDirectory,
                &args.tool_id,
                &args.args_json,
                &digest,
            );
            let hook_request_id = args
                .request_correlation_id
                .clone()
                .or_else(|| Some(args.tool_call_id.clone()));

            append_permission_requested_event(
                clock,
                redactor,
                run_state,
                PermissionRequestedEventArgs {
                    permission_id: &permission_id,
                    tool_call_id: &args.tool_call_id,
                    kind: PermissionKind::ExternalDirectory,
                    summary: summary.clone(),
                    request_digest: digest,
                    timeout_ms,
                    default_decision: event_permission_decision(default_decision),
                    request_correlation_id: args.request_correlation_id.as_deref(),
                },
            )?;

            let requested_hook_batch = hooks::run_lifecycle_hooks(
                clock,
                hook_command_executor.as_ref(),
                &hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::PermissionRequested,
                    run_id: run_state.info.run_id.to_string(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(args.actor.clone()),
                    agent_id: args.actor.agent_id.clone(),
                    request_id: hook_request_id,
                    permission_id: Some(permission_id.clone()),
                    task_id: None,
                    tool_call_id: Some(args.tool_call_id.clone()),
                    tool_id: Some(args.tool_id.clone()),
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    profile: args.profile.clone(),
                    outcome: Some("requested".to_string()),
                    output_summary: Some(summary),
                    failure_reason: None,
                },
            )
            .await;

            let pending = PendingPermissionState {
                tool_call_id: args.tool_call_id.clone(),
                request_correlation_id: args.request_correlation_id.clone(),
                hook_executions: requested_hook_batch.hook_executions.clone(),
                grant_request: Some(grant_request),
                resolution: PendingPermissionResolution::ToolCall {
                    tool_id: args.tool_id,
                    args_json: args.args_json,
                    actor: args.actor,
                    profile: args.profile,
                    respond_to: args.respond_to,
                },
            };

            if let Some(reason) = requested_hook_batch.critical_failure {
                let final_reason = format!("critical lifecycle hook failed: {reason}");
                append_permission_resolved_event(
                    clock,
                    redactor,
                    run_state,
                    permission_id,
                    EventPermissionDecision::Deny,
                    Some(final_reason.clone()),
                )?;
                let response_message = format!("tool call denied: {final_reason}");
                let pending_hook_executions = pending.hook_executions.clone();
                reject_pending_permission(
                    clock,
                    redactor,
                    run_state,
                    &final_reason,
                    &response_message,
                    pending,
                    &pending_hook_executions,
                )?;
                return Err(CoordinatorError::LifecycleHookFailed(final_reason));
            }

            run_state.insert_pending_permission(permission_id.clone(), pending);

            if timeout_ms > 0 {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = job_tx
                        .send(Command::PermissionTimedOut { permission_id })
                        .await;
                });
            }
            Ok(())
        }
        PolicyDecision::Allow
        | PolicyDecision::Ask {
            timeout_ms: _,
            default_decision: _,
        } => {
            args.external_directory_allow_prefixes =
                call_scoped_external_allow_prefixes(&collection.paths);
            start_tool_call_execution(
                clock,
                redactor,
                hook_command_executor,
                job_tx,
                run_state,
                hook_runtime_config,
                args,
            )
            .await
        }
    }
}

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
        profile,
        permission_ruleset: _,
        hook_executions,
        tool_registry,
        request_correlation_id,
        respond_to,
        external_directory_allow_prefixes,
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
            run_id: run_state.info.run_id.to_string(),
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
            profile: profile.clone(),
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
            task_id: task_id.clone().into(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.queue_key()),
        }),
    )?;

    let cancellation_token = run_state.shutdown_token.child_token();
    let tool_state = run_state.tool_state.clone();
    let run_id = run_state.info.run_id.to_string();
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
            profile: profile.clone(),
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
            profile,
            tool_call_id: tool_call_id.clone().into(),
            current_model_ref: current_model
                .as_ref()
                .map(|(model_ref, _)| model_ref.clone()),
            current_model_settings: current_model.as_ref().map(|(_, settings)| settings.clone()),
            tool_state,
            external_directory_allow_prefixes,
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
