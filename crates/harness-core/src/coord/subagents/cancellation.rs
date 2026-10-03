use super::{super::runtime::*, *};

impl Runtime {
    pub(in crate::coord) fn subagent_cancel(
        &mut self,
        actor: EventActor,
        command: SubagentCommandRequest,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        self.accepting()?;
        let root = self.info()?.run_id.to_string();
        let mut targets = Vec::new();
        match &command {
            SubagentCommandRequest::ExplicitChildKill { child_id } => {
                Runtime::authorize_agent_state(self, &actor, &child_id.0)?;
                if self.agents[&child_id.0].info.parent_agent_id.is_none() {
                    return Err(CoordinatorError::Invalid(
                        "explicit kill requires a child identity".into(),
                    ));
                }
                targets.push(child_id.clone());
            }
            SubagentCommandRequest::ChildSessionCancel {
                session_id,
                descendants,
            } => {
                Runtime::authorize_agent_state(self, &actor, session_id)?;
                if self.agents[session_id].info.parent_agent_id.is_none() {
                    return Err(CoordinatorError::Invalid(
                        "child session cancel requires a child".into(),
                    ));
                }
                targets.push(SubagentId(session_id.clone()));
                for id in self.agents.keys() {
                    let mut parent = self.execution_parent(id);
                    while parent.is_some_and(|owner| owner != session_id) {
                        parent = parent.and_then(|owner| self.execution_parent(owner));
                    }
                    if parent == Some(session_id.as_str()) {
                        targets.push(SubagentId(id.clone()));
                    }
                }
                if descendants.iter().any(|id| !targets.contains(id)) {
                    return Err(CoordinatorError::PermissionDenied(
                        "descendant intent includes a foreign child".into(),
                    ));
                }
            }
            SubagentCommandRequest::ParentPromptCancel { prompt_id } => {
                let id = self
                    .running
                    .get(prompt_id)
                    .and_then(|job| job.actor.agent_id.clone())
                    .or_else(|| {
                        self.agents
                            .iter()
                            .find(|(_, agent)| agent.queue.iter().any(|turn| &turn.id == prompt_id))
                            .map(|(id, _)| id.clone())
                    })
                    .ok_or_else(|| CoordinatorError::UnknownTask(prompt_id.clone()))?;
                Runtime::authorize_agent_state(self, &actor, &id)?;
                targets.push(SubagentId(id));
            }
            SubagentCommandRequest::ParentSessionStop { session_id } => {
                if session_id == &root {
                    if actor.agent_id.is_some() {
                        return Err(CoordinatorError::PermissionDenied(
                            "root admission stop requires the root owner".into(),
                        ));
                    }
                    targets.extend(self.agents.keys().cloned().map(SubagentId));
                } else {
                    Runtime::authorize_agent_state(self, &actor, session_id)?;
                    targets.push(SubagentId(session_id.clone()));
                }
            }
            SubagentCommandRequest::WaiterCancel { waiter_id } => {
                // A waiter is an existing approved spawn-capable tool, not a child job.
                let job = self
                    .running
                    .get(waiter_id)
                    .ok_or_else(|| CoordinatorError::UnknownTask(waiter_id.clone()))?;
                if !matches!(
                    job.kind,
                    JobKind::Tool {
                        capability: crate::tool::ToolCapability::SpawnAgent,
                        ..
                    }
                ) {
                    return Err(CoordinatorError::Invalid(
                        "waiter cancellation requires an orchestration waiter".into(),
                    ));
                }
                if let Some(id) = &job.actor.agent_id {
                    Runtime::authorize_agent_state(self, &actor, id)?;
                }
            }
            SubagentCommandRequest::RootShutdown => {
                if actor.agent_id.is_some() {
                    return Err(CoordinatorError::PermissionDenied(
                        "shutdown requires the root owner".into(),
                    ));
                }
                targets.extend(self.agents.keys().cloned().map(SubagentId));
            }
        }
        self.emit(
            actor,
            None,
            EventV1::SubagentCancelRequested(SubagentCancelIntentV1 {
                payload_version: 1,
                command: command.clone(),
                targets: targets.clone(),
            }),
        )?;
        match &command {
            SubagentCommandRequest::ExplicitChildKill { child_id } => {
                self.killed_agents.insert(child_id.0.clone());
            }
            SubagentCommandRequest::ParentSessionStop { session_id } => {
                self.stopped_sessions.insert(session_id.clone());
            }
            SubagentCommandRequest::RootShutdown => {
                self.stopped_sessions.insert(root);
            }
            SubagentCommandRequest::WaiterCancel { waiter_id } => {
                if let Some(job) = self.running.get_mut(waiter_id) {
                    job.reason = Some("waiter cancelled".into());
                    job.cancellation.cancel();
                }
                // Release the real delegation channel, not only the tool token.
                self.detach_child_waiter(waiter_id, "waiter cancelled");
                return Ok(targets);
            }
            _ => {}
        }
        let jobs: Vec<_> = self
            .running
            .iter()
            .filter(|(id, job)| match &command {
                SubagentCommandRequest::ParentPromptCancel { prompt_id } => {
                    (*id == prompt_id || job.parent.as_ref() == Some(prompt_id))
                        && job
                            .actor
                            .agent_id
                            .as_ref()
                            .is_some_and(|id| targets.iter().any(|target| &target.0 == id))
                }
                _ => job
                    .actor
                    .agent_id
                    .as_ref()
                    .is_some_and(|id| targets.iter().any(|target| &target.0 == id)),
            })
            .map(|(id, _)| id.clone())
            .collect();
        // Do not follow generic parent/display edges into other agents.
        for id in &jobs {
            if let Some(job) = self.running.get_mut(id) {
                job.reason = Some("subagent cancellation requested".into());
                job.cancellation.cancel();
            }
            self.detach_child_waiter(id, "subagent cancellation requested");
        }
        let permissions: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| jobs.contains(&pending.id))
            .map(|(id, _)| id.clone())
            .collect();
        for permission in permissions {
            self.resolve_permission(
                &permission,
                PermissionDecision::Deny,
                Some("subagent cancellation requested".into()),
            )?;
        }
        for target in &targets {
            let queued: Vec<_> = self.agents[&target.0]
                .queue
                .iter()
                .filter(|turn| {
                    !matches!(&command, SubagentCommandRequest::ParentPromptCancel { prompt_id } if &turn.id != prompt_id)
                })
                .map(|turn| turn.id.clone())
                .collect();
            for id in queued {
                self.cancel(&id, "subagent cancellation requested")?;
            }
        }
        Ok(targets)
    }

    fn execution_parent<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        match self
            .subagent_history
            .records
            .get(id)
            .and_then(|r| r.metadata.as_ref())
            .map(|m| &m.execution_owner)
        {
            Some(SubagentExecutionOwner::ChildSession { child_id }) => Some(&child_id.0),
            Some(SubagentExecutionOwner::RootSession { .. }) => None,
            None => super::state_parent(self, id),
        }
    }
}
