use super::*;
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::mem;

impl Worker {
    pub(super) async fn run(mut self, mut messages: super::super::context::Context) -> Completion {
        let (id, actor) = (self.turn.id.clone(), self.actor.clone());
        let started = self
            .handle
            .call(move |s| {
                s.check_task(&id)?;
                s.emit_hooked(
                    actor.clone(),
                    Some(id.clone()),
                    EventV1::TaskScheduled(TaskScheduledEvent {
                        task_id: id.into(),
                        state: TaskScheduleState::Started,
                        queue_key: actor.agent_id,
                        metadata: None,
                    }),
                )
                .map(|_| ())
            })
            .await;
        if let Err(error) = started {
            if let Some(manual) = self.turn.manual.take() {
                let _ = manual
                    .reply
                    .send(Err(CoordinatorError::Invalid(error.to_string())));
            }
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result: Err(error),
            };
        }
        self.update_prompt(&mut messages);
        if let Some(manual) = self.turn.manual.take() {
            let outcome = self
                .compact(
                    &mut messages,
                    manual.through.as_deref(),
                    &manual.reason,
                    manual.instructions.as_deref(),
                    None,
                )
                .await;
            let result = outcome
                .as_ref()
                .map(|_| "Context compaction finished.".into())
                .map_err(|e| CoordinatorError::Invalid(e.to_string()));
            let _ = manual.reply.send(outcome);
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result,
            };
        }
        let mut dispatched = false;
        let result = async {
            self.preload_skills().await?;
            let context = self.prompt_context(&self.turn.prompt.tags).await?;
            let mut text = self.turn.prompt.text.clone();
            if !context.is_empty() {
                text.push_str(&format!("\n\nSelected context:\n{context}"));
            }
            if let Some(completion) = self.turn.prompt.child_completion.clone() {
                let parent = self.actor.agent_id.clone().unwrap_or_default();
                let request = self.turn.id.clone();
                let deliver = self
                    .handle
                    .call(move |runtime| {
                        runtime.claim_native_completion_wake(&parent, &completion, &request)
                    })
                    .await?;
                if !deliver {
                    return Ok(String::new());
                }
            }
            messages.push(
                CompletionMessage::text(MessageRole::User, text),
                self.turn.seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.attachments = mem::take(&mut self.turn.prompt.attachments);
            }
            self.converse(&mut messages, &mut dispatched).await
        }
        .await;
        if result.is_err() && !dispatched {
            messages.discard_turn(&self.turn.id);
        }
        Completion::Turn {
            id: self.turn.id,
            messages,
            result,
        }
    }
    async fn converse(
        &mut self,
        messages: &mut super::super::context::Context,
        dispatched: &mut bool,
    ) -> Result<String, CoordinatorError> {
        let mut iteration = 0;
        loop {
            for (seq, text) in self
                .handle
                .drain_subagent_messages(self.actor.clone(), self.turn.id.clone())
                .await?
            {
                messages.push(
                    CompletionMessage::text(MessageRole::User, text),
                    seq,
                    Some(&self.turn.id),
                );
            }
            if self.cancellation.is_cancelled() {
                return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
            }
            if self
                .profile
                .max_iters
                .is_some_and(|limit| iteration >= limit)
            {
                return Err(CoordinatorError::Invalid(
                    "agent iteration limit reached".into(),
                ));
            }
            iteration += 1;
            let (request_id, response) = loop {
                match self.dispatch(messages, dispatched).await {
                    Err(error @ CoordinatorError::Provider { .. }) => {
                        let next = self.fallbacks.pop_front().ok_or(error)?;
                        self.turn.model = next.model_ref.clone();
                        self.turn.settings = (&next).into();
                        self.turn.target = Some(next);
                        self.update_prompt(messages);
                    }
                    result => break result?,
                }
            };
            if self.native {
                if let Some(usage) = &response.usage {
                    messages.native_context_usage = Some(crate::subagent::SubagentContextUsage {
                        total_tokens: u64::from(usage.total_tokens),
                        estimate_at_last_response: super::super::context::native_tokens(
                            &messages.entries,
                        ),
                    });
                }
            }
            let calls = response.calls;
            if let Some(request) = response.logical_request {
                messages.model_request = Some(Box::new(request));
            }
            if response.settled_reasoning.is_none() {
                messages.unavailable =
                    Some(crate::subagent::FinalizedStateUnavailable::UnsupportedReasoning);
            }
            messages
                .usage
                .push(crate::subagent::FinalizedProviderUsage {
                    request_id: request_id.clone(),
                    attempt_id: self.turn.id.clone(),
                    model_ref: self.turn.model.clone(),
                    usage: response.usage,
                    thinking: response.thinking,
                    usage_complete: response.usage_complete,
                    settled_reasoning: response.settled_reasoning.clone(),
                });
            messages.push(
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: response.text.clone(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: (!calls.is_empty()).then(|| calls.clone()),
                },
                response.event_seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.settled_reasoning = response.settled_reasoning.unwrap_or_default();
            }
            if calls.is_empty() {
                return Ok(response.text);
            }
            self.run_tools(&request_id, response.event_seq, calls, messages)
                .await?;
            self.tools = self
                .registry
                .definitions(&self.profile, (&self.permissions.0, &self.permissions.1));
            super::super::public_subagent_hooks::apply_native_schema_hints(
                &mut self.tools,
                &self.native_schema,
                self.native,
            );
        }
    }
    async fn run_tools(
        &self,
        request: &str,
        message_seq: u64,
        calls: Vec<AssistantToolCall>,
        messages: &mut super::super::context::Context,
    ) -> Result<(), CoordinatorError> {
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
        for (index, call) in calls.into_iter().enumerate() {
            let output = outputs
                .remove(&index)
                .ok_or_else(|| CoordinatorError::Invalid("tool output missing".into()))?;
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
            messages.push(
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: text,
                    name: Some(call.function_name),
                    tool_call_id: Some(call.tool_call_id),
                    assistant_tool_calls: None,
                },
                message_seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.attachments = attachments;
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
