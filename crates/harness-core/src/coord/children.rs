use super::{
    runtime::{digest, JobKind},
    *,
};
use crate::{
    foreground_demote::{DemoteToBackgroundResult, ForegroundKind},
    tool::ToolCapability,
};
use serde_json::json;
mod output;
mod restore;

pub struct ChildTaskRequest {
    pub session_id: Option<String>,
    pub profile: String,
    pub prompt: String,
    pub description: String,
    pub run_in_background: bool,
}
pub(super) struct Child {
    parent_agent: String,
    parent_tool: String,
    parent_request: Option<String>,
    parent_session: String,
    description: String,
    pub request: String,
    background: bool,
    pub complete: bool,
    reply: Option<Reply<ToolResult>>,
}
impl Child {
    fn result(&self, session: &str, status: &str, summary: &str) -> ToolResult {
        ToolResult::structured(
            summary,
            json!({
                "session_id":session, "task_id":session, "request_id":self.request,
                "agent_id":session,
                "status":status, "description":self.description, "run_in_background":self.background,
                "child_summary":summary.chars().take(1024).collect::<String>(),
                "is_error":matches!(status,"failed"|"cancelled"),
            }),
        )
    }
}
impl CoordinatorHandle {
    pub async fn delegate_task(
        &self,
        tool: impl Into<String>,
        request: ChildTaskRequest,
    ) -> Result<ToolResult, CoordinatorError> {
        let tool = tool.into();
        let (tx, rx) = oneshot::channel();
        self.call(move |s| s.delegate_task(&tool, request, tx))
            .await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
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
            let requests: Vec<_> = s
                .children
                .values()
                .filter(|c| !c.complete && !c.background)
                .map(|c| c.request.clone())
                .collect();
            requests.iter().map(|id| s.demote_child(id)).collect()
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
        let reserved = self
            .children
            .values()
            .filter(|child| {
                child.parent_agent == agent
                    && child.background
                    && !child.complete
                    && child.parent_request.is_some()
            })
            .count();
        if state.queue.len().saturating_add(reserved) >= self.config.command_buffer {
            return Err(CoordinatorError::Invalid(
                "agent prompt queue is full, including reserved background notifications".into(),
            ));
        }
        Ok(())
    }
    fn delegate_task(
        &mut self,
        tool: &str,
        request: ChildTaskRequest,
        reply: Reply<ToolResult>,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        self.check_task(tool)?;
        let job = self
            .running
            .get(tool)
            .ok_or_else(|| CoordinatorError::UnknownTask(tool.into()))?;
        if job.join_id.is_none()
            || !matches!(
                job.kind,
                JobKind::Tool {
                    capability: ToolCapability::SpawnAgent,
                    ..
                }
            )
        {
            return Err(CoordinatorError::PermissionDenied(
                "delegation requires a running spawn-capable tool".into(),
            ));
        }
        let parent = job.actor.agent_id.clone().ok_or_else(|| {
            CoordinatorError::Invalid("delegation requires a parent agent".into())
        })?;
        let parent_request = job.parent.clone();
        if request.run_in_background && parent_request.is_some() {
            self.check_turn_capacity(&parent)?;
        }
        let parent_session = if self.children.contains_key(&parent) {
            parent.clone()
        } else {
            self.info()?.run_id.to_string()
        };
        if self
            .children
            .values()
            .filter(|child| !child.complete)
            .count()
            >= self.config.command_buffer
            || request.description.len() > 1024
        {
            return Err(CoordinatorError::Invalid(
                "child context capacity or description limit exceeded".into(),
            ));
        }
        let prompt = super::prompt::Prompt::from(request.prompt);
        prompt.validate(self.redactor.as_ref())?;
        let agent = if let Some(session) = request.session_id {
            let child = self
                .children
                .get(&session)
                .ok_or_else(|| CoordinatorError::UnknownAgent(session.clone()))?;
            if child.parent_agent != parent || !child.complete {
                return Err(CoordinatorError::PermissionDenied(
                    "continuation requires an idle direct child".into(),
                ));
            }
            let state = &self.agents[&session];
            if !request.profile.is_empty() && request.profile != state.profile.name {
                return Err(CoordinatorError::Invalid(
                    "continuation cannot change the child's profile".into(),
                ));
            }
            let messages = super::history::messages(
                &crate::store::read_events(&self.info()?.events_path)?,
                &session,
                false,
                &state.profile.system_prompt,
                &self.info()?.run_dir,
            )?;
            if let Some(state) = self.agents.get_mut(&session) {
                state.messages = messages;
            }
            session
        } else {
            let actor = EventActor::new(ActorKind::Worker, Some(parent.clone()));
            let id = self.spawn_agent(actor, &request.profile, Some(parent.clone()))?;
            let owner = &self.agents[&parent];
            let (model, settings, target) = (
                owner.info.model_ref.clone(),
                owner.settings.clone(),
                owner.target.clone(),
            );
            if let Some(child) = self
                .agents
                .get_mut(&id)
                .filter(|child| !child.profile.model_ref_explicit)
            {
                child.info.model_ref = model;
                child.settings = settings;
                child.target = target;
            }
            id
        };
        self.children.insert(
            agent.clone(),
            Child {
                parent_agent: parent,
                parent_tool: tool.into(),
                parent_request,
                parent_session,
                description: request.description,
                request: String::new(),
                background: request.run_in_background,
                complete: false,
                reply: Some(reply),
            },
        );
        let id = self.queue_turn(
            EventActor::new(ActorKind::Worker, Some(agent.clone())),
            &agent,
            prompt,
            None,
            None,
            None,
        )?;
        if !request.run_in_background {
            if let Some(job) = self.running.get_mut(&id) {
                job.parent = Some(tool.into());
            }
        } else if let Some(child) = self.children.get_mut(&agent) {
            if let Some(reply) = child.reply.take() {
                let _ = reply.send(Ok(child.result(
                    &agent,
                    "running",
                    "Child task started in the background.",
                )));
            }
        }
        Ok(())
    }
    fn demote_child(&mut self, handle: &str) -> Result<DemoteToBackgroundResult, CoordinatorError> {
        self.accepting()?;
        let session = self
            .children
            .iter()
            .find(|(id, c)| {
                (id.as_str() == handle || c.request == handle) && !c.complete && !c.background
            })
            .map(|(id, _)| id.clone());
        let Some(session) = session else {
            return Ok(DemoteToBackgroundResult::Rejected {
                handle_id: handle.into(),
                reason: "no running foreground child matches this handle".into(),
            });
        };
        let child = &self.children[&session];
        if child.parent_request.is_some() && self.check_turn_capacity(&child.parent_agent).is_err()
        {
            return Ok(DemoteToBackgroundResult::Rejected {
                handle_id: handle.into(),
                reason: "parent queue has no capacity for a background notification".into(),
            });
        }
        let id = child.request.clone();
        let actor = EventActor::new(ActorKind::User, Some(child.parent_agent.clone()));
        self.emit(
            actor,
            Some(id.clone()),
            EventV1::UiIntentReceived(UiIntentReceivedEvent {
                intent: "background_foreground_child".into(),
                params: [
                    ("request_id".into(), id.clone()),
                    ("session_id".into(), session.clone()),
                ]
                .into(),
            }),
        )?;
        if let Some(job) = self.running.get_mut(&id) {
            job.parent = None;
        }
        if let Some(child) = self.children.get_mut(&session) {
            child.background = true;
            if let Some(reply) = child.reply.take() {
                let _ = reply.send(Ok(child.result(
                    &session,
                    "running",
                    "Child task continues in the background.",
                )));
            }
        }
        Ok(DemoteToBackgroundResult::Demoted {
            handle_id: handle.into(),
            background_id: id,
            kind: ForegroundKind::Task,
        })
    }
    pub fn child_lineage(&self, actor: &EventActor, payload: &mut EventV1) {
        let Some((session, child)) = actor
            .agent_id
            .as_ref()
            .and_then(|id| self.children.get(id).map(|c| (id, c)))
        else {
            return;
        };
        let model = self
            .agents
            .get(session)
            .map(|a| crate::agent::AgentModelRef::parse(&a.info.model_ref));
        let lineage = TaskLineageMetadata {
            parent_tool_call_id: Some(child.parent_tool.clone()),
            parent_task_id: child.parent_request.clone(),
            parent_request_id: child.parent_request.clone(),
            parent_session_id: Some(child.parent_session.clone()),
            child_session_id: Some(session.clone()),
            child_request_id: Some(child.request.clone()),
            child_provider_id: model.as_ref().map(|m| m.provider_id.clone()),
            child_model_id: model.map(|m| m.model_id),
        };
        let target = match payload {
            EventV1::TaskScheduled(e) => &mut e.metadata.get_or_insert_default().lineage,
            EventV1::TaskCompleted(e) => &mut e.metadata.get_or_insert_default().lineage,
            EventV1::ToolCallRequested(e) => &mut e.metadata.get_or_insert_default().lineage,
            EventV1::ToolCallFinished(e) => &mut e.metadata.get_or_insert_default().lineage,
            _ => return,
        };
        if target.is_none() {
            *target = Some(lineage);
        }
    }
    pub fn finish_child(
        &mut self,
        agent: &str,
        terminal: &EventEnvelopeV1,
    ) -> Result<(), CoordinatorError> {
        let Some(child) = self.children.get_mut(agent) else {
            return Ok(());
        };
        let (status, summary, notification) = match &terminal.payload {
            EventV1::TaskCompleted(e) => (
                "completed",
                e.result_summary.as_str(),
                BackgroundTaskNotificationStatus::Completed,
            ),
            EventV1::TaskCancelled(e) if e.failure => (
                "failed",
                e.reason.as_str(),
                BackgroundTaskNotificationStatus::Failed,
            ),
            EventV1::TaskCancelled(e) => (
                "cancelled",
                e.reason.as_str(),
                BackgroundTaskNotificationStatus::Cancelled,
            ),
            _ => return Ok(()),
        };
        child.complete = true;
        if let Some(reply) = child.reply.take() {
            let result = if status == "completed" {
                Ok(child.result(agent, status, summary))
            } else {
                Err(CoordinatorError::Invalid(summary.into()))
            };
            let _ = reply.send(result);
        }
        if !child.background {
            return Ok(());
        }
        let parent = child.parent_agent.clone();
        let deliver = child.parent_request.is_some()
            && self.stopping.is_none()
            && self.rewind.is_none()
            && self.fault.is_none()
            && self.agents.contains_key(&parent);
        let actor = EventActor::new(ActorKind::Worker, Some(child.parent_agent.clone()));
        let request = child.request.clone();
        let mut notification = BackgroundTaskNotificationEvent {
            parent_session_id: child.parent_session.clone().into(),
            parent_agent_id: Some(child.parent_agent.clone()),
            child_session_id: agent.into(),
            child_request_id: request.clone(),
            task_id: request.clone().into(),
            description: child.description.clone(),
            status: notification,
            summary: summary.chars().take(1024).collect(),
            terminal_event_id: terminal.event_id.clone(),
            terminal_task_id: request.clone(),
            delivered_turn_request_id: None,
        };
        let prompt = format!(
            "Background child {agent} {status}.\nReport:\n{}",
            notification.summary
        );
        let reserved_id = if deliver {
            Some(self.id("turn")?)
        } else {
            None
        };
        notification
            .delivered_turn_request_id
            .clone_from(&reserved_id);
        self.emit(
            actor.clone(),
            Some(request.clone()),
            EventV1::BackgroundTaskNotification(notification),
        )?;
        if let Some(id) = reserved_id {
            self.queue_turn(
                actor,
                &parent,
                super::prompt::Prompt {
                    text: prompt,
                    reserved_id: Some(id),
                    child_completion: Some(request),
                    ..Default::default()
                },
                None,
                None,
                None,
            )?;
        }
        Ok(())
    }
    pub fn consumed_child_notifications(
        &self,
        job: &runtime::Job,
        result: &Result<ToolResult, CoordinatorError>,
    ) -> Vec<String> {
        let (JobKind::Tool { tool_id, .. }, Ok(output), Some(parent)) =
            (&job.kind, result, job.actor.agent_id.as_deref())
        else {
            return Vec::new();
        };
        if !matches!(
            tool_id.as_str(),
            "task" | "background_output" | "background_cancel"
        ) {
            return Vec::new();
        }
        let Some(value) = &output.structured_json else {
            return Vec::new();
        };
        // Like grok-build, consume only reports actually returned by a tool,
        // not intermediate polls made while that tool is still waiting.
        let consumed: Vec<_> = std::iter::once(value)
            .chain(value["tasks"].as_array().into_iter().flatten())
            .filter(|report| {
                matches!(
                    report["status"].as_str(),
                    Some("completed" | "failed" | "cancelled")
                ) && report["session_id"]
                    .as_str()
                    .and_then(|id| self.children.get(id))
                    .is_some_and(|child| child.parent_agent == parent)
            })
            .filter_map(|report| report["request_id"].as_str())
            .collect();
        self.agents
            .get(parent)
            .into_iter()
            .flat_map(|agent| &agent.queue)
            .filter(|turn| {
                turn.prompt
                    .child_completion
                    .as_deref()
                    .is_some_and(|id| consumed.contains(&id))
            })
            .map(|turn| turn.id.clone())
            .collect()
    }
    pub fn fail_child_waiters(&mut self, message: &str) {
        for child in self.children.values_mut() {
            if let Some(reply) = child.reply.take() {
                let _ = reply.send(Err(CoordinatorError::Invalid(message.into())));
            }
        }
    }
    pub fn child_agent_id(&mut self) -> Result<String, CoordinatorError> {
        let id = self.id("child")?;
        Ok(format!(
            "child-{}",
            digest(&format!("{}:{id}", self.info()?.run_id))
        ))
    }
}
