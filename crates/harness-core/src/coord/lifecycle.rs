use super::{handle::system, runtime::*, *};
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

impl Runtime {
    pub fn start(&mut self, name: String, root: PathBuf) -> Result<RunInfo, CoordinatorError> {
        if let Some(error) = &self.fault {
            return Err(CoordinatorError::Invalid(error.clone()));
        }
        if self.info.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }
        if self.config.command_buffer == 0
            || self.config.tool_concurrency == 0
            || self.config.provider_model_concurrency == 0
        {
            return Err(CoordinatorError::Invalid(
                "runtime concurrency and command buffer must be positive".into(),
            ));
        }
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(CoordinatorError::Invalid(
                "workspace must be a directory".into(),
            ));
        }
        let grants = super::grants::load_grants(&root, &[])?;
        let id = self.config.run_id_override.clone().unwrap_or_else(|| {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            format!("run_{}_{nanos}", std::process::id())
        });
        let store = crate::store::Journal::open(
            &self.config.session_dir,
            &id,
            self.config.deterministic_store,
        )?;
        if store.next_seq()? != 1 {
            return Err(CoordinatorError::Invalid(
                "session already exists; resume it explicitly".into(),
            ));
        }
        let dir = self.config.session_dir.join(&id);
        let info = RunInfo {
            run_id: id.into(),
            run_name: name.clone().into(),
            workspace_root: root.clone(),
            artifacts_dir: dir.join("artifacts"),
            events_path: store.file_path().into(),
            run_dir: dir,
        };
        self.info = Some(info.clone());
        self.store = Some(Arc::new(store));
        self.grants = grants;
        self.fault = None;
        self.emit(
            system(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: name.into(),
                workspace_root: root.to_string_lossy().into(),
            }),
        )?;
        if self.config.yolo_on_start {
            self.set_yolo_mode(true)?;
        }
        self.start_hooks()?;
        self.metadata = Some(self.new_metadata()?);
        self.write_metadata()?;
        Ok(info)
    }
    pub fn spawn_agent(
        &mut self,
        actor: EventActor,
        name: &str,
        parent: Option<String>,
    ) -> Result<String, CoordinatorError> {
        let profile = self
            .profiles
            .get(name)
            .cloned()
            .ok_or_else(|| CoordinatorError::UnknownProfile(name.into()))?;
        self.spawn_agent_with_profile(actor, profile, parent, None)
    }
    pub(super) fn spawn_agent_with_profile(
        &mut self,
        actor: EventActor,
        profile: Arc<AgentProfile>,
        parent: Option<String>,
        requested_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        let profile_name = profile.name.clone();
        let name = profile_name.as_str();
        self.accepting()?;
        if self.stopped_sessions.contains(self.info()?.run_id.as_str())
            || parent.as_ref().is_some_and(|id| {
                self.stopped_sessions.contains(id) || self.killed_agents.contains(id)
            })
        {
            return Err(CoordinatorError::Stopping);
        }
        if let Some(id) = &parent
            && !self.agents.contains_key(id)
        {
            return Err(CoordinatorError::UnknownAgent(id.clone()));
        }
        let policy = PermissionPolicy::from_rules(profile.permission_ruleset.clone())
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        let id = match requested_id {
            Some(id) => {
                if self.agents.contains_key(&id) {
                    return Err(CoordinatorError::Invalid(
                        "agent identity already exists".into(),
                    ));
                }
                id
            }
            None if parent.is_some() => self.child_agent_id()?,
            None => self.id("agent")?,
        };
        self.emit_hooked(
            actor,
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: id.clone(),
                profile: name.into(),
                parent_agent_id: parent.clone(),
            }),
        )?;
        let info = AgentRuntimeInfo {
            agent_id: id.clone(),
            profile_name: name.into(),
            model_ref: profile.model_ref.clone(),
            model_ref_explicit: profile.model_ref_explicit,
            toolset: profile.toolset.clone(),
            parent_agent_id: parent,
        };
        let messages = super::context::Context::new(&profile.system_prompt);
        let execution = crate::subagent::ResolvedSubagentContext {
            effective_cwd: self.info()?.workspace_root.to_string_lossy().into_owned(),
            policy_roots: vec![self.info()?.workspace_root.to_string_lossy().into_owned()],
            isolation: crate::subagent::ResolvedSubagentIsolation::SharedWorkspace,
        };
        let tool_state = self.tool_state.fresh_owner();
        let cwd = self.info()?.workspace_root.clone();
        self.agents.insert(
            id.clone(),
            Agent {
                info,
                target: self.config.agent_model_targets.get(name).cloned(),
                settings: self
                    .config
                    .agent_model_targets
                    .get(name)
                    .map(AgentModelSettings::from)
                    .unwrap_or_default(),
                profile,
                policy,
                messages,
                queue: VecDeque::new(),
                busy: false,
                execution,
                cwd,
                tool_state,
                generation: 0,
                prompt_turns: 0,
                tool_calls: 0,
                pending_model_tools: BTreeMap::new(),
                tools_used: Vec::new(),
                error_count: 0,
                native_context_tokens: Some(0),
                native_context_window: Some(0),
                attempt: None,
                attempt_started_ms: 0,
                notification_seq: 0,
                finalized: None,
                source_reference: None,
                skill_startup: None,
                skill_preloads: None,
            },
        );
        self.record_selection(&id)?;
        Ok(id)
    }
    pub fn stop(
        &mut self,
        failure: Option<String>,
        reply: Reply<()>,
    ) -> Result<(), CoordinatorError> {
        self.info()?;
        if self.stopping.is_some() {
            return Err(CoordinatorError::Stopping);
        }
        self.stopping = Some((failure, reply));
        let queued: Vec<_> = self
            .agents
            .values()
            .flat_map(|a| a.queue.iter().map(|t| t.id.clone()))
            .collect();
        for id in queued {
            self.cancel(&id, "run stopped")?;
        }
        let tasks: Vec<_> = self.running.keys().cloned().collect();
        for id in tasks {
            if self.running.contains_key(&id) {
                self.cancel(&id, "run stopped")?;
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self, id: &str, reason: &str) -> Result<(), CoordinatorError> {
        if let Some(task) = self.running.get_mut(id) {
            task.reason = Some(reason.into());
            task.cancellation.cancel();
        } else {
            let queued = self.agents.iter_mut().find_map(|(agent_id, agent)| {
                let index = agent.queue.iter().position(|turn| turn.id == id)?;
                let turn = agent.queue.remove(index)?;
                Some((agent_id.clone(), turn))
            });
            let (agent, turn) = queued.ok_or_else(|| CoordinatorError::UnknownTask(id.into()))?;
            if let Some(manual) = turn.manual {
                let _ = manual
                    .reply
                    .send(Err(CoordinatorError::CompactionCancelled {
                        agent_id: agent.clone(),
                    }));
            }
            self.emit(
                EventActor::new(ActorKind::Worker, Some(agent)),
                Some(id.into()),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: id.into(),
                    reason: reason.into(),
                    failure: false,
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            )?;
            return Ok(());
        }
        let children: Vec<_> = self
            .running
            .iter()
            .filter(|(_, t)| t.parent.as_deref() == Some(id))
            .map(|(id, _)| id.clone())
            .collect();
        for child in children {
            self.cancel(&child, reason)?;
        }
        let waiting: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.id == id)
            .map(|(id, _)| id.clone())
            .collect();
        for permission in waiting {
            self.resolve_permission(&permission, PermissionDecision::Deny, Some(reason.into()))?;
        }
        Ok(())
    }
    pub async fn finish_stop(&mut self) {
        if !self.jobs.is_empty() || !self.running.is_empty() {
            return;
        }
        let Some((failure, reply)) = self.stopping.take() else {
            return;
        };
        let mut cleanup = Ok(());
        if let Some(info) = &self.info {
            cleanup = self
                .config
                .tool_registry
                .close_run(info.run_id.as_str())
                .await
                .map_err(|e| CoordinatorError::Invalid(e.to_string()));
        }
        let mut fault = self.fault.clone();
        let mut failure = failure.or_else(|| fault.clone());
        if failure.is_none()
            && let Err(error) = self.hook(
                crate::config::HookLifecycleEvent::RunFinished,
                &system(),
                None,
                serde_json::json!({"outcome":"finished","output_summary":"run stopped"}),
            )
        {
            fault = Some(error.to_string());
            failure.clone_from(&fault);
        }
        if let Some(error) = &failure
            && let Err(hook_error) = self.hook(
                crate::config::HookLifecycleEvent::RunFailed,
                &system(),
                None,
                serde_json::json!({"outcome":"failed","failure_reason":error}),
            )
        {
            fault = Some(hook_error.to_string());
            failure = Some(format!("{error}; {hook_error}"));
        }
        let event = failure.map_or_else(
            || {
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "run stopped".into(),
                })
            },
            |error| EventV1::RunFailed(RunFailedEvent { error }),
        );
        let result = self
            .emit(system(), None, event)
            .map(|_| ())
            .and_then(|()| fault.map_or(Ok(()), |error| Err(CoordinatorError::Invalid(error))));
        let closed = self
            .store
            .as_ref()
            .map_or(Ok(()), |store| store.close_writer())
            .map_err(CoordinatorError::from);
        self.info = None;
        self.metadata = None;
        self.store = None;
        self.agents.clear();
        self.commands.clear();
        self.children.clear();
        self.child_journals.clear();
        self.grants.clear();
        self.last_tool = None;
        self.edits.clear();
        self.snapshots.clear();
        self.compacting.clear();
        self.tool_state = Default::default();
        self.subagent_history = Default::default();
        self.projection_owner = None;
        self.raw_tool_results.clear();
        self.stopped_sessions.clear();
        self.killed_agents.clear();
        let _ = reply.send(result.and(closed).and(cleanup));
    }

    pub fn start_hooks(&mut self) -> Result<(), CoordinatorError> {
        let result = self.hook(
            crate::config::HookLifecycleEvent::RunStarted,
            &system(),
            None,
            serde_json::json!({"outcome":"started"}),
        );
        if let Err(error) = &result {
            let _ = self.hook(
                crate::config::HookLifecycleEvent::RunFailed,
                &system(),
                None,
                serde_json::json!({"outcome":"failed","failure_reason":error.to_string()}),
            );
            let _ = self.emit(
                system(),
                None,
                EventV1::RunFailed(RunFailedEvent {
                    error: error.to_string(),
                }),
            );
            if let Some(store) = self.store.take() {
                let _ = store.close_writer();
            }
            self.info = None;
            self.metadata = None;
            self.agents.clear();
            self.children.clear();
            self.child_journals.clear();
            self.grants.clear();
        }
        result
    }
}
