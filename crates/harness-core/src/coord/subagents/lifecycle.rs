use super::{super::context::Context, super::runtime::*, *};

impl Runtime {
    pub(in crate::coord) fn execution_cwd(
        &self,
        actor: &EventActor,
    ) -> Result<PathBuf, CoordinatorError> {
        match actor.agent_id.as_ref() {
            Some(id) => {
                let agent = self
                    .agents
                    .get(id)
                    .ok_or_else(|| CoordinatorError::UnknownAgent(id.clone()))?;
                let cwd = agent.cwd.canonicalize()?;
                if cwd != agent.cwd
                    || !cwd.is_dir()
                    || !agent
                        .execution
                        .policy_roots
                        .iter()
                        .any(|root| cwd.starts_with(root))
                {
                    return Err(CoordinatorError::PermissionDenied(
                        "execution cwd changed from its accepted canonical context".into(),
                    ));
                }
                Ok(cwd)
            }
            None => Ok(self.info()?.workspace_root.clone()),
        }
    }

    pub(in crate::coord) fn begin_agent_attempt(
        &mut self,
        agent: &str,
        attempt: &str,
    ) -> Result<(), CoordinatorError> {
        let state = &self.agents[agent];
        let generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| CoordinatorError::Invalid("attempt generation exhausted".into()))?;
        if state.info.parent_agent_id.is_some() || self.native_subagents.contains_key(agent) {
            let transition = self.agent_transition(
                agent,
                attempt,
                generation,
                SubagentTransitionKind::Spawned,
                None,
                None,
                None,
            )?;
            if !self.commit_subagent_transition(transition)? {
                return Err(CoordinatorError::Invalid(
                    "attempt admission transition was rejected".into(),
                ));
            }
        }
        let state = self
            .agents
            .get_mut(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        state.generation = generation;
        state.attempt = Some(attempt.into());
        state.attempt_started_ms = self.clock.mono_ms();
        Ok(())
    }

    pub(in crate::coord) fn agent_transition(
        &self,
        agent: &str,
        attempt: &str,
        generation: u64,
        transition: SubagentTransitionKind,
        outcome: Option<SubagentTerminalOutcome>,
        accounting: Option<SubagentTerminalAccounting>,
        finalized_state: Option<FinalizedAgentStateReferenceV1>,
    ) -> Result<SubagentTransitionV1, CoordinatorError> {
        let state = &self.agents[agent];
        let parent = state.info.parent_agent_id.as_ref().map(|id| {
            SubagentId(
                if self
                    .agents
                    .get(id)
                    .is_some_and(|a| a.info.parent_agent_id.is_none())
                {
                    self.info
                        .as_ref()
                        .map_or_else(|| id.clone(), |info| info.run_id.to_string())
                } else {
                    id.clone()
                },
            )
        });
        let prior = self
            .subagent_history
            .records
            .get(agent)
            .and_then(|r| r.metadata.as_ref());
        let ancestry = prior.map(|m| m.ancestry.clone()).unwrap_or_else(|| {
            let origin = parent
                .as_ref()
                .and_then(|p| {
                    self.subagent_history
                        .records
                        .get(&p.0)
                        .and_then(|r| r.metadata.as_ref())
                        .and_then(|m| m.ancestry.origin().cloned())
                })
                .or_else(|| parent.clone());
            SubagentAncestry::new(parent.clone(), origin)
        });
        let run = self.info()?.run_id.to_string();
        let child = self.children.get(agent);
        let background = child.is_none_or(|c| c.background);
        let mut transition = SubagentTransitionV1 {
            payload_version: 1,
            child_id: SubagentId(agent.into()),
            attempt_id: Some(attempt.into()),
            generation,
            // Native per-attempt sequence is deliberately not the journal sequence.
            notification_seq: Some(state.notification_seq.checked_add(1).ok_or_else(|| {
                CoordinatorError::Invalid("notification sequence exhausted".into())
            })?),
            origin: LifecycleOrigin::Stream,
            transition,
            metadata: SubagentTransitionMetadataV1 {
                ancestry,
                execution_owner: prior.map(|m| m.execution_owner.clone()).unwrap_or_else(|| {
                    state
                        .info
                        .parent_agent_id
                        .as_ref()
                        .filter(|parent| {
                            self.agents
                                .get(*parent)
                                .is_some_and(|a| a.info.parent_agent_id.is_some())
                        })
                        .map_or_else(
                            || SubagentExecutionOwner::RootSession {
                                session_id: run.clone(),
                            },
                            |parent| SubagentExecutionOwner::ChildSession {
                                child_id: SubagentId(parent.clone()),
                            },
                        )
                }),
                display_route: prior.map(|m| m.display_route.clone()).unwrap_or(
                    SubagentDisplayRoute {
                        root_session_id: run,
                        parent_session_id: parent.map(|p| p.0),
                        child_session_id: agent.into(),
                    },
                ),
                notification_route: prior
                    .filter(|m| m.display_route.parent_session_id.is_none())
                    .map(|m| m.notification_route.clone())
                    .unwrap_or(SubagentNotificationRoute {
                        parent_prompt_id: child.and_then(|c| c.parent_request.clone()),
                        background,
                        await_to_completion: !background,
                        surface_completion: background,
                    }),
                injected_depth: prior.map_or_else(
                    || {
                        let parent_depth = state
                            .info
                            .parent_agent_id
                            .as_ref()
                            .and_then(|p| self.subagent_history.records.get(p))
                            .and_then(|r| r.metadata.as_ref())
                            .map_or(0, |m| m.injected_depth.0);
                        InjectedSubagentDepth(parent_depth.saturating_add(1))
                    },
                    |m| m.injected_depth,
                ),
                isolation_requested: SubagentIsolationMode::None,
                context: state.execution.clone(),
            },
            outcome,
            accounting,
            finalized_state,
        };
        if let Some(metadata) = self.native_transition_metadata(agent) {
            transition.metadata = metadata;
        }
        Ok(transition)
    }

