use super::{
    runtime::digest,
    tools::{Pending, PendingWork},
    *,
};
use crate::tool::QuestionRequest;
use serde_json::Value;
use tokio::time::{Duration, Instant};

impl CoordinatorHandle {
    pub async fn request_question(
        &self,
        actor: EventActor,
        tool_call_id: impl Into<String>,
        request: Value,
    ) -> Result<ToolResult, CoordinatorError> {
        self.execute_tool(
            actor,
            None,
            Some(tool_call_id.into()),
            "question".into(),
            request,
        )
        .await
    }
    pub(crate) async fn wait_for_question(
        &self,
        task: String,
        request: QuestionRequest,
    ) -> Result<Value, CoordinatorError> {
        let (tx, rx) = oneshot::channel();
        self.call(move |s| {
            s.check_task(&task)?;
            let actor = s
                .running
                .get(&task)
                .ok_or_else(|| CoordinatorError::UnknownTask(task.clone()))?
                .actor
                .clone();
            let permission = s.id("question")?;
            let timeout_ms = s.config.permission_policy.ask_timeout_ms();
            let summary = serde_json::to_string(&request)?;
            s.emit_hooked(
                actor,
                Some(task.clone()),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: permission.clone(),
                    kind: "question".into(),
                    tool_call_id: Some(task.clone().into()),
                    request_digest: digest(&summary),
                    summary,
                    timeout_ms,
                    default_decision: PermissionDecision::Deny,
                }),
            )?;
            s.pending.insert(
                permission,
                Pending {
                    id: task,
                    deadline: (timeout_ms > 0)
                        .then(|| Instant::now() + Duration::from_millis(timeout_ms)),
                    work: PendingWork::Question { request, reply: tx },
                },
            );
            Ok(())
        })
        .await?;
        rx.await.map_err(|_| CoordinatorError::Closed)?
    }
}
