//! What happens to the agent's work after a maintenance pass.
//!
//! One scheduler decides whether the interrupted work continues, who owns the next call
//! and when. The worker loop acts on the answer; nothing else schedules a synthetic turn.
use crate::coord::{
    context::{Context, Entry},
    turn::Worker,
    *,
};
use harness_compaction::Stop;
use harness_providers::MessageRole;
use std::time::Duration;

/// Pause before a recovery retry or a delayed continuation starts.
pub(in crate::coord) const RECOVERY_DELAY: Duration = Duration::from_millis(100);

/// Where the scheduling question comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum ContinuationSource {
    /// After an automatic pass inside a running turn (post-turn, recovery).
    AfterPass {
        /// The caller owns the next call: nothing is scheduled, queued work included.
        suppress: bool,
        /// The request allows a synthetic continuation (the setting also applies).
        auto_continue: bool,
        /// The turn ended on a finished text answer.
        terminal_text_answer: bool,
        /// The pass paused automatic work on purpose (a dead end).
        blocked: bool,
    },
    /// The turn a manual pass interrupted. Nothing is looping, so a delayed wake turn
    /// carries the resume reminder.
    Interrupted,
    /// A recovery retry (promotion, incomplete retry, notes rollover retry).
    Recovery,
}

/// What the caller does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum Continuation {
    /// Nothing continues.
    Stop,
    /// Automatic work is paused on purpose; todo and output-contract reminders stay quiet.
    Blocked,
    /// Queued work owns the next call.
    Queue,
    /// The caller appends the compaction-resume reminder and loops.
    Resume,
    /// The caller loops; the recovery delay has already passed.
    Retry,
}

/// The facts the precedence rules read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Facts {
    suppress: bool,
    queued: bool,
    auto_continue: bool,
    terminal_text_answer: bool,
    blocked: bool,
}

/// Exact precedence: suppression, queued work, a deliberate pause or a disabled synthetic
/// continuation, a finished text answer, otherwise resume.
fn decide(facts: Facts) -> Continuation {
    if facts.suppress {
        Continuation::Stop
    } else if facts.queued {
        Continuation::Queue
    } else if facts.blocked {
        Continuation::Blocked
    } else if !facts.auto_continue || facts.terminal_text_answer {
        Continuation::Stop
    } else {
        Continuation::Resume
    }
}

/// The model finished with a text answer and nothing else: a complete stop, at least one
/// non-blank text block, no tool call and no image. Reasoning blocks may coexist.
pub(in crate::coord) fn terminal_text_answer(entry: &Entry) -> bool {
    if entry.message.role != MessageRole::Assistant || entry.stop != Stop::Complete {
        return false;
    }
    let calls = entry
        .message
        .assistant_tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty());
    let images = !entry.images.is_empty()
        || entry
            .attachments
            .iter()
            .any(|attachment| attachment.mime.starts_with("image/"));
    let text = if entry.parts.is_empty() {
        !entry.message.content.trim().is_empty()
    } else {
        entry.parts.iter().any(|part| {
            matches!(part, harness_compaction::ItemPart::Text(text) if !text.trim().is_empty())
        })
    };
    let part_calls = entry
        .parts
        .iter()
        .any(|part| matches!(part, harness_compaction::ItemPart::Call(_)));
    text && !calls && !part_calls && !images
}

/// A response delivered something the user or a tool can act on: non-blank text, a tool
/// call or an image.
pub(in crate::coord) fn delivered_content(text: &str, calls: bool, images: bool) -> bool {
    calls || images || !text.trim().is_empty()
}

/// Removes a trailing assistant entry whose tool calls lack results, together with the
/// partial results after it. Providers reject a tool use without its result. Returns true
/// when anything was removed.
pub(in crate::coord) fn drop_orphan_tool_use(context: &mut Context) -> bool {
    let Some(at) = context.entries.iter().rposition(|entry| {
        entry.message.role == MessageRole::Assistant
            && entry
                .message
                .assistant_tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
    }) else {
        return false;
    };
    let tail = &context.entries[at + 1..];
    if tail
        .iter()
        .any(|entry| entry.message.role != MessageRole::Tool)
    {
        return false;
    }
    let answered = |id: &str| {
        tail.iter()
            .any(|entry| entry.message.tool_call_id.as_deref() == Some(id))
    };
    let complete = context.entries[at]
        .message
        .assistant_tool_calls
        .iter()
        .flatten()
        .all(|call| answered(&call.tool_call_id));
    if complete {
        return false;
    }
    context.entries.truncate(at);
    true
}

