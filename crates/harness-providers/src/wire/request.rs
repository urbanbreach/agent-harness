use super::Protocol;
use crate::{CacheRetention, CompletionRequest, MessageRole, ToolChoice};
use serde_json::{json, Value};

pub(crate) fn encode(req: &CompletionRequest, protocol: Protocol) -> Result<Value, &'static str> {
    if req
        .tools
        .as_ref()
        .is_some_and(|tools| tools.len() > crate::MAX_TOOL_DEFINITIONS)
    {
        return Err("request exceeds 128 tool definitions");
    }
    crate::attachment_protocol::validate_request(req, protocol).map_err(|e| e.0)?;
    if req.model_id.trim().is_empty() || req.max_tokens == Some(0) {
        return Err("model and positive output limit are required");
    }
    if req
        .temperature
        .is_some_and(|n| !n.is_finite() || !(0.0..=2.0).contains(&n))
    {
        return Err("invalid temperature");
    }
    let messages = messages(req, protocol)?;
    let mut body = json!({"model":req.model_id,"stream":true});
    body[if protocol == Protocol::Responses {
        "input"
    } else {
        "messages"
    }] = messages.into();
    let output_key = match protocol {
        Protocol::Chat => "max_completion_tokens",
        Protocol::Responses => "max_output_tokens",
        Protocol::Anthropic => "max_tokens",
    };
    if let Some(n) = req.max_tokens {
        body[output_key] = n.into();
    }
    if let Some(n) = req.temperature {
        body["temperature"] = n.into();
    }
    if protocol == Protocol::Chat {
        body["stream_options"] = json!({"include_usage":true});
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning_effort"] = effort.clone().into();
        }
    }
    if protocol == Protocol::Responses {
        body["store"] = false.into();
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning"]["effort"] = effort.clone().into();
        }
        if let Some(summary) = &req.reasoning_summary {
            body["reasoning"]["summary"] = summary.clone().into();
        }
        if let Some(verbosity) = &req.text_verbosity {
            body["text"]["verbosity"] = verbosity.clone().into();
        }
    }
    if protocol == Protocol::Anthropic {
        body["max_tokens"] = req.max_tokens.unwrap_or(4096).into();
        let system = req
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::System)
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !system.is_empty() {
            body["system"] = system.into();
        }
        if let Some(thinking) = &req.thinking {
            body["thinking"] = thinking.clone();
        }
    } else if req.context.cache_retention != CacheRetention::None {
        if let Some(session) = &req.context.session_id {
            body["prompt_cache_key"] = session.clone().into();
        }
    }
    add_tools(&mut body, req, protocol)?;
    Ok(body)
}

fn add_tools(
    body: &mut Value,
    req: &CompletionRequest,
    protocol: Protocol,
) -> Result<(), &'static str> {
    if let Some(tools) = &req.tools {
        let mut definitions = Vec::with_capacity(tools.len());
        let mut names = std::collections::BTreeSet::new();
        for tool in tools {
            if tool.function_name.is_empty()
                || tool.function_name.len()
                    > if protocol == Protocol::Anthropic {
                        128
                    } else {
                        64
                    }
                || !tool
                    .function_name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || !names.insert(&tool.function_name)
                || tool.parameters["type"] != "object"
            {
                return Err("tools require distinct names and object schemas");
            }
            let mut definition = json!({"name":tool.function_name});
            if let Some(description) = &tool.description {
                definition["description"] = description.clone().into();
            }
            definition[if protocol == Protocol::Anthropic {
                "input_schema"
            } else {
                "parameters"
            }] = tool.parameters.clone();
            definitions.push(match protocol {
                Protocol::Chat => json!({"type":"function","function":definition}),
                Protocol::Responses => {
                    definition["type"] = "function".into();
                    definition["strict"] = false.into();
                    definition
                }
                Protocol::Anthropic => definition,
            });
        }
        if !definitions.is_empty() {
            body["tools"] = definitions.into();
            if let Some(choice) = req.tool_choice {
                let choice = if choice == ToolChoice::None {
                    "none"
                } else {
                    "auto"
                };
                body["tool_choice"] = if protocol == Protocol::Anthropic {
                    json!({"type":choice})
                } else {
                    choice.into()
                };
            }
        }
    }
    Ok(())
}

fn messages(req: &CompletionRequest, protocol: Protocol) -> Result<Vec<Value>, &'static str> {
    let mut result = Vec::new();
    let mut pending = std::collections::BTreeSet::new();
    let mut tool_media = Vec::new();
    for (index, message) in req.messages.iter().enumerate() {
        let attached = req.attachments.get(&index).map_or(&[][..], Vec::as_slice);
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
                    result.push(json!({"role":role,"tool_call_id":id,"content":message.content}));
                    if !attached.is_empty() {
                        tool_media.extend(content_blocks(
                            &format!("Attachments returned by tool call {id}."),
                            attached,
                            protocol,
                        )?);
                    }
                    if pending.is_empty() && !tool_media.is_empty() {
                        result
                            .push(json!({"role":"user","content":std::mem::take(&mut tool_media)}));
                    }
                }
                Protocol::Responses | Protocol::Anthropic => {
                    let content = if attached.is_empty() {
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
                Protocol::Chat => json!({"id":call.tool_call_id,"type":"function","function":{"name":name,"arguments":call.arguments_json}}),
                Protocol::Responses => json!({"type":"function_call","call_id":call.tool_call_id,"name":name,"arguments":call.arguments_json}),
                Protocol::Anthropic => json!({"type":"tool_use","id":call.tool_call_id,"name":name,"input":args}),
            });
        }
        let attached = req.attachments.get(&index).filter(|a| !a.is_empty());
        let mut content = content_blocks(
            &message.content,
            attached.map_or(&[], Vec::as_slice),
            protocol,
        )?;
        match protocol {
            Protocol::Chat => {
                let mut item = json!({"role":role,"content":message.content});
                if attached.is_some() {
                    item["content"] = content.into();
                }
                if let Some(name) = &message.name {
                    item["name"] = name.clone().into();
                }
                if !calls.is_empty() {
                    item["tool_calls"] = calls.into();
                }
                result.push(item);
            }
            Protocol::Responses => {
                if !message.content.is_empty() || calls.is_empty() {
                    result.push(json!({"role":role,"content":if attached.is_some() { Value::Array(content) } else { Value::String(message.content.clone()) }}));
                }
                result.extend(calls);
            }
            Protocol::Anthropic => {
                content.extend(calls);
                result.push(json!({"role":role,"content":content}));
            }
        }
    }
    if !pending.is_empty() {
        return Err("assistant tool calls lack results");
    }
    Ok(result)
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
        content.push(match protocol {
            Protocol::Chat => json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{data}",attachment.mime)}}),
            Protocol::Responses => json!({"type":"input_image","image_url":format!("data:{};base64,{data}",attachment.mime)}),
            Protocol::Anthropic => json!({"type":"image","source":{"type":"base64","media_type":attachment.mime,"data":data}}),
        });
    }
    Ok(content)
}
