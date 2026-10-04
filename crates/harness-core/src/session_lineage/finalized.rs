//! An existing authorized branch copy adopts state into its destination namespace.
//! Source artifacts and all model-facing content remain immutable.
use super::*;
use crate::subagent::*;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) fn references(
    events: &[EventEnvelopeV1],
    source: &Path,
    destination: &Path,
    source_run: &str,
    projection_owner: Option<&str>,
    destination_run: &str,
    children: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, FinalizedAgentStateReferenceV1>, SessionLineageError> {
    let mut adopted = BTreeMap::new();
    for event in events {
        let reference = match &event.payload {
            EventV1::FinalizedAgentState(reference) => Some(reference),
            EventV1::SubagentTransition(transition) => transition.finalized_state.as_ref(),
            EventV1::AgentContextInitialized(initialized) => Some(&initialized.source),
            _ => None,
        };
        let Some(reference) = reference else { continue };
        let key = serde_json::to_string(reference)?;
        if adopted.contains_key(&key) {
            continue;
        }
        if reference.owner_run_id != source_run
            && Some(reference.owner_run_id.as_str()) != projection_owner
        {
            return Err(SessionLineageError::Invalid(
                "finalized state belongs to another source run".into(),
            ));
        }
        let mut next = reference.clone();
        next.owner_run_id = destination_run.into();
        next.state.owner.0 = children
            .get(&reference.state.owner.0)
            .cloned()
            .unwrap_or_else(|| reference.state.owner.0.clone());
        next.owner_session_id = if reference.owner_session_id == source_run
            || Some(reference.owner_session_id.as_str()) == projection_owner
        {
            destination_run.into()
        } else {
            children
                .get(&reference.owner_session_id)
                .cloned()
                .unwrap_or_else(|| reference.owner_session_id.clone())
        };
        if !reference.state.sha256.is_empty() {
            let mut state = crate::subagent::read_finalized_payload(
                source,
                reference,
                &reference.owner_run_id,
                &reference.state.owner,
                &reference.attempt_id,
            )
            .map_err(|reason| {
                SessionLineageError::Invalid(format!(
                    "finalized source cannot be adopted: {reason:?}"
                ))
            })?;
            state.owner_run_id.clone_from(&next.owner_run_id);
            state.owner_agent_id = next.state.owner.clone();
            state.owner_session_id.clone_from(&next.owner_session_id);
            state.source_reference = Some(Box::new(reference.clone()));
            let bytes = serde_json::to_vec(&state)?;
            next.state.sha256 = hex::encode(Sha256::digest(&bytes));
            next.byte_length = bytes.len() as u64;
            if next.byte_length > MAX_FINALIZED_STATE_BYTES {
                return Err(SessionLineageError::Invalid(
                    "adopted finalized state exceeds its bound".into(),
                ));
            }
            crate::store::create_private_dir(&destination.join("artifacts"))?;
            crate::store::write_private_atomic(&destination.join(next.artifact_path()), &bytes)?;
        }
        adopted.insert(key, next);
    }
    Ok(adopted)
}
