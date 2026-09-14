use super::super::*;

impl Coordinator {
    pub(in crate::coord) async fn compaction_generated_internal(
        &mut self,
        agent_id: String,
        generation: CompactionGenerationToken,
        result: Box<Result<GeneratedSessionCompaction, CoordinatorError>>,
    ) {
        let current_events = match self.run_state.as_ref() {
            Some(run_state) => match super::preparation::collect_events(run_state).await {
                Ok(events) => events,
                Err(error) => {
                    let Some(run_state) = self.run_state.as_mut() else {
                        return;
                    };
                    let Some(pending) = run_state.pending_compactions.get(&agent_id) else {
                        return;
                    };
                    if pending.generation != generation || pending.agent_id != agent_id {
                        return;
                    }
                    let Some(pending) = run_state.pending_compactions.remove(&agent_id) else {
                        return;
                    };
                    pending.cancellation_token.cancel();
                    pending.response.finish(Err(error));
                    return;
                }
            },
            None => return,
        };
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };
        let Some(pending) = run_state.pending_compactions.get(&agent_id) else {
            return;
        };
        if pending.generation != generation || pending.agent_id != agent_id {
            return;
        }
        let Some(pending) = run_state.pending_compactions.remove(&agent_id) else {
            return;
        };
        if pending.cancellation_token.is_cancelled() {
            pending
                .response
                .finish(Err(CoordinatorError::CompactionCancelled {
                    agent_id,
                    reason: "generation cancelled".to_string(),
                }));
            return;
        }
        let durable_agent_tail_seq =
            super::super::provider_context::latest_agent_event_seq(&current_events, &agent_id);
        if !pending.allow_appended && !pending.base.is_current(run_state, durable_agent_tail_seq) {
            pending
                .response
                .finish(Err(CoordinatorError::CompactionStale {
                    agent_id: agent_id.clone(),
                }));
            return;
        }

        let completion = match *result {
            Ok(generated) if generated.agent_id() != agent_id => {
                Err(CoordinatorError::CompactionStale {
                    agent_id: agent_id.clone(),
                })
            }
            Ok(mut generated) => {
                if pending.allow_appended {
                    if let Err(error) = generated.rebase(
                        &current_events,
                        pending.request_budget,
                        &crate::agent::AgentModelRef::parse(
                            &super::preparation::determine_model_ref(run_state, &agent_id),
                        ),
                    ) {
                        pending.response.finish(Err(error));
                        return;
                    }
                }
                if pending.background {
                    run_state.compaction_state.entry(agent_id).or_default().warm = Some(generated);
                    return;
                }
                generated.refresh_committed_events(current_events);
                match generated.commit(self.clock.as_ref(), self.redactor.as_ref(), run_state) {
                    Ok(applied) => {
                        let context = run_state
                            .provider_context_by_agent
                            .get(&agent_id)
                            .cloned()
                            .unwrap_or_default();
                        match run_state.cached_canonical_provider_view(&agent_id).cloned() {
                            Some(view) => Ok(CompactAgentContextResult::Compacted {
                                context,
                                view: Box::new(view),
                                applied,
                            }),
                            None => Err(CoordinatorError::CompactionFailed(format!(
                                "canonical provider view omitted compacted agent `{agent_id}`"
                            ))),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        if !matches!(
            &completion,
            Err(CoordinatorError::CompactionCancelled { .. }
                | CoordinatorError::CompactionStale { .. })
        ) {
            run_state
                .compaction_state
                .entry(agent_id)
                .or_default()
                .record(completion.is_ok(), self.clock.mono_ms());
        }
        pending.response.finish(completion);
    }
}
