use crate::context_budget::RequestBudgetSnapshot;
use crate::event::{EventEnvelopeV1, EventV1};

#[derive(Debug, Clone, Copy)]
pub(super) struct CompactionBudget(Option<RequestBudgetSnapshot>);

impl CompactionBudget {
    pub(super) fn resolve(
        prepared: Option<RequestBudgetSnapshot>,
        events: &[EventEnvelopeV1],
        agent_id: &str,
    ) -> Self {
        Self(prepared.or_else(|| {
            events.iter().rev().find_map(|event| {
                if event.actor.agent_id.as_deref() != Some(agent_id) {
                    return None;
                }
                match &event.payload {
                    EventV1::ProviderRequestStarted(started) => started
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.context_budget),
                    _ => None,
                }
            })
        }))
    }

    pub(super) fn requires_compaction(self) -> bool {
        self.0.and_then(|snapshot| snapshot.requires_compaction) == Some(true)
    }

    pub(super) fn history_allowance(self, keep_recent_tokens: u32) -> u32 {
        let Some(snapshot) = self.0 else {
            return keep_recent_tokens;
        };
        let Some(threshold) = snapshot.compaction_threshold_tokens else {
            return keep_recent_tokens;
        };
        let components = snapshot.components;
        let non_history_tokens = [
            components.system_tokens,
            components.tools_tokens,
            components.attachments_tokens,
            components.framing_tokens,
            components.pending_prompt_tokens,
        ]
        .into_iter()
        .fold(0_u32, u32::saturating_add);
        keep_recent_tokens.min(threshold.saturating_sub(non_history_tokens))
    }
}