impl Worker {
    /// Decides what continues after a maintenance pass. `generation` is the input
    /// generation (or, for an interrupted turn, the prompt generation) the caller saw
    /// when the pass started; newer input or a newer turn cancels a synthetic resume.
    pub(in crate::coord) async fn schedule_continuation(
        &mut self,
        source: ContinuationSource,
        generation: u64,
    ) -> Result<Continuation, CoordinatorError> {
        match source {
            ContinuationSource::AfterPass {
                suppress,
                auto_continue,
                terminal_text_answer,
                blocked,
            } => {
                let queued = !suppress && self.has_queued_work().await?;
                let facts = Facts {
                    suppress,
                    queued,
                    auto_continue: auto_continue && self.compaction.auto_continue,
                    terminal_text_answer,
                    blocked,
                };
                let decision = decide(facts);
                if decision != Continuation::Resume {
                    return Ok(decision);
                }
                // Yield once so work queued in the meantime is seen, then confirm.
                tokio::task::yield_now().await;
                self.check_continuation_abort()?;
                if self.has_queued_work().await? {
                    return Ok(Continuation::Queue);
                }
                if self.guidance.input_generation != generation {
                    return Ok(Continuation::Stop);
                }
                Ok(Continuation::Resume)
            }
            ContinuationSource::Recovery => {
                self.recovery_pause().await?;
                Ok(Continuation::Retry)
            }
            ContinuationSource::Interrupted => {
                if self.has_queued_work().await? {
                    return Ok(Continuation::Queue);
                }
                self.queue_delayed_wake(generation);
                Ok(Continuation::Queue)
            }
        }
    }

