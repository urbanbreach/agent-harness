//! Direct idle compaction waits while its agent still owns work that will wake it.
use crate::coord::{
    runtime::{JobKind, Runtime},
    tools::PendingWork,
};

impl Runtime {
    /// Whether `agent` still owns work whose arrival will start or change a turn: a running
    /// turn, a queued turn, a running job it owns (command, tool, branch summary), a busy or
    /// queued child, a tool call waiting on approval, guidance or steering not yet delivered,
    /// an unobserved command notice, or output of a monitor it owns that waits for delivery.
    /// Other agents' jobs and monitors do not count.
    pub(in crate::coord) fn owns_pending_work(&self, agent: &str) -> bool {
        let Some(state) = self.agents.get(agent) else {
            return false;
        };
        state.busy
            || !state.queue.is_empty()
            || self.running.values().any(|job| {
                job.actor.agent_id.as_deref() == Some(agent)
                    || matches!(&job.kind, JobKind::Turn { agent: owner } if owner == agent)
            })
            || self.agents.values().any(|child| {
                child.info.parent_agent_id.as_deref() == Some(agent)
                    && (child.busy || !child.queue.is_empty())
            })
            || self.pending.values().any(|pending| {
                matches!(
                    &pending.work,
                    PendingWork::Tool(work)
                        if work.context.actor.agent_id.as_deref() == Some(agent)
                )
            })
            || self
                .pending_reminders
                .get(agent)
                .is_some_and(|reminders| !reminders.is_empty())
            || self
                .steering
                .get(agent)
                .is_some_and(|steering| !steering.is_empty())
            || self
                .command_notices
                .get(agent)
                .is_some_and(|backlog| !backlog.pending.is_empty())
            || self.monitor_pending_for(agent)
    }
}
