//! Historical child lineage and the shared coordinator hooks. The old delegated
//! task executor is retired; native execution lives in public_subagents.
use super::{runtime::digest, *};
use crate::foreground_demote::{DemoteToBackgroundResult, ForegroundKind};
mod restore;

/// Read-only historical decoding material. It never schedules a child.
pub(super) struct Child {
    pub(super) parent_agent: String,
    pub(super) parent_tool: String,
    pub(super) parent_request: Option<String>,
    pub(super) parent_session: String,
    pub(super) description: String,
    pub request: String,
    pub(super) background: bool,
    pub complete: bool,
}

impl CoordinatorHandle {
    pub async fn demote_foreground_child_task(
        &self,
        handle: impl Into<String>,
    ) -> Result<DemoteToBackgroundResult, CoordinatorError> {
        let handle = handle.into();
        self.call(move |s| s.demote_child(&handle)).await
    }

    pub async fn demote_all_foreground_child_tasks(
        &self,
    ) -> Result<Vec<DemoteToBackgroundResult>, CoordinatorError> {
        self.call(|s| {
            s.accepting()?;
            let ids = s.native_foreground_children();
            ids.iter().map(|id| s.demote_child(id)).collect()
        })
        .await
    }
}

impl Runtime {
    pub fn check_turn_capacity(&self, agent: &str) -> Result<(), CoordinatorError> {
        let state = self
            .agents
            .get(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        let reserved = self.native_notification_reservations(agent)
            + self
                .detached_evals
                .iter()
                .filter(|id| {
                    self.running
                        .get(*id)
                        .is_some_and(|job| job.actor.agent_id.as_deref() == Some(agent))
                })
                .count();
        if state.queue.len().saturating_add(reserved) >= self.config.command_buffer {
            return Err(CoordinatorError::Invalid(
                "agent prompt queue is full, including reserved background notifications".into(),
            ));
        }
        Ok(())
    }

    fn demote_child(&mut self, handle: &str) -> Result<DemoteToBackgroundResult, CoordinatorError> {
        self.accepting()?;
        let Some((id, request)) = self.native_foreground_handle(handle) else {
            return Ok(DemoteToBackgroundResult::Rejected {
                handle_id: handle.into(),
                reason: "no running foreground child matches this handle".into(),
            });
        };
        self.demote_native_subagent(&id)?;
        Ok(DemoteToBackgroundResult::Demoted {
            handle_id: handle.into(),
            background_id: request,
            kind: ForegroundKind::Task,
        })
    }

    pub fn child_lineage(&self, actor: &EventActor, payload: &mut EventV1) {
        let Some(session) = actor.agent_id.as_deref() else {
            return;
        };
        let material = self.native_lineage(session).or_else(|| {
            let child = self.children.get(session)?;
            Some((
                child.parent_tool.clone(),
                child.parent_request.clone(),
                child.parent_session.clone(),
                child.request.clone(),
            ))
        });
        let Some((tool, parent, parent_session, request)) = material else {
            return;
        };
        let model = self
            .agents
            .get(session)
            .map(|a| crate::agent::AgentModelRef::parse(&a.info.model_ref));
        let lineage = TaskLineageMetadata {
            parent_tool_call_id: Some(tool),
            parent_task_id: parent.clone(),
            parent_request_id: parent,
            parent_session_id: Some(parent_session),
            child_session_id: Some(session.into()),
            child_request_id: Some(request),
            child_provider_id: model.as_ref().map(|m| m.provider_id.clone()),
            child_model_id: model.map(|m| m.model_id),
        };
        let target = match payload {
            EventV1::TaskScheduled(event) => &mut event.metadata.get_or_insert_default().lineage,
            EventV1::TaskCompleted(event) => &mut event.metadata.get_or_insert_default().lineage,
            EventV1::ToolCallRequested(event) => {
                &mut event.metadata.get_or_insert_default().lineage
            }
            EventV1::ToolCallFinished(event) | EventV1::EvalCellFinished(event) => {
                &mut event.metadata.get_or_insert_default().lineage
            }
            _ => return,
        };
        if target.is_none() {
            *target = Some(lineage);
        }
    }

    /// Generic completion still calls one shared hook; legacy children have no
    /// executor or continuation side effects.
    pub fn finish_child(
        &mut self,
        agent: &str,
        terminal: &EventEnvelopeV1,
    ) -> Result<(), CoordinatorError> {
        if self.native_subagents.contains_key(agent) {
            self.native_subagent_finished(agent, terminal)
        } else {
            if let Some(child) = self.children.get_mut(agent) {
                child.complete = true;
            }
            Ok(())
        }
    }

    pub fn consumed_child_notifications(
        &self,
        job: &runtime::Job,
        result: &Result<ToolResult, CoordinatorError>,
    ) -> Vec<String> {
        self.native_consumed_notifications(job, result)
    }

    pub fn fail_child_waiters(&mut self, message: &str) {
        self.fail_native_waiters(message);
    }

    pub(super) fn detach_child_waiter(&mut self, waiter: &str, reason: &str) {
        self.detach_native_waiter(waiter, reason);
    }

    /// Generic non-native orchestration keeps its historical identity contract.
    pub fn child_agent_id(&mut self) -> Result<String, CoordinatorError> {
        let id = self.id("child")?;
        Ok(format!(
            "child-{}",
            digest(&format!("{}:{id}", self.info()?.run_id))
        ))
    }
}
