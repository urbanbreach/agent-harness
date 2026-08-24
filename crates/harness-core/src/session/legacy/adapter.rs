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
            .map(|event| boundary.classify(event))
            .collect::<Result<Vec<_>, _>>()?;
        super::projection::project_facts(run_id, &facts, boundary.warnings)
    }
}
#[derive(Debug, Clone)]
struct ProviderRelationship {
    turn_key: String,
    provider_call_id: Option<String>,
    finished: bool,
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
    seen_users: BTreeSet<String>,
    providers: BTreeMap<String, ProviderRelationship>,
    latest_provider_by_turn: BTreeMap<String, String>,
    tools: BTreeMap<String, ToolRelationship>,
    warnings: Vec<LegacyWarning>,
    run_started: bool,
    terminal: bool,
}

impl LegacyBoundary {
    fn new(users: BTreeSet<String>) -> Self {
        Self {
            users,
            seen_users: BTreeSet::new(),
            providers: BTreeMap::new(),
            latest_provider_by_turn: BTreeMap::new(),
            tools: BTreeMap::new(),
            warnings: vec![LegacyWarning::InferredSessionIdentity],
            run_started: false,
            terminal: false,
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
        if Self::correlation(event).is_some_and(|correlation| correlation != relationship.turn_key)
        {
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
