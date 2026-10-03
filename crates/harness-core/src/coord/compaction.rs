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
                let summary = s.redactor.redact_text(&payload.summary);
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
