// allow: SIZE_OK — provider context restore (historical event replay + checkpoint discovery + conversation reconstruction)
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::agent::{
    ProviderContext, ProviderContextCheckpoint, ProviderConversationTurn,
    ProviderConversationTurnStatus,
};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    EventArtifactRef, EventEnvelopeV1, EventV1, SessionCompactionEvent, TaskCancelledEvent,
    TaskCompletedEvent, TaskTerminalScope,
};
use crate::provider_args::provider_tool_arguments_json;
use crate::session_paths::EVENTS_FILE_NAME;
use crate::text::non_empty_trimmed;

use super::super::CoordinatorError;
use super::truncated_failure_reason;

pub(super) fn read_historical_events_until(
    run_id: &str,
    events_path: &Path,
    through_seq: u64,
) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;
    let mut expected_seq = 1_u64;
    let mut events = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);
        if event.seq > through_seq {
            break;
        }
        events.push(event);
    }
    Ok(events)
}

fn parse_historical_event_line(
    run_id: &str,
    events_path: &Path,
    line_number: usize,
    line: io::Result<String>,
) -> Result<Option<EventEnvelopeV1>, CoordinatorError> {
    let line = line.map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "failed to read historical event line {} in {}: {source}",
            line_number + 1,
            events_path.display()
        ),
    })?;
    if line.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "invalid historical event line {} in {}: {source}",
                line_number + 1,
                events_path.display()
            ),
        })
}

fn validate_historical_event_seq(
    run_id: &str,
    events_path: &Path,
    event: &EventEnvelopeV1,
    expected_seq: u64,
) -> Result<(), CoordinatorError> {
    if event.seq == expected_seq {
        return Ok(());
    }
    Err(CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "historical sequence mismatch at {}: expected {expected_seq}, got {}",
            events_path.display(),
            event.seq
        ),
    })
}

pub(super) fn collect_historical_agent_turns_until(
    run_id: &str,
    events_path: &Path,
    agent_id: &str,
    lower_bound_seq: u64,
    through_seq: u64,
) -> Result<Vec<HistoricalCompletedAgentTurn>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;

    let mut expected_seq = 1_u64;
    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut turns = Vec::new();

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);

        if event.seq > through_seq {
            break;
        }
        if event.seq <= lower_bound_seq {
            continue;
        }

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let request = requests.entry(payload.request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.agent_id = Some(agent_id.to_string());
            }
            EventV1::ProviderStreamDelta(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(payload.request_id.to_string())
                    .or_default()
                    .assistant_output
                    .push_str(&payload.delta);
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

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;
                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
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
    messages: Vec<ConversationMessage>,
    active_assistant_message_index: Option<usize>,
    tool_ids_by_call_id: BTreeMap<String, String>,
    agent_id: Option<String>,
    provider_request_id: Option<String>,
    provider_finish_reason: Option<String>,
    first_seq: Option<u64>,
}

#[derive(Debug, Clone)]
struct AppliedCheckpointRecord {
    checkpoint_id: String,
    artifact_path: String,
    through_seq: u64,
    through_request_id: Option<String>,
}

#[derive(Debug, Clone)]
enum AppliedCheckpoint {
    File(AppliedCheckpointRecord),
    Inline(SessionCompactionEvent),
}

#[derive(Debug, Clone)]
pub(super) struct HistoricalCompletedAgentTurn {
    pub(super) request_id: String,
    pub(super) user_prompt: String,
    pub(super) assistant_response: String,
    pub(super) artifact_refs: Vec<EventArtifactRef>,
}

fn historical_conversation_messages_for_completed_turn(
    user_prompt: &str,
    assistant_response: &str,
    request_state: &HistoricalRequestState,
) -> Vec<ConversationMessage> {
    if request_state.messages.is_empty() {
        return Vec::new();
    }

    let request_id = request_state
        .messages
        .iter()
        .find_map(|message| match message {
            ConversationMessage::Assistant(assistant) => Some(assistant.request_id.clone()),
            ConversationMessage::ToolResult(tool_result) => Some(tool_result.request_id.clone()),
            ConversationMessage::User(user) => Some(user.request_id.clone()),
            ConversationMessage::Checkpoint(_) => None,
        })
        .unwrap_or_default();
    let agent_id = request_state.agent_id.clone();

    let mut messages = Vec::with_capacity(request_state.messages.len() + 1);
    messages.push(ConversationMessage::User(ConversationUserMessage {
        request_id,
        text: user_prompt.to_string(),
        seq: request_state.first_seq,
        agent_id,
    }));
    messages.extend(request_state.messages.clone());
    if let Some(ConversationMessage::Assistant(assistant)) = messages.last_mut() {
        if assistant.tool_calls.is_empty() && assistant.text != assistant_response {
            assistant.text = assistant_response.to_string();
        }
    }
    messages
}

