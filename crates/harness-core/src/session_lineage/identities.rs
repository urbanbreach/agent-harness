use super::*;
use serde_json::Value;

pub(super) fn child_ids(events: &[EventEnvelopeV1], fork: &str) -> BTreeMap<String, String> {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::AgentSpawned(e) if e.parent_agent_id.is_some() => Some((
                e.agent_id.clone(),
                format!(
                    "child-{}",
                    blake3::hash(format!("{fork}:{}", e.agent_id).as_bytes()).to_hex()
                ),
            )),
            _ => None,
        })
        .collect()
}

pub(super) fn remap(
    mut event: EventEnvelopeV1,
    children: &BTreeMap<String, String>,
    source: Option<&str>,
    projection_owner: Option<&str>,
    fork: &str,
    finalized: &BTreeMap<String, crate::subagent::FinalizedAgentStateReferenceV1>,
) -> Result<EventEnvelopeV1, SessionLineageError> {
    use crate::subagent::{SubagentAncestry, SubagentCommandRequest, SubagentExecutionOwner};
    let identity = |id: &str| {
        children
            .get(id)
            .cloned()
            .or_else(|| {
                (Some(id) == source || Some(id) == projection_owner).then(|| fork.to_owned())
            })
            .unwrap_or_else(|| id.to_owned())
    };
    let reference = |value: &mut crate::subagent::FinalizedAgentStateReferenceV1| -> Result<(), SessionLineageError> {
        *value = finalized.get(&serde_json::to_string(value)?).cloned()
            .ok_or_else(|| SessionLineageError::Invalid("finalized adoption reference is missing".into()))?;
        Ok(())
    };
    match &mut event.payload {
        EventV1::SubagentTransition(transition) => {
            transition.child_id.0 = children
                .get(&transition.child_id.0)
                .cloned()
                .unwrap_or_else(|| transition.child_id.0.clone());
            transition.metadata.ancestry = SubagentAncestry::new(
                transition
                    .metadata
                    .ancestry
                    .spawner()
                    .map(|id| crate::subagent::SubagentId(identity(&id.0))),
                transition
                    .metadata
                    .ancestry
                    .origin()
                    .map(|id| crate::subagent::SubagentId(identity(&id.0))),
            );
            match &mut transition.metadata.execution_owner {
                SubagentExecutionOwner::RootSession { session_id } => {
                    *session_id = identity(session_id)
                }
                SubagentExecutionOwner::ChildSession { child_id } => {
                    child_id.0 = identity(&child_id.0)
                }
            }
            let display = &mut transition.metadata.display_route;
            display.root_session_id = identity(&display.root_session_id);
            display.parent_session_id = display.parent_session_id.as_deref().map(identity);
            display.child_session_id = identity(&display.child_session_id);
            if let Some(value) = &mut transition.finalized_state {
                reference(value)?;
            }
        }
        EventV1::FinalizedAgentState(value) => reference(value)?,
        EventV1::AgentContextInitialized(initialized) => {
            initialized.agent_id.0 = children
                .get(&initialized.agent_id.0)
                .cloned()
                .unwrap_or_else(|| initialized.agent_id.0.clone());
            reference(&mut initialized.source)?;
        }
        EventV1::AgentExecutionContextChanged(changed) => {
            changed.agent_id.0 = children
                .get(&changed.agent_id.0)
                .cloned()
                .unwrap_or_else(|| changed.agent_id.0.clone())
        }
        EventV1::SubagentCancelRequested(intent) => {
            for target in &mut intent.targets {
                target.0 = children
                    .get(&target.0)
                    .cloned()
                    .unwrap_or_else(|| target.0.clone());
            }
            match &mut intent.command {
                SubagentCommandRequest::ExplicitChildKill { child_id } => {
                    child_id.0 = children
                        .get(&child_id.0)
                        .cloned()
                        .unwrap_or_else(|| child_id.0.clone())
                }
                SubagentCommandRequest::ChildSessionCancel {
                    session_id,
                    descendants,
                } => {
                    *session_id = identity(session_id);
                    for child in descendants {
                        child.0 = identity(&child.0);
                    }
                }
                SubagentCommandRequest::ParentSessionStop { session_id } => {
                    *session_id = identity(session_id)
                }
                _ => {}
            }
        }
        _ => {}
    }
    let mut value = serde_json::to_value(event)?;
    visit(&mut value, children, source, fork);
    Ok(serde_json::from_value(value)?)
}

fn visit(value: &mut Value, children: &BTreeMap<String, String>, source: Option<&str>, fork: &str) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                // Only identity fields change. Prompts, summaries, and argument digests stay historical.
                if let Value::String(id) = value {
                    let session = matches!(
                        key.as_str(),
                        "session_id" | "parent_session_id" | "child_session_id"
                    );
                    if !session
                        && !matches!(key.as_str(), "agent_id" | "parent_agent_id" | "task_id")
                    {
                        continue;
                    }
                    let mapped = children
                        .get(id)
                        .map(String::as_str)
                        .or_else(|| (session && Some(id.as_str()) == source).then_some(fork));
                    if let Some(mapped) = mapped {
                        *id = mapped.into();
                    }
                } else {
                    visit(value, children, source, fork);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                visit(value, children, source, fork);
            }
        }
        _ => {}
    }
}
