use super::{CompletionRequest, MessageRole, Protocol};
use serde_json::{json, Value};

#[expect(
    clippy::cognitive_complexity,
    reason = "Existing protocol serializer also handles transient native replay."
)]
pub(super) fn messages(
    req: &CompletionRequest,
    protocol: Protocol,
) -> Result<Vec<Value>, &'static str> {
    let mut result = Vec::new();
    let mut pending = std::collections::BTreeSet::new();
    let mut tool_media = Vec::new();
    let mut files = Vec::new();
    for (index, message) in req.messages.iter().enumerate() {
        let attached = req.attachments.get(&index).map_or(&[][..], Vec::as_slice);
        let ordered = req.message_content.get(&index);
        if message.role == MessageRole::User
            && let Some(state) = message
                .continuation
                .as_deref()
                .filter(|s| s.protocol == protocol && s.provider_id == req.provider_id)
        {
            if protocol == Protocol::Responses
                && state.data["type"] == "openaiResponsesHistory"
                && let Some(items) = state.data["items"].as_array()
            {
                result.extend(items.iter().cloned());
                continue;
            }
            if protocol == Protocol::Anthropic
                && crate::native_compaction::anthropic_capable(&req.model_id)
                && state.data["type"] == "anthropicCompaction"
            {
                result.push(json!({"role":"assistant","content":[state.data["block"].clone()]}));
                let summary_at = req.message_times_ms.get(index).copied().flatten();
                queue_anthropic_files(state, index, req.messages.len(), summary_at, &mut files);
                continue;
            }
        }
        let continuation = message
            .continuation
            .as_deref()
            .filter(|s| message.role == MessageRole::Assistant && s.matches(req, protocol));
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        if protocol == Protocol::Anthropic && message.role == MessageRole::System {
            continue;
        }
        if message.role == MessageRole::Tool {
            let id = message
                .tool_call_id
                .as_ref()
                .ok_or("tool result lacks a call id")?;
            if !pending.remove(id) {
                return Err("tool result has no matching call");
            }
            match protocol {
                Protocol::Chat => {
                    // Chat tool messages carry text only: the leading text stays, and
                    // everything from the first attachment on follows as a user message.
                    let (text, rest) = match ordered {
                        Some(parts) => {
                            let split = parts
                                .iter()
                                .position(|p| matches!(p, crate::MessageContentPart::Attachment(_)))
                                .unwrap_or(parts.len());
                            let text = parts[..split]
                                .iter()
                                .filter_map(|p| match p {
                                    crate::MessageContentPart::Text(text) => Some(text.as_str()),
                                    crate::MessageContentPart::Attachment(_) => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            (text, Some(&parts[split..]))
                        }
                        None => (message.content.clone(), None),
                    };
                    result.push(json!({"role":role,"tool_call_id":id,"content":text}));
                    let media = match rest {
                        Some(parts) if !attached.is_empty() => {
                            ordered_content_blocks(parts, attached, protocol)?
                        }
                        Some(_) => Vec::new(),
                        None => content_blocks("", attached, protocol)?,
                    };
                    if !media.is_empty() {
                        tool_media.push(json!({"type":"text","text":format!("Attachments returned by tool call {id}.")}));
                        tool_media.extend(media);
                    }
                    if pending.is_empty() && !tool_media.is_empty() {
                        result
                            .push(json!({"role":"user","content":std::mem::take(&mut tool_media)}));
                    }
                }
                Protocol::Responses | Protocol::Anthropic => {
                    let content = if let Some(parts) = ordered {
                        ordered_content_blocks(parts, attached, protocol)?.into()
                    } else if attached.is_empty() {
                        Value::String(message.content.clone())
                    } else {
                        content_blocks(&message.content, attached, protocol)?.into()
                    };
                    if protocol == Protocol::Responses {
                        result.push(
                            json!({"type":"function_call_output","call_id":id,"output":content}),
                        );
                        continue;
                    }
                    let block = json!({"type":"tool_result","tool_use_id":id,"content":content});
                    if let Some(Value::Array(content)) = result
                        .last_mut()
                        .filter(|v| v["role"] == "user")
                        .and_then(|v| v.get_mut("content"))
                    {
                        content.push(block);
                    } else {
                        result.push(json!({"role":"user","content":[block]}));
                    }
                }
            }
            continue;
        }
        if !pending.is_empty() {
            return Err("assistant tool calls lack results");
        }
        if protocol == Protocol::Anthropic {
            let message_at = req.message_times_ms.get(index).copied().flatten();
            files.retain(|file| {
                if file.due(index, message_at) {
                    result
                        .push(json!({"role":"user","content":[{"type":"text","text":file.text}]}));
                    false
                } else {
                    true
                }
            });
        }
        let mut calls = Vec::new();
        for call in message.assistant_tool_calls.iter().flatten() {
            if message.role != MessageRole::Assistant
                || call.tool_call_id.is_empty()
                || !pending.insert(&call.tool_call_id)
            {
                return Err("invalid assistant tool call");
            }
            let args: Value =
                serde_json::from_str(&call.arguments_json).map_err(|_| "invalid tool arguments")?;
            if !args.is_object() {
                return Err("tool arguments must be an object");
            }
            let name = req
                .tools
                .iter()
                .flatten()
                .find(|tool| tool.tool_id == call.function_name)
                .map_or_else(
                    || crate::tool_function_name(&call.function_name),
                    |tool| tool.function_name.clone(),
                );
            calls.push(match protocol {
                Protocol::Chat => {
                    let mut item = json!({"id":call.tool_call_id,"type":"function","function":{"name":name,"arguments":call.arguments_json}});
                    if let Some(extra) = continuation.and_then(|s| s.data["tool_calls"].get(&call.tool_call_id)) {
                        item["extra_content"] = extra.clone();
                    }
                    item
                },
                Protocol::Responses => json!({"type":"function_call","call_id":call.tool_call_id,"name":name,"arguments":call.arguments_json}),
                Protocol::Anthropic => json!({"type":"tool_use","id":call.tool_call_id,"name":name,"input":args}),
            });
        }
        let attached = req.attachments.get(&index).filter(|a| !a.is_empty());
        let mut content = if let Some(parts) = ordered {
            ordered_content_blocks(parts, attached.map_or(&[], Vec::as_slice), protocol)?
        } else {
            content_blocks(
                &message.content,
                attached.map_or(&[], Vec::as_slice),
                protocol,
            )?
        };
        match protocol {
            Protocol::Chat => result.push(chat_message(
                message,
                calls,
                content,
                attached.is_some() || ordered.is_some(),
                continuation,
            )),
            Protocol::Responses => {
                if ordered.is_some() || !message.content.is_empty() || calls.is_empty() {
                    result.push(json!({"role":role,"content":if attached.is_some() || ordered.is_some() { Value::Array(content) } else { Value::String(message.content.clone()) }}));
                }
                result.extend(calls);
            }
            Protocol::Anthropic => {
                content.extend(calls);
                let content = if let Some(state) =
                    continuation.filter(|_| message.role == MessageRole::Assistant)
                {
                    state.data.clone()
                } else {
                    Value::Array(content)
                };
                if let Some(previous) = result.last_mut().filter(|v| {
                    v["role"] == "assistant"
                        && v["content"]
                            .as_array()
                            .is_some_and(|a| a.len() == 1 && a[0]["type"] == "compaction")
                }) && role == "assistant"
                    && let (Some(before), Some(after)) =
                        (previous["content"].as_array_mut(), content.as_array())
                {
                    before.extend(after.iter().cloned());
                    continue;
                }
                result.push(json!({"role":role,"content":content}));
            }
        }
    }
    if !pending.is_empty() {
        return Err("assistant tool calls lack results");
    }
    for file in files {
        result.push(json!({"role":"user","content":[{"type":"text","text":file.text}]}));
    }
    Ok(result)
}

fn ordered_content_blocks(
    parts: &[crate::MessageContentPart],
    attachments: &[crate::attachment_protocol::AttachmentMetadata],
    protocol: Protocol,
) -> Result<Vec<Value>, &'static str> {
    let mut content = Vec::with_capacity(parts.len());
    let mut next_attachment = 0;
    for part in parts {
        match part {
            crate::MessageContentPart::Text(text) => {
                let kind = if protocol == Protocol::Responses {
                    "input_text"
                } else {
                    "text"
                };
                if !text.is_empty() {
                    content.push(json!({"type":kind,"text":text}));
                }
            }
            crate::MessageContentPart::Attachment(index) => {
                if *index != next_attachment {
                    return Err("ordered attachment slots must preserve attachment order");
                }
                let attachment = attachments
                    .get(*index)
                    .ok_or("ordered attachment slot is missing")?;
                content.extend(content_blocks(
                    "",
                    std::slice::from_ref(attachment),
                    protocol,
                )?);
                next_attachment += 1;
            }
        }
    }
    if next_attachment != attachments.len() {
        return Err("ordered content omits attachments");
    }
    Ok(content)
}

