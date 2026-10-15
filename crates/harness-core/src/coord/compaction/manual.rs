//! Manual maintenance passes: `/compact`, `/handoff`, `/shake` and idle requests.
//!
//! A pass owns the agent's maintenance from the moment it is queued, before the interrupted
//! turn has finished tearing down. [`ManualClaims`] keeps the pass, the callers waiting on its
//! cleanup and the resume the interrupted turn is owed apart.
use super::super::{context::Context, turn::Worker, *};
use super::{
    maintain::{lifecycle::End, Outcome},
    speculate, Progress, Reason, Request, ShakeOutcome,
};
use harness_compaction::{CompactionMethod, ManualMode, ShakeMode};
mod claims;
#[cfg(test)]
mod claims_tests;
mod handle;
#[cfg(test)]
mod manual_tests;
mod rejection;
pub(in crate::coord) use claims::{ManualClaims, Resume};
pub use handle::ResumeClaim;
pub(in crate::coord) use rejection::{hook_veto, translate_cancellation};
pub use rejection::{HandoffRejection, ManualRejection};
pub(in crate::coord) enum ManualKind {
    Compact {
        through: Option<String>,
        instructions: Option<String>,
        guidance: Option<String>,
        mode: Option<ManualMode>,
    },
    Handoff {
        focus: Option<String>,
    },
    Shake {
        mode: ShakeMode,
    },
    Idle,
}

impl ManualKind {
    /// Compaction and handoff passes own the agent's maintenance; shake and idle do not.
    pub(in crate::coord) const fn owns_pass(&self) -> bool {
        matches!(self, Self::Compact { .. } | Self::Handoff { .. })
    }
}

pub(in crate::coord) enum ManualReply {
    Compaction(Reply<ManualCompactionOutcome>),
    Shake(Reply<ShakeOutcome>),
}

