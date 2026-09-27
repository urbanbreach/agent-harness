use harness_core::{
    event::{EventEnvelopeV1, EventV1},
    proj::ResumePlan,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) fn resolve_session_run_dir(
    selector: &str,
    sessions: &Path,
    cwd: &Path,
) -> Result<PathBuf, String> {
    let path = Path::new(selector);
    let path = if path.is_absolute() || path.components().count() > 1 {
        cwd.join(path)
    } else {
        harness_core::store::validate_session_id(selector).map_err(|e| e.to_string())?;
        sessions.join(path)
    };
    if !std::fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("session path must be a directory, not a symlink".into());
    }
    harness_core::store::open_private_file(&path.join("events.jsonl"))
        .map_err(|e| e.to_string())?;
    Ok(path)
}

pub(crate) fn latest_run_name(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionTitleUpdated(title) => Some(title.title.clone()),
        EventV1::RunStarted(run) => Some(run.run_name.to_string()),
        _ => None,
    })
}
pub(crate) fn most_recent_conversational_agent_id(
    events: &[EventEnvelopeV1],
    agents: &BTreeMap<String, String>,
) -> Option<String> {
    harness_core::conversation_rewind::active_events(events)
        .iter()
        .rev()
        .find_map(|event| {
            if !matches!(
                event.payload,
                EventV1::UserMessageSubmitted(_)
                    | EventV1::ProviderRequestStarted(_)
                    | EventV1::AssistantMessageFinished(_)
            ) {
                return None;
            }
            event
                .actor
                .agent_id
                .as_ref()
                .filter(|id| agents.contains_key(*id))
                .cloned()
        })
}
pub(crate) fn select_resume_agent_id(
    plan: &ResumePlan,
    events: &[EventEnvelopeV1],
    run: &str,
) -> Result<String, String> {
    most_recent_conversational_agent_id(events, &plan.known_agents)
        .or_else(|| {
            events.iter().rev().find_map(|event| match &event.payload {
                EventV1::AgentSpawned(agent)
                    if agent.parent_agent_id.is_none()
                        && plan.known_agents.contains_key(&agent.agent_id) =>
                {
                    Some(agent.agent_id.clone())
                }
                _ => None,
            })
        })
        .ok_or_else(|| format!("session {run} has no resumable primary agent"))
}
