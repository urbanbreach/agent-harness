use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::agent::ProviderContext;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::RunId;
use crate::session::legacy::{recover_event_history, LegacyEventLogAdapter, LegacyWarning};
use crate::session::{CanonicalProviderView, ProviderViewInput, ProviderViewOwner};
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
    warnings: Vec<LegacyWarning>,
    run_id: &str,
    runtime_fallbacks: &BTreeMap<String, crate::session::CanonicalRuntimeSelection>,
) -> Result<CanonicalProviderRecovery, CoordinatorError> {
    let owners = provider_context_owners(&events);
    let adapter = LegacyEventLogAdapter::new();
    adapter
        .validate(&events)
        .map_err(|error| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: error.to_string(),
        })?;
    let by_agent = owners
        .into_iter()
        .map(|agent_id| {
            let snapshot = adapter
                .project_owner_validated(&events, &agent_id)
                .map_err(|error| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.to_string(),
                    reason: error.to_string(),
                })?;
            if !lower::has_recoverable_provider_context(&snapshot.session) {
                return Ok(None);
            }
            let runtime_selection = selection::from_session(&snapshot.session)
                .or_else(|| {
                    selection::from_completed_request(
                        &events,
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
            let view = snapshot
                .session
                .provider_view(ProviderViewInput {
                    owner: ProviderViewOwner::root(
                        agent_id.clone(),
                        snapshot.session.session_id().clone(),
                    ),
                    selected_leaf: snapshot.session.active_leaf().cloned(),
                    pending_prompt: None,
                    runtime_selection,
                })
                .map_err(|error| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.to_string(),
                    reason: error.to_string(),
                })?;
            lower::provider_context(view.clone(), &events, &agent_id)
                .map(|context| Some((agent_id, RecoveredProviderContext { view, context })))
                .map_err(|reason| CoordinatorError::ResumeRestoreFailed {
                    run_id: run_id.to_string(),
                    reason,
                })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(CanonicalProviderRecovery { by_agent, warnings })
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