    /// Candidate fold, authoritative append, then state application. Rejection appends nothing.
    pub(in crate::coord) fn commit_subagent_transition(
        &mut self,
        transition: SubagentTransitionV1,
    ) -> Result<bool, CoordinatorError> {
        self.info()?;
        if self.fault.is_some() {
            return Err(CoordinatorError::Invalid(
                "storage authority is unavailable".into(),
            ));
        }
        let agent = self
            .agents
            .get(&transition.child_id.0)
            .ok_or_else(|| CoordinatorError::UnknownAgent(transition.child_id.0.clone()))?;
        let valid_generation = transition.generation == agent.generation
            || (transition.transition == SubagentTransitionKind::Spawned
                && transition.generation == agent.generation.saturating_add(1));
        if !valid_generation
            || (transition.transition != SubagentTransitionKind::Spawned
                && transition.attempt_id != agent.attempt)
            || transition.metadata.context != agent.execution
            || transition.metadata.ancestry.spawner().map(|p| p.0.as_str())
                != agent.info.parent_agent_id.as_ref().map(|id| {
                    if self
                        .agents
                        .get(id)
                        .is_some_and(|a| a.info.parent_agent_id.is_none())
                    {
                        self.info
                            .as_ref()
                            .map_or(id.as_str(), |info| info.run_id.as_str())
                    } else {
                        id.as_str()
                    }
                })
            || transition.finalized_state.as_ref().is_some_and(|r| {
                r.state.owner != transition.child_id
                    || Some(r.attempt_id.as_str()) != transition.attempt_id.as_deref()
            })
            || transition
                .finalized_state
                .as_ref()
                .is_some_and(|r| r.generation != transition.generation)
        {
            return Err(CoordinatorError::Invalid(
                "subagent transition owner or source attempt mismatch".into(),
            ));
        }
        let mut candidate = self.subagent_history.clone();
        if !candidate.apply_transition(&transition, self.clock.mono_ms()) {
            return Ok(false);
        }
        let finalized = transition.finalized_state.clone();
        let owner = transition.child_id.0.clone();
        let notification_seq = transition.notification_seq;
        let spawned = (transition.transition == SubagentTransitionKind::Spawned).then(|| {
            (
                transition.generation,
                transition.attempt_id.clone(),
                self.clock.mono_ms(),
            )
        });
        self.emit_applied(
            EventActor::new(ActorKind::Worker, Some(transition.child_id.0.clone())),
            transition.attempt_id.clone(),
            EventV1::SubagentTransition(Box::new(transition)),
            move |runtime, _| {
                runtime.subagent_history = candidate;
                if let Some(agent) = runtime.agents.get_mut(&owner) {
                    if let Some(reference) = finalized {
                        agent.finalized = Some(reference);
                    }
                    if let Some(seq) = notification_seq {
                        agent.notification_seq = seq;
                    }
                    if let Some((generation, attempt, started)) = spawned {
                        agent.generation = generation;
                        agent.attempt = attempt;
                        agent.attempt_started_ms = started;
                    }
                }
            },
        )?;
        Ok(true)
    }

