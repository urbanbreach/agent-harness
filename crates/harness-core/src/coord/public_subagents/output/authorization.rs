use super::*;

impl Runtime {
    pub(in crate::coord::public_subagents) fn native_reachable(
        &self,
        actor: &EventActor,
        id: &str,
    ) -> bool {
        if matches!(actor.kind, ActorKind::System | ActorKind::Supervisor)
            && actor.agent_id.is_none()
        {
            return self.native_subagents.contains_key(id);
        }
        let Some(owner) = actor.agent_id.as_deref() else {
            return false;
        };
        let Some(child) = self.native_subagents.get(id) else {
            return false;
        };
        if self
            .native_subagents
            .get(owner)
            .is_some_and(|caller| caller.registration.root_agent == child.registration.root_agent)
        {
            return true;
        }
        if child.registration.root_agent == owner {
            return true;
        }
        let mut current = child.registration.spawner.as_str();
        loop {
            if current == owner {
                return true;
            }
            let Some(parent) = self.native_subagents.get(current) else {
                return false;
            };
            current = &parent.registration.spawner;
        }
    }

    pub(in crate::coord) fn cancel_native_subagent(
        &mut self,
        id: &str,
        reason: &str,
        explicit: bool,
    ) -> Result<(), CoordinatorError> {
        if explicit && !self.killed_agents.contains(id) {
            let actor = self
                .native_subagents
                .get(id)
                .map(|child| {
                    EventActor::new(ActorKind::Worker, Some(child.registration.spawner.clone()))
                })
                .unwrap_or_else(super::super::handle::system);
            self.emit(
                actor,
                None,
                EventV1::SubagentCancelRequested(SubagentCancelIntentV1 {
                    payload_version: 1,
                    command: SubagentCommandRequest::ExplicitChildKill {
                        child_id: SubagentId(id.into()),
                    },
                    targets: vec![SubagentId(id.into())],
                }),
            )?;
            self.killed_agents.insert(id.into());
        }
        let Some(child) = self.native_subagents.get_mut(id) else {
            return Ok(());
        };
        if child.phase == NativePhase::Terminal {
            return Ok(());
        }
        child.explicitly_killed |= explicit;
        child.cancellation.cancel();
        let phase = child.phase;
        let request = child.request.clone();
        let preparation = child.preparation.clone();
        if phase == NativePhase::Queued {
            self.native_subagent_queue.retain(|queued| queued != id);
            self.finish_native_before_start(id, CoordinatorError::Cancelled(reason.into()))?;
        } else if let Some(preparation) = preparation {
            if self.running.contains_key(&preparation) {
                self.cancel(&preparation, reason)?;
            }
        } else if let Some(request) = request
            && self.running.contains_key(&request)
        {
            self.cancel(&request, reason)?;
        }
        Ok(())
    }

    pub(in crate::coord::public_subagents) fn cancel_native_foreground(
        &mut self,
        id: &str,
    ) -> Result<(), CoordinatorError> {
        if self
            .native_subagents
            .get(id)
            .is_some_and(|child| child.foreground_attached)
        {
            self.cancel_native_subagent(id, "Subagent was cancelled", false)?;
        }
        Ok(())
    }

    pub(in crate::coord) fn demote_native_subagent(
        &mut self,
        id: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(child) = self.native_subagents.get_mut(id) else {
            return Ok(());
        };
        child.registration.background = true;
        child.foreground_attached = false;
        let tool = child.registration.parent_tool.clone();
        let request = child.request.clone();
        if let Some(request) = &request
            && let Some(job) = self.running.get_mut(request)
        {
            job.parent = None;
        }
        if let Some(attempt) = self.agents[id].attempt.clone() {
            let transition = self.agent_transition(
                id,
                &attempt,
                self.agents[id].generation,
                SubagentTransitionKind::Routed,
                None,
                None,
                None,
            )?;
            let _ = self.commit_subagent_transition(transition)?;
        }
        if let Some(child) = self.native_subagents.get_mut(id) {
            child
                .updates
                .send_modify(|snapshot| snapshot.demoted = true);
        }
        self.detach_native_spawn_waiter(id, &tool)
    }
}
