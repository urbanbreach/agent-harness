use super::{context::Context, turn::Worker, *};
use crate::config::HookLifecycleEvent as Hook;
use harness_providers::{CompletionMessage, MessageRole};
use num_traits::ToPrimitive;
use serde_json::json;
pub(super) mod plan;
mod summary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualCompactionOutcome {
    NoOp,
    Compacted {
        tokens_before: u32,
        tokens_after: u32,
        summary_preview: String,
    },
}
pub(super) struct Manual {
    pub through: Option<String>,
    pub reason: String,
    pub instructions: Option<String>,
    pub reply: Reply<ManualCompactionOutcome>,
}
impl CoordinatorHandle {
    pub async fn compact_agent_context_with_instructions(
        &self,
        agent: impl Into<String>,
        through_request_id: Option<String>,
        trigger_reason: impl Into<String>,
        custom_instructions: Option<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        let (agent, reason) = (agent.into(), trigger_reason.into());
        if reason.is_empty()
            || reason.len() > 256
            || custom_instructions
                .as_ref()
                .is_some_and(|s| s.len() > 32 * 1024)
        {
            return Err(CoordinatorError::Invalid(
                "invalid compaction instructions or reason".into(),
            ));
        }
        let (reply, response) = oneshot::channel();
        self.call(move |s| {
            s.queue_compaction(
                &agent,
                Manual {
                    through: through_request_id,
                    reason,
                    instructions: custom_instructions,
                    reply,
                },
            )
        })
        .await?;
        response.await.map_err(|_| CoordinatorError::Closed)?
    }
    /// Compact context with a model handoff plus coordinator-retained user requests and todos.
    /// Request retention keeps the first and eleven most recent messages, bounded to 2000
    /// characters each and 12 KiB of request text total. Worker wake messages and cancelled,
    /// never-started turns are excluded. Session todos are included only for root agents.
    /// The combined summary is redacted and persisted.
    pub async fn compact_agent_context(
        &self,
        agent: impl Into<String>,
        through: Option<String>,
        reason: impl Into<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        self.compact_agent_context_with_instructions(agent, through, reason, None)
            .await
    }
    pub async fn cancel_compaction(
        &self,
        agent: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| {
            let state = s
                .agents
                .get(&agent)
                .ok_or_else(|| CoordinatorError::UnknownAgent(agent.clone()))?;
            let queued: Vec<_> = state
                .queue
                .iter()
                .filter(|t| t.manual.is_some())
                .map(|t| t.id.clone())
                .collect();
            for task in queued {
                s.cancel(&task, "compaction cancelled")?;
            }
            if let Some((_, cancellation)) = s.compacting.get(&agent) {
                cancellation.cancel();
            }
            Ok(())
        })
        .await
    }
}
impl Worker {
    pub(super) async fn compact(
        &self,
        context: &mut Context,
        through: Option<&str>,
        reason: &str,
        instructions: Option<&str>,
        budget: Option<&crate::RequestBudget>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        if !self.compaction.enabled {
            return Ok(ManualCompactionOutcome::NoOp);
        }
        let keep = budget
            .and_then(|budget| {
                self.compaction_threshold(budget).map(|threshold| {
                    threshold.saturating_sub(
                        budget
                            .occupied_input_tokens
                            .saturating_sub(budget.components.history_tokens),
                    )
                })
            })
            .map_or(self.compaction.keep_recent_tokens, |allowance| {
                self.compaction.keep_recent_tokens.min(allowance)
            });
        let Some(plan) = plan::Plan::new(context, &self.compaction, keep, through)? else {
            return Ok(ManualCompactionOutcome::NoOp);
        };
        let agent = self
            .actor
            .agent_id
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("compaction needs an agent".into()))?;
        let cancel = self.cancellation.child_token();
        let (task, owner, id, token, trigger) = (
            self.turn.id.clone(),
            self.actor.clone(),
            agent.clone(),
            cancel.clone(),
            reason.to_owned(),
        );
        let generation = self
            .handle
            .call(move |s| {
                s.check_task(&task)?;
                if let Err(error) = s.hook(
                    Hook::CompactionRequested,
                    &owner,
                    Some(&task),
                    json!({"outcome":"requested","output_summary":trigger}),
                ) {
                    let _ = s.hook(
                        Hook::CompactionFailed,
                        &owner,
                        Some(&task),
                        json!({"outcome":"failed","failure_reason":error.to_string()}),
                    );
                    return Err(error);
                }
                let generation = s.counter;
                s.compacting.insert(id.clone(), (generation, token));
                s.live(
                    owner,
                    task,
                    LiveEventV1::CompactionProgress {
                        agent_id: id,
                        generation,
                        trigger_reason: trigger,
                        preview: Some("Summarizing earlier context…".into()),
                    },
                )?;
                Ok(generation)
            })
            .await?;
        let result = self.summarize(context, &plan, instructions, &cancel).await;
        let result = match result {
            Ok((summary, usage)) => {
                self.apply_compaction(context, plan, &agent, summary, usage, reason, &cancel)
                    .await
            }
            Err(error) => Err(error),
        };
        let (task, owner, id, trigger) = (
            self.turn.id.clone(),
            self.actor.clone(),
            agent.clone(),
            reason.to_owned(),
        );
        let failure = result.as_ref().err().map(ToString::to_string);
        let cleared = self
            .handle
            .call(move |s| {
                if s.compacting
                    .get(&id)
                    .is_some_and(|(current, _)| *current == generation)
                {
                    s.compacting.remove(&id);
                    s.live(
                        owner.clone(),
                        task.clone(),
                        LiveEventV1::CompactionProgress {
                            agent_id: id,
                            generation,
                            trigger_reason: trigger,
                            preview: None,
                        },
                    )?;
                }
                if let Some(error) = failure {
                    s.hook(
                        Hook::CompactionFailed,
                        &owner,
                        Some(&task),
                        json!({"outcome":"failed","failure_reason":error}),
                    )?;
                }
                Ok(())
            })
            .await;
        if cancel.is_cancelled() && result.is_err() {
            return Err(CoordinatorError::CompactionCancelled { agent_id: agent });
        }
        cleared?;
        result
    }
    async fn apply_compaction(
        &self,
        context: &mut Context,
        plan: plan::Plan,
        agent: &str,
        summary: String,
        usage: Option<harness_providers::CompletionUsage>,
        reason: &str,
        cancel: &CancellationToken,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        let tokens_before = context.tokens();
        let retained = &context.entries[plan.cut];
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        let payload = SessionCompactionEvent {
            agent_id: agent.into(),
            summary,
            first_kept_event_seq: retained.seq,
            first_kept_request_id: retained.turn.clone(),
            first_kept_entry_id: None,
            tokens_before,
            tokens_after: None,
            summary_usage: usage,
            summary_provider_id: Some(model.provider_id),
            summary_model_id: Some(model.model_id),
            read_files: plan.read_files,
            modified_files: plan.modified_files,
            task_intent: None,
            current_intent: None,
            trigger_reason: reason.into(),
            from_hook: false,
        };
        let kept_tokens = super::context::tokens(&context.entries[..plan.start])
            .saturating_add(super::context::tokens(&context.entries[plan.cut..]));
        let (task, owner, token) = (self.turn.id.clone(), self.actor.clone(), cancel.clone());
        let applied = self
            .handle
            .call(move |s| {
                s.check_task(&task)?;
                if token.is_cancelled() {
                    return Err(CoordinatorError::Cancelled(task));
                }
                let mut summary = payload.summary;
                s.append_compaction_state(&payload.agent_id, &mut summary)?;
                let summary = s.redactor.redact_text(&summary);
                let message = CompletionMessage::text(
                    MessageRole::User,
                    format!("Conversation summary:\n{summary}"),
                );
                let mut replacement = Context::default();
                replacement.push(message.clone(), 0, None);
                let tokens_after = kept_tokens.saturating_add(replacement.tokens());
                if tokens_after >= tokens_before {
                    return Err(CoordinatorError::Invalid(
                        "summary did not reduce context; original context retained".into(),
                    ));
                }
                // Critical completion hooks gate the same commit as the summary, so replay and memory cannot diverge.
                for stage in [Hook::CompactionWritten, Hook::CompactionApplied] {
                    s.hook(
                        stage,
                        &owner,
                        Some(&task),
                        json!({"outcome":"compacted","output_summary":summary}),
                    )?;
                }
                s.emit(
                    owner,
                    Some(task),
                    EventV1::SessionCompaction(SessionCompactionEvent {
                        summary,
                        tokens_after: Some(tokens_after),
                        ..payload
                    }),
                )?;
                Ok((message, tokens_after))
            })
            .await?;
        self.provider
            .session_event(&harness_providers::ProviderSessionEvent::Compacted {
                session_id: self.session.clone(),
            });
        let preview = applied.0.content.chars().take(512).collect();
        context.entries.splice(
            plan.start..plan.cut,
            [super::context::Entry {
                message: applied.0,
                seq: 0,
                turn: None,
                attachments: Vec::new(),
                settled_reasoning: Vec::new(),
                raw_tool_result: None,
            }],
        );
        if let Some(previous) = context.native_context_usage {
            context.native_context_usage =
                super::context::native_tokens(&context.entries).map(|estimate| {
                    let scaled = previous
                        .estimate_at_last_response
                        .filter(|old| *old > 0)
                        .filter(|_| previous.total_tokens > 0)
                        .and_then(|old| {
                            let scaled = (estimate.to_f64()?
                                * (previous.total_tokens.to_f64()? / old.to_f64()?))
                            .round();
                            Some(scaled.to_u64().unwrap_or(u64::MAX))
                        })
                        .unwrap_or(estimate);
                    crate::subagent::SubagentContextUsage {
                        total_tokens: if previous.total_tokens > 0 {
                            scaled.min(previous.total_tokens)
                        } else {
                            scaled
                        },
                        estimate_at_last_response: Some(estimate),
                        estimate_after_last_response: Some(estimate),
                    }
                });
        }
        Ok(ManualCompactionOutcome::Compacted {
            tokens_before,
            tokens_after: applied.1,
            summary_preview: preview,
        })
    }
}
impl Runtime {
    /// Keep the first request and up to eleven recent requests, with at most 2000
    /// characters each and 12 KiB of request text total, plus root agents' current todo statuses.
    /// The combined summary is redacted before either persistence or live context use.
    fn append_compaction_state(
        &self,
        agent: &str,
        summary: &mut String,
    ) -> Result<(), CoordinatorError> {
        const MAX_REQUESTS: usize = 12;
        const MAX_REQUEST_CHARS: usize = 2000;
        const MAX_REQUEST_BYTES: usize = 12 * 1024;
        let events = crate::store::read_events(&self.info()?.events_path)?;
        let events = crate::conversation_rewind::active_events(&events);
        let primary = events.iter().find_map(|event| match &event.payload {
            EventV1::AgentSpawned(spawn) if spawn.parent_agent_id.is_none() => {
                Some(spawn.agent_id.as_str())
            }
            _ => None,
        }) == Some(agent);
        let belongs_to_agent = |event: &EventEnvelopeV1| {
            event
                .actor
                .agent_id
                .as_deref()
                .map_or(primary, |id| id == agent)
        };
        let mut user_turns = std::collections::HashMap::new();
        let mut started = std::collections::HashSet::new();
        let mut cancelled = std::collections::HashSet::new();
        let mut latest_turn = None;
        for event in events.iter().filter(|event| belongs_to_agent(event)) {
            match &event.payload {
                EventV1::UserMessageSubmitted(request) => {
                    let turn = event
                        .correlation_id
                        .as_deref()
                        .and_then(|id| user_turns.get(id))
                        .copied()
                        .unwrap_or(request.request_id.as_str());
                    user_turns.insert(request.request_id.as_str(), turn);
                    latest_turn = Some(turn);
                }
                EventV1::ProviderRequestStarted(request) => {
                    if let Some(turn) = request
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.turn_id.as_deref())
                        .or(event.correlation_id.as_deref())
                        .and_then(|id| user_turns.get(id))
                        .copied()
                        .or(latest_turn)
                    {
                        started.insert(turn);
                    }
                }
                EventV1::TaskCancelled(task)
                    if task.task_scope != Some(TaskTerminalScope::ToolCall) =>
                {
                    cancelled.insert(task.task_id.as_str());
                }
                _ => {}
            }
        }
        let mut requests = events.iter().filter_map(|event| {
            if !belongs_to_agent(event) || event.actor.kind != ActorKind::User {
                return None;
            }
            match &event.payload {
                EventV1::UserMessageSubmitted(request) => {
                    let turn = user_turns[request.request_id.as_str()];
                    (!cancelled.contains(turn) || started.contains(turn))
                        .then_some(request.text.as_str())
                }
                _ => None,
            }
        });
        summary.push_str("\n\n## User Requests (verbatim)");
        if let Some(first) = requests.next() {
            let mut recent: Vec<_> = requests.rev().take(MAX_REQUESTS - 1).collect();
            recent.reverse();
            let count = recent.len() + 1;
            let mut bytes_left = MAX_REQUEST_BYTES;
            for (index, request) in std::iter::once(first).chain(recent).enumerate() {
                let char_end = request
                    .char_indices()
                    .nth(MAX_REQUEST_CHARS)
                    .map_or(request.len(), |(offset, _)| offset);
                let end = request.floor_char_boundary(char_end.min(bytes_left / (count - index)));
                summary.push_str("\n\n");
                summary.push_str(&request[..end]);
                if end < request.len() {
                    summary.push_str("\n[truncated]");
                }
                bytes_left -= end;
            }
        }
        let todos = self.root_todo_items(agent);
        if !todos.is_empty() {
            summary.push_str("\n\n## Current Todo List\n");
            for item in todos {
                summary.push_str("- [");
                summary.push_str(&item.status);
                summary.push_str("] ");
                summary.push_str(&item.content);
                summary.push('\n');
            }
        }
        Ok(())
    }
}
