use tokio_stream::StreamExt;

use crate::conversation::{project_conversation, ConversationMessage};
use std::collections::BTreeSet;

use crate::event::{EventEnvelopeV1, EventV1, UiIntentReceivedEvent};
use crate::store::EventStore;

use super::super::compaction::{
    extract_file_ops_from_tool_call, merge_file_operations, CutPointResult, FileOperations,
};
use super::super::provider_context::event_belongs_to_agent;
use super::super::{CoordinatorError, RunState};

pub(super) struct DurableCompactionState {
    pub(super) current_intent: Option<UiIntentReceivedEvent>,
    pub(super) read_files: Vec<String>,
    pub(super) modified_files: Vec<String>,
}

pub(super) fn durable_compaction_state(
    events: &[EventEnvelopeV1],
    agent_id: &str,
    read_files: Vec<String>,
    modified_files: Vec<String>,
) -> DurableCompactionState {
    let mut current_intent = None;
    let mut reads = BTreeSet::from_iter(read_files);
    let mut modified = BTreeSet::from_iter(modified_files);
    for event in events {
        match &event.payload {
            EventV1::UiIntentReceived(intent)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                current_intent = Some(intent.clone());
            }
            EventV1::SessionCompaction(compaction) if compaction.agent_id == agent_id => {
                if let Some(intent) = compaction.current_intent.as_ref() {
                    current_intent = Some(intent.clone());
                }
                reads.extend(compaction.read_files.iter().cloned());
                modified.extend(compaction.modified_files.iter().cloned());
            }
            _ => {}
        }
    }
    reads.retain(|path| !modified.contains(path));
    DurableCompactionState {
        current_intent,
        read_files: reads.into_iter().collect(),
        modified_files: modified.into_iter().collect(),
    }
}

pub(super) async fn collect_events(
    run_state: &RunState,
) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let stream = run_state.event_store.replay(1)?;
    let mut events = Vec::new();
    let mut stream = std::pin::pin!(stream);
    while let Some(result) = stream.next().await {
        events.push(result?);
    }
    Ok(events)
}

pub(super) fn build_agent_conversation_messages(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Vec<ConversationMessage> {
    let stream_key = format!("agent:{agent_id}");
    let agent_events = events
        .iter()
        .filter(|event| event_belongs_to_agent(event, agent_id, &stream_key))
        .cloned()
        .collect::<Vec<_>>();
    project_conversation(&agent_events, &[])
        .unwrap_or_default()
        .messages
}

pub(super) fn split_messages_at_cut_point(
    messages: &[ConversationMessage],
    cut_point: &CutPointResult,
) -> (
    Vec<ConversationMessage>,
    Vec<ConversationMessage>,
    Vec<ConversationMessage>,
) {
    if cut_point.is_split_turn {
        let turn_start_seq = cut_point.turn_start_seq.unwrap_or(0);
        let messages_to_summarize = messages
            .iter()
            .filter(|message| message_seq(message) < turn_start_seq)
            .cloned()
            .collect();
        let turn_prefix_messages = messages
            .iter()
            .filter(|message| {
                let seq = message_seq(message);
                seq >= turn_start_seq && seq < cut_point.first_kept_event_seq
            })
            .cloned()
            .collect();
        let preserved_messages = messages
            .iter()
            .filter(|message| message_seq(message) >= cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        (
            messages_to_summarize,
            turn_prefix_messages,
            preserved_messages,
        )
    } else {
        let messages_to_summarize = messages
            .iter()
            .filter(|message| message_seq(message) < cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        let preserved_messages = messages
            .iter()
            .filter(|message| message_seq(message) >= cut_point.first_kept_event_seq)
            .cloned()
            .collect();
        (messages_to_summarize, Vec::new(), preserved_messages)
    }
}

fn message_seq(message: &ConversationMessage) -> u64 {
    match message {
        ConversationMessage::User(message) => message.seq.unwrap_or(0),
        ConversationMessage::Assistant(message) => {
            message.last_seq.or(message.first_seq).unwrap_or(0)
        }
        ConversationMessage::ToolResult(message) => message.seq.unwrap_or(0),
        ConversationMessage::Checkpoint(message) => message.through_seq,
    }
}

pub(super) fn extract_file_ops_from_messages(
    messages_to_summarize: &[ConversationMessage],
    turn_prefix_messages: &[ConversationMessage],
) -> FileOperations {
    let mut operations = FileOperations::new();
    for message in messages_to_summarize
        .iter()
        .chain(turn_prefix_messages.iter())
    {
        let ConversationMessage::Assistant(assistant) = message else {
            continue;
        };
        for tool_call in &assistant.tool_calls {
            let Ok(arguments) = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
            else {
                continue;
            };
            if let Some(operation) = extract_file_ops_from_tool_call(&tool_call.tool_id, &arguments)
            {
                merge_file_operations(&mut operations, operation);
            }
        }
    }
    operations
}

pub(super) fn find_previous_summary(events: &[EventEnvelopeV1], agent_id: &str) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionCompaction(compaction) if compaction.agent_id == agent_id => {
            Some(compaction.summary.clone())
        }
        _ => None,
    })
}

pub(super) fn determine_model_ref(run_state: &RunState, agent_id: &str) -> String {
    for running in run_state.running_agent_turns.values() {
        if running.agent_id == agent_id {
            return running.model_ref.clone();
        }
    }
    run_state.agents.get(agent_id).map_or_else(
        || "default:default".to_string(),
        |profile| profile.model_ref.clone(),
    )
}
