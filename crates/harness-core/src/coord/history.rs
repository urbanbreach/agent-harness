use super::{
    context::{Context, Entry},
    *,
};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::collections::{HashMap, HashSet};
pub(super) const INTERRUPTED_TOOL_RESULT: &str =
    "Execution was interrupted. Its outcome is unknown; inspect current state before retrying.";

/// Provider context comes from settled records, grouped by turn rather than arrival time.
pub(super) fn messages(
    events: &[EventEnvelopeV1],
    agent: &str,
    primary: bool,
    system_prompt: &str,
    run_dir: &std::path::Path,
) -> Result<Context, CoordinatorError> {
    let events = crate::conversation_rewind::active_events(events);
    let mut turns: Vec<Vec<Entry>> = Vec::new();
    let mut users = HashMap::new();
    let mut providers = HashMap::new();
    let mut tools = HashMap::new();
    let mut started = HashSet::new();
    let mut cancelled = HashSet::new();
    let mut legacy_text: HashMap<String, String> = HashMap::new();
    let mut summaries = Vec::new();
    let mut request_seqs = Vec::new();
    let mut contexts = HashMap::new();
    for event in events.iter().filter(|e| {
        e.actor
            .agent_id
            .as_deref()
            .map_or(primary, |id| id == agent)
    }) {
        match &event.payload {
            EventV1::UserMessageSubmitted(e) => {
                users.insert(e.request_id.to_string(), turns.len());
                request_seqs.push((e.request_id.to_string(), event.seq));
                turns.push(vec![Entry {
                    message: CompletionMessage::text(MessageRole::User, &e.text),
                    seq: event.seq,
                    turn: Some(e.request_id.to_string()),
                    attachments: Vec::new(),
                    settled_reasoning: Vec::new(),
                    raw_tool_result: None,
                }]);
            }
            EventV1::PromptAttachmentsSubmitted(e) => {
                if let Some(turn) = users
                    .get(e.request_id.as_str())
                    .and_then(|index| turns.get_mut(*index))
                {
                    if let Some(user) = turn.first_mut() {
                        user.attachments.clone_from(&e.attachments);
                    }
                }
            }
            EventV1::ArtifactWritten(e) => {
                if let Some(request) = e.metadata.get("prompt_context") {
                    contexts.insert(request.clone(), e);
                }
            }
            EventV1::TaskCancelled(e) if e.task_scope != Some(TaskTerminalScope::ToolCall) => {
                cancelled.insert(e.task_id.to_string());
            }
            EventV1::ProviderRequestStarted(e) => {
                let turn = e
                    .metadata
                    .as_ref()
                    .and_then(|m| m.turn_id.as_ref())
                    .or(event.correlation_id.as_ref())
                    .and_then(|id| users.get(id))
                    .copied()
                    .or_else(|| turns.len().checked_sub(1));
                if let Some(turn) = turn {
                    if let Some((id, _)) = request_seqs.get(turn) {
                        started.insert(id.clone());
                    }
                    providers.insert(e.request_id.to_string(), turn);
                }
            }
            EventV1::ProviderStreamDelta(e) => {
                legacy_text
                    .entry(e.request_id.to_string())
                    .or_default()
                    .push_str(&e.delta);
            }
            EventV1::AssistantMessageFinished(e) => {
                let Some(index) = providers.get(e.request_id.as_str()).copied().or_else(|| {
                    event
                        .correlation_id
                        .as_ref()
                        .and_then(|id| users.get(id))
                        .copied()
                }) else {
                    continue;
                };
                let mut message = CompletionMessage::text(MessageRole::Assistant, "");
                let mut calls = Vec::new();
                let mut results = Vec::new();
                for part in &e.parts {
                    match part {
                        crate::session::AssistantPart::Text { text } => {
                            message.content.push_str(text)
                        }
                        crate::session::AssistantPart::ToolCall(call) => {
                            let provider_id = call
                                .provider_tool_call_id
                                .clone()
                                .unwrap_or_else(|| call.tool_call_id.to_string());
                            tools.insert(
                                call.tool_call_id.to_string(),
                                (index, turns[index].len() + 1 + results.len()),
                            );
                            // Reserve call order, including an explicit unknown result for interrupted tools.
                            results.push(Entry {
                                attachments: Vec::new(),
                                settled_reasoning: Vec::new(),
                                raw_tool_result: None,
                                seq: event.seq,
                                turn: Some(request_seqs[index].0.clone()),
                                message: CompletionMessage {
                                    role: MessageRole::Tool,
                                    content: INTERRUPTED_TOOL_RESULT.into(),
                                    tool_call_id: Some(provider_id.clone()),
                                    name: Some(call.tool_id.clone()),
                                    assistant_tool_calls: None,
                                },
                            });
                            calls.push(AssistantToolCall {
                                tool_call_id: provider_id,
                                function_name: call.tool_id.clone(),
                                arguments_json: canonical_arguments(&call.args_summary),
                            });
                        }
                        crate::session::AssistantPart::Reasoning { .. } => {}
                    }
                }
                if e.parts.is_empty() {
                    message.content = legacy_text
                        .remove(e.request_id.as_str())
                        .unwrap_or_default();
                }
                message.assistant_tool_calls = (!calls.is_empty()).then_some(calls);
                turns[index].push(Entry {
                    attachments: Vec::new(),
                    settled_reasoning: Vec::new(),
                    raw_tool_result: None,
                    message,
                    seq: event.seq,
                    turn: Some(request_seqs[index].0.clone()),
                });
                turns[index].extend(results);
            }
            EventV1::ToolCallFinished(e) => {
                if let Some((turn, result)) = tools.remove(e.tool_call_id.as_str()) {
                    turns[turn][result].attachments = e
                        .metadata
                        .as_ref()
                        .map(|m| m.attachments.clone())
                        .unwrap_or_default();
                    let summary = e.output_summary.clone().unwrap_or_else(|| match e.status {
                        ToolCallStatus::Succeeded => "Tool completed.".into(),
                        ToolCallStatus::Failed => "Tool execution failed.".into(),
                    });
                    let message = &mut turns[turn][result].message;
                    message.content = crate::tool::provider_text(
                        message.name.as_deref().unwrap_or_default(),
                        summary,
                        e.output_json.clone(),
                    );
                }
            }
            EventV1::SessionCompaction(e) if e.agent_id == agent => {
                summaries.push(e);
            }
            _ => {}
        }
    }
    let mut output = Context::new(system_prompt);
    output.unavailable = Some(crate::subagent::FinalizedStateUnavailable::LegacySummaryOnly);
    if let Some(summary) = summaries.last() {
        output.push(
            CompletionMessage::text(
                MessageRole::User,
                format!("Conversation summary:\n{}", summary.summary),
            ),
            0,
            None,
        );
    }
    let boundary = summaries
        .last()
        .map(|summary| {
            summary
                .first_kept_request_id
                .as_ref()
                .map(|id| users.get(id).copied())
                .unwrap_or_else(|| {
                    request_seqs
                        .iter()
                        .position(|(_, seq)| *seq >= summary.first_kept_event_seq)
                })
                .ok_or_else(|| {
                    CoordinatorError::Invalid(
                        "compaction retention boundary is missing from history".into(),
                    )
                })
        })
        .transpose()?;
    for (index, ((id, _), turn)) in request_seqs.into_iter().zip(turns).enumerate() {
        if cancelled.contains(&id) && !started.contains(&id) {
            continue;
        }
        if boundary.is_some_and(|first| index < first) {
            continue;
        }
        output.entries.extend(turn.into_iter().filter(|entry| {
            boundary.is_none_or(|first| index != first)
                || summaries
                    .last()
                    .is_none_or(|summary| entry.seq >= summary.first_kept_event_seq)
        }));
    }
    super::prompt::restore_content(&mut output, run_dir, &contexts)?;
    Ok(output)
}
fn canonical_arguments(summary: &str) -> String {
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .filter(serde_json::Value::is_object)
        .map_or_else(|| "{}".into(), |value| value.to_string())
}
