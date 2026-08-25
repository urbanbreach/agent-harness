// allow: SIZE_OK — conversation projection (message reconstruction + tool metadata + error mapping)
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::agent::{
    ProviderContextCheckpoint, ProviderConversationTurn, ProviderConversationTurnStatus,
};
use crate::event::{EventArtifactRef, EventEnvelopeV1, EventV1, ToolCallMetadata, ToolCallStatus};
use crate::session::AssistantPart;
use crate::text::non_empty_trimmed;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConversationProjectionError {
    #[error("events are not seq-ordered: event seq {seq} followed {previous_seq}")]
    EventsOutOfOrder { previous_seq: u64, seq: u64 },
    #[error("provider delta at seq {seq} references request `{request_id}` before its start")]
    ProviderDeltaBeforeStart { request_id: String, seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConversationProjection {
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<ConversationCheckpointMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ConversationMessage {
    Checkpoint(ConversationCheckpointMessage),
    User(ConversationUserMessage),
    Assistant(ConversationAssistantMessage),
    ToolResult(Box<ConversationToolResultMessage>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCheckpoint {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_turns: Vec<ConversationCheckpointTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCheckpointTurn {
    pub user_prompt: String,
    pub assistant_response: String,
    #[serde(
        default,
        skip_serializing_if = "ProviderConversationTurnStatus::is_completed"
    )]
    pub status: ProviderConversationTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<crate::ids::RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCheckpointMetadata {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCheckpointMessage {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationUserMessage {
    pub request_id: crate::ids::RequestId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAssistantMessage {
    pub request_id: crate::ids::RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ConversationToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationToolCall {
    pub tool_call_id: crate::ids::ToolCallId,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationToolResultMessage {
    pub request_id: crate::ids::RequestId,
    pub tool_call_id: crate::ids::ToolCallId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    pub status: ToolCallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

pub fn project_conversation(
    events: &[EventEnvelopeV1],
    checkpoints: &[ConversationCheckpoint],
) -> Result<ConversationProjection, ConversationProjectionError> {
    ensure_seq_ordered(events)?;

    let mut projection = ConversationProjection::default();
    let latest_compaction = events.iter().rev().find_map(|event| {
        let EventV1::SessionCompaction(compaction) = &event.payload else {
            return None;
        };
        compaction_first_kept_sequence(events, compaction)
            .map(|first_kept_seq| (event, compaction, first_kept_seq))
    });
    let skip_through_seq = if let Some((event, compaction, first_kept_seq)) = latest_compaction {
        let through_seq = first_kept_seq.saturating_sub(1);
        projection.messages.push(ConversationMessage::Checkpoint(
            ConversationCheckpointMessage {
                checkpoint_id: format!("session-compaction-{}", event.seq),
                agent_id: compaction.agent_id.clone(),
                through_seq,
                summary: compaction.summary.clone(),
            },
        ));
        through_seq
    } else {
        let mut checkpoint_refs = checkpoints.iter().collect::<Vec<_>>();
        checkpoint_refs.sort_by_key(|checkpoint| checkpoint.through_seq);
        for checkpoint in checkpoint_refs {
            projection.checkpoints.push(checkpoint.metadata());
            if non_empty_trimmed(&checkpoint.summary).is_some() {
                projection.messages.push(ConversationMessage::Checkpoint(
                    ConversationCheckpointMessage {
                        checkpoint_id: checkpoint.checkpoint_id.clone(),
                        agent_id: checkpoint.agent_id.clone(),
                        through_seq: checkpoint.through_seq,
                        summary: checkpoint.summary.clone(),
                    },
                ));
            }
            for turn in &checkpoint.recent_turns {
                append_checkpoint_turn(&mut projection.messages, checkpoint, turn);
            }
        }
        checkpoints
            .iter()
            .map(|checkpoint| checkpoint.through_seq)
            .max()
            .unwrap_or(0)
    };
    let mut request_states = BTreeMap::<String, RequestProjectionState>::new();
    let mut request_order = Vec::<OrderedConversationItem>::new();
    let mut emitted_users = BTreeSet::<String>::new();
    let mut emitted_assistants = BTreeSet::<String>::new();
    let mut started_provider_requests = BTreeSet::<String>::new();
    let mut tool_results = BTreeMap::<String, ToolResultProjectionState>::new();

    for event in events.iter().filter(|event| event.seq > skip_through_seq) {
        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let state = request_states
                    .entry(payload.request_id.to_string())
                    .or_default();
                state.user = Some(ConversationUserMessage {
                    request_id: payload.request_id.clone(),
                    text: payload.text.clone(),
                    seq: Some(event.seq),
                    agent_id: event.actor.agent_id.clone(),
                });
                if emitted_users.insert(payload.request_id.to_string()) {
                    request_order.push(OrderedConversationItem::User(
                        payload.request_id.to_string(),
                    ));
                }
            }
            EventV1::ProviderRequestStarted(payload) => {
                started_provider_requests.insert(payload.request_id.to_string());
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let state = request_states
                    .entry(payload.request_id.to_string())
                    .or_default();
                state.assistant.first_seq.get_or_insert(event.seq);
                state.assistant.request_id = request_id.into();
                state.assistant.agent_id = event.actor.agent_id.clone();
                state.assistant.provider_id = Some(payload.provider_id.clone());
                state.assistant.model_id = Some(payload.model_id.clone());
            }
            EventV1::ProviderStreamDelta(payload) => {
                if !started_provider_requests.contains(payload.request_id.as_str()) {
                    return Err(ConversationProjectionError::ProviderDeltaBeforeStart {
                        request_id: payload.request_id.to_string(),
                        seq: event.seq,
                    });
                }
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let state_key = payload.request_id.to_string();
                let state = request_states.entry(state_key.clone()).or_default();
                state.assistant.request_id = request_id.into();
                state.assistant.text.push_str(&payload.delta);
                state.assistant.last_seq = Some(event.seq);
                if emitted_assistants.insert(state_key.clone()) {
                    request_order.push(OrderedConversationItem::Assistant(state_key));
                }
            }
            EventV1::ProviderRequestFinished(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let state = request_states
                    .entry(payload.request_id.to_string())
                    .or_default();
                state.assistant.request_id = request_id.into();
                state.assistant.stop_reason = Some(payload.finish_reason.clone());
                state.assistant.output_digest = payload.output_digest.clone();
                state.assistant.last_seq = Some(event.seq);
            }
            EventV1::AssistantMessageFinished(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let state_key = payload.request_id.to_string();
                let state = request_states.entry(state_key.clone()).or_default();
                state.assistant.request_id = request_id.into();
                if !payload.parts.is_empty() {
                    state.semantic_parts_authoritative = true;
                    state.semantic_tool_requests_seen = 0;
                    state.assistant.text.clear();
                    state.tool_calls.clear();
                    for part in &payload.parts {
                        match part {
                            AssistantPart::Text { text } => state.assistant.text.push_str(text),
                            AssistantPart::Reasoning { .. } => {}
                            AssistantPart::ToolCall(tool_call) => {
                                state.tool_calls.push(ConversationToolCall {
                                    tool_call_id: tool_call.tool_call_id.clone(),
                                    tool_id: tool_call.tool_id.clone(),
                                    args_summary: tool_call.args_summary.clone(),
                                    args_digest: tool_call.args_digest.clone(),
                                    seq: Some(event.seq),
                                    metadata: None,
                                });
                            }
                        }
                    }
                    if let Some(provenance) = payload.provenance.as_ref() {
                        state.assistant.provider_id = Some(provenance.provider_id.clone());
                        state.assistant.model_id = Some(provenance.model_id.clone());
                        state
                            .assistant
                            .stop_reason
                            .clone_from(&provenance.stop_reason);
                    }
                }
                state.assistant.last_seq = Some(event.seq);
                if emitted_assistants.insert(state_key.clone()) {
                    request_order.push(OrderedConversationItem::Assistant(state_key));
                }
            }
            EventV1::ToolCallRequested(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let semantic_state_key = request_states.iter().find_map(|(key, state)| {
                    state
                        .tool_calls
                        .iter()
                        .any(|tool_call| tool_call.tool_call_id == payload.tool_call_id)
                        .then(|| key.clone())
                });
                let state_key = semantic_state_key
                    .clone()
                    .unwrap_or_else(|| request_id.to_string());
                let state = request_states.entry(state_key.clone()).or_default();
                if semantic_state_key.is_none() && emitted_assistants.insert(state_key.clone()) {
                    request_order.push(OrderedConversationItem::Assistant(state_key));
                }
                if state.semantic_parts_authoritative {
                    let index = state
                        .tool_calls
                        .iter()
                        .position(|tool_call| tool_call.tool_call_id == payload.tool_call_id);
                    if let Some(index) = index {
                        let tool_call = &mut state.tool_calls[index];
                        tool_call.tool_call_id = payload.tool_call_id.clone();
                        tool_call.seq = Some(event.seq);
                        tool_call.metadata.clone_from(&payload.metadata);
                        state.semantic_tool_requests_seen =
                            state.semantic_tool_requests_seen.saturating_add(1);
                    }
                } else {
                    state.tool_calls.push(ConversationToolCall {
                        tool_call_id: payload.tool_call_id.clone(),
                        tool_id: payload.tool_id.clone(),
                        args_summary: payload.args_summary.clone(),
                        args_digest: payload.args_digest.clone(),
                        seq: Some(event.seq),
                        metadata: payload.metadata.clone(),
                    });
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                tool_results.insert(
                    payload.tool_call_id.to_string(),
                    ToolResultProjectionState {
                        request_id: request_id.to_string(),
                        tool_call_id: payload.tool_call_id.to_string(),
                        status: payload.status,
                        output_summary: payload.output_summary.clone(),
                        output_digest: payload.output_digest.clone(),
                        output_json: payload.output_json.clone(),
                        seq: Some(event.seq),
                        metadata: payload.metadata.clone(),
                    },
                );
            }
            _ => {}
        }
    }

    for item in request_order {
        match item {
            OrderedConversationItem::User(request_id) => {
                if let Some(user) = request_states
                    .get(&request_id)
                    .and_then(|state| state.user.clone())
                {
                    projection.messages.push(ConversationMessage::User(user));
                }
            }
            OrderedConversationItem::Assistant(request_id) => {
                let Some(state) = request_states.get(&request_id) else {
                    continue;
                };
                let mut assistant = state.assistant.clone();
                if assistant.request_id.as_str().is_empty() {
                    assistant.request_id = request_id.clone().into();
                }
                assistant.tool_calls = state.tool_calls.clone();
                projection
                    .messages
                    .push(ConversationMessage::Assistant(assistant));

                for tool_call in &state.tool_calls {
                    if let Some(result) = tool_results.get(tool_call.tool_call_id.as_str()) {
                        projection
                            .messages
                            .push(ConversationMessage::ToolResult(Box::new(
                                ConversationToolResultMessage {
                                    request_id: result.request_id.clone().into(),
                                    tool_call_id: result.tool_call_id.clone().into(),
                                    tool_id: Some(tool_call.tool_id.clone()),
                                    status: result.status,
                                    output_summary: result.output_summary.clone(),
                                    output_digest: result.output_digest.clone(),
                                    output_json: result.output_json.clone(),
                                    seq: result.seq,
                                    metadata: result.metadata.clone(),
                                },
                            )));
                    }
                }
            }
        }
    }

    Ok(projection)
}

fn typed_boundary_sequence(
    events: &[EventEnvelopeV1],
    entry_id: &crate::ids::EntryId,
) -> Option<u64> {
    let run_id = &events.first()?.run_id;
    let namespace = crate::session::legacy::LegacyIdentityNamespace::new(run_id);
    events.iter().find_map(|event| {
        let semantic_kind = match event.payload {
            EventV1::SessionTitleUpdated(_) => "session_metadata",
            EventV1::UserMessageSubmitted(_) => "user_message",
            EventV1::ProviderRequestStarted(_) => "assistant_message",
            EventV1::SessionCompaction(_) => "compaction_summary",
            EventV1::BranchSummary(_) => "branch_summary",
            EventV1::ToolCallFinished(_) => "tool_result",
            _ => return None,
        };
        (namespace.entry_id(event.seq, &event.event_id, semantic_kind) == *entry_id)
            .then_some(event.seq)
    })
}

pub(crate) fn compaction_first_kept_sequence(
    events: &[EventEnvelopeV1],
    compaction: &crate::event::SessionCompactionEvent,
) -> Option<u64> {
    match compaction.first_kept_entry_id.as_ref() {
        Some(entry_id) => typed_boundary_sequence(events, entry_id),
        None => Some(compaction.first_kept_event_seq),
    }
}

impl ConversationCheckpoint {
    fn metadata(&self) -> ConversationCheckpointMetadata {
        ConversationCheckpointMetadata {
            checkpoint_id: self.checkpoint_id.clone(),
            agent_id: self.agent_id.clone(),
            through_seq: self.through_seq,
            through_request_id: self.through_request_id.clone(),
        }
    }
}

impl From<&ProviderContextCheckpoint> for ConversationCheckpoint {
    fn from(checkpoint: &ProviderContextCheckpoint) -> Self {
        Self {
            checkpoint_id: checkpoint.metadata.checkpoint_id.clone(),
            agent_id: checkpoint.metadata.agent_id.clone(),
            through_seq: checkpoint.metadata.through_seq,
            through_request_id: checkpoint.metadata.through_request_id.clone(),
            summary: checkpoint.summary.clone(),
            recent_turns: checkpoint
                .recent_turns
                .iter()
                .map(ConversationCheckpointTurn::from)
                .collect(),
        }
    }
}

impl From<&ProviderConversationTurn> for ConversationCheckpointTurn {
    fn from(turn: &ProviderConversationTurn) -> Self {
        Self {
            user_prompt: turn.user_prompt.clone(),
            assistant_response: turn.assistant_response.clone(),
            status: turn.status,
            failure_stage: turn.failure_stage.clone(),
            failure_reason: turn.failure_reason.clone(),
            request_id: turn.request_id.clone(),
            first_seq: turn.first_seq,
            last_seq: turn.last_seq,
            artifacts: turn.artifacts.clone(),
            messages: turn.messages.clone(),
            attachments: turn.attachments.clone(),
        }
    }
}

#[derive(Debug, Clone)]
enum OrderedConversationItem {
    User(String),
    Assistant(String),
}

#[derive(Debug, Clone, Default)]
struct RequestProjectionState {
    user: Option<ConversationUserMessage>,
    assistant: ConversationAssistantMessage,
    tool_calls: Vec<ConversationToolCall>,
    semantic_parts_authoritative: bool,
    semantic_tool_requests_seen: usize,
}

#[derive(Debug, Clone)]
struct ToolResultProjectionState {
    request_id: String,
    tool_call_id: String,
    status: ToolCallStatus,
    output_summary: Option<String>,
    output_digest: Option<String>,
    output_json: Option<Value>,
    seq: Option<u64>,
    metadata: Option<ToolCallMetadata>,
}

fn append_checkpoint_turn(
    messages: &mut Vec<ConversationMessage>,
    checkpoint: &ConversationCheckpoint,
    turn: &ConversationCheckpointTurn,
) {
    if !turn.messages.is_empty() {
        messages.extend(turn.messages.clone());
        return;
    }

    let request_id = turn.request_id.clone().unwrap_or_else(|| {
        checkpoint
            .through_request_id
            .clone()
            .unwrap_or_default()
            .into()
    });

    messages.push(ConversationMessage::User(ConversationUserMessage {
        request_id: request_id.clone(),
        text: turn.user_prompt.clone(),
        seq: turn.first_seq,
        agent_id: Some(checkpoint.agent_id.clone()),
    }));
    messages.push(ConversationMessage::Assistant(
        ConversationAssistantMessage {
            request_id,
            agent_id: Some(checkpoint.agent_id.clone()),
            text: checkpoint_turn_assistant_text(turn),
            tool_calls: Vec::new(),
            stop_reason: None,
            first_seq: turn.first_seq,
            last_seq: turn.last_seq,
            provider_id: None,
            model_id: None,
            output_digest: None,
        },
    ));
}

fn checkpoint_turn_assistant_text(turn: &ConversationCheckpointTurn) -> String {
    if turn.status.is_completed() {
        return turn.assistant_response.clone();
    }

    let status = match turn.status {
        ProviderConversationTurnStatus::Completed => "completed",
        ProviderConversationTurnStatus::Failed => "failed",
        ProviderConversationTurnStatus::Aborted => "aborted",
    };
    let mut text = format!(
        "Harness preserved an incomplete provider turn for continuity. Do not treat it as a completed answer.\nStatus: {status}"
    );
    if let Some(stage) = turn.failure_stage.as_deref() {
        text.push_str("\nStage: ");
        text.push_str(stage);
    }
    if let Some(reason) = turn.failure_reason.as_deref() {
        text.push_str("\nReason: ");
        text.push_str(reason);
    }
    if non_empty_trimmed(&turn.assistant_response).is_some() {
        text.push_str("\nPartial response:\n");
        text.push_str(&turn.assistant_response);
    }
    text
}

fn provider_turn_request_id(event: &EventEnvelopeV1, provider_request_id: &str) -> String {
    event
        .correlation_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(provider_request_id)
        .to_string()
}

fn ensure_seq_ordered(events: &[EventEnvelopeV1]) -> Result<(), ConversationProjectionError> {
    let mut previous_seq = None;
    for event in events {
        if let Some(previous_seq) = previous_seq {
            if event.seq < previous_seq {
                return Err(ConversationProjectionError::EventsOutOfOrder {
                    previous_seq,
                    seq: event.seq,
                });
            }
        }
        previous_seq = Some(event.seq);
    }
    Ok(())
}
