use super::{
    runtime::{JobKind, Runtime},
    *,
};
use crate::tool::ToolCapability;
use serde_json::Value;

impl CoordinatorHandle {
    /// A redirect may reuse an existing allow rule. Other destinations need a new tool call.
    pub async fn network_redirect_allowed(
        &self,
        task: impl Into<String>,
        url: impl Into<String>,
    ) -> Result<bool, CoordinatorError> {
        let (task, url) = (task.into(), url.into());
        self.call(move |s| {
            s.check_task(&task)?;
            let job = &s.running[&task];
            let JobKind::Tool {
                tool_id,
                capability: ToolCapability::Network,
                ..
            } = &job.kind
            else {
                return Err(CoordinatorError::Invalid(
                    "redirect requires a network tool".into(),
                ));
            };
            let tool = s
                .config
                .tool_registry
                .get(tool_id)
                .ok_or_else(|| CoordinatorError::UnknownTool(tool_id.clone()))?;
            let profile = job
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| s.agents.get(id))
                .map(|a| &a.policy);
            Ok(tool
                .permission_requests(&serde_json::json!({"url":url}))
                .iter()
                .all(|(permission, selector)| {
                    match s
                        .config
                        .permission_policy
                        .check(permission, selector, profile)
                    {
                        crate::perm::PermissionAction::Allow => true,
                        crate::perm::PermissionAction::Ask => s.config.yolo_on_start,
                        crate::perm::PermissionAction::Deny => false,
                    }
                }))
        })
        .await
    }
    pub async fn execute_nested_tool(
        &self,
        parent: impl Into<String>,
        tool: impl Into<String>,
        args: Value,
    ) -> Result<ToolResult, CoordinatorError> {
        self.execute_nested_tool_with_id(parent, tool, args, None, None)
            .await
    }
    pub async fn execute_nested_tool_with_id(
        &self,
        parent: impl Into<String>,
        tool: impl Into<String>,
        args: Value,
        id: Option<String>,
        cancellation: Option<CancellationToken>,
    ) -> Result<ToolResult, CoordinatorError> {
        let (parent, tool) = (parent.into(), tool.into());
        let (tx, rx) = oneshot::channel();
        let child_id = id.clone();
        self.call(move |s| {
            s.check_task(&parent)?;
            let job = s
                .running
                .get(&parent)
                .ok_or_else(|| CoordinatorError::UnknownTask(parent.clone()))?;
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
                    "nested calls require a running orchestration tool".into(),
                ));
            }
            if tool == "eval" {
                return Err(CoordinatorError::Invalid("eval cannot be nested".into()));
            }
            s.request_tool(job.actor.clone(), Some(parent), id, tool, args, Some(tx))
        })
        .await?;
        match (cancellation, child_id) {
            (Some(cancel), Some(id)) => {
                tokio::select! {
                    result = rx => result.map_err(|_| CoordinatorError::Closed)?,
                    () = cancel.cancelled() => {
                        self.cancel_task(id.clone(), "eval bridge call cancelled").await?;
                        Err(CoordinatorError::Cancelled(id))
                    }
                }
            }
            _ => rx.await.map_err(|_| CoordinatorError::Closed)?,
        }
    }
}
impl Runtime {
    pub fn tool_lineage(&self, parent: Option<&str>) -> Option<TaskLineageMetadata> {
        let parent = parent?;
        let job = self.running.get(parent)?;
        if !matches!(job.kind, JobKind::Tool { .. }) {
            return None;
        }
        let child = job.actor.agent_id.as_ref().and_then(|id| {
            self.native_lineage(id)
                .map(|(_, _, _, request)| (id, request))
                .or_else(|| {
                    self.children
                        .get(id)
                        .map(|child| (id, child.request.clone()))
                })
        });
        let model = child
            .as_ref()
            .and_then(|(id, _)| self.agents.get(*id))
            .map(|agent| crate::agent::AgentModelRef::parse(&agent.info.model_ref));
        Some(TaskLineageMetadata {
            parent_tool_call_id: Some(parent.into()),
            parent_task_id: job.parent.clone(),
            parent_request_id: job.parent.clone(),
            parent_session_id: Some(child.as_ref().map_or_else(
                || self.info.as_ref().map(|info| info.run_id.to_string()),
                |(id, _)| Some((*id).clone()),
            )?),
            child_session_id: child.as_ref().map(|(id, _)| (*id).clone()),
            child_request_id: child.map(|(_, request)| request),
            child_provider_id: model.as_ref().map(|m| m.provider_id.clone()),
            child_model_id: model.map(|m| m.model_id),
        })
    }
}
