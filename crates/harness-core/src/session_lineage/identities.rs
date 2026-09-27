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
    event: EventEnvelopeV1,
    children: &BTreeMap<String, String>,
    source: Option<&str>,
    fork: &str,
) -> Result<EventEnvelopeV1, SessionLineageError> {
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
