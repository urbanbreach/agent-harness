use super::{handle::system, runtime::*, *};
use crate::conversation_rewind::RewindPoint;
mod recorded;
mod restore;
pub(super) use recorded::restored_target;
use recorded::{checked_projection_owner, id_number, recorded_id, recorded_target};

impl CoordinatorHandle {
    pub async fn resume_run(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        let (id, name) = (id.into(), name.into());
        self.call(move |s| s.resume(&id, name)).await
    }
    pub async fn rewind_points(&self) -> Result<Option<Vec<RewindPoint>>, CoordinatorError> {
        self.call(|s| {
            if s.info.is_none() {
                return Ok(None);
            }
            Ok(Some(crate::conversation_rewind::rewind_points(
                &crate::store::read_events(&s.info()?.events_path)?,
            )))
        })
        .await
    }
    pub async fn rewind_conversation(
        &self,
        request: impl Into<String>,
    ) -> Result<RewindPoint, CoordinatorError> {
        let request = request.into();
        let (tx, rx) = oneshot::channel();
        self.call(move |s| s.request_rewind(&request, tx)).await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
}
impl Runtime {
    fn resume(&mut self, id: &str, name: String) -> Result<RunInfo, CoordinatorError> {
        if let Some(error) = &self.fault {
            return Err(CoordinatorError::Invalid(error.clone()));
        }
        if self.info.is_some() {
            return Err(CoordinatorError::RunAlreadyStarted);
        }
        crate::store::validate_session_id(id)?;
        let dir = self.config.session_dir.join(id);
        if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(CoordinatorError::Invalid(
                "session directory cannot be a symlink".into(),
            ));
        }
        let metadata = crate::proj::read_run_metadata(&dir)?;
        if metadata.as_ref().is_some_and(|m| {
            m.run_id != id
                || matches!(
                    m.mode_source,
                    Some(
                        crate::proj::SessionModeSource::ReplayOnly
                            | crate::proj::SessionModeSource::ScenarioFixture
                    )
                )
        }) {
            return Err(CoordinatorError::Invalid(
                "session metadata does not permit continuation".into(),
            ));
        }
        let journal = crate::store::Journal::open_existing(
            &self.config.session_dir,
            id,
            self.config.deterministic_store,
        )?;
        let mut events = crate::store::read_events(journal.file_path())?;
        crate::proj::checked_history(&events)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        self.projection_owner = checked_projection_owner(&self.config.session_dir, id, &events)?;
        let mut interrupted = crate::proj::InFlight::default();
        for event in &events {
            interrupted.apply(event);
        }
        let start = events
            .iter()
            .find_map(|e| match &e.payload {
                EventV1::RunStarted(e) => Some(e),
                _ => None,
            })
            .ok_or_else(|| CoordinatorError::Invalid("journal has no run start".into()))?;
        let root = PathBuf::from(&start.workspace_root).canonicalize()?;
        if !root.is_dir() {
            return Err(CoordinatorError::Invalid(
                "workspace is no longer a directory".into(),
            ));
        }
        self.restore_native_profiles(&events)?;
        let mut agents = self.restore_agents(
            &events,
            &dir,
            metadata
                .as_ref()
                .and_then(|m| m.recorded_runtime_context.as_ref()),
        )?;
        let grants = super::grants::load_grants(&root, &events)?;
        for (kind, request) in interrupted.open.keys() {
            if *kind == "user" {
                for agent in agents.values_mut() {
                    agent.messages.discard_turn(request);
                }
            }
        }
        self.counter = events
            .iter()
            .map(|e| id_number(&e.event_id).max(e.seq))
            .max()
            .unwrap_or(0);
        for event in &events {
            if let Some(id) = recorded_id(&event.payload) {
                self.counter = self.counter.max(id_number(id));
            }
            self.todos.apply(event);
            self.apply_guidance_state(event);
        }
        self.rebuild_instruction_state(&events);
        let info = RunInfo {
            run_id: id.into(),
            run_name: name.clone().into(),
            workspace_root: root,
            run_dir: dir.clone(),
            artifacts_dir: dir.join("artifacts"),
            events_path: journal.file_path().into(),
        };
        self.info = Some(info.clone());
        self.store = Some(Arc::new(journal));
        self.agents = agents;
        self.subagent_history = crate::subagent::SubagentHistory::from_events(&events);
        self.restore_native_subagents(&events)?;
        for event in &events {
            if let EventV1::SubagentCancelRequested(intent) = &event.payload {
                match &intent.command {
                    crate::subagent::SubagentCommandRequest::ExplicitChildKill { child_id }
                        if self.agents.contains_key(&child_id.0) =>
                    {
                        self.killed_agents.insert(child_id.0.clone());
                    }
                    crate::subagent::SubagentCommandRequest::ParentSessionStop { session_id }
                        if self.agents.contains_key(session_id) =>
                    {
                        self.stopped_sessions.insert(session_id.clone());
                    }
                    _ => {}
                }
            }
        }
        self.restore_command_notices();
        self.grants = grants;
        self.fault = None;
        self.metadata = Some(metadata.unwrap_or(self.new_metadata()?));
        self.last_seq = events.last().map_or(0, |event| event.seq);
        if let Err(error) = self.restore_child_journals(&events) {
            self.child_journals.clear();
            self.store = None;
            self.info = None;
            self.metadata = None;
            self.agents.clear();
            self.grants.clear();
            return Err(error.into());
        }
        let was_active = interrupted.status == Some(crate::proj::RunStatus::Running);
        let terminals = interrupted.terminals(super::history::INTERRUPTED_TOOL_RESULT);
        for event in terminals {
            events.push(self.emit(event.actor, event.correlation_id, event.payload)?);
        }
        self.restore_children(&events);
        self.reconcile_native_preparations(&events)?;
        self.reconcile_subagents()?;
        self.restore_native_subagents(&crate::store::read_events(&info.events_path)?)?;
        if was_active {
            self.emit(
                system(),
                None,
                EventV1::RunFailed(RunFailedEvent {
                    error: "session owner disconnected before the run finished".into(),
                }),
            )?;
        }
        self.emit(
            system(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: name.into(),
                workspace_root: info.workspace_root.to_string_lossy().into(),
            }),
        )?;
        if self.config.yolo_on_start
            || events
                .iter()
                .rev()
                .find_map(|event| match event.payload {
                    EventV1::YoloModeChanged { enabled } => Some(enabled),
                    _ => None,
                })
                .unwrap_or(false)
        {
            self.set_yolo_mode(true)?;
        }
        self.start_hooks()?;
        self.write_metadata()?;
        Ok(info)
    }
    fn request_rewind(
        &mut self,
        request: &str,
        reply: Reply<RewindPoint>,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        let events = crate::store::read_events(&self.info()?.events_path)?;
        let point = crate::conversation_rewind::rewind_points(&events)
            .into_iter()
            .find(|p| p.request_id == request)
            .ok_or_else(|| {
                CoordinatorError::Invalid("rewind target is not an active user message".into())
            })?;
        self.rewind = Some((point, reply));
        let queued: Vec<_> = self
            .agents
            .values()
            .flat_map(|a| a.queue.iter().map(|t| t.id.clone()))
            .collect();
        for id in queued {
            self.cancel(&id, "conversation rewind")?;
        }
        let tasks: Vec<_> = self.running.keys().cloned().collect();
        for id in tasks {
            if self.running.contains_key(&id) {
                self.cancel(&id, "conversation rewind")?;
            }
        }
        Ok(())
    }
    pub fn finish_rewind(&mut self) {
        if !self.jobs.is_empty() || !self.running.is_empty() {
            return;
        }
        if self.config.compaction.branch_summary.enabled
            && self.rewind_summary.is_none()
            && let Some((point, _)) = &self.rewind
        {
            match self.start_branch_summary(point.seq) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    self.branch_summary_warning(&error);
                    self.rewind_summary = Some(Err(error));
                }
            }
        }
        let Some((point, reply)) = self.rewind.take() else {
            return;
        };
        let branch_summary = self.rewind_summary.take();
        let result = (|| {
            self.accepting()?;
            self.emit(
                system(),
                Some(point.request_id.clone()),
                EventV1::ConversationRewound(
                    crate::conversation_rewind::ConversationRewoundEvent {
                        target_seq: point.seq,
                        request_id: point.request_id.clone(),
                    },
                ),
            )?;
            if let Some(Ok(summary)) = branch_summary
                && let Err(error) = self.emit(
                    EventActor::new(ActorKind::Worker, Some(summary.agent_id.clone())),
                    Some(point.request_id.clone()),
                    EventV1::BranchSummary(summary),
                )
            {
                self.branch_summary_warning(&error);
            }
            let events = crate::store::read_events(&self.info()?.events_path)?;
            self.rebuild_instruction_state(&events);
            self.agents = self.restore_agents(
                &events,
                &self.info()?.run_dir,
                self.metadata
                    .as_ref()
                    .and_then(|m| m.recorded_runtime_context.as_ref()),
            )?;
            self.restore_children(&events);
            self.subagent_history = crate::subagent::SubagentHistory::from_events(&events);
            for session_id in self.provider_sessions() {
                self.config.provider.session_event(
                    &harness_providers::ProviderSessionEvent::Rewound { session_id },
                );
            }
            Ok(point)
        })();
        let _ = reply.send(result);
    }
}
