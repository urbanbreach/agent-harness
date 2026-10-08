//! User messages sent into an agent's running turn.
//!
//! Steering joins the running turn before its next provider request instead of waiting for
//! the turn to end. Acceptance is journaled before the caller hears back. A message that
//! arrives too late for the turn becomes a queued turn, so nothing the user sent is lost;
//! only a stopping or rewinding run cancels it, as it does every queued prompt.
use super::reminders::Steering;
use super::runtime::JobKind;
use super::*;
use std::collections::VecDeque;

/// Where a steering message went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    /// Accepted for the running turn and journaled; it joins the turn's next request.
    Steering { message_id: String, turn_id: String },
    /// No turn is running; nothing was recorded, so the caller submits a normal prompt
    /// with its own model selection, tags, and attachments.
    Idle,
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

    /// After the user interrupts a turn, returns the input still waiting for the agent so
    /// it can go back into the editor instead of starting new turns.
    pub async fn withdraw_queued_input(
        &self,
        agent: impl Into<String>,
    ) -> Result<Vec<String>, CoordinatorError> {
        let agent = agent.into();
        self.call(move |s| s.withdraw_queued_input(&agent)).await
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
                self.steering
                    .entry(agent.to_owned())
                    .or_default()
                    .push_back(Steering {
                        id: message_id.clone(),
                        text: prompt.text,
                        seq: accepted.seq,
                    });
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

    /// Takes back the user input still waiting for the agent: undelivered steering and
    /// queued plain prompts, in the order the user sent them. Each is recorded as
    /// cancelled; prompts with tags or attachments stay queued.
    fn withdraw_queued_input(&mut self, agent: &str) -> Result<Vec<String>, CoordinatorError> {
        let mut withdrawn: Vec<_> = self
            .steering
            .remove(agent)
            .unwrap_or_default()
            .into_iter()
            .map(|steering| (steering.seq, steering.id, steering.text))
            .collect();
        withdrawn.extend(self.take_queued_plain_prompts(agent));
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

    /// Steering that missed its turn starts the next turns, in the order it was sent.
    /// Called while the agent is still busy, so the finished turn's bookkeeping completes
    /// before any of them starts. A run that is stopping or rewinding cancels them instead.
    pub(super) fn requeue_steering(&mut self, agent: &str) -> Result<(), CoordinatorError> {
        let Some(pending) = self.steering.remove(agent) else {
            return Ok(());
        };
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
                Ok(_) => {}
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
        Ok(())
    }
}
