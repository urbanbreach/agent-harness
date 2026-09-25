use crate::conversation_rewind::{rewind_points, ConversationRewoundEvent, RewindPoint};
use crate::event::EventV1;

use super::{append_payload_event_with_correlation, system_actor, Coordinator, CoordinatorError};

impl Coordinator {
    pub(in crate::coord) fn rewind_conversation_internal(
        &mut self,
        request_id: String,
    ) -> Result<RewindPoint, CoordinatorError> {
        let state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        if !state.running_agent_turns.is_empty()
            || !state.queued_agent_turns.is_empty()
            || !state.pending_compactions.is_empty()
            || !state.tasks.is_empty()
            || !state.queued_tool_calls.is_empty()
        {
            return Err(CoordinatorError::RevertFailed(
                "A turn is currently running.".into(),
            ));
        }
        let point = rewind_points(&state.canonical_event_history)
            .into_iter()
            .find(|point| point.request_id == request_id)
            .ok_or_else(|| CoordinatorError::RevertFailed("No undoable prompts".into()))?;
        // Validate recovery before committing the append-only rewind marker.
        let retained = &state.canonical_event_history[..state
            .canonical_event_history
            .partition_point(|event| event.seq < point.seq)];
        let recovery = super::provider_context::recover_canonical_provider_context_from_events(
            retained,
            Vec::new(),
            state.info.run_id.as_str(),
            &Default::default(),
        )?;
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            state,
            system_actor(),
            None,
            Some(request_id.clone()),
            EventV1::ConversationRewound(ConversationRewoundEvent {
                target_seq: point.seq,
                request_id,
            }),
        )?;
        state.provider_context_by_agent.clear();
        state.canonical_provider_view_by_agent.clear();
        state.provider_context_cache_key_by_agent.clear();
        state.live_incomplete_provider_turns_by_agent.clear();
        state.compaction_state.clear();
        state.advance_compaction_boundary();
        for recovered in recovery.by_agent.into_values() {
            state.install_canonical_provider_recovery(recovered.view, recovered.context);
        }
        Ok(point)
    }
}
