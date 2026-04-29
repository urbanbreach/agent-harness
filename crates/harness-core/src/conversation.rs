use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::agent::{ProviderContextCheckpoint, ProviderConversationTurn};
use crate::event::{EventArtifactRef, EventEnvelopeV1, EventV1, ToolCallMetadata, ToolCallStatus};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConversationProjectionError {
    #[error("events are not seq-ordered: event seq {seq} followed {previous_seq}")]
    EventsOutOfOrder { previous_seq: u64, seq: u64 },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
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
    pub request_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAssistantMessage {
    pub request_id: String,
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
    pub tool_call_id: String,
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
    pub request_id: String,
    pub tool_call_id: String,
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
    let mut checkpoint_refs = checkpoints.iter().collect::<Vec<_>>();
    checkpoint_refs.sort_by_key(|checkpoint| checkpoint.through_seq);

    for checkpoint in checkpoint_refs {
        projection.checkpoints.push(checkpoint.metadata());
        if !checkpoint.summary.trim().is_empty() {
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

    let skip_through_seq = checkpoints
        .iter()
        .map(|checkpoint| checkpoint.through_seq)
        .max()
        .unwrap_or(0);
    let mut request_states = BTreeMap::<String, RequestProjectionState>::new();
    let mut request_order = Vec::<OrderedConversationItem>::new();
    let mut emitted_users = BTreeSet::<String>::new();
    let mut emitted_assistants = BTreeSet::<String>::new();
    let mut tool_results = BTreeMap::<String, ToolResultProjectionState>::new();

    for event in events.iter().filter(|event| event.seq > skip_through_seq) {
        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let state = request_states
                    .entry(payload.request_id.clone())
                    .or_default();
                state.user = Some(ConversationUserMessage {
                    request_id: payload.request_id.clone(),
                    text: payload.text.clone(),
                    seq: Some(event.seq),
                    agent_id: event.actor.agent_id.clone(),
                });
                if emitted_users.insert(payload.request_id.clone()) {
                    request_order.push(OrderedConversationItem::User(payload.request_id.clone()));
                }
            }
            EventV1::ProviderRequestStarted(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let state = request_states.entry(request_id.clone()).or_default();
                state.assistant.first_seq.get_or_insert(event.seq);
                state.assistant.request_id = request_id.clone();
                state.assistant.agent_id = event.actor.agent_id.clone();
                state.assistant.provider_id = Some(payload.provider_id.clone());
                state.assistant.model_id = Some(payload.model_id.clone());
                if emitted_assistants.insert(request_id.clone()) {
                    request_order.push(OrderedConversationItem::Assistant(request_id));
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let state = request_states.entry(request_id.clone()).or_default();
                state.assistant.request_id = request_id;
                state.assistant.text.push_str(&payload.delta);
                state.assistant.last_seq = Some(event.seq);
            }
            EventV1::ProviderRequestFinished(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let state = request_states.entry(request_id.clone()).or_default();
                state.assistant.request_id = request_id;
                state.assistant.stop_reason = Some(payload.finish_reason.clone());
                state.assistant.output_digest = payload.output_digest.clone();
                state.assistant.last_seq = Some(event.seq);
            }
            EventV1::AssistantMessageFinished(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let state = request_states.entry(request_id.clone()).or_default();
                state.assistant.request_id = request_id;
                state.assistant.last_seq = Some(event.seq);
            }
            EventV1::ToolCallRequested(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let state = request_states.entry(request_id.to_string()).or_default();
                state.tool_calls.push(ConversationToolCall {
                    tool_call_id: payload.tool_call_id.clone(),
                    tool_id: payload.tool_id.clone(),
                    args_summary: payload.args_summary.clone(),
                    args_digest: payload.args_digest.clone(),
                    seq: Some(event.seq),
                    metadata: payload.metadata.clone(),
                });
            }
            EventV1::ToolCallFinished(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                tool_results.insert(
                    payload.tool_call_id.clone(),
                    ToolResultProjectionState {
                        request_id: request_id.to_string(),
                        tool_call_id: payload.tool_call_id.clone(),
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
                if assistant.request_id.is_empty() {
                    assistant.request_id = request_id.clone();
                }
                assistant.tool_calls = state.tool_calls.clone();
                projection
                    .messages
                    .push(ConversationMessage::Assistant(assistant));

                for tool_call in &state.tool_calls {
                    if let Some(result) = tool_results.get(&tool_call.tool_call_id) {
                        projection
                            .messages
                            .push(ConversationMessage::ToolResult(Box::new(
                                ConversationToolResultMessage {
                                    request_id: result.request_id.clone(),
                                    tool_call_id: result.tool_call_id.clone(),
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
            request_id: turn.request_id.clone(),
            first_seq: turn.first_seq,
            last_seq: turn.last_seq,
            artifacts: turn.artifacts.clone(),
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
    let request_id = turn
        .request_id
        .clone()
        .unwrap_or_else(|| checkpoint.through_request_id.clone().unwrap_or_default());

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
            text: turn.assistant_response.clone(),
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

fn provider_turn_request_id(event: &EventEnvelopeV1, provider_request_id: &str) -> String {
    event
        .correlation_id
        .as_deref()
        .map(str::trim)
        .filter(|request_id| !request_id.is_empty())
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
