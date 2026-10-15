//! Compaction input for the OpenAI lanes: the live history made safe to send, and the
//! identity strings (cache key, routing session) the lanes carry.
use crate::{CompletionMessage, CompletionRequest, MessageRole, ProviderContinuation};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

/// Longest id Responses accepts, in UTF-16 code units.
const MAX_ID_UNITS: usize = 64;
/// Item id prefixes a provider generates; a payload from another model cannot reuse them.
const GENERATED_ID_PREFIXES: [&str; 3] = ["fc_", "fcr_", "ctc_"];

/// Drops tool results without a call and demotes calls without a usable result to a note,
/// so the encoder never rejects history the live turn could still produce.
pub(super) fn sanitize(mut req: CompletionRequest) -> CompletionRequest {
    let mut messages = std::mem::take(&mut req.messages);
    let mut keep = vec![true; messages.len()];
    let mut index = 0;
    while index < messages.len() {
        match messages[index].role {
            MessageRole::Tool => {
                keep[index] = false;
                index += 1;
            }
            MessageRole::Assistant if messages[index].assistant_tool_calls.is_some() => {
                let end = (index + 1..messages.len())
                    .find(|&next| messages[next].role != MessageRole::Tool)
                    .unwrap_or(messages.len());
                let answers: Vec<&str> = messages[index + 1..end]
                    .iter()
                    .filter_map(|m| m.tool_call_id.as_deref())
                    .collect();
                let (content, calls) = demote_calls(&messages[index], &answers);
                let mut ids: BTreeSet<String> =
                    calls.iter().map(|c| c.tool_call_id.clone()).collect();
                messages[index].content = content;
                messages[index].assistant_tool_calls = (!calls.is_empty()).then_some(calls);
                keep[index] = !messages[index].content.is_empty()
                    || messages[index].assistant_tool_calls.is_some();
                for next in index + 1..end {
                    keep[next] = messages[next]
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|id| ids.remove(id));
                }
                index = end;
            }
            _ => index += 1,
        }
    }
    let mut attachments = std::mem::take(&mut req.attachments);
    let mut content = std::mem::take(&mut req.message_content);
    let times = std::mem::take(&mut req.message_times_ms);
    for (old, message) in messages
        .into_iter()
        .enumerate()
        .filter(|(old, _)| keep[*old])
    {
        let new = req.messages.len();
        if let Some(value) = attachments.remove(&old) {
            req.attachments.insert(new, value);
        }
        if let Some(value) = content.remove(&old) {
            req.message_content.insert(new, value);
        }
        if let Some(time) = times.get(old) {
            req.message_times_ms.push(*time);
        }
        req.messages.push(message);
    }
    req
}

/// The assistant text with a note per unusable call, and the calls that stay.
fn demote_calls(
    message: &CompletionMessage,
    answers: &[&str],
) -> (String, Vec<crate::AssistantToolCall>) {
    let mut text = message.content.clone();
    let mut seen = BTreeSet::new();
    let mut usable = Vec::new();
    for call in message.assistant_tool_calls.iter().flatten() {
        let well_formed = !call.tool_call_id.is_empty()
            && seen.insert(call.tool_call_id.as_str())
            && answers.contains(&call.tool_call_id.as_str())
            && serde_json::from_str::<Value>(&call.arguments_json)
                .is_ok_and(|args| args.is_object());
        if well_formed {
            usable.push(call.clone());
        } else {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!(
                "[tool call {} did not complete]",
                call.function_name
            ));
        }
    }
    (text, usable)
}

/// Turns the encoder's plain items into Responses message items and applies the
/// compaction rules: assistant text is `output_text`, user images use `detail: auto`, an
/// overlong id is hashed and, when a payload came from another model, generated ids go.
pub(super) fn finish_items(items: &mut [Value], strip_generated_ids: bool) {
    for (index, item) in items.iter_mut().enumerate() {
        if item["type"].is_null() {
            if let Some(text) = item["content"].as_str() {
                let block = if item["role"] == "assistant" {
                    json!({"type":"output_text","text":text,"annotations":[]})
                } else {
                    json!({"type":"input_text","text":text})
                };
                item["content"] = json!([block]);
            }
            if item["role"].is_string() {
                item["type"] = "message".into();
            }
        }
        if item["type"] == "message" && item["role"] == "assistant" && item.get("id").is_none() {
            item["id"] = format!("msg_{index}").into();
        }
        for part in item["content"].as_array_mut().into_iter().flatten() {
            if part["type"] == "input_image" {
                part["detail"] = "auto".into();
            }
        }
        let overlong = item["id"]
            .as_str()
            .filter(|id| id.encode_utf16().count() > MAX_ID_UNITS)
            .map(|id| format!("msg_{}", &blake3::hash(id.as_bytes()).to_hex()[..32]));
        if let Some(id) = overlong {
            item["id"] = id.into();
        }
        if strip_generated_ids
            && item["id"]
                .as_str()
                .is_some_and(|id| GENERATED_ID_PREFIXES.iter().any(|p| id.starts_with(p)))
            && let Some(object) = item.as_object_mut()
        {
            object.remove("id");
        }
    }
}

