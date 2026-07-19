// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::*;

impl Coordinator {
    pub(in crate::coord) fn current_run_info_internal(&self) -> Result<RunInfo, CoordinatorError> {
        self.run_state
            .as_ref()
            .map(|run_state| run_state.info.clone())
            .ok_or(CoordinatorError::RunNotStarted)
    }

    pub(in crate::coord) fn update_session_title_internal(
        &mut self,
        title: String,
    ) -> Result<RunInfo, CoordinatorError> {
        let title = non_empty_trimmed(&title)
            .ok_or(CoordinatorError::InvalidSessionTitle)?
            .to_string();
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        let run_stream_key = format!("run:{}", run_state.info.run_id);
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            system_actor(),
            Some(run_stream_key),
            EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                title: title.clone(),
            }),
        )?;
        run_state.info.run_name = title.into();
        write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
        Ok(run_state.info.clone())
    }

    #[cfg(test)]
    pub(in crate::coord) fn start_run_internal(
        &mut self,
        run_name: String,
        workspace_root: PathBuf,
    ) -> Result<RunInfo, CoordinatorError> {
        block_on_coordinator_future(self.start_run_internal_async(run_name, workspace_root))
    }

    pub(in crate::coord) async fn start_run_internal_async(
        &mut self,
        run_name: String,
        workspace_root: PathBuf,
    ) -> Result<RunInfo, CoordinatorError> {
        if self.run_state.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }

        let run_id = if let Some(run_id) = self.config.run_id_override.clone() {
            run_id
        } else {
            let run_id = format!("run_{:06}", self.next_run_id);
            self.next_run_id += 1;
            run_id
        };

        let run_dir = self.config.session_dir.join(&run_id);
        let artifacts_dir = run_dir.join(ARTIFACTS_DIR_NAME);
        fs::create_dir_all(&artifacts_dir).map_err(|source| {
            CoordinatorError::CreateSessionDirectory {
                path: artifacts_dir.display().to_string(),
                source,
            }
        })?;

        let event_store = JsonlFileEventStore::open(
            &self.config.session_dir,
            &run_id,
            self.config.deterministic_store,
        )?;
        let event_store = Arc::new(event_store);
        let events_path = event_store.file_path().to_path_buf();

        let run_info = RunInfo {
            run_id: crate::ids::RunId::from(run_id.clone()),
            run_name: run_name.clone().into(),
            workspace_root: workspace_root.clone(),
            run_dir,
            artifacts_dir,
            events_path,
        };

        let next_agent_id = next_agent_counter_for_run(&self.config.session_dir, &run_id, 0)?;

        let mut run_state = RunState {
            info: run_info.clone(),
            event_store,
            next_event_seq: 1,
            next_agent_id,
            next_tool_call_id: 1,
            next_task_id: 1,
            next_provider_request_id: 1,
            next_permission_id: 1,
            agents: BTreeMap::new(),
            provider_context_by_agent: BTreeMap::new(),
            tasks: BTreeMap::new(),
            task_hook_state: BTreeMap::new(),
            agent_hook_state: BTreeMap::new(),
            subagent_parent_by_id: BTreeMap::new(),
            child_session_mirrors: BTreeMap::new(),
            child_request_session_by_id: BTreeMap::new(),
            background_notification_child_requests: BTreeSet::new(),
            pending_agent_wakeups: BTreeMap::new(),
            pending_permissions: BTreeMap::new(),
            active_permission_grants: PermissionGrantSet::default(),
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            failed_terminal_compaction_attempts: BTreeSet::new(),
            overflow_retry_compacted_context_by_attempt: BTreeMap::new(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: true,
            shutdown_token: CancellationToken::new(),
            tool_state: ToolRunState::default(),
            last_identical_tool_key: None,
            identical_tool_call_streak: 0,
            doom_loop_always_granted: false,
            edit_attribution: crate::edit_attribution::EditAttributionJournal::open(
                workspace_root.clone(),
            )
            .unwrap_or_else(|_| {
                crate::edit_attribution::EditAttributionJournal::empty(workspace_root.clone())
            }),
            team_registry: crate::team_registry::TeamRegistry::new(),
            cron_schedules: crate::cron_schedule::CronScheduleRegistry::new(),
            plugin_lifecycle: crate::integrations::PluginLifecycleRegistry::new(
                workspace_root.clone(),
            ),
        };

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(format!("run:{run_id}")),
            EventV1::RunStarted(RunStartedEvent {
                run_name: run_name.into(),
                workspace_root: workspace_root.display().to_string(),
            }),
        )?;

        write_run_metadata(&run_state, &self.config, self.clock.as_ref())?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunStarted,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(system_actor()),
                agent_id: None,
                request_id: None,
                permission_id: None,
                task_id: None,
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: None,
                outcome: Some("started".to_string()),
                output_summary: Some(run_state.info.run_name.to_string()),
                failure_reason: None,
            },
        )
        .await;
        if let Some(reason) = hook_batch.critical_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        self.run_state = Some(run_state);
        Ok(run_info)
    }

    pub(in crate::coord) fn resume_run_internal(
        &mut self,
        run_id: String,
        run_name: String,
    ) -> Result<RunInfo, CoordinatorError> {
        if self.run_state.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }

        let run_dir = self.config.session_dir.join(&run_id);
        let event_store = JsonlFileEventStore::open_existing(
            &self.config.session_dir,
            &run_id,
            self.config.deterministic_store,
        )?;
        let event_store = Arc::new(event_store);

        let resume_plan = inspect_resume_plan(&run_dir);
        if !resume_plan.is_resumable {
            let reason = resume_plan
                .resume_disabled_reason
                .unwrap_or_else(|| "resume disabled without reason".to_string());
            return Err(CoordinatorError::ResumeDisabled { run_id, reason });
        }

        if resume_plan.run_id != run_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.clone(),
                reason: format!(
                    "resume plan run_id mismatch: expected `{}`, actual `{}`",
                    run_id, resume_plan.run_id
                ),
            });
        }

        let workspace_root = resume_plan
            .workspace_root
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(PathBuf::from)
            .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.clone(),
                reason: "workspace root is missing from resume plan".to_string(),
            })?;

        let next_event_seq = checked_next_counter(resume_plan.max_seq, &run_id, "event sequence")?;
        let store_next_seq = event_store.next_seq()?;
        if store_next_seq != next_event_seq {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id,
                reason: format!(
                    "event-store sequence mismatch: resume plan expects {next_event_seq}, store reports {store_next_seq}"
                ),
            });
        }

        let mut agents = BTreeMap::new();
        let mut restored_agent_bindings = Vec::new();
        let mut restored_subagent_parent_by_id = BTreeMap::new();
        let mut max_agent_id = 0_u64;
        for (agent_id, profile_name) in &resume_plan.known_agents {
            let parsed_agent_id = parse_prefixed_counter(agent_id, "agent_").ok_or_else(|| {
                CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.clone(),
                    reason: format!("invalid agent id in resume plan: `{agent_id}`"),
                }
            })?;
            max_agent_id = max_agent_id.max(parsed_agent_id);

            let profile_cfg = self
                .config
                .agent_profiles
                .get(profile_name)
                .cloned()
                .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.clone(),
                    reason: format!(
                        "historical agent `{agent_id}` references missing profile binding `{profile_name}`"
                    ),
                })?;
            let parent_agent_id = resume_plan
                .child_sessions
                .get(agent_id)
                .and_then(|child| child.parent_session_id.as_deref())
                .and_then(non_empty_trimmed)
                .map(str::to_string);

            if let Some(parent_agent_id) = parent_agent_id.as_ref() {
                restored_subagent_parent_by_id.insert(agent_id.clone(), parent_agent_id.clone());
            }

            agents.insert(agent_id.clone(), profile_cfg);
            restored_agent_bindings.push((agent_id.clone(), profile_name.clone(), parent_agent_id));
        }

        let provider_context_by_agent =
            restore_provider_context_from_history(&self.config.session_dir, &run_id)?;

        let next_agent_id =
            next_agent_counter_for_run(&self.config.session_dir, &run_id, max_agent_id)?;
        let next_tool_call_id = checked_next_counter(
            resume_plan.id_watermarks.max_tool_call_id,
            &run_id,
            "tool call id",
        )?;
        let next_task_id =
            checked_next_counter(resume_plan.id_watermarks.max_task_id, &run_id, "task id")?;
        let next_provider_request_id = checked_next_counter(
            resume_plan.id_watermarks.max_request_id,
            &run_id,
            "provider request id",
        )?;
        let next_permission_id = checked_next_counter(
            resume_plan.id_watermarks.max_permission_id,
            &run_id,
            "permission id",
        )?;

        let artifacts_dir = run_dir.join(ARTIFACTS_DIR_NAME);
        fs::create_dir_all(&artifacts_dir).map_err(|source| {
            CoordinatorError::CreateSessionDirectory {
                path: artifacts_dir.display().to_string(),
                source,
            }
        })?;

        let events_path = event_store.file_path().to_path_buf();
        let run_info = RunInfo {
            run_id: crate::ids::RunId::from(run_id.clone()),
            run_name: run_name.clone().into(),
            workspace_root: workspace_root.clone(),
            run_dir,
            artifacts_dir,
            events_path,
        };

        let mut run_state = RunState {
            info: run_info.clone(),
            event_store,
            next_event_seq,
            next_agent_id,
            next_tool_call_id,
            next_task_id,
            next_provider_request_id,
            next_permission_id,
            agents,
            provider_context_by_agent,
            tasks: BTreeMap::new(),
            task_hook_state: BTreeMap::new(),
            agent_hook_state: BTreeMap::new(),
            subagent_parent_by_id: restored_subagent_parent_by_id,
            child_session_mirrors: BTreeMap::new(),
            child_request_session_by_id: BTreeMap::new(),
            background_notification_child_requests: BTreeSet::new(),
            pending_agent_wakeups: BTreeMap::new(),
            pending_permissions: BTreeMap::new(),
            active_permission_grants: resume_plan.active_permission_grants,
            cancelled_running_tasks: BTreeSet::new(),
            queued_agent_turns: BTreeMap::new(),
            running_agent_turns: BTreeMap::new(),
            failed_terminal_compaction_attempts: BTreeSet::new(),
            overflow_retry_compacted_context_by_attempt: BTreeMap::new(),
            scheduler: Scheduler::new(SchedulerLimits {
                provider_model: self.config.provider_model_concurrency,
                tool: self.config.tool_concurrency,
            }),
            recorded_runtime_context: None,
            allow_initial_runtime_context_recording: false,
            shutdown_token: CancellationToken::new(),
            tool_state: ToolRunState::default(),
            last_identical_tool_key: None,
            identical_tool_call_streak: 0,
            doom_loop_always_granted: false,
            edit_attribution: crate::edit_attribution::EditAttributionJournal::open(
                workspace_root.clone(),
            )
            .unwrap_or_else(|_| {
                crate::edit_attribution::EditAttributionJournal::empty(workspace_root.clone())
            }),
            team_registry: crate::team_registry::TeamRegistry::new(),
            cron_schedules: crate::cron_schedule::CronScheduleRegistry::new(),
            plugin_lifecycle: crate::integrations::PluginLifecycleRegistry::new(
                workspace_root.clone(),
            ),
        };

        restore_child_session_mirrors(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &self.config,
            &mut run_state,
            &restored_agent_bindings,
        )?;

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(format!("run:{run_id}")),
            EventV1::RunStarted(RunStartedEvent {
                run_name: run_name.into(),
                workspace_root: workspace_root.display().to_string(),
            }),
        )?;

        for (agent_id, profile, parent_agent_id) in restored_agent_bindings {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                &mut run_state,
                system_actor(),
                Some(format!("agent:{agent_id}")),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id,
                    profile,
                    parent_agent_id,
                }),
            )?;
        }

        self.run_state = Some(run_state);
        Ok(run_info)
    }

    pub(in crate::coord) fn get_event_store_internal(
        &self,
    ) -> Result<Arc<JsonlFileEventStore>, CoordinatorError> {
        let run_state = self
            .run_state
            .as_ref()
            .ok_or(CoordinatorError::RunNotStarted)?;
        Ok(Arc::clone(&run_state.event_store))
    }

    pub(in crate::coord) async fn stop_run_internal(
        &mut self,
        summary: String,
    ) -> Result<(), CoordinatorError> {
        let mut run_state = self
            .run_state
            .take()
            .ok_or(CoordinatorError::RunNotStarted)?;

        run_state.shutdown_token.cancel();
        for task in run_state.tasks.values() {
            task.cancellation_token.cancel();
        }
        for task in run_state.running_agent_turns.values() {
            task.cancellation_token.cancel();
        }

        let run_stream_key = format!("run:{}", run_state.info.run_id);

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(run_stream_key),
            EventV1::RunFinished(RunFinishedEvent {
                summary: summary.clone(),
            }),
        )?;
        finish_child_session_mirrors(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &run_state,
            &summary,
        )?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunFinished,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(system_actor()),
                agent_id: None,
                request_id: None,
                permission_id: None,
                task_id: None,
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: None,
                outcome: Some("finished".to_string()),
                output_summary: Some(summary),
                failure_reason: None,
            },
        )
        .await;
        if let Some(reason) = hook_batch.critical_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        Ok(())
    }

    pub(in crate::coord) async fn fail_run_internal(
        &mut self,
        error: String,
    ) -> Result<(), CoordinatorError> {
        let mut run_state = self
            .run_state
            .take()
            .ok_or(CoordinatorError::RunNotStarted)?;

        run_state.shutdown_token.cancel();
        for task in run_state.tasks.values() {
            task.cancellation_token.cancel();
        }
        for task in run_state.running_agent_turns.values() {
            task.cancellation_token.cancel();
        }

        let run_stream_key = format!("run:{}", run_state.info.run_id);

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            &mut run_state,
            system_actor(),
            Some(run_stream_key),
            EventV1::RunFailed(RunFailedEvent {
                error: error.clone(),
            }),
        )?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::RunFailed,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(system_actor()),
                agent_id: None,
                request_id: None,
                permission_id: None,
                task_id: None,
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: None,
                outcome: Some("failed".to_string()),
                output_summary: None,
                failure_reason: Some(error),
            },
        )
        .await;
        if let Some(reason) = hook_batch.critical_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        Ok(())
    }

    pub(in crate::coord) async fn spawn_agent_internal(
        &mut self,
        actor: EventActor,
        profile: String,
        parent_agent_id: Option<String>,
        child_session_title: Option<String>,
        auto_start_turn: bool,
    ) -> Result<String, CoordinatorError> {
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        if actor.kind != ActorKind::Supervisor {
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("run:{}", run_state.info.run_id)),
                EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                    policy: "spawn_agent_requires_supervisor".to_string(),
                    detail: format!(
                        "only supervisor may spawn agents; got actor kind {:?}",
                        actor.kind
                    ),
                }),
            )?;

            return Err(CoordinatorError::PolicyViolation(
                "only supervisor may spawn agents".to_string(),
            ));
        }

        let mut profile_cfg = self
            .config
            .agent_profiles
            .get(&profile)
            .cloned()
            .ok_or_else(|| CoordinatorError::UnknownAgent(profile.clone()))?;

        if let Some(parent_id) = parent_agent_id.as_ref() {
            if let Some(parent_profile) = run_state.agents.get(parent_id) {
                let mut child_permission = profile_cfg.permission_ruleset.clone();
                if profile_cfg.toolset.iter().any(|tool| tool == "task")
                    && !child_permission
                        .iter()
                        .any(|rule| rule.permission == "task")
                {
                    child_permission.push(crate::perm::PermissionRule {
                        permission: "task".to_string(),
                        pattern: "*".to_string(),
                        action: crate::perm::PermissionAction::Allow,
                    });
                }
                if profile_cfg.toolset.iter().any(|tool| tool == "todowrite")
                    && !child_permission
                        .iter()
                        .any(|rule| rule.permission == "todowrite")
                {
                    child_permission.push(crate::perm::PermissionRule {
                        permission: "todowrite".to_string(),
                        pattern: "*".to_string(),
                        action: crate::perm::PermissionAction::Allow,
                    });
                }
                let derived = crate::perm::derive_subagent_session_permission(
                    &parent_profile.permission_ruleset,
                    &child_permission,
                );
                profile_cfg.permission_ruleset =
                    crate::perm::merge_rulesets([child_permission, derived]);
                profile_cfg.toolset.retain(|tool_id| {
                    !crate::perm::is_tool_disabled(tool_id, &profile_cfg.permission_ruleset)
                });
            }
        }

        let agent_id = format!("agent_{:06}", run_state.next_agent_id);
        run_state.next_agent_id += 1;

        let mut subagent_spawn_hook_executions = Vec::new();
        if let Some(parent) = parent_agent_id.as_ref() {
            let hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::SubagentSpawned,
                    run_id: run_state.info.run_id.to_string(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(actor.clone()),
                    agent_id: Some(agent_id.clone()),
                    request_id: None,
                    permission_id: None,
                    task_id: None,
                    tool_call_id: None,
                    tool_id: None,
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: Some(parent.clone()),
                    category: Some(profile.clone()),
                    outcome: Some("spawned".to_string()),
                    output_summary: Some(profile.clone()),
                    failure_reason: None,
                },
            )
            .await;
            subagent_spawn_hook_executions = hook_batch.hook_executions;
            if let Some(reason) = hook_batch.critical_failure {
                return Err(CoordinatorError::LifecycleHookFailed(reason));
            }
        }

        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("agent:{agent_id}")),
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: agent_id.clone(),
                profile: profile.clone(),
                parent_agent_id: parent_agent_id.clone(),
            }),
        )?;

        if parent_agent_id.is_some() {
            create_child_session_mirror(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                &self.config,
                run_state,
                &agent_id,
                &profile,
                child_session_title.as_deref(),
            )?;
        }

        let should_record_runtime_context =
            run_state.allow_initial_runtime_context_recording && parent_agent_id.is_none();
        run_state
            .agents
            .insert(agent_id.clone(), profile_cfg.clone());

        if should_record_runtime_context {
            run_state.recorded_runtime_context = Some(RecordedRuntimeContext::from_profile_model(
                &profile_cfg.name,
                &profile_cfg.model_ref,
            ));
            write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
            run_state.allow_initial_runtime_context_recording = false;
        }

        if let Some(parent) = parent_agent_id {
            run_state
                .subagent_parent_by_id
                .insert(agent_id.clone(), parent);
            if !subagent_spawn_hook_executions.is_empty() {
                run_state
                    .agent_hook_state
                    .entry(agent_id.clone())
                    .or_default()
                    .extend(subagent_spawn_hook_executions);
            }
        }

        if auto_start_turn {
            let request_id = allocate_provider_request_id(run_state);
            if run_state.child_session_mirrors.contains_key(&agent_id) {
                run_state
                    .child_request_session_by_id
                    .insert(request_id.clone(), agent_id.clone());
            }

            let request = AgentRequest {
                agent_id: agent_id.clone(),
                prompt: if profile_cfg.system_prompt.is_empty() {
                    format!("execute one-shot turn for {}", profile_cfg.name)
                } else {
                    profile_cfg.system_prompt.clone()
                },
                prompt_context: None,
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                model_ref: profile_cfg.model_ref.clone(),
                model_settings: default_model_settings_for_profile(&profile_cfg.name),
            };

            let model_fallback_chain = self
                .config
                .agent_model_fallbacks
                .get(&profile_cfg.name)
                .cloned()
                .unwrap_or_default();
            schedule_agent_turn(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                Arc::clone(&self.config.hook_command_executor),
                self.job_tx.clone(),
                run_state,
                self.config.hook_runtime_config.clone(),
                self.config.compaction.clone(),
                self.config.provider_retry,
                ScheduleAgentTurnArgs {
                    provider: Arc::clone(&self.config.provider),
                    tool_registry: Arc::clone(&self.config.tool_registry),
                    profile: profile_cfg,
                    request,
                    request_id,
                    child_task: None,
                    model_fallback_chain,
                },
            )
            .await?;
        }

        Ok(agent_id)
    }
}

