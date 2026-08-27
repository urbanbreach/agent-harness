use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::agent::ProviderContext;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::RunId;
use crate::session::legacy::{recover_event_history, LegacyWarning};
use crate::session::{
    CanonicalProviderView, CanonicalSessionProjection, ProviderViewInput, ProviderViewOwner,
};
use crate::session_paths::EVENTS_FILE_NAME;

use super::super::CoordinatorError;

mod lower;
mod selection;

pub(in crate::coord) struct RecoveredProviderContext {
    pub(in crate::coord) view: CanonicalProviderView,
    pub(in crate::coord) context: ProviderContext,
}

pub(in crate::coord) struct CanonicalProviderRecovery {
    pub(in crate::coord) by_agent: BTreeMap<String, RecoveredProviderContext>,
    pub(in crate::coord) warnings: Vec<LegacyWarning>,
}

impl CanonicalProviderRecovery {
    fn into_contexts(self) -> BTreeMap<String, ProviderContext> {
        self.by_agent
            .into_iter()
            .map(|(agent_id, recovered)| (agent_id, recovered.context))
            .collect()
    }
}

pub(in crate::coord) fn restore_provider_context_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<BTreeMap<String, ProviderContext>, CoordinatorError> {
    recover_canonical_provider_context_from_history(session_dir, run_id)
        .map(CanonicalProviderRecovery::into_contexts)
}

pub(in crate::coord) fn recover_canonical_provider_context_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<CanonicalProviderRecovery, CoordinatorError> {
    recover_canonical_provider_context_from_history_with_fallbacks(
        session_dir,
        run_id,
        &BTreeMap::new(),
    )
}

pub(in crate::coord) fn recover_canonical_provider_context_from_history_with_fallbacks(
    session_dir: &Path,
    run_id: &str,
    runtime_fallbacks: &BTreeMap<String, crate::session::CanonicalRuntimeSelection>,
) -> Result<CanonicalProviderRecovery, CoordinatorError> {
    let events_path = session_dir.join(run_id).join(EVENTS_FILE_NAME);
    let recovery = recover_event_history(&events_path, &RunId::new(run_id)).map_err(|error| {
        CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: error.to_string(),
        }
    })?;
    let (events, warnings) = recovery.into_parts();
    recover_canonical_provider_context_from_events(&events, warnings, run_id, runtime_fallbacks)
}

pub(in crate::coord) fn recover_canonical_provider_context_from_events(
    events: &[EventEnvelopeV1],
    mut warnings: Vec<LegacyWarning>,
    run_id: &str,
    runtime_fallbacks: &BTreeMap<String, crate::session::CanonicalRuntimeSelection>,
) -> Result<CanonicalProviderRecovery, CoordinatorError> {
    let owners = provider_context_owners(events);
    let root_projection =
        CanonicalSessionProjection::from_event_history(events).map_err(|error| {
            CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: error.to_string(),
            }
        })?;
    extend_unique_warnings(&mut warnings, root_projection.compatibility_warnings);
    let mut by_agent = BTreeMap::new();
    for agent_id in owners {
        let stream_key = format!("agent:{agent_id}");
        let owner_events = events
            .iter()
            .filter(|event| super::event_belongs_to_agent(event, &agent_id, &stream_key))
            .cloned()
            .collect::<Vec<_>>();
        let projection =
            CanonicalSessionProjection::from_owner_event_history(events, &owner_events, &agent_id)
                .map_err(|error| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.to_string(),
                    reason: error.to_string(),
                })?;
        extend_unique_warnings(&mut warnings, projection.compatibility_warnings);
        if !lower::has_recoverable_provider_context(&projection.session) {
            continue;
        }
        let runtime_selection = selection::from_session(&projection.session)
            .or_else(|| {
                selection::from_completed_request(
                    events,
                    &agent_id,
                    runtime_fallbacks.get(&agent_id),
                )
            })
            .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "canonical provider view for agent `{agent_id}` has no provider selection"
                ),
            })?;
        let view = projection
            .session
            .provider_view(ProviderViewInput {
                owner: ProviderViewOwner::root(
                    agent_id.clone(),
                    projection.session.session_id().clone(),
                ),
                selected_leaf: projection.session.active_leaf().cloned(),
                pending_prompt: None,
                runtime_selection,
            })
            .map_err(|error| CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: error.to_string(),
            })?;
        let context =
            lower::provider_context(view.clone(), events, &agent_id).map_err(|reason| {
                CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.to_string(),
                    reason,
                }
            })?;
        by_agent.insert(agent_id, RecoveredProviderContext { view, context });
    }
    Ok(CanonicalProviderRecovery { by_agent, warnings })
}

fn extend_unique_warnings(target: &mut Vec<LegacyWarning>, warnings: Vec<LegacyWarning>) {
    for warning in warnings {
        if !target.contains(&warning) {
            target.push(warning);
        }
    }
}

fn provider_context_owners(events: &[EventEnvelopeV1]) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(_) => event
                .actor
                .agent_id
                .as_ref()
                .filter(|agent_id| agent_id.as_str() != super::super::COORDINATOR_AGENT_ID)
                .cloned(),
            EventV1::SessionCompaction(compaction) => Some(compaction.agent_id.clone()),
            _ => None,
        })
        .collect()
}
