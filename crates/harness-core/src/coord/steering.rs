//! User messages sent into an agent's running turn.
//!
//! Steering joins the running turn before its next provider request instead of waiting for
//! the turn to end. Acceptance is journaled before the caller hears back. A message that
//! arrives too late for the turn becomes a queued turn, so nothing the user sent is lost;
//! only a stopping or rewinding run cancels it, as it does every queued prompt. Long waits
//! inside a turn watch [`SteeringArrival`] and end early, as their timeout would, so the
//! message does not sit behind them.
use super::reminders::Steering;
use super::runtime::JobKind;
use super::*;
use std::collections::VecDeque;
use tokio::sync::watch;

/// Where a steering message went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    /// Accepted for the running turn and journaled; it joins the turn's next request.
    Steering { message_id: String, turn_id: String },
    /// No turn is running; nothing was recorded, so the caller submits a normal prompt
    /// with its own model selection, tags, and attachments.
    Idle,
}

/// Resolves once user steering waits for an agent's turn. Taken from the coordinator, it
/// also covers steering accepted before the wait began.
pub struct SteeringArrival {
    pending: bool,
    signal: Option<watch::Receiver<()>>,
}

impl SteeringArrival {
    /// Never resolves for a caller without an agent.
    pub async fn arrived(self) {
        if self.pending {
            return;
        }
        let Some(mut signal) = self.signal else {
            return std::future::pending().await;
        };
        if signal.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Steering messages waiting for one turn; the turn's own prompt is limited to 1 MiB too.
const MAX_PENDING_STEERING: usize = 32;

impl CoordinatorHandle {
    /// Sends a user message into the agent's running turn. Returns `Idle`, recording
    /// nothing, when the agent has no running turn.
    pub async fn steer_agent_turn(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<SteerOutcome, CoordinatorError> {
        let (agent, text) = (agent.into(), text.into());
        self.call(move |s| s.steer(actor, &agent, text)).await
    }

    /// Watches for steering sent to the agent's running turn, so a tool that blocks inside
    /// that turn can return early and let the message reach the model.
    pub async fn steering_arrival(
        &self,
        agent: Option<String>,
    ) -> Result<SteeringArrival, CoordinatorError> {
        self.call(move |s| Ok(s.steering_arrival(agent.as_deref())))
            .await
    }

    /// After the user interrupts a turn, returns the input still waiting for the agent so
    /// it can go back into the editor instead of starting new turns.
    pub async fn withdraw_queued_input(
        &self,
        agent: impl Into<String>,
    ) -> Result<Vec<String>, CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| s.withdraw_queued_input(&agent, false))
            .await
    }

    /// Takes back everything the user queued for the agent, prompts with tags or
    /// attachments included, so a failed delivery the caller restores from its own copy
    /// leaves nothing behind that would run a second time. Returns the texts.
    pub async fn withdraw_accepted_input(
        &self,
        agent: impl Into<String>,
    ) -> Result<Vec<String>, CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| s.withdraw_queued_input(&agent, true))
            .await
    }
}

impl Runtime {
    fn steer(
        &mut self,
        actor: EventActor,
        agent: &str,
        text: String,
    ) -> Result<SteerOutcome, CoordinatorError> {
        self.accepting()?;
        let prompt = super::prompt::Prompt::from(text);
        prompt.validate(self.redactor.as_ref())?;
        let running = self.running.iter().find_map(|(id, job)| match &job.kind {
            JobKind::Turn { agent: owner }
                if owner == agent && !job.cancellation.is_cancelled() =>
            {
                Some(id.clone())
            }
            _ => None,
        });
        let pending = self.steering.get(agent).map_or(0, VecDeque::len);
        match running {
            Some(turn_id) if pending < MAX_PENDING_STEERING => {
                // A late message becomes a queued turn, so it needs queue room now.
                self.check_turn_capacity(agent)?;
                let message_id = self.id("steer")?;
                let accepted = self.emit(
                    EventActor::new(actor.kind, Some(agent.into())),
                    Some(message_id.clone()),
                    EventV1::SteeringAccepted(SteeringAcceptedEvent {
                        request_id: message_id.clone().into(),
                        turn_id: turn_id.clone(),
                        text: prompt.text.clone(),
                    }),
                )?;
                if actor.kind == ActorKind::User {
                    self.user_resumes_monitors(agent);
                }
                self.steering
                    .entry(agent.to_owned())
                    .or_default()
                    .push_back(Steering {
                        id: message_id.clone(),
                        text: prompt.text,
                        seq: accepted.seq,
                    });
                if let Some(signal) = self.steering_signals.get(agent) {
                    signal.send_replace(());
                }
                Ok(SteerOutcome::Steering {
                    message_id,
                    turn_id,
                })
            }
            Some(_) => Err(CoordinatorError::Invalid(
                "too many steering messages are waiting for the running turn".into(),
            )),
            None => Ok(SteerOutcome::Idle),
        }
    }

