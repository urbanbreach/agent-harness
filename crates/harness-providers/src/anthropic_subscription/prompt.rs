//! Prompt framing.
use super::tools::map_host_tool_name_to_sdk;
use crate::{CompletionRequest, MessageRole, ToolDef};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
mod images;
pub use images::*;
mod dedupe;
pub use dedupe::*;

/// senpi message content: a string, or `{type:"text"|"image", ...}` entries.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    Blocks(Vec<Value>),
}
impl Content {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Text(text) => json!(text),
            Self::Blocks(blocks) => Value::Array(blocks.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaneToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LaneMessage {
    User {
        content: Content,
    },
    Assistant {
        text: String,
        tool_calls: Vec<LaneToolCall>,
        timestamp: i64,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Content,
        is_error: bool,
        timestamp: i64,
    },
}

/// senpi `Context`, derived from a harness request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaneContext {
    pub system_prompt: Option<String>,
    pub messages: Vec<LaneMessage>,
    pub tools: Option<Vec<ToolDef>>,
}

impl LaneContext {
    pub fn from_request(request: &CompletionRequest) -> Self {
        let mut system = Vec::new();
        let mut messages = Vec::new();
        let mut call_names = HashMap::new();
        let mut leading = true;
        for (index, message) in request.messages.iter().enumerate() {
            let timestamp = i64::try_from(index).unwrap_or(i64::MAX);
            let content = || content_of(request, index, &message.content);
            match message.role {
                MessageRole::System => {
                    if leading {
                        system.push(message.content.clone());
                    }
                    continue;
                }
                MessageRole::User => messages.push(LaneMessage::User { content: content() }),
                MessageRole::Assistant => {
                    let tool_calls = message
                        .assistant_tool_calls
                        .iter()
                        .flatten()
                        .map(|call| {
                            let name = request
                                .tools
                                .iter()
                                .flatten()
                                .find(|tool| tool.tool_id == call.function_name)
                                .map_or_else(
                                    || crate::tool_function_name(&call.function_name),
                                    |tool| tool.function_name.clone(),
                                );
                            call_names.insert(call.tool_call_id.clone(), name.clone());
                            LaneToolCall {
                                id: call.tool_call_id.clone(),
                                name,
                                arguments: serde_json::from_str(&call.arguments_json)
                                    .unwrap_or_else(|_| json!({})),
                            }
                        })
                        .collect();
                    messages.push(LaneMessage::Assistant {
                        text: message.content.clone(),
                        tool_calls,
                        timestamp,
                    });
                }
                MessageRole::Tool => {
                    let id = message.tool_call_id.clone().unwrap_or_default();
                    let tool_name = call_names
                        .get(&id)
                        .cloned()
                        .or_else(|| message.name.clone())
                        .unwrap_or_default();
                    messages.push(LaneMessage::ToolResult {
                        tool_call_id: id,
                        tool_name,
                        content: content(),
                        is_error: false,
                        timestamp,
                    });
                }
            }
            leading = false;
        }
        Self {
            system_prompt: (!system.is_empty()).then(|| system.join("\n\n")),
            messages,
            tools: request.tools.clone(),
        }
    }
}

fn content_of(request: &CompletionRequest, index: usize, text: &str) -> Content {
    let Some(attachments) = request.attachments.get(&index).filter(|a| !a.is_empty()) else {
        return Content::Text(text.into());
    };
    let mut blocks = Vec::new();
    if !text.is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }
    for attachment in attachments {
        match attachment.bytes() {
            Ok(bytes) if attachment.mime == "text/plain" => blocks.push(json!({
                "type": "text",
                "text": String::from_utf8_lossy(bytes),
            })),
            Ok(bytes) => blocks.push(json!({
                "type": "image",
                "mimeType": attachment.mime,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            })),
            Err(_) => blocks.push(json!({"type": "image", "mimeType": attachment.mime})),
        }
    }
    Content::Blocks(blocks)
}

// ---- content blocks ----

const IMAGE_MEDIA_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

pub fn is_sdk_image_media_type(value: &str) -> bool {
    IMAGE_MEDIA_TYPES.contains(&value)
}

fn text_block(text: impl Into<String>) -> Value {
    json!({"type": "text", "text": text.into()})
}

fn omitted_placeholder(entry: &Value) -> String {
    match entry["type"].as_str() {
        Some("image") => {
            if entry["mimeType"].is_string() && entry["data"].is_string() {
                format!(
                    "[image block omitted: unsupported media type {}]",
                    entry["mimeType"].as_str().unwrap_or("")
                )
            } else {
                "[image block omitted: missing data]".into()
            }
        }
        Some(kind) => format!("[unsupported content block omitted: {kind}]"),
        None => "[unsupported content block omitted]".into(),
    }
}

