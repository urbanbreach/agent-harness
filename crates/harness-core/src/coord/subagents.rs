//! Subordinate state inside Runtime. This module has no worker or journal authority.
use super::{context::Context, handle::system, runtime::*, *};
use crate::subagent::*;

mod cancellation;
#[path = "subagents/context.rs"]
mod context_helpers;
mod finalized;
mod lifecycle;

pub(super) use context_helpers::{current_context, restored_context};

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
                    system_prompt: None,
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
}

fn state_parent<'a>(runtime: &'a Runtime, id: &str) -> Option<&'a str> {
    runtime.agents.get(id)?.info.parent_agent_id.as_deref()
}
