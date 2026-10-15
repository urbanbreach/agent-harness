use super::super::compaction::{
    delivered_content, drop_orphan_tool_use, terminal_text_answer, Continuation,
    ContinuationSource, Site,
};
use super::super::reminders::TurnInput;
use super::guidance::push_input;
use super::*;
use harness_compaction::{NoteKind, Origin, Stop};
use harness_providers::{CompletionMessage, MessageRole};
use std::mem;

mod tools;

/// What post-turn maintenance leaves for the stop handling that follows it.
#[derive(Default)]
struct PostTurn {
    /// History changed (a commit, a rewrite or a promotion).
    changed: bool,
    /// The pass owns the next call: it scheduled a continuation or paused on purpose.
    owns_next_call: bool,
    /// Append the compaction-resume reminder and keep looping.
    resume: bool,
}

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
                manual
                    .reply
                    .fail(CoordinatorError::Invalid(error.to_string()));
            }
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result: Err(error),
            };
        }
        if let Err(error) = self.update_prompt(&mut messages) {
            return Completion::Turn {
                id: self.turn.id,
                messages,
                result: Err(error),
            };
        }
        if let Some(manual) = self.turn.manual.take() {
            let result = self.run_manual(&mut messages, manual).await;
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
            let mut origin = if self.turn.prompt.native_subagent {
                Origin::Agent
            } else {
                Origin::User
            };
            let mut note = None;
            if let Some(completion) = self.turn.prompt.child_completion.clone() {
                let parent = self.actor.agent_id.clone().unwrap_or_default();
                let request = self.turn.id.clone();
                let wake_text = self
                    .handle
                    .call(move |runtime| {
                        runtime.native_completion_wake_text(&parent, &completion, &request, text)
                    })
                    .await?;
                let Some((seq, wake_text)) = wake_text else {
                    return Ok(String::new());
                };
                self.turn.seq = seq;
                text = wake_text;
                origin = Origin::Host;
                note = Some(NoteKind::ChildCompletion);
            }
            let at_ms = self.event_ms().await?;
            if self.turn.prompt.reminder_wake {
                let parent = self.actor.agent_id.clone().unwrap_or_default();
                let request = self.turn.id.clone();
                let reminders = self
                    .handle
                    .call(move |runtime| {
                        runtime.check_task(&request)?;
                        runtime.drain_pending_reminders(&parent, &request)
                    })
                    .await?;
                if reminders.is_empty() {
                    return Ok(String::new());
                }
                for input in reminders {
                    push_input(&mut messages, &self.turn.id, input, at_ms);
                }
            }
            let parent = self.actor.agent_id.clone().unwrap_or_default();
            let request = self.turn.id.clone();
            let completions = self
                .handle
                .call(move |runtime| {
                    runtime.check_task(&request)?;
                    runtime.drain_native_completion_reminders(&parent, &request)
                })
                .await?;
            for (seq, text) in completions {
                let input = TurnInput::child_completion(seq, text);
                push_input(&mut messages, &self.turn.id, input, at_ms);
            }
            if !self.turn.prompt.reminder_wake {
                let input = TurnInput {
                    seq: self.turn.seq,
                    text,
                    origin,
                    note,
                };
                push_input(&mut messages, &self.turn.id, input, at_ms);
                if let Some(entry) = messages.entries.last_mut() {
                    entry.attachments = mem::take(&mut self.turn.prompt.attachments);
                    super::super::history::derive_images(std::slice::from_mut(entry));
                }
            }
            self.converse(&mut messages, &mut dispatched).await
        }
        .await;
        if result.is_err() && !dispatched {
            messages.discard_turn(&self.turn.id);
        }
        // A promotion lasts one turn; failing to restore must not hide the turn's result.
        let _ = self.restore_operator_model().await;
        self.sync_compaction_status(&messages).await;
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
            // A cancelled turn leaves waiting steering to be queued as its own turn.
            if self.cancellation.is_cancelled() {
                return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
            }
            self.take_turn_inputs(messages, *dispatched).await?;
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
            let (request_id, response) = self.provider_response(messages, dispatched).await?;
            let reported_native_usage = self.native && response.usage.is_some();
            if self.native
                && let Some(usage) = &response.usage
            {
                messages.native_context_usage = Some(crate::subagent::SubagentContextUsage {
                    total_tokens: u64::from(usage.total_tokens),
                    estimate_after_last_response: None,
                    estimate_at_last_response: super::super::context::native_tokens_with(
                        &messages.entries,
                        &self.counter,
                    ),
                });
            }
            let output_tokens = response.usage.as_ref().map_or(0, |u| u.completion_tokens);
            let limited = response.output_limited();
            let parts = response.item_parts();
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
                    usage: response.usage.clone(),
                    thinking: response.thinking,
                    usage_complete: response.usage_complete,
                    settled_reasoning: response.settled_reasoning.clone(),
                });
            if limited
                && matches!(
                    self.length_recovery(
                        messages,
                        &response.text,
                        !calls.is_empty(),
                        output_tokens,
                        response.usage.as_ref(),
                    )
                    .await?,
                    super::super::compaction::LengthAction::Retry
                )
            {
                continue;
            }
            let at_ms = self.event_ms().await?;
            messages.push_timed(
                CompletionMessage {
                    continuation: response.continuation,
                    role: MessageRole::Assistant,
                    content: response.text.clone(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: (!calls.is_empty()).then(|| calls.clone()),
                },
                response.event_seq,
                Some(&self.turn.id),
                at_ms,
            );
            let model = self.active_model();
            if let Some(entry) = messages.entries.last_mut() {
                entry.parts = parts;
                entry.settled_reasoning = response.settled_reasoning.unwrap_or_default();
                entry.usage = response.usage;
                entry.model = Some(model);
                if limited {
                    entry.stop = Stop::Truncated;
                }
            }
            if delivered_content(&response.text, !calls.is_empty(), false) {
                self.guidance.length_recoveries = 0;
            }
            if reported_native_usage {
                let estimate =
                    super::super::context::native_tokens_with(&messages.entries, &self.counter);
                if let Some(usage) = &mut messages.native_context_usage {
                    usage.estimate_after_last_response = estimate;
                }
            }
            if calls.is_empty() {
                if self.after_final_answer(&response.text, messages).await? {
                    continue;
                }
                return Ok(response.text);
            }
            let batch_from = messages.entries.len();
            self.run_tools(&request_id, response.event_seq, calls.clone(), messages)
                .await?;
            self.after_tools(&calls, messages).await?;
            self.explicit_new_context(messages, batch_from).await?;
            self.update_prompt(messages)?;
        }
    }
    /// One provider response, switching to the next fallback model when a request fails and
    /// recording stream-guard retries.
    async fn provider_response(
        &mut self,
        messages: &mut super::super::context::Context,
        dispatched: &mut bool,
    ) -> Result<(String, super::super::streaming::Response), CoordinatorError> {
        loop {
            match self.dispatch(messages, dispatched).await {
                Err(error @ CoordinatorError::Provider { .. }) => {
                    let next = self.fallbacks.pop_front().ok_or(error)?;
                    self.turn.model = next.model_ref.clone();
                    self.turn.settings = (&next).into();
                    self.turn.target = Some(next);
                    self.update_prompt(messages)?;
                }
                Err(CoordinatorError::StreamGuard(failure)) => {
                    messages
                        .usage
                        .push(crate::subagent::FinalizedProviderUsage {
                            request_id: failure.request_id.clone(),
                            attempt_id: self.turn.id.clone(),
                            model_ref: self.turn.model.clone(),
                            usage: failure.usage.clone(),
                            thinking: None,
                            usage_complete: failure.usage_complete,
                            settled_reasoning: None,
                        });
                    self.retry_after_stream_guard(failure, messages).await?;
                }
                result => return result,
            }
        }
    }

    /// The model answered without tool calls. Terminal maintenance runs before any automatic
    /// continuation, so a deliberate dead-end pause also suppresses todo reminders. Returns
    /// true when the turn goes on.
    async fn after_final_answer(
        &mut self,
        text: &str,
        messages: &mut super::super::context::Context,
    ) -> Result<bool, CoordinatorError> {
        let terminal = messages.entries.last().is_some_and(terminal_text_answer);
        let post = self.post_turn_maintenance(messages, terminal).await?;
        if post.resume {
            drop_orphan_tool_use(messages);
            self.remind(
                RuntimeReminderKind::CompactionResume,
                harness_compaction::prompts::auto_continue_prompt().to_owned(),
                None,
                messages,
            )
            .await?;
            if post.changed {
                self.reinject_todos(messages).await?;
            }
            self.update_prompt(messages)?;
            return Ok(true);
        }
        self.continue_after_stop(text, messages, post.owns_next_call)
            .await
    }
    /// Epoch milliseconds from the coordinator clock; `None` when it has no wall time.
    pub(in crate::coord) async fn event_ms(&self) -> Result<Option<u64>, CoordinatorError> {
        self.handle
            .call(|s| {
                Ok(s.clock
                    .system_time_rfc3339_millis()
                    .as_deref()
                    .and_then(crate::clock::rfc3339_unix_ms)
                    .and_then(|ms| u64::try_from(ms).ok()))
            })
            .await
    }
    /// `provider/model` of the target this turn currently talks to.
    fn active_model(&self) -> std::sync::Arc<str> {
        match &self.turn.target {
            Some(target) => format!("{}/{}", target.provider, target.model).into(),
            None => self.turn.model.replacen(':', "/", 1).into(),
        }
    }
    /// Post-turn maintenance runs after the answer is delivered. Its failures leave the
    /// answer standing; a synthetic resume is the only way it keeps the turn going.
    async fn post_turn_maintenance(
        &mut self,
        messages: &mut super::super::context::Context,
        terminal_text_answer: bool,
    ) -> Result<PostTurn, CoordinatorError> {
        let mut post = PostTurn::default();
        if self.provider.manages_context(&self.request(messages)) {
            return Ok(post);
        }
        let generation = self.guidance.input_generation;
        let maintained = match self
            .threshold_maintenance(
                messages,
                Site::PostTurn {
                    terminal_text_answer,
                },
            )
            .await
        {
            Err(error) if self.cancellation.is_cancelled() => return Err(error),
            Err(error) => {
                self.warn(format!("Auto-compaction failed: {error}"))
                    .await?;
                return Ok(post);
            }
            Ok(maintained) => maintained,
        };
        post.changed = maintained.changed;
        post.owns_next_call =
            maintained.continuation_scheduled || maintained.synthetic_resume || maintained.blocked;
        if maintained.synthetic_resume {
            // The pass decided to resume; confirm nothing queued or arrived meanwhile.
            let source = ContinuationSource::AfterPass {
                suppress: false,
                auto_continue: true,
                terminal_text_answer: false,
                blocked: false,
            };
            post.resume = matches!(
                self.schedule_continuation(source, generation).await?,
                Continuation::Resume
            );
        }
        Ok(post)
    }
}