fn restore_historical_user_prompt(
    run_id: &str,
    request_id: &str,
    user_text: Option<String>,
    prompt_summary: Option<String>,
) -> Result<String, CoordinatorError> {
    if let Some(user_text) = user_text {
        return Ok(user_text);
    }

    let Some(prompt_summary) = prompt_summary.as_deref().and_then(non_empty_trimmed) else {
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

pub(in crate::coord) fn restore_provider_context_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<BTreeMap<String, ProviderContext>, CoordinatorError> {
    let run_dir = session_dir.join(run_id);
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let historical_events = read_historical_events_until(run_id, &events_path, u64::MAX)?;

    let applied_checkpoints = discover_applied_checkpoints(run_id, &run_dir, &historical_events)?;
    let checkpoint_boundaries = applied_checkpoints
        .iter()
        .map(|(agent_id, checkpoint)| {
            let boundary = match checkpoint {
                AppliedCheckpoint::File(record) => record.through_seq,
                AppliedCheckpoint::Inline(compaction) => {
                    compaction.first_kept_event_seq.saturating_sub(1)
                }
            };
            (agent_id.clone(), boundary)
        })
        .collect::<BTreeMap<_, _>>();

    let mut histories = BTreeMap::new();
    for (agent_id, checkpoint) in &applied_checkpoints {
        match checkpoint {
            AppliedCheckpoint::File(record) => {
                let checkpoint_artifact =
                    load_provider_context_checkpoint(run_id, &run_dir, record)?;
                if checkpoint_artifact.metadata.run_id.as_str() != run_id {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "checkpoint `{}` run mismatch: expected `{run_id}`, got `{}`",
                            record.checkpoint_id, checkpoint_artifact.metadata.run_id
                        ),
                    });
                }
                if checkpoint_artifact.metadata.checkpoint_id != record.checkpoint_id {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "checkpoint artifact id mismatch for agent `{agent_id}`: expected `{}`, got `{}`",
                            record.checkpoint_id, checkpoint_artifact.metadata.checkpoint_id
                        ),
                    });
                }
                if checkpoint_artifact.metadata.agent_id != *agent_id {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "checkpoint `{}` agent mismatch: expected `{agent_id}`, got `{}`",
                            record.checkpoint_id, checkpoint_artifact.metadata.agent_id
                        ),
                    });
                }
                if checkpoint_artifact.metadata.through_seq != record.through_seq {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "checkpoint `{}` through_seq mismatch: expected `{}`, got `{}`",
                            record.checkpoint_id,
                            record.through_seq,
                            checkpoint_artifact.metadata.through_seq
                        ),
                    });
                }
                if checkpoint_artifact.metadata.through_request_id != record.through_request_id {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "checkpoint `{}` through_request_id mismatch: expected `{:?}`, got `{:?}`",
                            record.checkpoint_id,
                            record.through_request_id,
                            checkpoint_artifact.metadata.through_request_id
                        ),
                    });
                }
                histories.insert(
                    agent_id.clone(),
                    ProviderContext::from_checkpoint(checkpoint_artifact),
                );
            }
            AppliedCheckpoint::Inline(compaction) => {
                histories.insert(
                    agent_id.clone(),
                    ProviderContext {
                        compacted_summary: Some(compaction.summary.clone()),
                        preserved_turns: Vec::new(),
                        checkpoint: None,
                    },
                );
            }
        }
    }

    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut agent_turn_agent_by_task: BTreeMap<String, String> = BTreeMap::new();

    for event in &historical_events {
        let replay_agent_event = should_replay_agent_scoped_event(
            event.seq,
            event.actor.agent_id.as_deref(),
            &checkpoint_boundaries,
        );

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(payload.request_id.as_str());
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.provider_request_id = Some(payload.request_id.to_string());
                request.messages.push(ConversationMessage::Assistant(
                    ConversationAssistantMessage {
                        request_id: request_id.into(),
                        agent_id: event.actor.agent_id.clone(),
                        text: String::new(),
                        tool_calls: Vec::new(),
                        stop_reason: None,
                        first_seq: Some(event.seq),
                        last_seq: Some(event.seq),
                        provider_id: Some(payload.provider_id.clone()),
                        model_id: Some(payload.model_id.clone()),
                        output_digest: None,
                    },
                ));
                request.active_assistant_message_index =
                    Some(request.messages.len().saturating_sub(1));
                if let Some(agent_id) = event.actor.agent_id.as_deref().and_then(non_empty_trimmed)
                {
                    request.agent_id = Some(agent_id.to_string());
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(payload.request_id.as_str());
                let request = requests.entry(request_id.to_string()).or_default();
                request.assistant_output.push_str(&payload.delta);
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.text.push_str(&payload.delta);
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ProviderRequestFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(payload.request_id.as_str());
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.provider_request_id = Some(payload.request_id.to_string());
                request.provider_finish_reason = Some(payload.finish_reason.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.stop_reason = Some(payload.finish_reason.clone());
                        assistant.output_digest = payload.output_digest.clone();
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::TaskScheduled(payload) => {
                if !replay_agent_event {
                    continue;
                }
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
                            requests
                                .entry(request_id.to_string())
                                .or_default()
                                .first_seq
                                .get_or_insert(event.seq);
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.to_string());
                            if let Some(agent_id) = event.actor.agent_id.as_deref() {
                                agent_turn_agent_by_task
                                    .insert(payload.task_id.to_string(), agent_id.to_string());
                            }
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
            EventV1::ToolCallRequested(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let request = requests.entry(request_id.to_string()).or_default();
                request
                    .tool_ids_by_call_id
                    .insert(payload.tool_call_id.to_string(), payload.tool_id.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.tool_calls.push(ConversationToolCall {
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: payload.tool_id.clone(),
                            args_summary: provider_tool_arguments_json(&payload.args_summary),
                            args_digest: payload.args_digest.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        });
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let Some(request) = requests.get_mut(request_id) else {
                    continue;
                };
                let Some(tool_id) = request
                    .tool_ids_by_call_id
                    .get(payload.tool_call_id.as_str())
                    .cloned()
                else {
                    continue;
                };
                request
                    .messages
                    .push(ConversationMessage::ToolResult(Box::new(
                        ConversationToolResultMessage {
                            request_id: request_id.into(),
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: Some(tool_id),
                            status: payload.status,
                            output_summary: payload.output_summary.clone(),
                            output_digest: payload.output_digest.clone(),
                            output_json: payload.output_json.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        },
                    )));
            }
            EventV1::TaskCompleted(payload) => {
                if matches!(
                    historical_task_scopes.get(payload.task_id.as_str()),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(payload.task_id.as_str());
                }
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(agent_id) = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                else {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task completion for request `{request_id}` missing agent actor"
                        ),
                    });
                };
                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
                } else {
                    payload.result_summary.clone()
                };
                let messages = historical_conversation_messages_for_completed_turn(
                    &user_prompt,
                    &assistant_response,
                    &request_state,
                );
                let mut artifacts = request_artifacts.remove(request_id).unwrap_or_default();
                artifacts.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifacts
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);

                histories
                    .entry(request_state.agent_id.unwrap_or(agent_id))
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response,
                        request_id: Some(request_id.into()),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts,
                        messages,
                        ..ProviderConversationTurn::default()
                    });
            }
            EventV1::TaskCancelled(payload) => {
                let agent_id_from_task = if matches!(
                    historical_task_scopes.get(payload.task_id.as_str()),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(payload.task_id.as_str())
                } else {
                    None
                };
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_cancellation_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(request_state) = requests.remove(request_id) else {
                    continue;
                };

                let agent_id = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                    .or(agent_id_from_task)
                    .or_else(|| request_state.agent_id.clone())
                    .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task cancellation for request `{request_id}` missing agent actor"
                        ),
                    })?;
                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let (status, failure_stage) = historical_cancelled_turn_status_stage(
                    request_state.provider_finish_reason.as_deref(),
                    &payload.reason,
                );
                let messages = if failure_stage == "max_iters" {
                    historical_conversation_messages_for_completed_turn(
                        &user_prompt,
                        &request_state.assistant_output,
                        &request_state,
                    )
                } else {
                    Vec::new()
                };
                let provider_request_id = request_state
                    .provider_request_id
                    .unwrap_or_else(|| request_id.to_string());
                histories
                    .entry(agent_id)
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response: request_state.assistant_output,
                        status,
                        failure_stage: Some(failure_stage),
                        failure_reason: truncated_failure_reason(&payload.reason),
                        request_id: Some(provider_request_id.into()),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts: Vec::new(),
                        messages,
                    });
            }
            _ => {}
        }
    }

    Ok(histories)
}

