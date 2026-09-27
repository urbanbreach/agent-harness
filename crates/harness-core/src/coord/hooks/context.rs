use super::*;
use crate::agent::AgentModelRef;

impl Runtime {
    pub(super) fn hook_context(
        &self,
        actor: &EventActor,
        task: Option<&str>,
    ) -> serde_json::Map<String, Value> {
        let agent = actor.agent_id.as_ref().and_then(|id| self.agents.get(id));
        let model = agent.map(|a| AgentModelRef::parse(&a.info.model_ref));
        let job = task.and_then(|id| self.running.get(id));
        let tool_id = job.and_then(|job| match &job.kind {
            runtime::JobKind::Tool { tool_id, .. } => Some(tool_id),
            _ => None,
        });
        let request = job.and_then(|job| job.parent.as_deref()).or(task);
        let kind = format!("{:?}", actor.kind).to_ascii_lowercase();
        serde_json::Map::from_iter([
            (
                "actor".into(),
                json!({"kind": kind, "agent_id": actor.agent_id}),
            ),
            ("actor_kind".into(), json!(kind)),
            ("actor_agent_id".into(), json!(actor.agent_id)),
            ("agent_id".into(), json!(actor.agent_id)),
            ("task_id".into(), json!(task)),
            ("request_id".into(), json!(request)),
            ("tool_id".into(), json!(tool_id)),
            ("tool_call_id".into(), json!(tool_id.and(task))),
            ("profile".into(), json!(agent.map(|a| &a.info.profile_name))),
            (
                "parent_agent_id".into(),
                json!(agent.and_then(|a| a.info.parent_agent_id.as_ref())),
            ),
            (
                "provider_id".into(),
                json!(model.as_ref().map(|m| &m.provider_id)),
            ),
            (
                "model_id".into(),
                json!(model.as_ref().map(|m| &m.model_id)),
            ),
        ])
    }

    /// Called only for new live transitions. Historical recovery uses emit directly.
    pub fn emit_hooked(
        &mut self,
        actor: EventActor,
        correlation: Option<String>,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, CoordinatorError> {
        if self.config.hook_runtime_config.hooks.lifecycle.is_empty() {
            return self.emit(actor, correlation, payload);
        }
        let hook = match &payload {
            EventV1::TaskScheduled(e) if e.state == TaskScheduleState::Started => {
                Some((Hook::AgentTurnStarted, json!({"outcome":"started"})))
            }
            EventV1::ToolCallStarted(_) => {
                Some((Hook::ToolCallStarted, json!({"outcome":"started"})))
            }
            EventV1::ProviderRequestStarted(e) => Some((
                Hook::ProviderRequestStarted,
                json!({"request_id":e.request_id,"provider_id":e.provider_id,"model_id":e.model_id,"outcome":"started"}),
            )),
            EventV1::ProviderRequestFinished(e) => Some((
                Hook::ProviderRequestFinished,
                json!({"request_id":e.request_id,"outcome":e.finish_reason}),
            )),
            EventV1::AgentSpawned(e) if e.parent_agent_id.is_some() => Some((
                Hook::SubagentSpawned,
                json!({"agent_id":e.agent_id,"parent_agent_id":e.parent_agent_id,"profile":e.profile,"outcome":"spawned"}),
            )),
            EventV1::PermissionRequested(e) => Some((
                Hook::PermissionRequested,
                json!({"permission_id":e.permission_id,"tool_call_id":e.tool_call_id,"outcome":"requested"}),
            )),
            _ => None,
        };
        if let Some((event, fields)) = hook {
            if event == Hook::ProviderRequestFinished {
                let written = self.emit(actor.clone(), correlation.clone(), payload)?;
                self.hook(event, &actor, correlation.as_deref(), fields)?;
                return Ok(written);
            }
            self.hook(event, &actor, correlation.as_deref(), fields)?;
        }
        self.emit(actor, correlation, payload)
    }
}
