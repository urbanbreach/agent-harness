use std::collections::BTreeMap;

use tokio::sync::mpsc;

use super::stream_event::unsupported_tool_call_error;
use super::stream_payload::{OpenAiChatToolCallDeltaChunk, OpenAiResponsesEvent};
use super::{non_empty_string, warn_stream_send_failure};
use crate::ProviderStreamEvent;

#[derive(Debug, Default)]
pub(super) struct ChatToolCallState {
    accumulators: BTreeMap<String, ToolCallAccumulator>,
    call_ids_by_index: BTreeMap<usize, String>,
}

#[derive(Debug, Default)]
struct ToolCallAccumulator {
    function_name: Option<String>,
    arguments_json: String,
}

pub(super) async fn consume_tool_call_deltas(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &[OpenAiChatToolCallDeltaChunk],
    state: &mut ChatToolCallState,
) -> bool {
    for tool_call in tool_calls {
        let Some(tool_call_id) = resolve_tool_call_id(tool_call, state) else {
            let _ = tx
                .send(unsupported_tool_call_error(
                    "openai_compatible stream omitted tool_call_id for chat tool call delta",
                ))
                .await;
            return false;
        };

        let accumulator = state.accumulators.entry(tool_call_id.clone()).or_default();

        let mut function_name_delta = None;
        let mut arguments_delta = String::new();
        if let Some(function) = &tool_call.function {
            if let Some(name) = function.name.clone().filter(|name| !name.is_empty()) {
                accumulator.function_name = Some(name.clone());
                function_name_delta = Some(name);
            }

            if let Some(arguments) = function
                .arguments
                .as_ref()
                .filter(|value| !value.is_empty())
            {
                accumulator.arguments_json.push_str(arguments);
                arguments_delta = arguments.clone();
            }
        }

        if (function_name_delta.is_some() || !arguments_delta.is_empty())
            && tx
                .send(ProviderStreamEvent::ToolCallDelta {
                    tool_call_id,
                    function_name: function_name_delta,
                    arguments_delta,
                })
                .await
                .is_err()
        {
            return false;
        }
    }

    true
}

fn resolve_tool_call_id(
    tool_call: &OpenAiChatToolCallDeltaChunk,
    state: &mut ChatToolCallState,
) -> Option<String> {
    if let Some(tool_call_id) = tool_call.id.as_ref().filter(|id| !id.is_empty()) {
        let tool_call_id = tool_call_id.clone();
        state
            .call_ids_by_index
            .insert(tool_call.index, tool_call_id.clone());
        return Some(tool_call_id);
    }

    state.call_ids_by_index.get(&tool_call.index).cloned()
}

pub(super) async fn emit_tool_call_completions(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    state: &mut ChatToolCallState,
) -> bool {
    if state.accumulators.is_empty() {
        return true;
    }

    let pending = std::mem::take(&mut state.accumulators);
    state.call_ids_by_index.clear();

    for (tool_call_id, accumulator) in pending {
        let Some(function_name) = accumulator
            .function_name
            .as_ref()
            .filter(|name| !name.is_empty())
            .cloned()
        else {
            let _ = tx
                .send(unsupported_tool_call_error(format!(
                    "openai_compatible chat tool call `{tool_call_id}` missing function name"
                )))
                .await;
            return false;
        };

        if serde_json::from_str::<serde_json::Value>(&accumulator.arguments_json).is_err() {
            let _ = tx
                .send(unsupported_tool_call_error(format!(
                    "openai_compatible chat tool call `{tool_call_id}` produced invalid arguments JSON"
                )))
                .await;
            return false;
        }

        if tx
            .send(ProviderStreamEvent::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json: accumulator.arguments_json,
            })
            .await
            .is_err()
        {
            warn_stream_send_failure("chat.tool_call_complete");
            return false;
        }
    }

    true
}

#[derive(Debug, Default)]
pub(super) struct ResponsesToolCallState {
    tool_calls: BTreeMap<String, ResponsesToolCallAccumulator>,
}

#[derive(Debug, Default)]
struct ResponsesToolCallAccumulator {
    tool_call_id: Option<String>,
    function_name: Option<String>,
    arguments_json: String,
}

pub(super) async fn handle_responses_tool_item_added(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut ResponsesToolCallState,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item) = event.item else {
        return true;
    };

    if item.item_type != "function_call" {
        return true;
    }

    let Some(key) = item.id.clone().or_else(|| item.call_id.clone()) else {
        let _ = tx
            .send(unsupported_tool_call_error(
                "openai_compatible responses tool call is missing both item id and call id",
            ))
            .await;
        return false;
    };

    let state = tool_calls.tool_calls.entry(key.clone()).or_default();
    if let Some(call_id) = item.call_id {
        state.tool_call_id = Some(call_id);
    }
    if let Some(function_name) = item.name {
        state.function_name = Some(function_name);
    }

    if let Some(arguments_delta) = item.arguments.filter(|value| !value.is_empty()) {
        state.arguments_json.push_str(&arguments_delta);
        let tool_call_id = state.tool_call_id.clone().unwrap_or_else(|| key.clone());

        if tx
            .send(ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name: state.function_name.clone(),
                arguments_delta,
            })
            .await
            .is_err()
        {
            return false;
        }
    }

    true
}