fn should_replay_agent_scoped_event(
    seq: u64,
    agent_id: Option<&str>,
    checkpoint_boundaries: &BTreeMap<String, u64>,
) -> bool {
    let Some(agent_id) = agent_id else {
        return true;
    };

    seq > checkpoint_boundaries.get(agent_id).copied().unwrap_or(0)
}

#[allow(
    deprecated,
    reason = "deprecated event variants kept for backward compatibility with existing session logs"
)]
fn discover_applied_checkpoints(
    run_id: &str,
    run_dir: &Path,
    events: &[EventEnvelopeV1],
) -> Result<BTreeMap<String, AppliedCheckpoint>, CoordinatorError> {
    let mut written_by_id = BTreeMap::new();
    let mut latest_applied_by_agent: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut latest_session_compaction_by_agent: BTreeMap<String, (u64, SessionCompactionEvent)> =
        BTreeMap::new();

    for event in events {
        match &event.payload {
            EventV1::CompactionWritten(payload) => {
                written_by_id.insert(payload.checkpoint_id.clone(), payload.clone());
            }
            EventV1::CompactionApplied(payload) => {
                latest_applied_by_agent.insert(
                    payload.agent_id.clone(),
                    (event.seq, payload.checkpoint_id.clone()),
                );
            }
            EventV1::SessionCompaction(payload) => {
                latest_session_compaction_by_agent
                    .insert(payload.agent_id.clone(), (event.seq, payload.clone()));
            }
            _ => {}
        }
    }

    let mut applied = BTreeMap::new();
    for (agent_id, (_, checkpoint_id)) in latest_applied_by_agent {
        let Some(written) = written_by_id.get(&checkpoint_id) else {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` was applied without a matching written event"
                ),
            });
        };

        if written.agent_id != agent_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` agent mismatch between applied `{agent_id}` and written `{}`",
                    written.agent_id
                ),
            });
        }

        applied.insert(
            agent_id.clone(),
            AppliedCheckpoint::File(AppliedCheckpointRecord {
                checkpoint_id: checkpoint_id.clone(),
                artifact_path: written.artifact_path.clone(),
                through_seq: written.through_seq,
                through_request_id: written.through_request_id.clone(),
            }),
        );
    }

    for (agent_id, (_, compaction)) in latest_session_compaction_by_agent {
        // Inline session compaction always wins over a legacy file checkpoint
        // because it represents the most recent compaction event.
        applied.insert(agent_id, AppliedCheckpoint::Inline(compaction));
    }

    let _ = run_dir;
    Ok(applied)
}

