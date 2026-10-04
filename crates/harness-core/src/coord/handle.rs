use super::*;
use serde_json::Value;

impl CoordinatorHandle {
    pub(super) async fn call<T: Send + 'static>(
        &self,
        command: impl FnOnce(&mut Runtime) -> Result<T, CoordinatorError> + Send + 'static,
    ) -> Result<T, CoordinatorError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Box::new(move |state| {
                let _ = tx.send(command(state));
            }))
            .await
            .map_err(|_| CoordinatorError::Closed)?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
    pub async fn start_run(
        &self,
        name: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Result<RunInfo, CoordinatorError> {
        let (name, root) = (name.into(), root.into());
        self.call(move |s| s.start(name, root)).await
    }
    pub async fn run_info(&self) -> Result<RunInfo, CoordinatorError> {
        self.call(|s| s.info().cloned()).await
    }
    pub async fn plugin_lifecycle_summary(
        &self,
    ) -> Result<crate::integrations::PluginLifecycleSummary, CoordinatorError> {
        self.call(|s| {
            crate::integrations::PluginRuntimeContract::open(&s.info()?.workspace_root)
                .map(|runtime| runtime.summary())
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))
        })
        .await
    }
    pub async fn event_store(&self) -> Result<Arc<dyn EventStore>, CoordinatorError> {
        self.call(|s| {
            s.store
                .clone()
                .map(crate::store::observer)
                .ok_or(CoordinatorError::RunNotStarted)
        })
        .await
    }
    /// A read-only redactor that also sees credentials registered during the run.
    pub async fn output_redactor(
        &self,
    ) -> Result<Arc<dyn Redactor + Send + Sync>, CoordinatorError> {
        self.call(|s| Ok(Arc::clone(&s.redactor))).await
    }
    pub async fn subscribe_new_events(
        &self,
    ) -> Result<crate::store::EventStream, CoordinatorError> {
        self.call(|s| {
            s.store
                .as_ref()
                .ok_or(CoordinatorError::RunNotStarted)?
                .subscribe(s.last_seq.saturating_add(1))
                .map_err(Into::into)
        })
        .await
    }
    pub async fn stop_run(&self) -> Result<(), CoordinatorError> {
        self.stop(None).await
    }
    pub async fn fail_run(&self, error: impl Into<String>) -> Result<(), CoordinatorError> {
        self.stop(Some(error.into())).await
    }
    async fn stop(&self, failure: Option<String>) -> Result<(), CoordinatorError> {
        let (tx, rx) = oneshot::channel();
        self.call(move |s| s.stop(failure, tx)).await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
    pub async fn update_session_title(
        &self,
        title: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        let title = title.into();
        self.call(move |s| {
            if title.trim().is_empty() || title.len() > 1024 {
                return Err(CoordinatorError::Invalid("invalid session title".into()));
            }
            s.emit(
                system(),
                None,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: title.clone(),
                }),
            )?;
            if let Some(info) = &mut s.info {
                info.run_name = title.clone().into();
            }
            if let Some(metadata) = &mut s.metadata {
                metadata.run_name = title;
            }
            s.write_metadata()?;
            s.info().cloned()
        })
        .await
    }
    pub async fn record_ui_intent(
        &self,
        agent: impl Into<String>,
        intent: impl Into<String>,
        params: BTreeMap<String, String>,
    ) -> Result<(), CoordinatorError> {
        let (agent, intent) = (agent.into(), intent.into());
        self.call(move |s| {
            s.emit(
                EventActor::new(ActorKind::User, Some(agent)),
                None,
                EventV1::UiIntentReceived(UiIntentReceivedEvent { intent, params }),
            )
            .map(|_| ())
        })
        .await
    }
    pub async fn spawn_agent(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent: Option<String>,
    ) -> Result<String, CoordinatorError> {
        self.spawn_agent_idle(actor, profile, parent).await
    }
    pub async fn spawn_agent_idle(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent: Option<String>,
    ) -> Result<String, CoordinatorError> {
        let profile = profile.into();
        self.call(move |s| s.spawn_agent(actor, &profile, parent))
            .await
    }
    pub async fn agent_runtime_info(
        &self,
        agent: impl Into<String>,
    ) -> Result<AgentRuntimeInfo, CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| {
            s.agents
                .get(&agent)
                .map(|a| a.info.clone())
                .ok_or(CoordinatorError::UnknownAgent(agent))
        })
        .await
    }
    pub async fn request_agent_turn(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        self.request_agent_turn_with_model(actor, agent, prompt, None, None)
            .await
    }
    pub async fn request_agent_turn_with_model(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        prompt: impl Into<String>,
        model: Option<String>,
        settings: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        let (agent, prompt) = (agent.into(), prompt.into());
        self.call(move |s| s.queue_turn(actor, &agent, prompt.into(), model, settings, None))
            .await
    }
    pub async fn request_agent_turn_with_model_target(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        prompt: impl Into<String>,
        target: ResolvedModelTarget,
    ) -> Result<String, CoordinatorError> {
        let (agent, prompt) = (agent.into(), prompt.into());
        self.call(move |s| {
            s.queue_turn(
                actor,
                &agent,
                prompt.into(),
                Some(target.model_ref.clone()),
                Some((&target).into()),
                Some(target),
            )
        })
        .await
    }
    pub async fn request_tool_call(
        &self,
        actor: EventActor,
        _legacy_profile_hint: Option<String>,
        tool: impl Into<String>,
        args: Value,
    ) -> Result<String, CoordinatorError> {
        let tool = tool.into();
        self.call(move |s| s.request_tool(actor, None, None, tool, args, None))
            .await
    }
    pub async fn execute_agent_tool_call(
        &self,
        actor: EventActor,
        _legacy_profile_hint: Option<String>,
        tool: impl Into<String>,
        args: Value,
    ) -> Result<ToolResult, String> {
        self.execute_tool(actor, None, None, tool.into(), args)
            .await
            .map_err(|e| e.to_string())
    }
    pub(super) async fn execute_tool(
        &self,
        actor: EventActor,
        parent: Option<String>,
        id: Option<String>,
        tool: String,
        args: Value,
    ) -> Result<ToolResult, CoordinatorError> {
        let (tx, rx) = oneshot::channel();
        self.call(move |s| s.request_tool(actor, parent, id, tool, args, Some(tx)))
            .await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
    pub async fn resolve_permission(
        &self,
        id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Result<(), CoordinatorError> {
        let id = id.into();
        self.call(move |s| s.resolve_permission(&id, decision, reason))
            .await
    }
    pub async fn set_yolo_mode(&self, enabled: bool) -> Result<(), CoordinatorError> {
        self.call(move |s| s.set_yolo_mode(enabled)).await
    }

    pub async fn yolo_mode(&self) -> Result<bool, CoordinatorError> {
        self.call(|s| {
            s.info()?;
            Ok(s.config.yolo_on_start)
        })
        .await
    }
    pub async fn cancel_task(
        &self,
        id: impl AsRef<str>,
        reason: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        let (id, reason) = (id.as_ref().to_owned(), reason.into());
        self.call(move |s| s.cancel(&id, &reason)).await
    }
    /// Directory searches skip files that need a separate read approval.
    pub async fn allowed_read_paths(
        &self,
        task: impl Into<String>,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<PathBuf>, CoordinatorError> {
        self.allowed_paths(task, paths, &["read"]).await
    }
    /// Filter a directory query using the running tool's actor and current policy.
    pub async fn allowed_paths(
        &self,
        task: impl Into<String>,
        paths: Vec<PathBuf>,
        permissions: &[&str],
    ) -> Result<Vec<PathBuf>, CoordinatorError> {
        let task = task.into();
        let permissions: Vec<String> = permissions.iter().map(|name| (*name).into()).collect();
        self.call(move |s| {
            s.check_task(&task)?;
            let job = s
                .running
                .get(&task)
                .ok_or_else(|| CoordinatorError::UnknownTask(task.clone()))?;
            let policy = job
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| s.agents.get(id))
                .map(|a| &a.policy);
            let cwd = s.execution_cwd(&job.actor)?;
            let root = &s.info()?.workspace_root;
            let allowed = |path: &std::path::Path| {
                let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
                permissions.iter().all(|permission| {
                    s.config
                        .permission_policy
                        .check(permission, &relative, policy)
                        == crate::perm::PermissionAction::Allow
                })
            };
            Ok(paths
                .into_iter()
                .filter_map(|path| {
                    let resolved = crate::tool::resolve_file_path(&cwd, &path).ok()?;
                    (allowed(&path) && allowed(&resolved)).then_some(resolved)
                })
                .collect())
        })
        .await
    }
}

pub(super) fn system() -> EventActor {
    EventActor::new(ActorKind::System, None)
}