fn content_blocks(
    text: &str,
    attachments: &[crate::attachment_protocol::AttachmentMetadata],
    protocol: Protocol,
) -> Result<Vec<Value>, &'static str> {
    use base64::Engine;
    if attachments.is_empty() && protocol != Protocol::Anthropic {
        return Ok(Vec::new());
    }
    let text_type = if protocol == Protocol::Responses {
        "input_text"
    } else {
        "text"
    };
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({"type":text_type,"text":text}));
    }
    for attachment in attachments {
        let bytes = attachment.bytes().map_err(|e| e.0)?;
        if attachment.mime == "text/plain" {
            content.push(json!({"type":text_type,"text":std::str::from_utf8(bytes).map_err(|_| "text attachment is not UTF-8")?}));
            continue;
        }
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let detail = attachment
            .detail
            .as_deref()
            .filter(|detail| matches!(*detail, "auto" | "low" | "high" | "original"));
        let url = format!("data:{};base64,{data}", attachment.mime);
        content.push(match protocol {
            Protocol::Chat => {
                let mut image = json!({"url":url});
                if let Some(detail) = detail {
                    image["detail"] = detail.into();
                }
                json!({"type":"image_url","image_url":image})
            }
            Protocol::Responses => {
                let mut image = json!({"type":"input_image","image_url":url});
                if let Some(detail) = detail {
                    image["detail"] = detail.into();
                }
                image
            }
            Protocol::Anthropic => json!({"type":"image","source":{"type":"base64","media_type":attachment.mime,"data":data}}),
        });
    }
    Ok(content)
}

