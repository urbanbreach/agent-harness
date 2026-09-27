use super::{context::Context, runtime::*, *};

impl Runtime {
    pub fn finished(&mut self, completion: Completion) -> Result<(), CoordinatorError> {
        let (id, messages, mut result) = match completion {
            Completion::Turn {
                id,
                messages,
                result,
            } => (id, Some(messages), result.map(ToolResult::text)),
            Completion::Tool { id, result } => (id, None, result),
        };
        let Some(mut job) = self.running.remove(&id) else {
            return Ok(());
        };
        if job.cancellation.is_cancelled() {
            result = Err(CoordinatorError::Cancelled(
                job.reason.take().unwrap_or_else(|| "task cancelled".into()),
            ));
        }
        if messages.is_none() {
            self.reject_unfinished_edits(&id)?;
            result = result.and_then(|output| self.bound_tool_output(&id, &job.actor, output));
        }
        self.snapshots.remove(&id);
        self.finish_hooks(&id, &mut job, messages.is_some(), &mut result);
        let success = result.as_ref().is_ok_and(|value| !value.is_error());
        let summary = match &result {
            Ok(value) => value.display_text.clone(),
            Err(error) => error.to_string(),
        };
        let scope = if messages.is_some() {
            TaskTerminalScope::AgentTurn
        } else {
            TaskTerminalScope::ToolCall
        };
        let written = if scope == TaskTerminalScope::ToolCall {
            self.record_tool_result(
                &id,
                &job.actor,
                job.parent.as_deref(),
                &result,
                std::mem::take(&mut job.hooks),
            )
        } else {
            Ok(())
        };
        let (agent, reply) = match job.kind {
            JobKind::Turn { agent } => (Some(agent), None),
            JobKind::Tool { reply, .. } => (None, reply),
        };
        if let Some(reply) = reply {
            let response = match &written {
                Ok(()) => result,
                Err(error) => Err(CoordinatorError::Invalid(error.to_string())),
            };
            let _ = reply.send(response);
        }
        written?;
        let payload = if success {
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: id.clone().into(),
                result_digest: digest(&summary),
                result_summary: summary,
                metadata: Some(TaskCompletionMetadata {
                    task_scope: Some(scope),
                    hook_executions: job.hooks,
                    lineage: self.tool_lineage(job.parent.as_deref()),
                    ..Default::default()
                }),
            })
        } else {
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: id.clone().into(),
                reason: summary,
                failure: !job.cancellation.is_cancelled(),
                task_scope: Some(scope),
            })
        };
        let terminal = self.emit(job.actor, Some(id), payload)?;
        if let Some(agent) = agent {
            self.finish_child(&agent, &terminal)?;
            if let Some(state) = self.agents.get_mut(&agent) {
                state.messages = if self.children.contains_key(&agent) {
                    Context::default()
                } else {
                    messages.unwrap_or_default()
                };
                state.busy = false;
            }
            self.start_next(&agent)?;
        }
        Ok(())
    }
    fn finish_hooks(
        &self,
        id: &str,
        job: &mut Job,
        is_turn: bool,
        result: &mut Result<ToolResult, CoordinatorError>,
    ) {
        if self.config.hook_runtime_config.hooks.lifecycle.is_empty() {
            return;
        }
        let tool_id = match &job.kind {
            JobKind::Tool { tool_id, .. } => Some(tool_id.as_str()),
            _ => None,
        };
        let fields = serde_json::json!({
            "tool_id":tool_id,"tool_call_id":tool_id.map(|_| id),
            "request_id":job.parent.as_deref().unwrap_or(id),
            "outcome":if result.as_ref().is_ok_and(|v| !v.is_error()) {"succeeded"} else {"failed"},
            "output_summary":result.as_ref().ok().map(|v| &v.display_text),
            "failure_reason":result.as_ref().err().map(ToString::to_string),
        });
        let mut stages = vec![if is_turn {
            crate::config::HookLifecycleEvent::AgentTurnFinished
        } else {
            crate::config::HookLifecycleEvent::ToolCallFinished
        }];
        if is_turn
            && job
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| self.agents.get(id))
                .is_some_and(|a| a.info.parent_agent_id.is_some())
        {
            stages.push(crate::config::HookLifecycleEvent::SubagentFinished);
        }
        for stage in stages {
            let batch = self.hooks(stage, &job.actor, Some(id), fields.clone());
            let remaining = 128usize.saturating_sub(job.hooks.len());
            job.hooks
                .extend(batch.executions.into_iter().take(remaining));
            if let Some(error) = batch.failure {
                *result = Err(CoordinatorError::Invalid(error));
            }
        }
    }
}
