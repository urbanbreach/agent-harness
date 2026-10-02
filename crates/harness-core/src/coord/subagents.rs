//! Subordinate state inside Runtime. This module has no worker or journal authority.
use super::{
    context::{Context, Entry},
    handle::system,
    runtime::*,
    *,
};
use crate::subagent::*;
use sha2::{Digest, Sha256};
use std::path::Path;

impl CoordinatorHandle {
    /// Resolve only authoritative, completed core state. No caller-provided snapshot.
    pub async fn raw_finalized_state(
        &self,
        agent: impl Into<String>,
    ) -> Result<FinalizedStateResult, CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| s.resolve_agent_finalized_state(&agent))
            .await
    }

    pub async fn subagent_history(&self) -> Result<SubagentHistory, CoordinatorError> {
        self.call(|s| {
            let mut history = s.subagent_history.clone();
            history.expire(s.clock.mono_ms());
            Ok(history)
        })
        .await
    }

    /// Initialize an existing idle actor; this is not a new spawn/tool surface.
    pub async fn initialize_agent_from_finalized(
        &self,
        actor: EventActor,
        target: String,
        source: String,
        mode: FinalizedContextCopy,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| s.initialize_finalized(actor, &target, &source, mode))
            .await
    }

    /// Cwd is canonicalized and policy checked before the actor context changes.
    pub async fn set_agent_execution_cwd(
        &self,
        actor: EventActor,
        agent: String,
        cwd: PathBuf,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| {
            s.accepting()?;
            s.authorize_agent_state(&actor, &agent)?;
            let state = &s.agents[&agent];
            if state.busy
                || !state.queue.is_empty()
                || s.running
                    .values()
                    .any(|job| job.actor.agent_id.as_deref() == Some(&agent))
            {
                return Err(CoordinatorError::Invalid(
                    "execution context requires an idle agent".into(),
                ));
            }
            let cwd = crate::tool::resolve_file_path(&state.cwd, &cwd)
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
            let root = &s.info()?.workspace_root;
            let selector = cwd.strip_prefix(root).unwrap_or(&cwd).to_string_lossy();
            if !cwd.is_dir()
                || !cwd.starts_with(root)
                || s.config
                    .permission_policy
                    .check("read", &selector, Some(&state.policy))
                    != crate::perm::PermissionAction::Allow
            {
                return Err(CoordinatorError::PermissionDenied(
                    "cwd is outside the allowed run workspace".into(),
                ));
            }
            let context = ResolvedSubagentContext {
                effective_cwd: cwd.to_string_lossy().into_owned(),
                policy_roots: vec![root.to_string_lossy().into_owned()],
                isolation: ResolvedSubagentIsolation::SharedWorkspace,
            };
            s.emit_applied(
                actor,
                None,
                EventV1::AgentExecutionContextChanged(AgentExecutionContextChangedV1 {
                    payload_version: 1,
                    agent_id: SubagentId(agent.clone()),
                    context: context.clone(),
                }),
                move |runtime, _| {
                    if let Some(state) = runtime.agents.get_mut(&agent) {
                        state.execution = context;
                        state.cwd = cwd;
                    }
                },
            )?;
            Ok(())
        })
        .await
    }

    /// Distinct intents remain distinct durable records and target selection.
    pub async fn request_subagent_cancel(
        &self,
        actor: EventActor,
        command: SubagentCommandRequest,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        if command == SubagentCommandRequest::RootShutdown {
            let (reply, finished) = oneshot::channel();
            let targets = self
                .call(move |s| {
                    let targets = s.subagent_cancel(actor, command)?;
                    s.stop(None, reply)?;
                    Ok(targets)
                })
                .await?;
            finished.await.map_err(|_| CoordinatorError::Closed)??;
            return Ok(targets);
        }
        self.call(move |s| s.subagent_cancel(actor, command)).await
    }

    /// Root reparenting changes ownership/routes/depth, never creation ancestry.
    pub async fn reparent_subagent_to_root(
        &self,
        actor: EventActor,
        child: String,
        injected_depth: InjectedSubagentDepth,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| {
            s.accepting()?;
            s.authorize_agent_state(&actor, &child)?;
            let record = s
                .subagent_history
                .records
                .get(&child)
                .ok_or_else(|| CoordinatorError::UnknownAgent(child.clone()))?;
            let attempt = record
                .lifecycle
                .current_attempt_id()
                .ok_or_else(|| {
                    CoordinatorError::Invalid(
                        "reparenting requires an admitted child attempt".into(),
                    )
                })?
                .to_owned();
            let mut transition = s.agent_transition(
                &child,
                &attempt,
                record.generation,
                SubagentTransitionKind::Routed,
                None,
                None,
                None,
            )?;
            transition.origin = LifecycleOrigin::Reconciliation;
            transition.metadata.execution_owner = SubagentExecutionOwner::RootSession {
                session_id: s.info()?.run_id.to_string(),
            };
            transition.metadata.display_route.parent_session_id = None;
            transition.metadata.notification_route.parent_prompt_id = None;
            transition.metadata.injected_depth = injected_depth;
            s.commit_subagent_transition(transition).map(|_| ())
        })
        .await
    }

    pub async fn retire_subagent_attempt(
        &self,
        actor: EventActor,
        child: String,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| {
            s.accepting()?;
            s.authorize_agent_state(&actor, &child)?;
            let agent = s
                .agents
                .get(&child)
                .ok_or_else(|| CoordinatorError::UnknownAgent(child.clone()))?;
            let attempt = agent
                .attempt
                .as_ref()
                .ok_or_else(|| CoordinatorError::Invalid("child attempt missing".into()))?;
            let transition = s.agent_transition(
                &child,
                attempt,
                agent.generation,
                SubagentTransitionKind::Retired,
                None,
                None,
                None,
            )?;
            if s.commit_subagent_transition(transition)? {
                Ok(())
            } else {
                Err(CoordinatorError::Invalid(
                    "only a finished attempt can retire".into(),
                ))
            }
        })
        .await
    }

    /// Explicit live recovery, never called by a history getter or artifact decoder.
    pub async fn reconcile_interrupted_subagents(
        &self,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        self.call(|s| s.reconcile_subagents()).await
    }
}

