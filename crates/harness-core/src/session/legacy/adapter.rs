use std::collections::{BTreeMap, BTreeSet};

use super::facts::{
    LegacyFact, LegacyFactKind, ProviderFinishFact, ProviderStartFact, ToolFinishFact,
};
use super::{LegacyAdapterError, LegacyEventLogAdapter, LegacySessionSnapshot, LegacyWarning};
use crate::event::{EventEnvelopeV1, EventV1, SCHEMA_VERSION};
use crate::ids::RunId;
use crate::session::{
    AssistantPart, AssistantToolCall, RunStatus, SessionStatus, ToolResultStatus,
};

mod classify;
mod relationships;
mod validation;

use validation::validate_envelopes;

impl LegacyEventLogAdapter {
    pub fn project(
        &self,
        events: &[EventEnvelopeV1],
    ) -> Result<LegacySessionSnapshot, LegacyAdapterError> {
        let run_id = validate_envelopes(events)?;
        let user_request_ids = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::UserMessageSubmitted(payload) => {
                    Some(payload.request_id.as_str().to_string())
                }
                _ => None,
            })
            .collect();
        let mut boundary = LegacyBoundary::new(user_request_ids);
        let facts = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                boundary.classify(event).map(|mut fact| {
                    if is_intermediate_terminal(events, index) {
                        fact.kind = LegacyFactKind::Noop;
                    }
                    fact
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        super::projection::project_facts(run_id, &facts, boundary.warnings, true)
    }

    pub(crate) fn project_owner(
        &self,
        events: &[EventEnvelopeV1],
        agent_id: &str,
    ) -> Result<LegacySessionSnapshot, LegacyAdapterError> {
        validate_envelopes(events)?;
        self.project_owner_validated(events, agent_id)
    }

    pub(crate) fn validate(&self, events: &[EventEnvelopeV1]) -> Result<(), LegacyAdapterError> {
        validate_envelopes(events).map(|_| ())
    }

    pub(crate) fn project_owner_validated(
        &self,
        events: &[EventEnvelopeV1],
        agent_id: &str,
    ) -> Result<LegacySessionSnapshot, LegacyAdapterError> {
        let run_id = events
            .first()
            .map(|event| event.run_id.clone())
            .ok_or(LegacyAdapterError::EmptyInput)?;
        let ownership = LegacyOwnership::from_events(events);
        let user_request_ids = events
            .iter()
            .filter(|event| ownership.event_belongs_to(event, agent_id))
            .filter_map(|event| match &event.payload {
                EventV1::UserMessageSubmitted(payload) => {
                    Some(payload.request_id.as_str().to_string())
                }
                _ => None,
            })
            .collect();
        let mut boundary = LegacyBoundary::new(user_request_ids);
        let facts = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                if ownership.event_belongs_to(event, agent_id) {
                    boundary.classify(event).map(|mut fact| {
                        if is_intermediate_terminal(events, index) {
                            fact.kind = LegacyFactKind::Noop;
                        }
                        fact
                    })
                } else {
                    Ok(LegacyBoundary::fact(event, LegacyFactKind::Noop))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        super::projection::project_facts(run_id, &facts, boundary.warnings, false)
    }
}

fn is_intermediate_terminal(events: &[EventEnvelopeV1], index: usize) -> bool {
    matches!(
        events[index].payload,
        EventV1::RunFinished(_) | EventV1::RunFailed(_)
    ) && events[index.saturating_add(1)..]
        .iter()
        .any(|event| matches!(event.payload, EventV1::RunStarted(_)))
}

#[derive(Default)]
struct LegacyOwnership {
    request_owner: BTreeMap<String, String>,
    tool_owner: BTreeMap<String, String>,
}

impl LegacyOwnership {
    fn from_events(events: &[EventEnvelopeV1]) -> Self {
        let mut ownership = Self::default();
        for event in events {
            match &event.payload {
                EventV1::ProviderRequestStarted(payload) => {
                    let Some(agent_id) = event.actor.agent_id.as_deref() else {
                        continue;
                    };
                    ownership
                        .request_owner
                        .insert(payload.request_id.to_string(), agent_id.to_string());
                    if let Some(correlation_id) = event.correlation_id.as_ref() {
                        ownership
                            .request_owner
                            .insert(correlation_id.clone(), agent_id.to_string());
                    }
                }
                EventV1::ToolCallRequested(payload) => {
                    let owner = event
                        .correlation_id
                        .as_ref()
                        .and_then(|correlation| ownership.request_owner.get(correlation))
                        .cloned()
                        .or_else(|| event.actor.agent_id.clone());
                    if let Some(owner) = owner {
                        ownership
                            .tool_owner
                            .insert(payload.tool_call_id.to_string(), owner);
                    }
                }
                _ => {}
            }
        }
        ownership
    }

    fn event_belongs_to(&self, event: &EventEnvelopeV1, agent_id: &str) -> bool {
        match &event.payload {
            EventV1::RunStarted(_)
            | EventV1::SessionTitleUpdated(_)
            | EventV1::RunFinished(_)
            | EventV1::RunFailed(_) => true,
            EventV1::UserMessageSubmitted(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::PromptAttachmentsSubmitted(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ToolCallStarted(payload) => self
                .tool_owner
                .get(payload.tool_call_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ToolCallFinished(payload) => self
                .tool_owner
                .get(payload.tool_call_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::SessionCompaction(payload) => payload.agent_id == agent_id,
            EventV1::ProviderRequestStarted(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ProviderStreamDelta(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ProviderReasoningDelta(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ProviderRequestFinished(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::AssistantMessageFinished(payload) => self
                .request_owner
                .get(payload.request_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::ToolCallRequested(payload) => self
                .tool_owner
                .get(payload.tool_call_id.as_str())
                .is_some_and(|owner| owner == agent_id),
            EventV1::UiIntentReceived(_)
            | EventV1::BranchSummary(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskCancelled(_) => event.actor.agent_id.as_deref() == Some(agent_id),
            _ => false,
        }
    }
}
#[derive(Debug, Clone)]
struct ProviderRelationship {
    turn_key: String,
    event_correlation: Option<String>,
    provider_call_id: Option<String>,
    finished: bool,
    stop_reason: Option<String>,
    assistant_finished: bool,
}

#[derive(Debug, Clone)]
struct ToolRelationship {
    request_id: String,
    turn_key: String,
    finished: bool,
}

struct LegacyBoundary {
    users: BTreeSet<String>,
    represented_user_turns: BTreeSet<String>,
    seen_users: BTreeSet<String>,
    providers: BTreeMap<String, ProviderRelationship>,
    latest_provider_by_turn: BTreeMap<String, String>,
    latest_provider_request_id: Option<String>,
    active_agent_turn_by_agent: BTreeMap<String, (String, String)>,
    agent_turn_agent_by_task: BTreeMap<String, String>,
    tools: BTreeMap<String, ToolRelationship>,
    ambiguous_tools: BTreeSet<String>,
    warnings: Vec<LegacyWarning>,
    run_started: bool,
    terminal: bool,
    current_intent_by_agent: BTreeMap<String, crate::event::UiIntentReceivedEvent>,
}

impl LegacyBoundary {
    fn new(users: BTreeSet<String>) -> Self {
        Self {
            represented_user_turns: users.clone(),
            users,
            seen_users: BTreeSet::new(),
            providers: BTreeMap::new(),
            latest_provider_by_turn: BTreeMap::new(),
            latest_provider_request_id: None,
            active_agent_turn_by_agent: BTreeMap::new(),
            agent_turn_agent_by_task: BTreeMap::new(),
            tools: BTreeMap::new(),
            ambiguous_tools: BTreeSet::new(),
            warnings: vec![LegacyWarning::InferredSessionIdentity],
            run_started: false,
            terminal: false,
            current_intent_by_agent: BTreeMap::new(),
        }
    }

    fn fact(event: &EventEnvelopeV1, kind: LegacyFactKind) -> LegacyFact {
        LegacyFact {
            sequence: event.seq,
            event_id: event.event_id.clone(),
            kind,
        }
    }

    fn invalid(event: &EventEnvelopeV1) -> LegacyAdapterError {
        LegacyAdapterError::InvalidIdentityRelationship {
            event_id: event.event_id.clone(),
        }
    }

    fn non_empty(value: &str) -> Option<&str> {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    fn correlation<'a>(event: &'a EventEnvelopeV1) -> Option<&'a str> {
        event.correlation_id.as_deref().and_then(Self::non_empty)
    }

    fn provider_relationship(
        &self,
        event: &EventEnvelopeV1,
        request_id: &str,
    ) -> Result<&ProviderRelationship, LegacyAdapterError> {
        let Some(relationship) = self.providers.get(request_id) else {
            return Err(Self::invalid(event));
        };
        if Self::correlation(event).is_some_and(|correlation| {
            correlation
                != relationship
                    .event_correlation
                    .as_deref()
                    .unwrap_or(&relationship.turn_key)
        }) {
            return Err(Self::invalid(event));
        }
        Ok(relationship)
    }

    fn unsupported(&mut self, event: &EventEnvelopeV1) -> LegacyFact {
        self.warnings.push(LegacyWarning::UnsupportedLegacyVariant {
            event_id: event.event_id.clone(),
        });
        Self::fact(event, LegacyFactKind::Noop)
    }
}
