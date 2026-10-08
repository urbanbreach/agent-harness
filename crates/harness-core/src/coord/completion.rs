use super::{context::Context, runtime::*, *};

impl Runtime {
    pub fn finished(&mut self, completion: Completion) -> Result<(), CoordinatorError> {
        let id = match &completion {
            Completion::Turn { id, .. }
            | Completion::Tool { id, .. }
            | Completion::Command { id, .. }
            | Completion::SubagentPrepared { id, .. } => id,
        };
        let agent = self.running.get(id).and_then(|job| match &job.kind {
            JobKind::Turn { agent } => Some(agent.clone()),
            JobKind::Tool { .. } | JobKind::Command | JobKind::SubagentPreparation { .. } => None,
        });
        let result = self.finish_completion(completion);
        if let Err(error) = &result {
            if let Some(agent) = agent {
                if let Some(state) = self.agents.get_mut(&agent) {
                    state.busy = false;
                }
                self.raw_tool_results
                    .retain(|_, (owner, _)| owner != &agent);
            }
            if self.fault.is_none() {
                self.storage_failed(&EventStoreError::Io(std::io::Error::other(
                    error.to_string(),
                )));
            }
        }
        result
    }
    fn finish_completion(&mut self, completion: Completion) -> Result<(), CoordinatorError> {
        let (id, messages, mut result, instructions) = match completion {
            Completion::Turn {
                id,
                messages,
                result,
            } => (id, Some(messages), result.map(ToolResult::text), Vec::new()),
            Completion::Tool {
                id,
                result,
                instructions,
            } => (id, None, result, instructions),
            Completion::Command { id, result } => return self.finish_command(id, result),
            Completion::SubagentPrepared { id, result } => {
                return self.finish_native_subagent_preparation(id, result)
            }
        };
        let Some(mut job) = self.running.remove(&id) else {
            return Ok(());
        };
        let detached_eval = self.detached_evals.remove(&id);
        self.finish_eval_pool_submission(&id)?;
        if job.cancellation.is_cancelled() {
            result = Err(CoordinatorError::Cancelled(
                job.reason.take().unwrap_or_else(|| "task cancelled".into()),
            ));
        }
        // Keep completion identities even when a large report spills to an artifact.
        let consumed_notifications = self.consumed_child_notifications(&job, &result);
        let mut raw_tool_result = if !detached_eval
            && messages.is_none()
            && job.parent.as_ref().is_some_and(|parent| {
                self.running
                    .get(parent)
                    .is_some_and(|job| matches!(job.kind, JobKind::Turn { .. }))
            }) {
            match &result {
                Ok(output)
                    if serde_json::to_vec(output)
                        .is_ok_and(|bytes| bytes.len() <= 8 * 1024 * 1024) =>
                {
                    Some(Ok(output.clone()))
                }
                Err(error) => Some(Err(error.to_string())),
                _ => None,
            }
        } else {
            None
        };
        if messages.is_none() {
            self.reject_unfinished_edits(&id)?;
            let command_handle = result
                .as_ref()
                .ok()
                .and_then(|output| output.structured_json.as_ref())
                .is_some_and(|value| {
                    value["task_id"].is_string()
                        && matches!(value["type"].as_str(), Some("Background" | "Foreground"))
                });
            let retain_structured = match &job.kind {
                JobKind::Tool { tool_id, .. } => {
                    matches!(
                        tool_id.as_str(),
                        "spawn_subagent"
                            | "get_command_or_subagent_output"
                            | "wait_commands_or_subagents"
                            | "kill_command_or_subagent"
                            | "send_subagent_message"
                            | "task"
                            | "get_task_output"
                            | "wait_tasks"
                            | "kill_task"
                    ) || tool_id == "eval"
                        || tool_id == "bash" && command_handle
                }
                _ => false,
            };
            result = result.and_then(|output| {
                self.bound_tool_output(&id, &job.actor, output, retain_structured)
            });
        }
        self.snapshots.remove(&id);
        self.finish_hooks(&id, &mut job, messages.is_some(), &mut result);
        self.attach_native_tool_reminders(&id, &job, &mut result, &mut raw_tool_result)?;
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
        let written = if detached_eval {
            self.finish_detached_eval(&id, &job.actor, &result)
        } else if scope == TaskTerminalScope::ToolCall {
            self.record_tool_result(
                &id,
                &job.actor,
                job.parent.as_deref(),
                &result,
                std::mem::take(&mut job.hooks),
            )
            .and_then(|()| {
                consumed_notifications
                    .into_iter()
                    .filter(|_| result.is_ok())
                    .try_for_each(|id| self.cancel(&id, "child result already delivered by a tool"))
            })
        } else {
            Ok(())
        };
        if success && written.is_ok() {
            self.queue_directory_instructions(&job, instructions);
        }
        let (agent, reply) = match job.kind {
            JobKind::Turn { agent } => (Some(agent), None),
            JobKind::Tool { reply, .. } => (None, reply),
            JobKind::Command | JobKind::SubagentPreparation { .. } => (None, None),
        };
        if let Some(reply) = reply {
            let response = match &written {
                Ok(()) => result,
                Err(error) => Err(CoordinatorError::Invalid(error.to_string())),
            };
            let _ = reply.send(response);
        }
        written?;
        if let (Some(output), Some(owner)) = (raw_tool_result, job.actor.agent_id.as_ref()) {
            self.raw_tool_results
                .insert(id.clone(), (owner.clone(), output));
        }
        let retain_snapshot = (success && !summary.is_empty())
            || (agent
                .as_ref()
                .is_some_and(|agent| self.native_subagents.contains_key(agent))
                && messages.as_ref().is_some_and(|context| {
                    context
                        .entries
                        .iter()
                        .any(|entry| entry.message.role != harness_providers::MessageRole::System)
                }));
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
        if let (Some(agent), Some(messages)) = (&agent, &messages) {
            self.finish_agent_state(
                agent,
                &id,
                messages,
                success,
                retain_snapshot,
                job.cancellation.is_cancelled(),
            )?;
        }
        let terminal = self.emit(job.actor, Some(id.clone()), payload)?;
        if let Some(agent) = agent {
            self.raw_tool_results
                .retain(|_, (owner, _)| owner != &agent);
            // Late steering is queued before the agent goes idle and before a finished
            // child publishes its result, so the child stays open for it.
            self.requeue_steering(&agent)?;
            if let Some(state) = self.agents.get_mut(&agent) {
                state.messages = if self.children.contains_key(&agent)
                    && !self.native_subagents.contains_key(&agent)
                {
                    Context::default()
                } else {
                    messages.unwrap_or_default()
                };
                state.busy = false;
            }
            self.finish_child(&agent, &terminal)?;
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