impl Runtime {
    pub(super) fn execution_cwd(&self, actor: &EventActor) -> Result<PathBuf, CoordinatorError> {
        match actor.agent_id.as_ref() {
            Some(id) => {
                let agent = self
                    .agents
                    .get(id)
                    .ok_or_else(|| CoordinatorError::UnknownAgent(id.clone()))?;
                let cwd = agent.cwd.canonicalize()?;
                if cwd != agent.cwd
                    || !cwd.is_dir()
                    || !cwd.starts_with(&self.info()?.workspace_root)
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

    fn authorize_agent_state(
        &self,
        actor: &EventActor,
        target: &str,
    ) -> Result<(), CoordinatorError> {
        let agent = self
            .agents
            .get(target)
            .ok_or_else(|| CoordinatorError::UnknownAgent(target.into()))?;
        if matches!(actor.kind, ActorKind::Supervisor | ActorKind::System)
            && actor.agent_id.is_none()
        {
            return Ok(());
        }
        let owner = actor.agent_id.as_ref().ok_or_else(|| {
            CoordinatorError::PermissionDenied("agent state requires an owner".into())
        })?;
        if !self.agents.contains_key(owner)
            || (target != owner && agent.info.parent_agent_id.as_ref() != Some(owner))
        {
            return Err(CoordinatorError::PermissionDenied(
                "agent state is not owned by this actor".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn begin_agent_attempt(
        &mut self,
        agent: &str,
        attempt: &str,
    ) -> Result<(), CoordinatorError> {
        let state = &self.agents[agent];
        let generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| CoordinatorError::Invalid("attempt generation exhausted".into()))?;
        if state.info.parent_agent_id.is_some() {
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

    fn agent_transition(
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
        Ok(SubagentTransitionV1 {
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
        })
    }

    /// Candidate fold, authoritative append, then state application. Rejection appends nothing.
    pub(super) fn commit_subagent_transition(
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

    pub(super) fn finish_agent_state(
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
        let reference = if retain_snapshot {
            Some(self.prepare_finalized_state(agent, attempt, messages)?)
        } else {
            None
        };
        if state_parent(self, agent).is_some() {
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
                    tool_calls: u32::try_from(
                        messages
                            .entries
                            .iter()
                            .filter(|e| {
                                e.turn.as_deref() == Some(attempt)
                                    && e.message.role == harness_providers::MessageRole::Tool
                            })
                            .count(),
                    )
                    .unwrap_or(u32::MAX),
                    turns: u32::try_from(request_count).unwrap_or(u32::MAX),
                    duration_ms: self
                        .clock
                        .mono_ms()
                        .saturating_sub(self.agents[agent].attempt_started_ms),
                    tokens_used: usage.map(|u| u.0),
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

    fn prepare_finalized_state(
        &mut self,
        agent_id: &str,
        attempt: &str,
        messages: &Context,
    ) -> Result<FinalizedAgentStateReferenceV1, CoordinatorError> {
        let agent = &self.agents[agent_id];
        let fidelity = if messages.unavailable == Some(FinalizedStateUnavailable::LegacySummaryOnly)
        {
            FinalizedStateFidelity::SummaryOnly
        } else {
            FinalizedStateFidelity::Exact
        };
        let snapshot = RawFinalizedState {
            payload_version: 1,
            owner_run_id: self.info()?.run_id.to_string(),
            owner_agent_id: SubagentId(agent_id.into()),
            owner_session_id: if agent.info.parent_agent_id.is_some() {
                agent_id.into()
            } else {
                self.info()?.run_id.to_string()
            },
            attempt_id: attempt.into(),
            generation: agent.generation,
            conversation_items: messages
                .entries
                .iter()
                .filter(|e| e.message.role != harness_providers::MessageRole::System)
                .map(|e| FinalizedConversationItem {
                    message: e.message.clone(),
                    event_seq: e.seq,
                    turn_id: e.turn.clone(),
                    attachments: e.attachments.clone(),
                    settled_reasoning: e.settled_reasoning.clone(),
                    raw_tool_result: e.raw_tool_result.clone(),
                })
                .collect(),
            model_request: messages.model_request.clone(),
            usage: messages.usage.clone(),
            source_model: agent.info.model_ref.clone(),
            model_target: agent.target.clone(),
            model_settings: agent.settings.clone(),
            execution_context: agent.execution.clone(),
            read_state: agent
                .tool_state
                .read_snapshot()
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?,
            source_reference: agent.source_reference.clone(),
            fidelity,
            availability: if messages.unavailable.is_some() {
                FinalizedStateAvailability::Unsupported
            } else {
                FinalizedStateAvailability::Available
            },
            unavailable: messages.unavailable,
        };
        let unrepresentable = agent.cwd.to_str().is_none()
            || snapshot
                .read_state
                .keys()
                .any(|path| path.to_str().is_none());
        let bytes = if unrepresentable {
            Vec::new()
        } else {
            serde_json::to_vec(&snapshot)?
        };
        let text = String::from_utf8_lossy(&bytes);
        let safe = crate::redact::redact_artifact_text(&text);
        let policy_modified =
            safe != text || self.redactor.redact_text(&text) != text || text.contains("[REDACTED");
        let unavailable = if unrepresentable {
            Some(FinalizedStateUnavailable::UnsupportedEncoding)
        } else if policy_modified {
            Some(FinalizedStateUnavailable::PolicyModified)
        } else if bytes.len() as u64 > MAX_FINALIZED_STATE_BYTES {
            Some(FinalizedStateUnavailable::Incomplete)
        } else {
            messages.unavailable
        };
        let mut reference = FinalizedAgentStateReferenceV1 {
            payload_version: 1,
            state: FinalizedSubagentStateReference {
                payload_version: 1,
                sha256: String::new(),
                owner: SubagentId(agent_id.into()),
                fidelity: if policy_modified {
                    FinalizedStateFidelity::Redacted
                } else {
                    fidelity
                },
                availability: if unavailable.is_some() {
                    FinalizedStateAvailability::Unsupported
                } else {
                    FinalizedStateAvailability::Available
                },
            },
            byte_length: 0,
            owner_run_id: snapshot.owner_run_id.clone(),
            owner_session_id: snapshot.owner_session_id,
            attempt_id: attempt.into(),
            generation: agent.generation,
            unavailable,
        };
        if !unrepresentable && !policy_modified && bytes.len() as u64 <= MAX_FINALIZED_STATE_BYTES {
            reference.state.sha256 = format!("{:x}", Sha256::digest(&bytes));
            reference.byte_length = bytes.len() as u64;
            crate::store::create_private_dir(&self.info()?.artifacts_dir)?;
            crate::store::write_private_atomic(
                &self.info()?.run_dir.join(reference.artifact_path()),
                &bytes,
            )?;
        }
        Ok(reference)
    }

    pub(super) fn resolve_agent_finalized_state(
        &self,
        id: &str,
    ) -> Result<FinalizedStateResult, CoordinatorError> {
        let agent = self
            .agents
            .get(id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(id.into()))?;
        if self.fault.is_some() {
            return Ok(FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Incomplete,
            });
        }
        if agent.busy
            || !agent.queue.is_empty()
            || self
                .running
                .values()
                .any(|job| job.actor.agent_id.as_deref() == Some(id))
        {
            return Ok(FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Active,
            });
        }
        let Some(reference) = &agent.finalized else {
            return Ok(FinalizedStateResult::Unavailable {
                reason: agent
                    .messages
                    .unavailable
                    .unwrap_or(FinalizedStateUnavailable::Missing),
            });
        };
        let result = resolve_finalized_state(
            &self.info()?.run_dir,
            reference,
            self.projection_owner
                .as_deref()
                .filter(|owner| *owner == reference.owner_run_id)
                .unwrap_or(self.info()?.run_id.as_str()),
            &SubagentId(id.into()),
            &reference.attempt_id,
        );
        if let FinalizedStateResult::Available { state } = &result {
            let value = serde_json::to_value(state)?;
            if crate::redact::redact_value(self.redactor.as_ref(), &value) != value {
                return Ok(FinalizedStateResult::Unavailable {
                    reason: FinalizedStateUnavailable::PolicyModified,
                });
            }
            for attachment in state
                .conversation_items
                .iter()
                .flat_map(|item| {
                    item.attachments
                        .iter()
                        .chain(item.raw_tool_result.iter().flat_map(|result| {
                            result.output.iter().flat_map(|output| &output.attachments)
                        }))
                })
                .chain(
                    state
                        .model_request
                        .iter()
                        .flat_map(|request| request.attachments.values().flatten()),
                )
            {
                let bytes = attachment
                    .bytes()
                    .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                let text = String::from_utf8_lossy(bytes);
                if self.redactor.redact_text(&text) != text {
                    return Ok(FinalizedStateResult::Unavailable {
                        reason: FinalizedStateUnavailable::PolicyModified,
                    });
                }
            }
        }
        Ok(result)
    }

    fn initialize_finalized(
        &mut self,
        actor: EventActor,
        target: &str,
        source: &str,
        mode: FinalizedContextCopy,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        self.authorize_agent_state(&actor, target)?;
        self.authorize_agent_state(&actor, source)?;
        let target_state = &self.agents[target];
        if target_state.busy
            || !target_state.queue.is_empty()
            || self
                .running
                .values()
                .any(|job| job.actor.agent_id.as_deref() == Some(target))
            || (mode != FinalizedContextCopy::Wake
                && (target == source || target_state.generation != 0))
            || (mode == FinalizedContextCopy::Wake
                && (target != source || self.killed_agents.contains(target)))
        {
            return Err(CoordinatorError::Invalid(
                "copy requires a fresh target or an idle same-identity wake".into(),
            ));
        }
        if mode == FinalizedContextCopy::Resume
            && target_state.info.parent_agent_id != self.agents[source].info.parent_agent_id
        {
            return Err(CoordinatorError::PermissionDenied(
                "resume source must have the same spawner".into(),
            ));
        }
        let FinalizedStateResult::Available { mut state } =
            self.resolve_agent_finalized_state(source)?
        else {
            return Err(CoordinatorError::Invalid(
                "finalized source is unavailable".into(),
            ));
        };
        let source_cwd = PathBuf::from(&state.execution_context.effective_cwd).canonicalize()?;
        let root = &self.info()?.workspace_root;
        let selector = source_cwd
            .strip_prefix(root)
            .unwrap_or(&source_cwd)
            .to_string_lossy();
        if source_cwd != Path::new(&state.execution_context.effective_cwd)
            || !source_cwd.is_dir()
            || !source_cwd.starts_with(root)
            || matches!(
                state.execution_context.isolation,
                ResolvedSubagentIsolation::Worktree { .. }
            )
            || self
                .config
                .permission_policy
                .check("read", &selector, Some(&target_state.policy))
                != crate::perm::PermissionAction::Allow
        {
            return Err(CoordinatorError::PermissionDenied(
                "source cwd is no longer supported by the target policy".into(),
            ));
        }
        let reference = self.agents[source]
            .finalized
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("source reference missing".into()))?;
        let context = restored_context(
            &mut state,
            &target_state.profile.system_prompt,
            &self.info()?.run_dir,
        )?;
        let reads = self
            .tool_state
            .with_read_snapshot(state.read_state.clone())
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        let target_id = target.to_owned();
        self.emit_applied(
            actor,
            None,
            EventV1::AgentContextInitialized(AgentContextInitializedV1 {
                payload_version: 1,
                agent_id: SubagentId(target.into()),
                mode,
                source: reference.clone(),
            }),
            move |runtime, _| {
                if let Some(target) = runtime.agents.get_mut(&target_id) {
                    target.messages = context;
                    target.tool_state = reads;
                    target.source_reference = Some(Box::new(reference));
                    if mode != FinalizedContextCopy::Fork {
                        target.execution = state.execution_context;
                        target.cwd = source_cwd;
                        target.info.model_ref = state.source_model;
                        target.settings = state.model_settings;
                        target.target = state.model_target;
                    }
                }
            },
        )?;
        Ok(())
    }

    fn subagent_cancel(
        &mut self,
        actor: EventActor,
        command: SubagentCommandRequest,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        self.accepting()?;
        let root = self.info()?.run_id.to_string();
        let mut targets = Vec::new();
        match &command {
            SubagentCommandRequest::ExplicitChildKill { child_id } => {
                self.authorize_agent_state(&actor, &child_id.0)?;
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
                self.authorize_agent_state(&actor, session_id)?;
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
                self.authorize_agent_state(&actor, &id)?;
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
                    self.authorize_agent_state(&actor, session_id)?;
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
                    self.authorize_agent_state(&actor, id)?;
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
            let queued: Vec<_> = self.agents[&target.0].queue.iter().filter(|turn| {
                !matches!(&command, SubagentCommandRequest::ParentPromptCancel { prompt_id } if &turn.id != prompt_id)
            }).map(|turn| turn.id.clone()).collect();
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
            None => state_parent(self, id),
        }
    }

    pub(super) fn reconcile_subagents(&mut self) -> Result<Vec<SubagentId>, CoordinatorError> {
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

fn state_parent<'a>(runtime: &'a Runtime, id: &str) -> Option<&'a str> {
    runtime.agents.get(id)?.info.parent_agent_id.as_deref()
}

pub(super) fn restored_context(
    state: &mut RawFinalizedState,
    system_prompt: &str,
    run_dir: &Path,
) -> Result<Context, CoordinatorError> {
    let mut context = Context::new(system_prompt);
    for item in &state.conversation_items {
        context.entries.push(Entry {
            message: item.message.clone(),
            seq: item.event_seq,
            turn: item.turn_id.clone(),
            attachments: item.attachments.clone(),
            settled_reasoning: item.settled_reasoning.clone(),
            raw_tool_result: item.raw_tool_result.clone(),
        });
    }
    context.usage = state.usage.clone();
    context.model_request = state.model_request.clone();
    context.unavailable = state.unavailable;
    super::prompt::restore_content(&mut context, run_dir, &std::collections::HashMap::new())?;
    Ok(context)
}

/// A reusable completed prefix is not the whole current conversation.
/// Later presentation-only turns remain visible without acquiring exact fidelity.
pub(super) fn current_context(
    state: &mut RawFinalizedState,
    historical: Context,
    system_prompt: &str,
    run_dir: &Path,
) -> Result<Context, CoordinatorError> {
    if historical.entries.iter().any(|entry| {
        entry.message.role != harness_providers::MessageRole::System && entry.turn.is_none()
    }) {
        return Ok(historical);
    }
    let covered: std::collections::BTreeSet<_> = state
        .conversation_items
        .iter()
        .filter_map(|item| item.turn_id.clone())
        .collect();
    let mut context = restored_context(state, system_prompt, run_dir)?;
    let suffix: Vec<_> = historical
        .entries
        .into_iter()
        .filter(|entry| {
            entry.message.role != harness_providers::MessageRole::System
                && entry
                    .turn
                    .as_ref()
                    .is_some_and(|turn| !covered.contains(turn))
        })
        .collect();
    if !suffix.is_empty() {
        context.unavailable = Some(FinalizedStateUnavailable::LegacySummaryOnly);
        context.model_request = None;
        context.entries.extend(suffix);
    }
    Ok(context)
}