fn append_entry(blocks: &mut Vec<Value>, entry: &Value) -> bool {
    if let Some(text) = entry.as_str() {
        blocks.push(text_block(text));
        return !text.trim().is_empty();
    }
    if !entry.is_object() {
        blocks.push(text_block(omitted_placeholder(entry)));
        return true;
    }
    match entry["type"].as_str() {
        Some("text") => match entry["text"].as_str() {
            Some(text) => {
                blocks.push(text_block(text));
                !text.trim().is_empty()
            }
            None => {
                blocks.push(text_block(omitted_placeholder(entry)));
                true
            }
        },
        Some("image") => {
            let mime = entry["mimeType"]
                .as_str()
                .filter(|m| is_sdk_image_media_type(m));
            match (mime, entry["data"].as_str()) {
                (Some(mime), Some(data)) => {
                    blocks.push(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}));
                    false
                }
                _ => {
                    blocks.push(text_block(omitted_placeholder(entry)));
                    true
                }
            }
        }
        _ => {
            blocks.push(text_block(omitted_placeholder(entry)));
            true
        }
    }
}

/// Appends SDK blocks; true when they carry visible text.
pub fn append_sdk_content_blocks(blocks: &mut Vec<Value>, content: &Content) -> bool {
    match content {
        Content::Text(text) => {
            if !text.is_empty() {
                blocks.push(text_block(text));
            }
            !text.trim().is_empty()
        }
        Content::Blocks(entries) => {
            let mut has_text = false;
            for entry in entries {
                has_text = append_entry(blocks, entry) || has_text;
            }
            has_text
        }
    }
}

// ---- prompt bridge ----

fn arguments_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

pub fn assistant_text(
    text: &str,
    tool_calls: &[LaneToolCall],
    custom: Option<&BTreeMap<String, String>>,
) -> String {
    let mut parts = Vec::new();
    if !text.is_empty() {
        parts.push(text.to_owned());
    }
    for call in tool_calls {
        parts.push(format!(
            "Historical tool call (non-executable): {} args={}",
            map_host_tool_name_to_sdk(&call.name, custom),
            arguments_json(&call.arguments)
        ));
    }
    parts.join("\n")
}

pub const PROMPT_INSTRUCTION: &str = "The above is the conversation history so far, provided as context. Respond as the assistant to the user message below only. Never emit \"USER:\" or \"ASSISTANT:\" labels or continue the transcript.";

/// The cold-seed prompt: history as labeled text, then the final user message.
pub fn build_prompt_blocks(
    context: &LaneContext,
    custom: Option<&BTreeMap<String, String>>,
    tool_watch_note: Option<&str>,
) -> Vec<Value> {
    let mut blocks = Vec::new();
    let final_user = match context.messages.last() {
        Some(LaneMessage::User { content }) => Some(content),
        _ => None,
    };
    let history = if final_user.is_some() {
        &context.messages[..context.messages.len() - 1]
    } else {
        &context.messages[..]
    };
    if !history.is_empty() {
        blocks.push(text_block("<conversation_history>\n"));
        let mut has_previous = false;
        let mut prefix = |blocks: &mut Vec<Value>, label: String| {
            blocks.push(text_block(format!(
                "{}{label}\n",
                if has_previous { "\n\n" } else { "" }
            )));
            has_previous = true;
        };
        let replayed =
            replay_history_images(history, &|name| map_host_tool_name_to_sdk(name, custom));
        for (index, message) in history.iter().enumerate() {
            let replay = |content: &Content| {
                replayed
                    .get(&index)
                    .map_or_else(|| content.clone(), |blocks| Content::Blocks(blocks.clone()))
            };
            match message {
                LaneMessage::User { content } => {
                    prefix(&mut blocks, "USER:".into());
                    if !append_sdk_content_blocks(&mut blocks, &replay(content)) {
                        blocks.push(text_block("(see attached image)"));
                    }
                }
                LaneMessage::Assistant {
                    text, tool_calls, ..
                } => {
                    prefix(&mut blocks, "ASSISTANT:".into());
                    let text = assistant_text(text, tool_calls, custom);
                    if !text.is_empty() {
                        blocks.push(text_block(text));
                    }
                }
                LaneMessage::ToolResult {
                    tool_call_id,
                    tool_name,
                    content,
                    ..
                } => {
                    prefix(
                        &mut blocks,
                        format!(
                            "TOOL RESULT (historical {}, id={tool_call_id}):",
                            map_host_tool_name_to_sdk(tool_name, custom)
                        ),
                    );
                    if !append_sdk_content_blocks(&mut blocks, &replay(content)) {
                        blocks.push(text_block("(see attached image)"));
                    }
                }
            }
        }
        blocks.push(text_block("\n</conversation_history>"));
    }
    if let Some(note) = tool_watch_note.map(str::trim).filter(|n| !n.is_empty()) {
        blocks.push(text_block("<recovered_tool_results>\n"));
        blocks.push(text_block(note));
        blocks.push(text_block("\n</recovered_tool_results>"));
    }
    blocks.push(text_block(PROMPT_INSTRUCTION));
    if let Some(content) = final_user
        && !append_sdk_content_blocks(&mut blocks, content)
    {
        blocks.push(text_block("(see attached image)"));
    }
    blocks
}