pub(super) async fn handle_responses_arguments_delta(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut ResponsesToolCallState,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item_id) = event.item_id else {
        let _ = tx
            .send(unsupported_tool_call_error(
                "openai_compatible responses function_call_arguments.delta missing item_id",
            ))
            .await;
        return false;
    };

    let Some(arguments_delta) = event.delta.filter(|value| !value.is_empty()) else {
        return true;
    };

    let state_key =
        find_responses_tool_call_key(tool_calls, &item_id).unwrap_or_else(|| item_id.clone());
    let state = tool_calls.tool_calls.entry(state_key.clone()).or_default();
    state.arguments_json.push_str(&arguments_delta);

    let tool_call_id = state
        .tool_call_id
        .clone()
        .unwrap_or_else(|| state_key.clone());

    tx.send(ProviderStreamEvent::ToolCallDelta {
        tool_call_id,
        function_name: None,
        arguments_delta,
    })
    .await
    .is_ok()
}

pub(super) async fn handle_responses_tool_item_done(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut ResponsesToolCallState,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item) = event.item else {
        return true;
    };

    if item.item_type != "function_call" {
        return true;
    }

    let Some(key) = item
        .id
        .clone()
        .or_else(|| item.call_id.clone())
        .or(event.item_id)
    else {
        let _ = tx
            .send(unsupported_tool_call_error(
                "openai_compatible responses tool completion missing both item id and call id",
            ))
            .await;
        return false;
    };

    let state_key = find_responses_tool_call_key(tool_calls, &key).unwrap_or(key);
    let state = tool_calls.tool_calls.entry(state_key.clone()).or_default();

    if let Some(call_id) = item.call_id {
        state.tool_call_id = Some(call_id);
    }
    if let Some(function_name) = item.name {
        state.function_name = Some(function_name);
    }
    if let Some(arguments_json) = item.arguments.filter(|value| !value.is_empty()) {
        state.arguments_json = arguments_json;
    }

    let Some(completed_state) = tool_calls.tool_calls.remove(&state_key) else {
        return true;
    };

    if let Err(message) = emit_responses_tool_call_complete(tx, &state_key, completed_state).await {
        let _ = tx.send(unsupported_tool_call_error(message)).await;
        return false;
    }

    true
}

fn find_responses_tool_call_key(
    tool_calls: &ResponsesToolCallState,
    item_or_call_id: &str,
) -> Option<String> {
    if tool_calls.tool_calls.contains_key(item_or_call_id) {
        return Some(item_or_call_id.to_string());
    }

    tool_calls.tool_calls.iter().find_map(|(key, state)| {
        (state.tool_call_id.as_deref() == Some(item_or_call_id)).then(|| key.clone())
    })
}

pub(super) async fn emit_pending_responses_tool_call_completions(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut ResponsesToolCallState,
) -> Result<(), String> {
    let pending_tool_calls = std::mem::take(&mut tool_calls.tool_calls);
    for (state_key, state) in pending_tool_calls {
        emit_responses_tool_call_complete(tx, &state_key, state).await?;
    }
    Ok(())
}

async fn emit_responses_tool_call_complete(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    state_key: &str,
    state: ResponsesToolCallAccumulator,
) -> Result<(), String> {
    let tool_call_id = state.tool_call_id.unwrap_or_else(|| state_key.to_string());

    let Some(function_name) = state.function_name.filter(|value| !value.is_empty()) else {
        return Err(format!(
            "openai_compatible responses tool call `{tool_call_id}` missing function name"
        ));
    };

    let arguments_json = normalize_responses_arguments_json(state.arguments_json);
    serde_json::from_str::<serde_json::Value>(&arguments_json).map_err(|err| {
        format!(
            "openai_compatible responses tool call `{tool_call_id}` has malformed arguments JSON: {err}"
        )
    })?;

    tx.send(ProviderStreamEvent::ToolCallComplete {
        tool_call_id,
        function_name,
        arguments_json,
    })
    .await
    .map_err(|_| {
        warn_stream_send_failure("responses.tool_call_complete");
        "openai_compatible stream receiver closed while sending tool completion".to_string()
    })
}

fn normalize_responses_arguments_json(arguments_json: String) -> String {
    if non_empty_string(&arguments_json).is_none() {
        "{}".to_string()
    } else {
        arguments_json
    }
}