pub(in crate::coord) fn write_run_metadata(
    run_state: &RunState,
    config: &CoordinatorConfig,
    clock: &dyn Clock,
) -> Result<(), CoordinatorError> {
    let metadata = RunMetadata {
        run_id: run_state.info.run_id.to_string(),
        run_name: run_state.info.run_name.to_string(),
        workspace_root: run_state.info.workspace_root.display().to_string(),
        created_at: if config.deterministic_store {
            None
        } else {
            clock.system_time_rfc3339()
        },
        config_digest: config.config_digest.clone(),
        harness_version: config.harness_version.clone(),
        recorded_runtime_context: run_state.recorded_runtime_context.clone(),
        mode_source: config.session_mode_source,
    };

    let meta_path = run_state.info.run_dir.join(META_FILE_NAME);
    let body = serde_json::to_string_pretty(&metadata)?;
    fs::write(&meta_path, body).map_err(|source| CoordinatorError::WriteRunMetadata {
        path: meta_path.display().to_string(),
        source,
    })?;

    Ok(())
}

fn checked_next_counter(
    value: u64,
    run_id: &str,
    counter_kind: &'static str,
) -> Result<u64, CoordinatorError> {
    value
        .checked_add(1)
        .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!("{counter_kind} counter overflow"),
        })
}

fn next_agent_counter_for_run(
    session_dir: &Path,
    run_id: &str,
    minimum_previous_agent_id: u64,
) -> Result<u64, CoordinatorError> {
    let mut max_agent_id = minimum_previous_agent_id;
    let entries =
        fs::read_dir(session_dir).map_err(|source| CoordinatorError::CreateSessionDirectory {
            path: session_dir.display().to_string(),
            source,
        })?;

    for entry in entries {
        let entry = entry.map_err(|source| CoordinatorError::CreateSessionDirectory {
            path: session_dir.display().to_string(),
            source,
        })?;
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(suffix) = name.strip_prefix("agent_") else {
            continue;
        };
        if suffix.len() != 6 || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if let Ok(parsed) = suffix.parse::<u64>() {
            max_agent_id = max_agent_id.max(parsed);
        }
    }

    checked_next_counter(max_agent_id, run_id, "agent id")
}
