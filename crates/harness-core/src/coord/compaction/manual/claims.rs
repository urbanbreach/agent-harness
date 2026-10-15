//! Who may resume a turn that a manual pass interrupted.
//!
//! Three things are tracked per agent and never folded into one boolean: the pass that
//! currently owns maintenance (with the barrier that resolves when its cleanup is done),
//! the number of dispatch claims that are waiting on that cleanup, and the generation of
//! the resume the agent is still owed.
use super::ManualRejection;
use std::collections::BTreeMap;
use tokio::sync::watch;

/// A resume the caller must schedule now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct Resume {
    pub agent: String,
    pub generation: u64,
}

/// A caller that joined the barrier of the pass in flight.
pub(in crate::coord) struct Joined {
    /// Resolves (or closes) when the pass finishes its cleanup; absent when no pass runs
    /// and only an earlier claim keeps the debt alive.
    pub barrier: Option<watch::Receiver<bool>>,
}

#[derive(Debug)]
struct Pass {
    /// The queued manual turn that carries the pass.
    task: String,
    barrier: watch::Sender<bool>,
    /// An entry was appended; set before any post-append work so a late error keeps the debt.
    committed: bool,
    /// A provider-driven turn was actually running when the pass took ownership.
    interrupted_streaming: bool,
    /// An earlier pass had earned a resume that was still withheld.
    inherited: bool,
    /// The caller owns continuation.
    suppress: bool,
    /// Prompt generation captured after the abort.
    generation: u64,
}

#[derive(Debug, Default)]
struct Debt {
    claims: u32,
    owed: Option<u64>,
}

#[derive(Debug, Default)]
pub(in crate::coord) struct ManualClaims {
    passes: BTreeMap<String, Pass>,
    debts: BTreeMap<String, Debt>,
    generations: BTreeMap<String, u64>,
    /// The latest turn per agent that opened a provider stream.
    streaming: BTreeMap<String, String>,
}

impl ManualClaims {
    /// Task of the pass that owns `agent`'s maintenance, if any.
    pub fn active_task(&self, agent: &str) -> Option<&str> {
        self.passes.get(agent).map(|pass| pass.task.as_str())
    }

    /// The agent whose pass `task` carries.
    pub fn owner_of(&self, task: &str) -> Option<String> {
        self.passes
            .iter()
            .find(|(_, pass)| pass.task == task)
            .map(|(agent, _)| agent.clone())
    }

    /// Drops a pass whose carrier turn no longer exists, resolving anything waiting on it.
    pub fn prune(&mut self, agent: &str, live: impl Fn(&str) -> bool) {
        if self.passes.get(agent).is_some_and(|pass| !live(&pass.task)) {
            self.passes.remove(agent);
        }
    }

    /// Takes ownership of `agent`'s maintenance. The debt an earlier pass left behind moves
    /// into this pass as `inherited`; the abort that follows bumps the prompt generation.
    pub fn begin(
        &mut self,
        agent: &str,
        task: &str,
        interrupted_streaming: bool,
        suppress: bool,
    ) -> Result<(), ManualRejection> {
        if self.passes.contains_key(agent) {
            return Err(ManualRejection::InProgress);
        }
        let inherited = self
            .debts
            .get_mut(agent)
            .is_some_and(|debt| debt.owed.take().is_some());
        let generation = self.bump(agent);
        let (barrier, _) = watch::channel(false);
        self.passes.insert(
            agent.to_owned(),
            Pass {
                task: task.to_owned(),
                barrier,
                committed: false,
                interrupted_streaming,
                inherited,
                suppress,
                generation,
            },
        );
        self.tidy(agent);
        Ok(())
    }

    /// The pass appended its entry.
    pub fn mark_committed(&mut self, agent: &str) {
        if let Some(pass) = self.passes.get_mut(agent) {
            pass.committed = true;
        }
    }

