use super::*;
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};

impl Worker {
    pub(super) async fn run_tools(
        &self,
        request: &str,
        message_seq: u64,
        calls: Vec<AssistantToolCall>,
        messages: &mut super::super::context::Context,
    ) -> Result<(), CoordinatorError> {
        let owner = self.actor.agent_id.clone().unwrap_or_default();
        let model = self.turn.model.clone();
        let settings = self.turn.settings.clone();
        self.handle
            .call(move |runtime| {
                let agent = runtime
                    .agents
                    .get_mut(&owner)
                    .ok_or(CoordinatorError::UnknownAgent(owner))?;
                agent.info.model_ref = model;
                agent.settings = settings;
                Ok(())
            })
            .await?;
        self.handle
            .capture_active_context(self.actor.clone(), self.turn.id.clone(), messages.clone())
            .await?;
        let mut workers = tokio::task::JoinSet::new();
        for (index, call) in calls.iter().enumerate() {
            let (handle, actor, parent) = (
                self.handle.clone(),
                self.actor.clone(),
                self.turn.id.clone(),
            );
            let id = format!("{request}-tool-{index}");
            let call = call.clone();
            workers.spawn(async move {
                let result = match serde_json::from_str(&call.arguments_json) {
                    Ok(args) => {
                        handle
                            .execute_tool(actor, Some(parent), Some(id), call.function_name, args)
                            .await
                    }
                    Err(error) => Err(CoordinatorError::Json(error)),
                };
                (index, result)
            });
        }
        let mut outputs = BTreeMap::new();
        while let Some(result) = workers.join_next().await {
            let (index, output) = result.map_err(|_| {
                CoordinatorError::Invalid("tool dispatch stopped unexpectedly".into())
            })?;
            outputs.insert(index, output);
        }
        let mut failed = false;
        let at_ms = self.event_ms().await?;
        for (index, call) in calls.into_iter().enumerate() {
            let output = outputs
                .remove(&index)
                .ok_or_else(|| CoordinatorError::Invalid("tool output missing".into()))?;
            let facts = match &output {
                Ok(output) => {
                    harness_compaction::prune::tool_facts(harness_compaction::prune::ResultFacts {
                        tool: &call.function_name,
                        is_error: output.is_error(),
                        useless: output.useless,
                        source_artifact: output.source_artifact,
                        structured: output.structured_json.as_ref(),
                    })
                }
                Err(_) => harness_compaction::ToolFacts {
                    is_error: true,
                    ..Default::default()
                },
            };
            let (text, attachments, dispatch_error) = match output {
                Ok(output) => {
                    failed |= output.is_error();
                    (
                        crate::tool::provider_text(
                            &call.function_name,
                            output.display_text,
                            output.structured_json,
                        ),
                        output.attachments,
                        None,
                    )
                }
                Err(error) => {
                    failed = true;
                    (
                        format!("Tool error: {error}"),
                        Vec::new(),
                        Some(error.to_string()),
                    )
                }
            };
            let raw_id = format!("{request}-tool-{index}");
            let owner = self.actor.agent_id.clone();
            let raw = self
                .handle
                .call(move |s| match s.raw_tool_results.remove(&raw_id) {
                    Some((agent, output)) if owner.as_deref() == Some(&agent) => {
                        let (output, error) = match output {
                            Ok(output) => (Some(output), None),
                            Err(error) => (None, Some(error)),
                        };
                        Ok(Some(crate::subagent::FinalizedToolResult {
                            tool_call_id: raw_id,
                            provider_tool_call_id: None,
                            output,
                            error,
                        }))
                    }
                    Some(_) => Err(CoordinatorError::PermissionDenied(
                        "raw tool result owner mismatch".into(),
                    )),
                    None => Ok(
                        dispatch_error.map(|error| crate::subagent::FinalizedToolResult {
                            tool_call_id: raw_id,
                            provider_tool_call_id: None,
                            output: None,
                            error: Some(error),
                        }),
                    ),
                })
                .await?;
            if raw.is_none() {
                messages.unavailable = Some(crate::subagent::FinalizedStateUnavailable::Incomplete);
            }
            let provider_call_id = call.tool_call_id.clone();
            messages.push_timed(
                CompletionMessage {
                    continuation: None,
                    role: MessageRole::Tool,
                    content: text,
                    name: Some(call.function_name),
                    tool_call_id: Some(call.tool_call_id),
                    assistant_tool_calls: None,
                },
                message_seq,
                Some(&self.turn.id),
                at_ms,
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.tool = facts;
                entry.attachments = attachments;
                super::super::super::history::derive_images(std::slice::from_mut(entry));
                entry.raw_tool_result = raw.map(|mut raw| {
                    raw.provider_tool_call_id = Some(provider_call_id);
                    raw
                });
            }
        }
        if self.cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        if failed && self.profile.tool_failure_mode == crate::config::ToolFailureMode::FailTurn {
            return Err(CoordinatorError::Invalid("tool execution failed".into()));
        }
        Ok(())
    }
}