/// True when a replayed payload belongs to another model than the one now compacting.
pub(super) fn payload_from_other_model(req: &CompletionRequest) -> bool {
    req.messages
        .iter()
        .filter_map(|m| m.continuation.as_deref())
        .any(|state| {
            state.data["type"] == "openaiResponsesHistory" && state.model_id != req.model_id
        })
}

/// The explicit previous payload, unless a message of the request already carries it or
/// it is not replacement history from this provider.
pub(super) fn unattached_previous<'a>(
    req: &CompletionRequest,
    extra: Option<&'a ProviderContinuation>,
) -> Option<&'a ProviderContinuation> {
    extra.filter(|state| {
        state.provider_id == req.provider_id
            && state.data["type"] == "openaiResponsesHistory"
            && !req
                .messages
                .iter()
                .any(|m| m.continuation.as_deref() == Some(*state))
    })
}

/// Previous replacement history from this provider, in order, wherever it is attached.
pub(super) fn previous_history<'a>(
    req: &'a CompletionRequest,
    extra: Option<&'a ProviderContinuation>,
) -> Vec<&'a Value> {
    req.messages
        .iter()
        .filter_map(|m| m.continuation.as_deref())
        .filter(|state| {
            state.provider_id == req.provider_id && state.data["type"] == "openaiResponsesHistory"
        })
        .chain(unattached_previous(req, extra))
        .flat_map(|state| state.data["items"].as_array().into_iter().flatten())
        .collect()
}

/// Cache and routing identities: Unicode-normalized, and a hashed stand-in when longer
/// than 64 UTF-16 units.
pub(super) fn identity(text: &str) -> String {
    let text: String = text.nfc().collect();
    if text.encode_utf16().count() > MAX_ID_UNITS {
        format!("pc_{}", &blake3::hash(text.as_bytes()).to_hex()[..32])
    } else {
        text
    }
}

/// The retained user messages as a request of their own, with their attachments.
pub(super) fn retained_request(
    req: &CompletionRequest,
    retained: &[CompletionMessage],
) -> CompletionRequest {
    let mut request = CompletionRequest {
        model_id: req.model_id.clone(),
        provider_id: req.provider_id.clone(),
        messages: retained.to_vec(),
        ..Default::default()
    };
    for (index, message) in retained.iter().enumerate() {
        if let Some(original) = req.messages.iter().position(|m| m == message) {
            if let Some(attachments) = req.attachments.get(&original) {
                request.attachments.insert(index, attachments.clone());
            }
            if let Some(content) = req.message_content.get(&original) {
                request.message_content.insert(index, content.clone());
            }
        }
    }
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AssistantToolCall;

    fn call(id: &str, arguments: &str) -> AssistantToolCall {
        AssistantToolCall {
            tool_call_id: id.into(),
            function_name: "read".into(),
            arguments_json: arguments.into(),
        }
    }

    fn result(id: &str) -> CompletionMessage {
        let mut message = CompletionMessage::text(MessageRole::Tool, "out");
        message.tool_call_id = Some(id.into());
        message
    }

    #[test]
    fn orphans_are_dropped_and_malformed_calls_demoted() {
        let mut assistant = CompletionMessage::text(MessageRole::Assistant, "");
        assistant.assistant_tool_calls = Some(vec![
            call("ok", "{}"),
            call("bad", "[1]"),
            call("lost", "{}"),
        ]);
        let request = CompletionRequest {
            model_id: "m".into(),
            messages: vec![
                result("stray"),
                CompletionMessage::text(MessageRole::User, "q"),
                assistant,
                result("ok"),
                result("bad"),
            ],
            ..Default::default()
        };
        let clean = sanitize(request);
        let roles: Vec<_> = clean.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [MessageRole::User, MessageRole::Assistant, MessageRole::Tool]
        );
        assert_eq!(
            clean.messages[1]
                .assistant_tool_calls
                .as_ref()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            clean.messages[1]
                .content
                .matches("did not complete")
                .count(),
            2
        );
        assert!(crate::wire::encode(&clean, crate::Protocol::Responses).is_ok());
    }

    #[test]
    fn items_get_native_shapes_and_bounded_ids() {
        let long = "x".repeat(70);
        let mut items = vec![
            json!({"role":"assistant","content":"hi"}),
            json!({"role":"user","content":[{"type":"input_image","image_url":"data:"}]}),
            json!({"type":"message","role":"assistant","id":long,"content":[]}),
            json!({"type":"function_call","id":"fc_1","call_id":"c","name":"n","arguments":"{}"}),
        ];
        finish_items(&mut items, true);
        assert_eq!(
            items[0]["content"][0],
            json!({"type":"output_text","text":"hi","annotations":[]})
        );
        assert_eq!(items[0]["id"], "msg_0");
        assert_eq!(items[1]["content"][0]["detail"], "auto");
        assert!(items[2]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("msg_") && id.len() <= 64));
        assert!(items[3].get("id").is_none());
        assert_eq!(identity(&"y".repeat(65)).len(), 35);
        assert_eq!(identity("e\u{301}"), "\u{e9}");
    }
}