    pub(in crate::coord) fn finish_agent_state(
        &mut self,
        agent: &str,
        attempt: &str,
        messages: &Context,
        success: bool,
        retain_snapshot: bool,
        cancelled: bool,
    ) -> Result<(), CoordinatorError> {
        let state = &self.agents[agent];
        let generation = state.generation;
        let turns = state.prompt_turns;
        let reference = if retain_snapshot {
            Some(self.prepare_finalized_state(agent, attempt, messages)?)
        } else {
            None
        };
        if super::state_parent(self, agent).is_some() {
            let request_count = messages
                .usage
                .iter()
                .filter(|entry| entry.attempt_id == attempt)
                .count();
            let usage = messages
                .usage
                .iter()
                .filter(|entry| entry.attempt_id == attempt)
                .try_fold((0u64, 0u64, 0u64), |acc, entry| {
                    let u = entry.usage.as_ref().filter(|_| entry.usage_complete)?;
                    Some((
                        acc.0 + u64::from(u.prompt_tokens),
                        acc.1 + u64::from(u.completion_tokens),
                        acc.2 + u64::from(u.total_tokens),
                    ))
                })
                .filter(|_| request_count > 0);
            let tool_calls = u32::try_from(
                messages
                    .entries
                    .iter()
                    .filter(|e| {
                        e.turn.as_deref() == Some(attempt)
                            && e.message.role == harness_providers::MessageRole::Tool
                    })
                    .count(),
            )
            .unwrap_or(u32::MAX);
            let tokens_used = if self.native_subagents.contains_key(agent) {
                if !success && tool_calls == 0 {
                    Some(0)
                } else {
                    messages
                        .native_context_usage
                        .map(|usage| usage.total_tokens)
                }
            } else {
                usage.map(|u| u.0)
            };
            let transition = self.agent_transition(
                agent,
                attempt,
                generation,
                SubagentTransitionKind::Finished,
                Some(if success {
                    SubagentTerminalOutcome::Completed
                } else if cancelled {
                    SubagentTerminalOutcome::Cancelled
                } else {
                    SubagentTerminalOutcome::SessionError
                }),
                Some(SubagentTerminalAccounting {
                    tool_calls,
                    turns,
                    duration_ms: self
                        .clock
                        .mono_ms()
                        .saturating_sub(self.agents[agent].attempt_started_ms),
                    tokens_used,
                    output_tokens_used: usage.map(|u| u.1),
                    total_tokens_used: usage.map(|u| u.2),
                    output_usage_incomplete: usage.is_none(),
                }),
                reference,
            )?;
            if !self.commit_subagent_transition(transition)? {
                return Err(CoordinatorError::Invalid(
                    "terminal transition was rejected".into(),
                ));
            }
        } else if let Some(reference) = reference {
            let applied = reference.clone();
            let owner = agent.to_owned();
            self.emit_applied(
                EventActor::new(ActorKind::Worker, Some(agent.into())),
                Some(attempt.into()),
                EventV1::FinalizedAgentState(reference),
                move |runtime, _| {
                    runtime
                        .subagent_history
                        .finalized
                        .insert(owner.clone(), applied.clone());
                    if let Some(agent) = runtime.agents.get_mut(&owner) {
                        agent.finalized = Some(applied);
                    }
                },
            )?;
        }
        Ok(())
    }

    pub(in crate::coord) fn reconcile_subagents(
        &mut self,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        self.accepting()?;
        let interrupted: Vec<_> = self
            .subagent_history
            .records
            .iter()
            .filter(|(id, record)| {
                !record.legacy
                    && record.lifecycle.phase() == Some(LifecyclePhase::Running)
                    && !self
                        .running
                        .values()
                        .any(|job| job.actor.agent_id.as_ref() == Some(id))
                    && self.agents.get(*id).is_some_and(|agent| !agent.busy)
            })
            .map(|(id, record)| (id.clone(), record.generation))
            .collect();
        let mut repaired = Vec::new();
        for (id, generation) in interrupted {
            let attempt = self.agents[&id]
                .attempt
                .clone()
                .ok_or_else(|| CoordinatorError::Invalid("interrupted attempt missing".into()))?;
            let mut transition = self.agent_transition(
                &id,
                &attempt,
                generation,
                SubagentTransitionKind::Finished,
                Some(SubagentTerminalOutcome::SessionError),
                None,
                None,
            )?;
            transition.origin = LifecycleOrigin::Reconciliation;
            if self.commit_subagent_transition(transition)? {
                repaired.push(SubagentId(id));
            }
        }
        Ok(repaired)
    }
}