/// The single-message prompt stream of a non-resident query.
pub fn prompt_message(blocks: Vec<Value>) -> Value {
    json!({
        "type": "user",
        "message": {"role": "user", "content": blocks},
        "parent_tool_use_id": null,
        "session_id": "prompt",
    })
}

/// Resident delta: only the user/tool messages after the synced prefix.
pub fn build_delta_prompt_blocks(
    messages: &[&LaneMessage],
    custom: Option<&BTreeMap<String, String>>,
) -> Vec<Value> {
    let mut blocks = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index > 0 {
            blocks.push(text_block("\n\n"));
        }
        let content = match message {
            LaneMessage::ToolResult {
                tool_call_id,
                tool_name,
                content,
                ..
            } => {
                blocks.push(text_block(format!(
                    "Tool result ({}, id={tool_call_id}):\n",
                    map_host_tool_name_to_sdk(tool_name, custom)
                )));
                content
            }
            LaneMessage::User { content } => content,
            LaneMessage::Assistant { .. } => continue,
        };
        if *content == Content::Text(String::new()) {
            blocks.push(text_block(""));
        } else {
            append_sdk_content_blocks(&mut blocks, content);
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_keeps_the_last_directive_and_skips_nesting() {
        let block = |t: &str| text_block(t);
        let result = dedupe_ultrawork_blocks(vec![
            block("a <ultrawork-mode>1</ultrawork-mode> b"),
            block("<ultrawork-mode>2</ultrawork-mode>"),
        ]);
        assert_eq!(result.collapsed_directives, 1);
        assert_eq!(
            result.blocks[0]["text"],
            format!("a {} b", dedupe::SUPERSEDED_PLACEHOLDER)
        );
        assert_eq!(
            result.blocks[1]["text"],
            "<ultrawork-mode>2</ultrawork-mode>"
        );
        let nested = dedupe_ultrawork_blocks(vec![
            block("<ultrawork-mode>x"),
            block("<ultrawork-mode>y</ultrawork-mode></ultrawork-mode>"),
        ]);
        assert_eq!(nested.collapsed_directives, 0);
    }

    #[test]
    fn history_images_are_deduplicated_labeled_and_noted() {
        let png = base64::engine::general_purpose::STANDARD.encode(b"png-bytes");
        let image = json!({"type": "image", "mimeType": "image/png", "data": png});
        let context = LaneContext {
            messages: vec![
                LaneMessage::User {
                    content: Content::Blocks(vec![image.clone()]),
                },
                LaneMessage::Assistant {
                    text: String::new(),
                    tool_calls: vec![],
                    timestamp: 1,
                },
                LaneMessage::ToolResult {
                    tool_call_id: "c1".into(),
                    tool_name: "read".into(),
                    content: Content::Blocks(vec![
                        image,
                        json!({"type": "image", "mimeType": "image/png", "data": "!!"}),
                    ]),
                    is_error: false,
                    timestamp: 2,
                },
                LaneMessage::User {
                    content: Content::Text("now".into()),
                },
            ],
            ..LaneContext::default()
        };
        let blocks = build_prompt_blocks(&context, None, None);
        let texts: Vec<_> = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
        assert!(texts.iter().any(
            |t| t.contains("identical to an image already shown above (attached by the user)")
        ));
        assert!(texts.iter().any(|t| t.contains(
            "image returned by the Read tool: omitted because its image data is missing"
        )));
        assert_eq!(blocks.iter().filter(|b| b["type"] == "image").count(), 1);
        assert_eq!(texts.last(), Some(&"now"));
    }
}