fn load_provider_context_checkpoint(
    run_id: &str,
    run_dir: &Path,
    checkpoint: &AppliedCheckpointRecord,
) -> Result<ProviderContextCheckpoint, CoordinatorError> {
    let checkpoint_path = run_dir.join(&checkpoint.artifact_path);
    let body = fs::read_to_string(&checkpoint_path).map_err(|source| {
        CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to read checkpoint artifact {}: {source}",
                checkpoint_path.display()
            ),
        }
    })?;

    serde_json::from_str(&body).map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "invalid checkpoint artifact {}: {source}",
            checkpoint_path.display()
        ),
    })
}

fn historical_task_completion_marks_agent_turn(
    request_id: &str,
    payload: &TaskCompletedEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
    {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(payload.task_id.as_str()) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id.as_str() == payload.task_id.as_str();
    }

    true
}

fn historical_task_cancellation_marks_agent_turn(
    request_id: &str,
    payload: &TaskCancelledEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload.task_scope {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(payload.task_id.as_str()) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id.as_str() == payload.task_id.as_str();
    }

    false
}

fn historical_cancelled_turn_status_stage(
    provider_finish_reason: Option<&str>,
    cancellation_reason: &str,
) -> (ProviderConversationTurnStatus, String) {
    if provider_finish_reason == Some("error") {
        return (
            ProviderConversationTurnStatus::Failed,
            "provider_error".to_string(),
        );
    }

    if cancellation_reason.contains("overflow persisted after checkpoint compaction") {
        return (
            ProviderConversationTurnStatus::Failed,
            "overflow_retry_failed".to_string(),
        );
    }

    if cancellation_reason.contains("failed closed") {
        return (
            ProviderConversationTurnStatus::Failed,
            "tool_failure".to_string(),
        );
    }

    if cancellation_reason.contains("critical lifecycle hook failed")
        || cancellation_reason.contains("lifecycle hook failed")
    {
        return (
            ProviderConversationTurnStatus::Failed,
            "hook_failure".to_string(),
        );
    }

    if cancellation_reason.contains("agent turn exceeded profile max_iters=") {
        return (
            ProviderConversationTurnStatus::Aborted,
            "max_iters".to_string(),
        );
    }

    (
        ProviderConversationTurnStatus::Aborted,
        "cancelled".to_string(),
    )
}