    /// Ends `task`'s pass: releases ownership, resolves the barrier, then settles the debt.
    /// A resume is earned by this pass's own interrupted turn after a commit or a typed
    /// no-op, or by inherited debt; it waits for the claims when any are outstanding.
    pub fn end(
        &mut self,
        agent: &str,
        task: &str,
        rejected_noop: bool,
        auto_continue: bool,
    ) -> Option<Resume> {
        if self.passes.get(agent).is_none_or(|pass| pass.task != task) {
            return None;
        }
        let pass = self.passes.remove(agent)?;
        let _ = pass.barrier.send(true);
        let eligible =
            (pass.interrupted_streaming || pass.inherited) && !pass.suppress && auto_continue;
        let earned = pass.committed || rejected_noop || pass.inherited;
        if !(eligible && earned) {
            self.tidy(agent);
            return None;
        }
        let debt = self.debts.entry(agent.to_owned()).or_default();
        if debt.claims > 0 {
            debt.owed = Some(pass.generation);
            return None;
        }
        Some(Resume {
            agent: agent.to_owned(),
            generation: pass.generation,
        })
    }

    /// A prompt arrives while a pass cleans up: join its barrier so the resume decision
    /// waits for this caller. Without a pass, only an existing claim can be joined.
    pub fn join(&mut self, agent: &str) -> Option<Joined> {
        if let Some(pass) = self.passes.get(agent) {
            let barrier = pass.barrier.subscribe();
            self.debts.entry(agent.to_owned()).or_default().claims += 1;
            return Some(Joined {
                barrier: Some(barrier),
            });
        }
        self.claim_pending(agent)
            .then_some(Joined { barrier: None })
    }

    /// Adds a claim only while one is already outstanding.
    pub fn claim_pending(&mut self, agent: &str) -> bool {
        match self.debts.get_mut(agent) {
            Some(debt) if debt.claims > 0 => {
                debt.claims += 1;
                true
            }
            _ => false,
        }
    }

    /// Gives a claim back. `dispatched` means a real turn owns the session (or the prompt
    /// was queued behind one), which settles the debt for good; otherwise the last claim to
    /// leave takes the owed resume.
    pub fn release(&mut self, agent: &str, dispatched: bool) -> Option<Resume> {
        let debt = self.debts.get_mut(agent)?;
        debt.claims = debt.claims.saturating_sub(1);
        let resume = if dispatched {
            debt.owed = None;
            None
        } else if debt.claims == 0 {
            debt.owed.take()
        } else {
            None
        };
        self.tidy(agent);
        resume.map(|generation| Resume {
            agent: agent.to_owned(),
            generation,
        })
    }

    /// An actual turn started: nothing is owed any more, whatever the claim count.
    pub fn note_turn_started(&mut self, agent: &str) {
        if let Some(debt) = self.debts.get_mut(agent) {
            debt.owed = None;
        }
        self.bump(agent);
        self.tidy(agent);
    }

    pub fn generation(&self, agent: &str) -> u64 {
        self.generations.get(agent).copied().unwrap_or(0)
    }

    pub fn note_stream_started(&mut self, agent: &str, turn: &str) {
        self.streaming.insert(agent.to_owned(), turn.to_owned());
    }

    /// Whether `turn` ever opened a provider stream; prompt setup alone does not count.
    pub fn was_streaming(&self, agent: &str, turn: &str) -> bool {
        self.streaming.get(agent).is_some_and(|t| t == turn)
    }

    /// The barrier of the pass in flight, for callers that only wait.
    pub fn barrier(&self, agent: &str) -> Option<watch::Receiver<bool>> {
        self.passes.get(agent).map(|pass| pass.barrier.subscribe())
    }

    fn bump(&mut self, agent: &str) -> u64 {
        let generation = self.generations.entry(agent.to_owned()).or_default();
        *generation = generation.wrapping_add(1);
        *generation
    }

    fn tidy(&mut self, agent: &str) {
        if self
            .debts
            .get(agent)
            .is_some_and(|debt| debt.claims == 0 && debt.owed.is_none())
        {
            self.debts.remove(agent);
        }
    }
}