    /// Sleeps for the recovery delay unless the turn is cancelled first.
    pub(in crate::coord) async fn recovery_pause(&self) -> Result<(), CoordinatorError> {
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {
                Err(CoordinatorError::Cancelled(self.turn.id.clone()))
            }
            () = tokio::time::sleep(RECOVERY_DELAY) => Ok(()),
        }
    }

    /// The agent's current prompt generation: the number of turns it has started.
    pub(in crate::coord) async fn prompt_generation(&self) -> Result<u64, CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        self.handle
            .call(move |s| {
                Ok(s.agents
                    .get(&agent)
                    .map_or(0, |agent| u64::from(agent.prompt_turns)))
            })
            .await
    }

    fn check_continuation_abort(&self) -> Result<(), CoordinatorError> {
        if self.cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        Ok(())
    }

    /// Steering waiting for this agent's turn, or turns already queued behind it.
    async fn has_queued_work(&self) -> Result<bool, CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        self.handle
            .call(move |s| {
                Ok(s.steering.get(&agent).is_some_and(|q| !q.is_empty())
                    || s.agents.get(&agent).is_some_and(|a| !a.queue.is_empty()))
            })
            .await
    }

    /// Queues a reminder-wake turn after the recovery delay. The task gives up when the
    /// agent is busy again, work is queued, a newer prompt started or the runtime stops.
    fn queue_delayed_wake(&self, generation: u64) {
        let handle = self.handle.clone();
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        tokio::spawn(async move {
            tokio::time::sleep(RECOVERY_DELAY).await;
            let _ = handle
                .call(move |s| {
                    let Some(state) = s.agents.get(&agent) else {
                        return Ok(());
                    };
                    if state.busy
                        || !state.queue.is_empty()
                        || u64::from(state.prompt_turns) != generation
                    {
                        return Ok(());
                    }
                    s.pending_reminders.entry(agent.clone()).or_default().push(
                        crate::coord::reminders::PendingReminder {
                            kind: RuntimeReminderKind::CompactionResume,
                            body: harness_compaction::prompts::auto_continue_prompt().to_owned(),
                            source: None,
                        },
                    );
                    match s.queue_turn(
                        EventActor::new(ActorKind::Worker, Some(agent.clone())),
                        &agent,
                        crate::coord::prompt::Prompt {
                            reminder_wake: true,
                            ..Default::default()
                        },
                        None,
                        None,
                        None,
                    ) {
                        Ok(_) | Err(CoordinatorError::Stopping) => Ok(()),
                        Err(error) => Err(error),
                    }
                })
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_compaction::ItemPart;
    use harness_providers::{AssistantToolCall, CompletionMessage};

    fn assistant(content: &str, calls: Vec<AssistantToolCall>) -> Entry {
        let message = CompletionMessage {
            continuation: None,
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: (!calls.is_empty()).then_some(calls),
        };
        Entry::new(message, 1, None)
    }

    fn call(id: &str) -> AssistantToolCall {
        AssistantToolCall {
            tool_call_id: id.into(),
            function_name: "read".into(),
            arguments_json: "{}".into(),
        }
    }

    fn tool_result(id: &str) -> Entry {
        let message = CompletionMessage {
            continuation: None,
            role: MessageRole::Tool,
            content: "ok".into(),
            name: Some("read".into()),
            tool_call_id: Some(id.into()),
            assistant_tool_calls: None,
        };
        Entry::new(message, 1, None)
    }

    #[test]
    fn terminal_answers() {
        let mut thinking_and_text = assistant("answer", Vec::new());
        thinking_and_text.parts = vec![ItemPart::reasoning("hmm"), ItemPart::text("answer")];
        let mut only_thinking = assistant("", Vec::new());
        only_thinking.parts = vec![ItemPart::reasoning("hmm")];
        let mut truncated = assistant("answer", Vec::new());
        truncated.stop = Stop::Truncated;
        let mut with_image = assistant("answer", Vec::new());
        with_image.images = vec![harness_compaction::ImageMeta {
            mime: "image/png".into(),
            width: None,
            height: None,
            detail: None,
            origin: harness_compaction::ImageOrigin::Tool,
        }];
        let cases = [
            ("plain text", assistant("done", Vec::new()), true),
            ("blank text", assistant("  \n", Vec::new()), false),
            ("thinking beside text", thinking_and_text, true),
            ("thinking only", only_thinking, false),
            ("tool call", assistant("working", vec![call("a")]), false),
            ("truncated", truncated, false),
            ("image", with_image, false),
            ("user entry", tool_result("a"), false),
        ];
        for (name, entry, expected) in cases {
            assert_eq!(terminal_text_answer(&entry), expected, "{name}");
        }
    }

    #[test]
    fn delivered_content_table() {
        for (text, calls, images, expected) in [
            ("", false, false, false),
            (" \t", false, false, false),
            ("hi", false, false, true),
            ("", true, false, true),
            ("", false, true, true),
        ] {
            assert_eq!(
                delivered_content(text, calls, images),
                expected,
                "{text:?} {calls} {images}"
            );
        }
    }

    #[test]
    fn precedence() {
        let base = Facts {
            suppress: false,
            queued: false,
            auto_continue: true,
            terminal_text_answer: false,
            blocked: false,
        };
        let cases = [
            ("resume by default", base, Continuation::Resume),
            (
                "suppress beats queue",
                Facts {
                    suppress: true,
                    queued: true,
                    ..base
                },
                Continuation::Stop,
            ),
            (
                "queue beats disabled continue",
                Facts {
                    queued: true,
                    auto_continue: false,
                    ..base
                },
                Continuation::Queue,
            ),
            (
                "queue beats dead end",
                Facts {
                    queued: true,
                    blocked: true,
                    ..base
                },
                Continuation::Queue,
            ),
            (
                "disabled continue stops",
                Facts {
                    auto_continue: false,
                    ..base
                },
                Continuation::Stop,
            ),
            (
                "dead end blocks",
                Facts {
                    blocked: true,
                    ..base
                },
                Continuation::Blocked,
            ),
            (
                "dead end blocks without synthetic continue",
                Facts {
                    blocked: true,
                    auto_continue: false,
                    ..base
                },
                Continuation::Blocked,
            ),
            (
                "text answer stops",
                Facts {
                    terminal_text_answer: true,
                    ..base
                },
                Continuation::Stop,
            ),
        ];
        for (name, facts, expected) in cases {
            assert_eq!(decide(facts), expected, "{name}");
        }
    }

    fn context(entries: Vec<Entry>) -> Context {
        Context {
            entries,
            ..Default::default()
        }
    }

    #[test]
    fn orphan_tool_use_cleanup() {
        let cases: [(&str, Vec<Entry>, usize); 5] = [
            ("no calls", vec![assistant("hi", Vec::new())], 1),
            ("unanswered call", vec![assistant("", vec![call("a")])], 0),
            (
                "answered call",
                vec![assistant("", vec![call("a")]), tool_result("a")],
                2,
            ),
            (
                "partial results",
                vec![assistant("", vec![call("a"), call("b")]), tool_result("a")],
                0,
            ),
            (
                "earlier orphan is not trailing",
                vec![
                    assistant("", vec![call("a")]),
                    assistant("later", Vec::new()),
                ],
                2,
            ),
        ];
        for (name, entries, kept) in cases {
            let mut context = context(entries);
            drop_orphan_tool_use(&mut context);
            assert_eq!(context.entries.len(), kept, "{name}");
        }
    }
}