impl ManualReply {
    pub(in crate::coord) fn fail(self, error: CoordinatorError) {
        match self {
            Self::Compaction(reply) => {
                let _ = reply.send(Err(error));
            }
            Self::Shake(reply) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    pub(in crate::coord) fn noop(self, reason: &str) {
        match self {
            Self::Compaction(reply) => {
                let _ = reply.send(Ok(ManualCompactionOutcome::NoOp {
                    reason: reason.to_owned(),
                }));
            }
            Self::Shake(reply) => {
                let _ = reply.send(Ok(ShakeOutcome {
                    summary: reason.to_owned(),
                    tokens_freed: 0,
                }));
            }
        }
    }
}

/// A maintenance pass queued on an agent: manual compaction, handoff, shake or idle.
pub(in crate::coord) struct Manual {
    pub kind: ManualKind,
    pub reason: String,
    pub reply: ManualReply,
    /// A shake interrupted a turn that was actually streaming; resume it afterwards when
    /// allowed. Compaction and handoff passes keep this in their claims instead.
    pub interrupted: bool,
    /// The caller owns continuation: never resume an interrupted turn.
    pub suppress: bool,
}

impl Runtime {
    /// Installs `task`'s pass as the agent's owner of maintenance. This runs before the
    /// interrupted turn is aborted, so nothing that arrives during teardown can slip past it.
    /// A second compaction or handoff while one owns the agent is rejected.
    pub(in crate::coord) fn begin_manual_pass(
        &mut self,
        agent: &str,
        task: &str,
        interrupted_streaming: bool,
        suppress: bool,
    ) -> Result<(), CoordinatorError> {
        let live: Vec<String> = self
            .agents
            .get(agent)
            .map(|a| a.queue.iter().map(|turn| turn.id.clone()).collect())
            .unwrap_or_default();
        let running = &self.running;
        self.manual_claims.prune(agent, |id| {
            running.contains_key(id) || live.iter().any(|queued| queued == id)
        });
        self.manual_claims
            .begin(agent, task, interrupted_streaming, suppress)
            .map_err(CoordinatorError::CompactionRejected)?;
        speculate::cancel(self, agent);
        // An automatic pass in flight loses to the manual one instead of racing it.
        if let Some((_, token)) = self.compacting.get(agent) {
            token.cancel();
        }
        Ok(())
    }

    /// Cleanup of a finished pass: ownership is released, the barrier resolves, and the
    /// interrupted turn's resume is scheduled or parked behind the outstanding claims.
    pub(in crate::coord) fn end_manual_pass(
        &mut self,
        agent: &str,
        task: &str,
        rejected_noop: bool,
    ) -> Result<(), CoordinatorError> {
        let auto_continue = self.config.compaction.auto_continue;
        match self
            .manual_claims
            .end(agent, task, rejected_noop, auto_continue)
        {
            Some(resume) => self.resume_interrupted(&resume),
            None => Ok(()),
        }
    }

    /// Settles a pass whose carrier turn finished without cleaning up itself. The pass is
    /// over either way, so a resume that cannot be queued is dropped.
    pub(in crate::coord) fn settle_manual_task(&mut self, task: &str) {
        if let Some(agent) = self.manual_claims.owner_of(task) {
            drop(self.end_manual_pass(&agent, task, false));
        }
    }

    pub(in crate::coord) fn release_resume_claim(
        &mut self,
        agent: &str,
        dispatched: bool,
    ) -> Result<(), CoordinatorError> {
        match self.manual_claims.release(agent, dispatched) {
            Some(resume) => self.resume_interrupted(&resume),
            None => Ok(()),
        }
    }

    /// Queues the synthetic turn that resumes interrupted work, unless the user already
    /// queued work, a newer turn started, or the session is going away. No finished-answer
    /// check applies: an interrupted turn never ended on an answer.
    pub(in crate::coord) fn resume_interrupted(
        &mut self,
        resume: &Resume,
    ) -> Result<(), CoordinatorError> {
        let agent = &resume.agent;
        if self.manual_claims.generation(agent) != resume.generation
            || self.agents.get(agent).is_none_or(|a| !a.queue.is_empty())
        {
            return Ok(());
        }
        self.pending_reminders
            .entry(agent.clone())
            .or_default()
            .push(super::super::reminders::PendingReminder {
                kind: RuntimeReminderKind::CompactionResume,
                body: harness_compaction::prompts::auto_continue_prompt().to_owned(),
                source: None,
            });
        match self.queue_turn(
            EventActor::new(ActorKind::Worker, Some(agent.clone())),
            agent,
            super::super::prompt::Prompt {
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
    }
}

/// What a dispatched pass produced: a shake result, or the outcome of a maintenance pass.
type Dispatched = Result<Result<(String, u32), Outcome>, CoordinatorError>;

impl Worker {
    /// Runs a queued maintenance pass and answers its caller. The pass is cleaned up before
    /// the caller hears back, so a replacement prompt never races the cleanup.
    pub(in crate::coord) async fn run_manual(
        &mut self,
        context: &mut Context,
        manual: Manual,
    ) -> Result<String, CoordinatorError> {
        let Manual {
            kind,
            reason,
            reply,
            interrupted,
            suppress,
        } = manual;
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let (owns_pass, idle, shake, handoff) = (
            kind.owns_pass(),
            matches!(kind, ManualKind::Idle),
            matches!(kind, ManualKind::Shake { .. }),
            matches!(kind, ManualKind::Handoff { .. }),
        );
        if shake {
            let agent = agent.clone();
            self.handle
                .call(move |s| {
                    speculate::cancel(s, &agent);
                    Ok(())
                })
                .await?;
        }
        let result = self.dispatch_manual(context, kind, reason).await;
        let rejection = rejection_of(&result, idle, handoff);
        let cleanup = if owns_pass {
            let (task, rejected) = (self.turn.id.clone(), rejection.is_some());
            self.handle
                .call(move |s| s.end_manual_pass(&agent, &task, rejected))
                .await
        } else if shake && interrupted && !suppress && result.is_ok() {
            self.handle
                .call(move |s| {
                    if !s.config.compaction.auto_continue {
                        return Ok(());
                    }
                    let generation = s.manual_claims.generation(&agent);
                    s.resume_interrupted(&Resume { agent, generation })
                })
                .await
        } else {
            Ok(())
        };
        let text = answer(result, reply, rejection);
        cleanup?;
        text
    }

    async fn dispatch_manual(
        &mut self,
        context: &mut Context,
        kind: ManualKind,
        reason: String,
    ) -> Dispatched {
        match kind {
            ManualKind::Shake { mode } => self.run_shake(context, mode, &reason).await.map(Ok),
            ManualKind::Compact {
                through,
                instructions,
                guidance,
                mode,
            } => {
                let pending = self.pending_through(context, through.as_deref())?;
                let (order, forced) = match mode {
                    Some(mode) => (mode.forced_order(), true),
                    None => (self.compaction.method_order.clone(), false),
                };
                let request = Request {
                    label: reason,
                    focus: instructions,
                    internal_guidance: guidance,
                    forced,
                    suppress_continuation: true,
                    ..Request::automatic(Reason::Manual, &order, pending)
                };
                self.run_pass(context, request).await.map(Err)
            }
            ManualKind::Handoff { focus } => {
                let request = Request {
                    label: reason,
                    focus,
                    forced: true,
                    handoff_requested: true,
                    suppress_continuation: true,
                    ..Request::automatic(Reason::Manual, &[CompactionMethod::Handoff], None)
                };
                self.run_pass(context, request).await.map(Err)
            }
            ManualKind::Idle => {
                let order = self.compaction.method_order.clone();
                let request = Request {
                    label: reason,
                    ..Request::automatic(Reason::Idle, &order, None)
                };
                self.run_pass(context, request).await.map(Err)
            }
        }
    }

    /// Entries after the last entry of `through` are treated as not yet sent, so a manual
    /// compaction "through" a request never summarizes anything later.
    fn pending_through(
        &self,
        context: &Context,
        through: Option<&str>,
    ) -> Result<Option<usize>, CoordinatorError> {
        let Some(through) = through else {
            return Ok(None);
        };
        context
            .entries
            .iter()
            .rposition(|e| e.turn.as_deref() == Some(through))
            .map(|last| Some(last + 1))
            .ok_or_else(|| {
                CoordinatorError::Invalid("compaction request boundary is not in context".into())
            })
    }

    async fn run_shake(
        &mut self,
        context: &mut Context,
        mode: ShakeMode,
        reason: &str,
    ) -> Result<(String, u32), CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let progress = Progress::start(
            &self.handle,
            &self.actor,
            &self.turn.id,
            &agent,
            reason,
            &self.cancellation,
        )
        .await?;
        let request = Request {
            label: reason.to_owned(),
            ..Request::automatic(Reason::Manual, &[CompactionMethod::Shake], None)
        };
        self.lifecycle_start(&progress, &request).await?;
        let before = self.measure(context)?.tokens;
        progress
            .update(|stage| {
                stage.method = Some(CompactionMethod::Shake);
                stage.phase = crate::event::CompactionPhase::Shaking;
                stage.tokens_before = Some(before);
            })
            .await?;
        let result = self.manual_shake(context, mode, &progress).await;
        let outcome = result.as_ref().ok().map(|(summary, freed)| Outcome {
            shake: (*freed > 0).then(|| (summary.clone(), *freed)),
            ..Outcome::default()
        });
        self.lifecycle_end(
            &progress,
            &request,
            End {
                outcome: outcome.as_ref(),
                aborted: progress.cancel.is_cancelled(),
                error: result.as_ref().err().map(ToString::to_string),
            },
        )
        .await?;
        let phase = match &result {
            Ok((_, freed)) if *freed > 0 => crate::event::CompactionPhase::Done,
            Ok(_) => crate::event::CompactionPhase::Skipped,
            Err(_) => crate::event::CompactionPhase::Failed,
        };
        progress
            .finish(phase, result.as_ref().err().map(ToString::to_string))
            .await?;
        result
    }
}

/// A pass that ran but committed nothing is a typed rejection for a manual caller; an idle
/// pass keeps its plain no-op.
fn rejection_of(result: &Dispatched, idle: bool, handoff: bool) -> Option<ManualRejection> {
    let Ok(Err(outcome)) = result else {
        return None;
    };
    if outcome.committed.is_some() || idle {
        return None;
    }
    Some(match outcome.noop.as_deref() {
        Some(text) => ManualRejection::from_text(text),
        None if handoff => ManualRejection::NothingToHandOff(HandoffRejection::AlreadyCompacted),
        None => ManualRejection::SessionTooSmall,
    })
}

/// Answers the caller and returns the task's own result text.
fn answer(
    result: Dispatched,
    reply: ManualReply,
    rejection: Option<ManualRejection>,
) -> Result<String, CoordinatorError> {
    match (result, reply) {
        (Ok(Ok((summary, freed))), ManualReply::Shake(reply)) => {
            let text = summary.clone();
            let _ = reply.send(Ok(ShakeOutcome {
                summary,
                tokens_freed: freed,
            }));
            Ok(text)
        }
        (Ok(Err(outcome)), ManualReply::Compaction(reply)) => {
            let (text, value) = match (outcome.committed, rejection) {
                (Some(committed), _) => (
                    "Context compaction finished.".to_owned(),
                    Ok(ManualCompactionOutcome::Compacted {
                        tokens_before: committed.tokens_before,
                        tokens_after: committed.tokens_after,
                        summary_preview: committed.preview,
                        method: committed.method,
                        saved_path: outcome.saved_path,
                    }),
                ),
                (None, Some(rejection)) => (
                    rejection.to_string(),
                    Err(CoordinatorError::CompactionRejected(rejection)),
                ),
                (None, None) => {
                    let reason = outcome
                        .noop
                        .unwrap_or_else(|| ManualRejection::SessionTooSmall.to_string());
                    (reason.clone(), Ok(ManualCompactionOutcome::NoOp { reason }))
                }
            };
            let _ = reply.send(value);
            Ok(text)
        }
        (Err(error), reply) => {
            let text = error.to_string();
            let cancelled = matches!(error, CoordinatorError::CompactionCancelled { .. });
            reply.fail(error);
            Err(if cancelled {
                CoordinatorError::Cancelled(text)
            } else {
                CoordinatorError::Invalid(text)
            })
        }
        (Ok(_), reply) => {
            let message = "maintenance reply kind mismatch";
            reply.fail(CoordinatorError::Invalid(message.into()));
            Err(CoordinatorError::Invalid(message.into()))
        }
    }
}