fn chat_message(
    message: &crate::CompletionMessage,
    calls: Vec<Value>,
    content: Vec<Value>,
    attached: bool,
    continuation: Option<&crate::ProviderContinuation>,
) -> Value {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let mut item = json!({"role":role,"content":message.content});
    if attached {
        item["content"] = content.into();
    }
    if let Some(extra) = continuation
        .and_then(|s| s.data.get("extra_content"))
        .filter(|v| !v.is_null())
    {
        item["extra_content"] = extra.clone();
    }
    if let Some(name) = &message.name {
        item["name"] = name.clone().into();
    }
    if !calls.is_empty() {
        item["tool_calls"] = calls.into();
    }
    item
}

/// File metadata waiting behind a signed summary.
struct QueuedFile<'a> {
    /// Message position it waits for; the order when either time is unknown.
    after: usize,
    /// Time it is due after: the summary's own commit time or the carried tag.
    at: Option<u64>,
    text: &'a str,
}

impl QueuedFile<'_> {
    /// Due before the message at `index`, whose time is `message_at`: strictly earlier by
    /// time when both are known, else by position.
    fn due(&self, index: usize, message_at: Option<u64>) -> bool {
        match (self.at, message_at) {
            (Some(at), Some(message_at)) => at < message_at,
            _ => self.after < index,
        }
    }
}

fn queue_anthropic_files<'a>(
    state: &'a crate::ProviderContinuation,
    index: usize,
    count: usize,
    summary_at: Option<u64>,
    files: &mut Vec<QueuedFile<'a>>,
) {
    if let Some(text) = state.data["filesText"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        let legacy = if state.data["exactTail"] == true {
            count
        } else {
            1
        };
        let after = state.data["retainedTail"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(legacy);
        files.push(QueuedFile {
            after: index.saturating_add(after),
            at: summary_at,
            text,
        });
    }
    for file in state.data["retainedFiles"].as_array().into_iter().flatten() {
        let (Some(text), Some(after)) = (
            file["text"].as_str(),
            file["after"].as_u64().and_then(|n| usize::try_from(n).ok()),
        ) else {
            continue;
        };
        files.push(QueuedFile {
            after: index.saturating_add(after),
            at: file["at"].as_u64(),
            text,
        });
    }
    files.sort_by_key(|file| (file.at.is_none(), file.at, file.after));
}