    pub(super) fn steering_arrival(&mut self, agent: Option<&str>) -> SteeringArrival {
        let Some(agent) = agent else {
            return SteeringArrival {
                pending: false,
                signal: None,
            };
        };
        SteeringArrival {
            pending: self
                .steering
                .get(agent)
                .is_some_and(|pending| !pending.is_empty()),
            signal: Some(
                self.steering_signals
                    .entry(agent.to_owned())
                    .or_insert_with(|| watch::Sender::new(()))
                    .subscribe(),
            ),
        }
    }

    /// Takes back the user input still waiting for the agent: undelivered steering and
    /// queued plain prompts, in the order the user sent them. Each is recorded as
    /// cancelled; prompts with tags or attachments stay queued unless `rich` asks for them.
    fn withdraw_queued_input(
        &mut self,
        agent: &str,
        rich: bool,
    ) -> Result<Vec<String>, CoordinatorError> {
        let mut withdrawn: Vec<_> = self
            .steering
            .remove(agent)
            .unwrap_or_default()
            .into_iter()
            .map(|steering| (steering.seq, steering.id, steering.text))
            .collect();
        withdrawn.extend(self.take_queued_user_prompts(agent, rich));
        withdrawn.sort_by_key(|(seq, ..)| *seq);
        for (_, id, _) in &withdrawn {
            self.emit(
                EventActor::new(ActorKind::User, Some(agent.into())),
                Some(id.clone()),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: id.clone().into(),
                    reason: RETURNED_TO_EDITOR_REASON.into(),
                    failure: false,
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            )?;
        }
        Ok(withdrawn.into_iter().map(|(.., text)| text).collect())
    }

    /// Delivers steering queued for this agent's running turn as user messages of that turn.
    pub(super) fn drain_steering(
        &mut self,
        agent: &str,
        request: &str,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        let Some(pending) = self.steering.remove(agent) else {
            return Ok(Vec::new());
        };
        let mut delivered = Vec::with_capacity(pending.len());
        for steering in pending {
            let event = self.emit(
                EventActor::new(ActorKind::User, Some(agent.into())),
                Some(request.into()),
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: steering.id.into(),
                    text: steering.text.clone(),
                }),
            )?;
            delivered.push((event.seq, steering.text));
        }
        Ok(delivered)
    }

    /// Steering that missed its turn starts the next turns, in the order it was sent and
    /// ahead of follow-ups queued behind that turn. Called while the agent is still busy, so
    /// the finished turn's bookkeeping completes before any of them starts. A run that is
    /// stopping or rewinding cancels them instead.
    pub(super) fn requeue_steering(&mut self, agent: &str) -> Result<(), CoordinatorError> {
        let Some(pending) = self.steering.remove(agent) else {
            return Ok(());
        };
        let mut requeued = 0;
        for steering in pending {
            let id = steering.id.clone();
            let queued = self.queue_turn(
                EventActor::new(ActorKind::User, None),
                agent,
                super::prompt::Prompt {
                    reserved_id: Some(steering.id),
                    ..super::prompt::Prompt::from(steering.text)
                },
                None,
                None,
                None,
            );
            match queued {
                Ok(_) => requeued += 1,
                Err(
                    error @ (CoordinatorError::Stopping
                    | CoordinatorError::Invalid(_)
                    | CoordinatorError::UnknownAgent(_)),
                ) => {
                    self.emit(
                        EventActor::new(ActorKind::User, Some(agent.into())),
                        Some(id.clone()),
                        EventV1::TaskCancelled(TaskCancelledEvent {
                            task_id: id.into(),
                            reason: format!("steering could not be queued: {error}"),
                            failure: false,
                            task_scope: Some(TaskTerminalScope::AgentTurn),
                        }),
                    )?;
                }
                Err(error) => return Err(error),
            }
        }
        // The agent is busy, so queue_turn only appended; nothing started or woke meanwhile.
        if let Some(state) = self.agents.get_mut(agent) {
            state.queue.rotate_right(requeued);
        }
        Ok(())
    }
}
