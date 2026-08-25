use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::event::{EventArtifactRef, EventV1, TaskCompletedEvent, TaskTerminalScope};
use crate::session::AssistantPart;
use crate::text::non_empty_trimmed;

use super::super::super::CoordinatorError;
use super::journal::{open_history, parse_event_line, validate_event_seq};

pub(in crate::coord::provider_context) fn collect_historical_agent_turns_until(
    run_id: &str,
    events_path: &Path,
    agent_id: &str,
    lower_bound_seq: u64,
    through_seq: u64,
) -> Result<Vec<HistoricalCompletedAgentTurn>, CoordinatorError> {
    let file = open_history(run_id, events_path)?;
    let mut expected_seq = 1_u64;
    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut turns = Vec::new();

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_event_line(run_id, events_path, line_number, line)? else {
            continue;
        };
        validate_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);
        if event.seq > through_seq {
            break;
        }
        if event.seq <= lower_bound_seq {
            continue;
        }

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                requests
                    .entry(payload.request_id.to_string())
                    .or_default()
                    .user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(historical_request_id(&event, payload.request_id.as_str()))
                    .or_default()
                    .prompt_summary = Some(payload.prompt_summary.clone());
            }
            EventV1::ProviderStreamDelta(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(historical_request_id(&event, payload.request_id.as_str()))
                    .or_default()
                    .assistant_output
                    .push_str(&payload.delta);
            }
            EventV1::AssistantMessageFinished(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(historical_request_id(&event, payload.request_id.as_str()))
                    .or_default()
                    .apply_semantic_parts(&payload.parts);
            }
            EventV1::TaskScheduled(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(queue_key) = payload.queue_key.as_deref() else {
                    continue;
                };
                let scope = if queue_key.starts_with("provider_model:") {
                    Some(TaskTerminalScope::AgentTurn)
                } else if queue_key.starts_with("tool:") {
                    Some(TaskTerminalScope::ToolCall)
                } else {
                    None
                };
                if let Some(scope) = scope {
                    historical_task_scopes.insert(payload.task_id.to_string(), scope);
                    if matches!(scope, TaskTerminalScope::AgentTurn) {
                        if let Some(request_id) = event.correlation_id.as_deref() {
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.to_string());
                        }
                    }
                }
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                request_artifacts
                    .entry(request_id.to_string())
                    .or_default()
                    .push(EventArtifactRef {
                        path: payload.path.clone(),
                        digest: Some(payload.digest.clone()),
                    });
            }
            EventV1::TaskCompleted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                if !completion_is_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }
                let request = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;
                let user_prompt = restore_user_prompt(run_id, request_id, &request)?;
                let assistant_response =
                    if request.semantic_parts_authoritative || payload.result_summary.is_empty() {
                        request.assistant_output
                    } else {
                        payload.result_summary.clone()
                    };
                let mut artifact_refs = request_artifacts.remove(request_id).unwrap_or_default();
                artifact_refs.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifact_refs
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);
                turns.push(HistoricalCompletedAgentTurn {
                    request_id: request_id.into(),
                    user_prompt,
                    assistant_response,
                    artifact_refs,
                });
            }
            _ => {}
        }
    }
    Ok(turns)
}

#[derive(Default)]
struct HistoricalRequestState {
    user_text: Option<String>,
    prompt_summary: Option<String>,
    assistant_output: String,
    semantic_parts_authoritative: bool,
}

impl HistoricalRequestState {
    fn apply_semantic_parts(&mut self, parts: &[AssistantPart]) {
        if parts.is_empty() {
            return;
        }
        self.semantic_parts_authoritative = true;
        self.assistant_output = parts
            .iter()
            .filter_map(|part| match part {
                AssistantPart::Text { text } => Some(text.as_str()),
                AssistantPart::Reasoning { .. } | AssistantPart::ToolCall(_) => None,
            })
            .collect();
    }
}

#[derive(Debug, Clone)]
pub(in crate::coord::provider_context) struct HistoricalCompletedAgentTurn {
    pub(in crate::coord::provider_context) request_id: String,
    pub(in crate::coord::provider_context) user_prompt: String,
    pub(in crate::coord::provider_context) assistant_response: String,
    pub(in crate::coord::provider_context) artifact_refs: Vec<EventArtifactRef>,
}

fn historical_request_id(
    event: &crate::event::EventEnvelopeV1,
    provider_request_id: &str,
) -> String {
    event
        .correlation_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(provider_request_id)
        .to_string()
}

fn restore_user_prompt(
    run_id: &str,
    request_id: &str,
    request: &HistoricalRequestState,
) -> Result<String, CoordinatorError> {
    if let Some(user_text) = request.user_text.as_ref() {
        return Ok(user_text.clone());
    }
    let Some(prompt_summary) = request
        .prompt_summary
        .as_deref()
        .and_then(non_empty_trimmed)
    else {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!("missing user message for completed request `{request_id}`"),
        });
    };
    if prompt_summary.ends_with('…') {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "missing user message for completed request `{request_id}` and prompt_summary is truncated"
            ),
        });
    }
    Ok(prompt_summary.to_string())
}

fn completion_is_agent_turn(
    request_id: &str,
    payload: &TaskCompletedEvent,
    task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
    {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }
    if let Some(scope) = task_scopes.get(payload.task_id.as_str()) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }
    request_task_ids
        .get(request_id)
        .is_none_or(|task_id| task_id.as_str() == payload.task_id.as_str())
}
