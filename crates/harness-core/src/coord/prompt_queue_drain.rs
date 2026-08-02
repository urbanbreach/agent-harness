use super::*;
use crate::prompt_queue::{DurablePromptQueue, PromptQueueError};

/// Outcome of an automatic drain of the durable session prompt queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptQueueAutoDrainOutcome {
    Drained {
        count: usize,
        request_ids: Vec<String>,
    },
    Skipped {
        reason: String,
    },
}

/// Outcome of a mid-turn drain of durable queue interjections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidTurnInterjectionDrainOutcome {
    Drained {
        count: usize,
        request_ids: Vec<String>,
    },
    Skipped {
        reason: String,
    },
}

fn queue_error(err: PromptQueueError) -> CoordinatorError {
    CoordinatorError::PromptQueue(err.to_string())
}

impl Coordinator {
    /// Drain the durable session prompt queue into live agent turns.
    ///
    /// Auto-drain conditions: the run is started, the agent is a root session
    /// agent (subagents never drain the session queue), the agent is idle (no
    /// active or queued turn), and the queue is non-empty. Each drained entry
    /// becomes a user-actor turn request in FIFO order; the scheduler queues
    /// same-agent turns sequentially so order is preserved. Entries are popped
    /// one at a time and re-enqueued best-effort when a turn request fails, so
    /// a failed drain never loses queued prompts.
    pub(in crate::coord) async fn drain_durable_prompt_queue_for_agent(
        &mut self,
        agent_id: &str,
    ) -> Result<PromptQueueAutoDrainOutcome, CoordinatorError> {
        let run_dir = {
            let Some(run_state) = self.run_state.as_ref() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            if !run_state.agents.contains_key(agent_id) {
                return Err(CoordinatorError::UnknownAgent(agent_id.to_string()));
            }
            if run_state.subagent_parent_by_id.contains_key(agent_id) {
                return Ok(PromptQueueAutoDrainOutcome::Skipped {
                    reason: "subagent turns do not auto-drain the session prompt queue".to_string(),
                });
            }
            if run_state.agent_has_active_or_queued_turn(agent_id) {
                return Ok(PromptQueueAutoDrainOutcome::Skipped {
                    reason: "agent already has an active or queued turn".to_string(),
                });
            }
            run_state.info.run_dir.clone()
        };

        let queue = DurablePromptQueue::for_session(&run_dir);
        let mut request_ids = Vec::new();
        while let Some(entry) = queue.dequeue().map_err(queue_error)? {
            let actor =
                EventActor::new(ActorKind::User, Some("prompt-queue-auto-drain".to_string()));
            match self
                .request_agent_turn_internal(
                    actor,
                    agent_id.to_string(),
                    entry.text.clone(),
                    crate::file_tag::SelectedPromptTags::default(),
                    None,
                    None,
                    None,
                )
                .await
            {
                Ok(request_id) => request_ids.push(request_id),
                Err(err) => {
                    if let Err(requeue_err) =
                        queue.enqueue(&entry.id, &entry.text, entry.enqueued_at_unix_ms)
                    {
                        tracing::warn!(
                            agent_id = %agent_id,
                            entry_id = %entry.id,
                            error = %requeue_err,
                            "failed to re-enqueue prompt queue entry after failed turn request"
                        );
                    }
                    return Err(err);
                }
            }
        }

        if request_ids.is_empty() {
            return Ok(PromptQueueAutoDrainOutcome::Skipped {
                reason: "prompt queue is empty".to_string(),
            });
        }
        let count = request_ids.len();
        Ok(PromptQueueAutoDrainOutcome::Drained {
            count,
            request_ids,
        })
    }

    /// Drain durable queue interjections while the agent's turn is still
    /// running (mid-turn), recording each one as a `UserMessageSubmitted`
    /// event and staging delivery as a pending agent wakeup.
    ///
    /// Interjections are the front-inserted entries created by
    /// `DurablePromptQueue::interject_mid_turn`; ordinary FIFO queue entries
    /// are untouched and remain for post-turn auto-drain. Staged wakeups are
    /// scheduled by `schedule_pending_agent_wakeups_for_idle_agent` as soon as
    /// the running turn finishes, ahead of any blocked-turn promotion. This
    /// never mutates conversation events of the in-flight turn.
    pub(in crate::coord) async fn drain_mid_turn_interjections(
        &mut self,
        agent_id: &str,
    ) -> Result<MidTurnInterjectionDrainOutcome, CoordinatorError> {
        let run_dir = {
            let Some(run_state) = self.run_state.as_ref() else {
                return Err(CoordinatorError::RunNotStarted);
            };
            if !run_state.agents.contains_key(agent_id) {
                return Err(CoordinatorError::UnknownAgent(agent_id.to_string()));
            }
            if !run_state.agent_has_running_turn(agent_id) {
                return Ok(MidTurnInterjectionDrainOutcome::Skipped {
                    reason: "agent has no running turn; interjections drain post-turn".to_string(),
                });
            }
            run_state.info.run_dir.clone()
        };

        let queue = DurablePromptQueue::for_session(&run_dir);
        let interjections = queue.drain_interjections().map_err(queue_error)?;
        if interjections.is_empty() {
            return Ok(MidTurnInterjectionDrainOutcome::Skipped {
                reason: "no queued interjections".to_string(),
            });
        }

        let Some(run_state) = self.run_state.as_mut() else {
            return Err(CoordinatorError::RunNotStarted);
        };
        let mut request_ids = Vec::new();
        for entry in interjections {
            let request_id = allocate_provider_request_id(run_state);
            append_payload_event_with_correlation(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                EventActor::new(ActorKind::User, Some("mid-turn-interjection".to_string())),
                Some(format!("agent:{agent_id}")),
                Some(request_id.clone()),
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: request_id.clone().into(),
                    text: entry.text.clone(),
                }),
            )?;
            run_state
                .pending_agent_wakeups
                .entry(agent_id.to_string())
                .or_default()
                .push(PendingAgentWakeup {
                    request_id: request_id.clone(),
                    notification_text: entry.text,
                });
            request_ids.push(request_id);
        }

        let count = request_ids.len();
        Ok(MidTurnInterjectionDrainOutcome::Drained {
            count,
            request_ids,
        })
    }
}
