use super::Protocol;
use crate::{CacheRetention, CompletionRequest, MessageRole, ToolChoice};
use serde_json::{json, Value};
mod messages;
use messages::messages;

pub(crate) fn encode(req: &CompletionRequest, protocol: Protocol) -> Result<Value, &'static str> {
    encode_at(req, protocol, None)
}

/// Encodes for the deployment at `host` (`None` = the provider's built-in endpoint).
pub(crate) fn encode_at(
    req: &CompletionRequest,
    protocol: Protocol,
    host: Option<&str>,
) -> Result<Value, &'static str> {
    if req
        .tools
        .as_ref()
        .is_some_and(|tools| tools.len() > crate::MAX_TOOL_DEFINITIONS)
    {
        return Err("request exceeds 128 tool definitions");
    }
    crate::attachment_protocol::validate_request_at(req, protocol, host).map_err(|e| e.0)?;
    if req.model_id.trim().is_empty() || req.max_tokens == Some(0) {
        return Err("model and positive output limit are required");
    }
    if req
        .temperature
        .is_some_and(|n| !n.is_finite() || !(0.0..=2.0).contains(&n))
    {
        return Err("invalid temperature");
    }
    for &index in req.message_content.keys() {
        if req
            .messages
            .get(index)
            .is_none_or(|m| !matches!(m.role, MessageRole::User | MessageRole::Tool))
        {
            return Err("ordered content requires a user or tool message");
        }
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
        anthropic_thinking(&mut body, req)?;
    } else if req.context.cache_retention != CacheRetention::None
        && let Some(key) = req
            .context
            .cache_key
            .as_ref()
            .or(req.context.session_id.as_ref())
    {
        body["prompt_cache_key"] = key.clone().into();
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

fn anthropic_thinking(body: &mut Value, req: &CompletionRequest) -> Result<(), &'static str> {
    let effort = match req.reasoning_effort.as_deref() {
        Some("minimal") => Some("low"),
        Some("xhigh") => Some("high"),
        Some("max") if !req.model_id.contains("opus") => Some("high"),
        effort => effort,
    };
    if let Some(thinking) = &req.thinking {
        body["thinking"] = thinking.clone();
        if thinking["type"] == "adaptive"
            && let Some(effort) = effort
        {
            body["output_config"]["effort"] = effort.into();
        }
        return Ok(());
    }
    let Some(effort) = effort else {
        return Ok(());
    };
    if effort == "none" {
        body["thinking"] = json!({"type":"disabled"});
        return Ok(());
    }
    let model = req.model_id.replace('.', "-");
    let parts: Vec<_> = model.split('-').collect();
    let version = parts.iter().enumerate().find_map(|(index, part)| {
        part.parse::<u32>()
            .ok()
            .filter(|n| (3..100).contains(n))
            .map(|major| {
                (
                    major,
                    parts
                        .get(index + 1)
                        .and_then(|p| p.parse::<u32>().ok())
                        .filter(|n| *n < 100)
                        .unwrap_or(0),
                )
            })
    });
    let adaptive = version.is_some_and(|version| version >= (4, 6));
    if adaptive {
        body["thinking"] = json!({"type":"adaptive"});
        body["output_config"]["effort"] = effort.into();
    } else {
        let budget = match effort {
            "minimal" | "low" => 1024,
            "medium" => 4096,
            "high" => 8192,
            "max" | "xhigh" => 16384,
            _ => return Err("unsupported Anthropic reasoning effort"),
        };
        let max = req.max_tokens.unwrap_or(4096);
        if max <= 1024 {
            return Err("Anthropic thinking requires max_tokens above 1024");
        }
        body["thinking"] = json!({"type":"enabled","budget_tokens":budget.min(max - 1)});
        if model.contains("opus-4-5") {
            body["output_config"]["effort"] = effort.into();
        }
    }
    Ok(())
}
