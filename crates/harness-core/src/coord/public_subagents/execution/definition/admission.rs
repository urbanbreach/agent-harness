use super::*;

impl Runtime {
    pub(in crate::coord::public_subagents) fn admit_native_subagent(
        &mut self,
        actor: EventActor,
        tool: String,
        mut input: SpawnSubagentInput,
        fork: bool,
        checkpoint: Option<WorkspaceCreationCheckpoint>,
    ) -> Result<NativeSubscription, CoordinatorError> {
        let job = self.authenticated_native_tool(&actor, &tool, true)?;
        if let Some(schema) = &input.output_schema
            && crate::redact::redact_map(self.redactor.as_ref(), schema) != *schema
        {
            return Err(native_invalid(
                "Invalid output_schema: redaction policy would change the persisted schema".into(),
            ));
        }
        let output_contract = input
            .output_schema
            .as_ref()
            .map(crate::subagent::output_contract::compile)
            .transpose()
            .map_err(native_invalid)?;
        let cancellation = job.cancellation.clone();
        let parent_request = job.parent.clone();
        let parent = actor.agent_id.clone().ok_or_else(|| {
            CoordinatorError::PermissionDenied("subagent spawn requires a parent agent".into())
        })?;
        let (resolved, source_state, source_reference) =
            self.resolve_native_definition(&actor, &input)?;
        for warning in &resolved.warnings {
            self.live(
                actor.clone(),
                tool.clone(),
                LiveEventV1::RuntimeWarning {
                    message: self.redactor.redact_text(warning),
                },
            )?;
        }
        if self.config.permission_policy.check(
            "task",
            &resolved.subagent_type,
            Some(&self.agents[&parent].policy),
        ) == crate::perm::PermissionAction::Deny
        {
            return Err(CoordinatorError::PermissionDenied(resolved.subagent_type));
        }
        let root = self.native_root(&parent)?;
        if self.stopped_sessions.contains(&root) || self.killed_agents.contains(&parent) {
            return Err(CoordinatorError::Stopping);
        }
        let id = native_id(input.task_id.as_deref())?;
        self.check_eval_pool_child(&id, &parent)?;
        if self.native_subagents.contains_key(&id) || self.agents.contains_key(&id) {
            return Err(CoordinatorError::Native {
                code: "spawn_rejected".into(),
                message: format!("Subagent id '{id}' already exists"),
            });
        }
        input.cwd = workspace::sanitize_cwd(input.cwd.as_deref());
        if input
            .cwd
            .as_deref()
            .is_some_and(|cwd| Path::new(cwd).is_dir())
            && input.isolation == Some(SubagentIsolationMode::Worktree)
        {
            return Err(native_invalid(
                "cwd and isolation=\"worktree\" are mutually exclusive. Use cwd to point the subagent at an existing directory, or isolation=\"worktree\" to create a new isolated worktree, but not both.".into()
            ));
        }
        if input.isolation == Some(SubagentIsolationMode::Worktree) {
            input.cwd = None;
        }
        if let Some(cwd) = input
            .cwd
            .as_deref()
            .filter(|cwd| source_state.is_none() && !Path::new(cwd).is_dir())
        {
            return Err(native_invalid(if Path::new(cwd).exists() {
                format!("cwd \"{cwd}\" exists but is not a directory")
            } else {
                format!("cwd \"{cwd}\" does not exist")
            }));
        }
        let active = self
            .native_subagents
            .values()
            .filter(|child| {
                child.registration.root_agent == root
                    && matches!(
                        child.phase,
                        NativePhase::Preparing | NativePhase::Running | NativePhase::Finalizing
                    )
            })
            .count();
        if active >= self.config.subagents.max_concurrent
            && self.config.subagents.limit_behavior == SubagentLimitBehavior::Fail
        {
            return Err(native_invalid(format!(
                "Concurrent subagent limit reached: {} subagents are already running for this session. Do not retry; spawning succeeds again when a running subagent finishes.",
                self.config.subagents.max_concurrent
            )));
        }
        let parent_state = &self.agents[&root];
        let cwd = source_state
            .as_ref()
            .map(|state| PathBuf::from(&state.execution_context.effective_cwd))
            .filter(|cwd| cwd.is_dir())
            .or_else(|| {
                source_state
                    .is_none()
                    .then(|| input.cwd.as_ref().map(PathBuf::from))
                    .flatten()
            })
            .unwrap_or_else(|| parent_state.cwd.clone());
        if let Some(schema) = &input.output_schema {
            input.prompt.push_str("\n\n");
            input
                .prompt
                .push_str(&crate::subagent::output_contract::instructions(schema));
        }
        let mut system_parts = vec![resolved.definition.prompt_body.clone().unwrap_or_default()];
        system_parts.extend(resolved.role_prompt.clone());
        system_parts.extend(resolved.persona_instructions.clone());
        if let Some(schema) = &input.output_schema {
            system_parts.push(crate::subagent::output_contract::instructions(schema));
        }
        let registration = NativeSubagentRegistration {
            payload_version: 1,
            child_id: id.clone(),
            spawner: parent.clone(),
            root_agent: root.clone(),
            parent_tool: tool.clone(),
            parent_request,
            subagent_type: resolved.subagent_type.clone(),
            persona: resolved.persona.clone(),
            role: resolved.role_name.clone(),
            fork_context: fork && source_state.is_none(),
            description: input.description.clone(),
            prompt: input.prompt.clone(),
            output_schema: input.output_schema.clone(),
            background: input.background,
            isolation: if source_state.is_some() {
                match source_state
                    .as_ref()
                    .map(|s| &s.execution_context.isolation)
                {
                    Some(ResolvedSubagentIsolation::Worktree { .. }) => {
                        SubagentIsolationMode::Worktree
                    }
                    _ => SubagentIsolationMode::None,
                }
            } else {
                resolved.isolation
            },
            source: optional(input.resume_from.as_deref()),
            model: resolved.model.clone(),
            system_prompt: system_parts
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
            tools: resolved.tools.iter().map(|tool| tool.id.clone()).collect(),
            permission_rules: self.agents[&parent].profile.permission_ruleset.clone(),
            max_iters: resolved.max_turns.map(|n| n.get() as usize),
            allowed_types: resolved.allowed_subagent_types.clone(),
            model_inherited: self
                .native_model_policies
                .get(&parent)
                .is_some_and(|p| p.selection == SubagentModelSelection::Inherited),
            messaging_granted: resolved
                .tools
                .iter()
                .any(|tool| tool.id == "send_subagent_message"),
        };
        let fork_context = if fork && source_state.is_none() {
            self.active_contexts.get(&parent).cloned().or_else(|| {
                (!self.agents[&parent].messages.entries.is_empty())
                    .then(|| self.agents[&parent].messages.clone())
            })
        } else {
            None
        };
        let fork_read_state = fork_context
            .as_ref()
            .map(|_| self.agents[&parent].tool_state.read_snapshot())
            .transpose()
            .map_err(CoordinatorError::from)?;
        let fork_model_policy = fork_context
            .as_ref()
            .and_then(|_| self.native_model_policies.get(&parent).cloned());
        let parent_context = self
            .active_contexts
            .get(&parent)
            .unwrap_or(&self.agents[&parent].messages);
        let continue_parent_work = parent_context
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.message.role == MessageRole::User)
            .take(3)
            .any(|entry| {
                let text = entry.message.content.to_ascii_lowercase();
                [
                    "smoke",
                    "op_chain",
                    "ci fail",
                    "still fail",
                    "failing",
                    "bazel test",
                    "pytest",
                    "cargo test",
                    "npm test",
                    "deploy",
                    "bringup",
                    "implement",
                    "unfinished",
                    "rebase",
                    "adler",
                    "nondetermin",
                    "fix the ",
                    "fix these ",
                    "fix all ",
                    "gt submit",
                    "check ",
                    " bug",
                    "bugs",
                    "run the test",
                    "run tests",
                    "pass/fail",
                    "integration test",
                    "hw5",
                    "pr check",
                    "ci check",
                ]
                .iter()
                .any(|word| text.contains(word))
                    || text.contains("while waiting")
                    || text.contains("whilst waiting")
            });
        let notified_on_completion = registration.parent_request.is_some()
            && registration.spawner == registration.root_agent;
        self.emit(
            actor.clone(),
            Some(tool.clone()),
            EventV1::NativeSubagentRegistered(Box::new(registration.clone())),
        )?;
        self.config
            .tool_registry
            .inherit_scoped(
                &self.tool_scope(Some(&parent)).unwrap_or_default(),
                &self.tool_scope(Some(&id)).unwrap_or_default(),
            )
            .map_err(|error| native_invalid(error.to_string()))?;
        self.spawn_agent_with_profile(
            actor,
            registration.profile(),
            Some(parent),
            Some(id.clone()),
        )?;
        if let Some(policy) = fork_model_policy {
            self.native_model_policies.insert(id.clone(), policy);
        }
        let initial = NativeSnapshot {
            result: GetCommandOrSubagentOutputResult {
                task_id: id.clone(), command: format!("[subagent:{}] {}", registration.subagent_type, registration.description),
                status: "initializing".into(), started: native_timestamp(self.clock.as_ref()),
                output: format!("Subagent is initializing (creating worktree, resolving config).\nType: {}\nDescription: {}\nElapsed: 0.0s",
                    registration.subagent_type, registration.description), ..Default::default()
            },
            completed: None, error: None, terminal: false, demoted: false,
        };
        let (updates, receiver) = watch::channel(initial);
        self.native_subagents.insert(
            id.clone(),
            NativeSubagent {
                registration,
                output_contract,
                resolved: Some(resolved),
                phase: NativePhase::Queued,
                request: None,
                cancellation: CancellationToken::new(),
                foreground_attached: !input.background,
                explicitly_killed: false,
                terminal_published: false,
                updates,
                started_ms: self.clock.mono_ms(),
                started: native_timestamp(self.clock.as_ref()),
                ended: None,
                cwd,
                worktree: None,
                snapshot_ref: None,
                preparation: None,
                source_state,
                source_reference,
                fork_context,
                fork_read_state,
                messages: VecDeque::new(),
                outbound: 0,
                sender_generation: 0,
                waiters: if input.background {
                    BTreeSet::new()
                } else {
                    BTreeSet::from([tool])
                },
                completion_age: 0,
                consumed: false,
                wait_interrupt: watch::channel(0).0,
                parked: VecDeque::new(),
                displaced: None,
                terminal_event: None,
                pending_terminal: None,
                buffered_for: None,
                routed_to_root: false,
                creation_checkpoint: checkpoint,
                ownership: None,
                cleanup_pending: false,
                progress: None,
            },
        );
        self.native_subagent_queue.push_back(id.clone());
        self.pump_native_subagents()?;
        Ok(NativeSubscription {
            id,
            cancellation,
            updates: receiver,
            continue_parent_work,
            notified_on_completion,
        })
    }
}
