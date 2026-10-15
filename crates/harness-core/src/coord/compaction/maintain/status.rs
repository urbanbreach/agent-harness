//! The footer's view of automatic compaction: threshold, speculation lead and the state of
//! the background summary, published only when one of them changes.
use super::*;
use crate::event::{CompactionStatusEvent, SpeculationState};

impl Runtime {
    fn speculation_state(&self, agent: &str) -> SpeculationState {
        match self.speculation.get(agent) {
            None => SpeculationState::Idle,
            Some(slot) if slot.is_running() => SpeculationState::Running,
            Some(_) => SpeculationState::Armed,
        }
    }

    /// Stores `status` and publishes it when it differs from the last one for its agent.
    fn set_compaction_status(
        &mut self,
        status: CompactionStatusEvent,
    ) -> Result<(), CoordinatorError> {
        if self.compaction_status.get(&status.agent_id) == Some(&status) {
            return Ok(());
        }
        self.compaction_status
            .insert(status.agent_id.clone(), status.clone());
        let actor = EventActor::new(ActorKind::Worker, Some(status.agent_id.clone()));
        self.live(actor, String::new(), LiveEventV1::CompactionStatus(status))
    }

    /// The worker's resolution of threshold and lead, with the live state of the slot.
    pub(in crate::coord) fn publish_compaction_status(
        &mut self,
        agent: &str,
        auto: bool,
        threshold_tokens: Option<u64>,
        speculation_tokens: Option<u64>,
    ) -> Result<(), CoordinatorError> {
        let speculation = self.speculation_state(agent);
        self.set_compaction_status(CompactionStatusEvent {
            agent_id: agent.to_owned(),
            auto,
            threshold_tokens,
            speculation_tokens,
            speculation,
        })
    }

    /// The slot changed: republish with the new speculation state. An agent whose worker has
    /// not published its thresholds yet has nothing to show.
    pub(in crate::coord) fn refresh_speculation_status(&mut self, agent: &str) {
        let Some(mut status) = self.compaction_status.get(agent).cloned() else {
            return;
        };
        status.speculation = self.speculation_state(agent);
        let _ = self.set_compaction_status(status);
    }
}

impl Worker {
    /// Publishes the status when this turn's model differs from the one last published: a
    /// turn start, a fallback switch, a promotion or its restoration.
    pub(in crate::coord) async fn sync_compaction_status(&mut self, context: &Context) {
        if self.guidance.status_model == self.turn.model {
            return;
        }
        if self.publish_compaction_status_for(context).await.is_ok() {
            self.guidance.status_model.clone_from(&self.turn.model);
        }
    }

    /// Publishes the status for this worker's agent and model, resolved by the same code the
    /// threshold and speculation paths use.
    pub(in crate::coord) async fn publish_compaction_status_for(
        &self,
        context: &Context,
    ) -> Result<(), CoordinatorError> {
        let auto = self.auto_compaction_available();
        let measure = self.measure(context)?;
        let threshold = (auto && measure.window > 0).then_some(measure.threshold);
        let speculation = match threshold {
            Some(threshold) if self.speculative_method_when(false).await?.is_some() => {
                Some(threshold.saturating_sub(policy::speculation_lead(threshold)))
            }
            _ => None,
        };
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let (threshold, speculation) = (threshold.map(u64::from), speculation.map(u64::from));
        self.handle
            .call(move |s| s.publish_compaction_status(&agent, auto, threshold, speculation))
            .await
    }
}
