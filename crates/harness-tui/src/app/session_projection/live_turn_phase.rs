use super::*;

/// Current work, independent of text retained from earlier samples in the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveTurnPhase {
    Waiting,
    Retrying(u32),
    Thinking,
    Responding,
    WritingToolCall {
        tool_call_id: String,
        ordinal: usize,
    },
    ToolRunning(String),
}

pub(super) struct ProviderPhase {
    request_id: String,
    phase: LiveTurnPhase,
    started_mono_ms: u64,
}

impl SessionProjection {
    pub(crate) fn live_turn_phase(&self, activity: &ActivityEntry) -> (LiveTurnPhase, u64) {
        let provider = self.provider_phases.get(&activity.request_id);
        let phase = provider.map_or(LiveTurnPhase::Waiting, |state| state.phase.clone());
        let started = provider.map_or_else(
            || {
                activity
                    .request_started_mono_ms
                    .unwrap_or(activity.first_mono_ms)
            },
            |state| state.started_mono_ms,
        );
        let running = activity
            .tool_calls
            .iter()
            .rev()
            .find(|tool| tool.status == ToolCallDisplayStatus::Running);
        // Match Grok's tracker: retry, blocking waits, writing/thinking, tools, response.
        if matches!(phase, LiveTurnPhase::Retrying(_)) {
            return (phase, started);
        }
        if let Some(tool) = running.filter(|tool| {
            matches!(
                tool.effective_tool_id(),
                "question" | "user.question" | "task" | "agent.spawn" | "background_output"
            )
        }) {
            return (
                LiveTurnPhase::ToolRunning(tool.tool_call_id.clone()),
                tool.first_mono_ms,
            );
        }
        if matches!(
            phase,
            LiveTurnPhase::Thinking | LiveTurnPhase::WritingToolCall { .. }
        ) {
            return (phase, started);
        }
        if let Some(tool) = running {
            return (
                LiveTurnPhase::ToolRunning(tool.tool_call_id.clone()),
                tool.first_mono_ms,
            );
        }
        let started = if phase == LiveTurnPhase::Waiting {
            // Waiting begins when the last running tool finishes, even before the next request.
            activity
                .tool_calls
                .iter()
                .map(|tool| tool.last_mono_ms)
                .max()
                .unwrap_or(0)
                .max(started)
        } else {
            started
        };
        (phase, started)
    }

    fn set_provider_phase(
        &mut self,
        turn_id: &str,
        request_id: &str,
        phase: LiveTurnPhase,
        mono_ms: u64,
    ) {
        let state = self
            .provider_phases
            .entry(turn_id.to_string())
            .or_insert_with(|| ProviderPhase {
                request_id: request_id.to_string(),
                phase: phase.clone(),
                started_mono_ms: mono_ms,
            });
        if state.request_id != request_id || state.phase != phase {
            state.request_id = request_id.to_string();
            state.phase = phase;
            state.started_mono_ms = mono_ms;
        }
    }

    pub(super) fn update_phase_for_event(&mut self, event: &EventEnvelopeV1) {
        let (request_id, phase) =
            if let Some(fragment) = canonical_provider_fragment_for_event(event) {
                if fragment.delta.is_empty() {
                    return;
                }
                (
                    fragment.request_id,
                    match fragment.kind {
                        CanonicalProviderFragmentKind::Reasoning => LiveTurnPhase::Thinking,
                        CanonicalProviderFragmentKind::Text => LiveTurnPhase::Responding,
                    },
                )
            } else {
                match &event.payload {
                    EventV1::ProviderRequestStarted(data) => (
                        data.request_id.as_str(),
                        data.metadata
                            .as_ref()
                            .and_then(|metadata| metadata.retry)
                            .filter(|retry| retry.attempt > 0)
                            .map_or(LiveTurnPhase::Waiting, |retry| {
                                LiveTurnPhase::Retrying(retry.attempt)
                            }),
                    ),
                    EventV1::ProviderRequestFinished(data) => {
                        (data.request_id.as_str(), LiveTurnPhase::Waiting)
                    }
                    EventV1::ToolCallRequested(_) => {
                        let turn_id = event.correlation_id.clone().or_else(|| {
                            self.activities
                                .back()
                                .map(|activity| activity.request_id.clone())
                        });
                        let Some(turn_id) = turn_id else {
                            return;
                        };
                        let request_id = self
                            .provider_phases
                            .get(&turn_id)
                            .map_or(turn_id.as_str(), |state| state.request_id.as_str())
                            .to_string();
                        self.set_provider_phase(
                            &turn_id,
                            &request_id,
                            LiveTurnPhase::Waiting,
                            event.mono_ms,
                        );
                        return;
                    }
                    _ => return,
                }
            };
        let turn_id = Self::canonical_provider_turn_id(event, request_id);
        self.set_provider_phase(turn_id, request_id, phase, event.mono_ms);
    }

    pub(super) fn update_phase_for_live_fragment(
        &mut self,
        event: &LiveEventEnvelope,
        index: usize,
    ) {
        let activity = &self.activities[index];
        let (request_id, delta, phase) = match &event.payload {
            LiveEventV1::ProviderReasoningDelta { request_id, delta } => {
                (request_id, delta, LiveTurnPhase::Thinking)
            }
            LiveEventV1::ProviderTextDelta { request_id, delta } => {
                (request_id, delta, LiveTurnPhase::Responding)
            }
            LiveEventV1::ProviderToolInputDelta {
                request_id,
                tool_call_id,
                delta,
            } => {
                let seen = self
                    .transient_assistants
                    .get(request_id.as_str())
                    .map(|state| &state.tool_call_ids);
                let ordinal = seen.map_or(1, |ids| {
                    activity
                        .tool_calls
                        .iter()
                        .filter(|tool| ids.contains(&tool.tool_call_id))
                        .position(|tool| tool.tool_call_id == tool_call_id.as_str())
                        .unwrap_or(ids.len())
                        + 1
                });
                (
                    request_id,
                    delta,
                    LiveTurnPhase::WritingToolCall {
                        tool_call_id: tool_call_id.to_string(),
                        ordinal,
                    },
                )
            }
        };
        if delta.is_empty() {
            return;
        }
        let turn_id = activity.request_id.clone();
        self.set_provider_phase(&turn_id, request_id.as_str(), phase, event.mono_ms);
    }
}
