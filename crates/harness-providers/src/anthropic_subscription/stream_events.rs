//! SDK `stream_event` -> harness stream events.
use super::tools::{map_sdk_tool_name_to_host, map_tool_args};
use crate::{CompletionUsage, ProviderStreamEvent as Event, ProviderStreamFinishedMetadata};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Debug, Clone)]
enum Block {
    Text {
        index: Option<u64>,
        text: String,
    },
    Thinking {
        index: Option<u64>,
    },
    Tool {
        index: Option<u64>,
        id: String,
        name: String,
        partial_json: String,
        arguments: Value,
        done: bool,
    },
}
impl Block {
    fn index(&self) -> Option<u64> {
        match self {
            Self::Text { index, .. } | Self::Thinking { index } | Self::Tool { index, .. } => {
                *index
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
}

/// The assistant output a turn builds, as senpi's `AssistantMessage` accumulates it.
#[derive(Debug, Default)]
pub struct StreamState {
    blocks: Vec<Block>,
    pub usage: Usage,
    usage_seen: bool,
    /// `stop` / `toolUse` / `length`.
    pub stop_reason: &'static str,
    raw_stop_reason: Option<String>,
    pub saw_stream_event: bool,
    fallback_text: Option<String>,
}

pub fn map_stop_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("tool_use") => "toolUse",
        Some("max_tokens") => "length",
        _ => "stop",
    }
}

fn count(value: &Value) -> Option<u32> {
    value.as_u64().map(|n| u32::try_from(n).unwrap_or(u32::MAX))
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            stop_reason: "stop",
            ..Self::default()
        }
    }

    pub fn update_usage(&mut self, usage: &Value) {
        if !usage.is_object() {
            return;
        }
        self.usage_seen = true;
        if let Some(n) = count(&usage["input_tokens"]) {
            self.usage.input = n;
        }
        if let Some(n) = count(&usage["output_tokens"]) {
            self.usage.output = n;
        }
        if let Some(n) = count(&usage["cache_read_input_tokens"]) {
            self.usage.cache_read = n;
        }
        if let Some(n) = count(&usage["cache_creation_input_tokens"]) {
            self.usage.cache_write = n;
        }
    }

    /// `applyStreamEvent`: one partial-message event, as harness events.
    pub fn apply(
        &mut self,
        event: &Value,
        custom_to_host: &BTreeMap<String, String>,
    ) -> Vec<Event> {
        self.saw_stream_event = true;
        let index = event["index"].as_u64();
        let find = |blocks: &[Block]| {
            blocks
                .iter()
                .position(|b| index.is_some() && b.index() == index)
        };
        match event["type"].as_str() {
            Some("message_start") => {
                self.update_usage(&event["message"]["usage"]);
                Vec::new()
            }
            Some("content_block_start") => {
                let block = &event["content_block"];
                match block["type"].as_str() {
                    Some("text") => self.blocks.push(Block::Text {
                        index,
                        text: String::new(),
                    }),
                    Some("thinking") => self.blocks.push(Block::Thinking { index }),
                    Some("tool_use") => {
                        let id = block["id"].as_str().unwrap_or("").to_owned();
                        let name = map_sdk_tool_name_to_host(
                            block["name"].as_str().unwrap_or(""),
                            Some(custom_to_host),
                        );
                        let arguments = if block["input"].is_object() {
                            block["input"].clone()
                        } else {
                            json!({})
                        };
                        self.blocks.push(Block::Tool {
                            index,
                            id: id.clone(),
                            name: name.clone(),
                            partial_json: String::new(),
                            arguments,
                            done: false,
                        });
                        return vec![Event::ToolCallDelta {
                            tool_call_id: id,
                            function_name: Some(name),
                            arguments_delta: String::new(),
                        }];
                    }
                    _ => {}
                }
                Vec::new()
            }
            Some("content_block_delta") => {
                let Some(position) = find(&self.blocks) else {
                    return Vec::new();
                };
                let delta = &event["delta"];
                match (delta["type"].as_str(), &mut self.blocks[position]) {
                    (Some("text_delta"), Block::Text { text, .. }) => {
                        let fragment = delta["text"].as_str().unwrap_or("");
                        text.push_str(fragment);
                        vec![Event::TextDelta(fragment.into())]
                    }
                    (Some("thinking_delta"), Block::Thinking { .. }) => {
                        vec![Event::ReasoningDelta(
                            delta["thinking"].as_str().unwrap_or("").into(),
                        )]
                    }
                    (
                        Some("input_json_delta"),
                        Block::Tool {
                            id, partial_json, ..
                        },
                    ) => {
                        let fragment = delta["partial_json"].as_str().unwrap_or("");
                        partial_json.push_str(fragment);
                        vec![Event::ToolCallDelta {
                            tool_call_id: id.clone(),
                            function_name: None,
                            arguments_delta: fragment.into(),
                        }]
                    }
                    _ => Vec::new(),
                }
            }
            Some("content_block_stop") => {
                let Some(position) = find(&self.blocks) else {
                    return Vec::new();
                };
                let block = &mut self.blocks[position];
                let events = match block {
                    Block::Tool {
                        id,
                        name,
                        partial_json,
                        arguments,
                        done,
                        ..
                    } => {
                        let parsed: Map<String, Value> =
                            serde_json::from_str(partial_json).unwrap_or_default();
                        *arguments = Value::Object(map_tool_args(name, &parsed));
                        *done = true;
                        vec![Event::ToolCallComplete {
                            tool_call_id: id.clone(),
                            function_name: name.clone(),
                            arguments_json: arguments.to_string(),
                        }]
                    }
                    _ => Vec::new(),
                };
                match block {
                    Block::Text { index, .. }
                    | Block::Thinking { index }
                    | Block::Tool { index, .. } => *index = None,
                }
                events
            }
            Some("message_delta") => {
                let reason = event["delta"]["stop_reason"].as_str();
                self.stop_reason = map_stop_reason(reason);
                self.raw_stop_reason = reason.map(str::to_owned);
                self.update_usage(&event["usage"]);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A successful terminal result: usage and stop reason (never downgrading `toolUse`),
    /// and the result text when nothing streamed.
    pub fn apply_success_result(&mut self, message: &Value) -> Vec<Event> {
        if message["usage"].is_object() {
            self.update_usage(&message["usage"]);
        }
        if let Some(reason) = message["stop_reason"].as_str()
            && self.stop_reason != "toolUse"
        {
            self.stop_reason = map_stop_reason(Some(reason));
            self.raw_stop_reason = Some(reason.into());
        }
        if self.saw_stream_event {
            return Vec::new();
        }
        let text = message["result"].as_str().unwrap_or("").to_owned();
        self.fallback_text = Some(text.clone());
        vec![Event::TextDelta(text)]
    }

    /// The text the turn produced, as the coordinator concatenates it.
    pub fn text(&self) -> String {
        let mut text: String = self
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if let Some(fallback) = &self.fallback_text {
            text.push_str(fallback);
        }
        text
    }

    /// Completed tool calls in output order: `(id, host name, arguments)`.
    pub fn tool_calls(&self) -> Vec<(String, String, Value)> {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Tool {
                    id,
                    name,
                    arguments,
                    done: true,
                    ..
                } => Some((id.clone(), name.clone(), arguments.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn completion_usage(&self) -> Option<CompletionUsage> {
        self.usage_seen.then(|| CompletionUsage {
            prompt_tokens: self.usage.input,
            completion_tokens: self.usage.output,
            total_tokens: self.usage.input.saturating_add(self.usage.output),
        })
    }

    pub fn done(&self) -> Event {
        Event::DoneWithMetadata {
            usage: self.completion_usage(),
            metadata: Some(ProviderStreamFinishedMetadata {
                provider_stop_reason: Some(
                    match self.stop_reason {
                        "toolUse" => "tool_use",
                        "length" => "max_tokens",
                        _ => self.raw_stop_reason.as_deref().unwrap_or("end_turn"),
                    }
                    .into(),
                ),
                cache_read_tokens: self.usage_seen.then_some(self.usage.cache_read),
                cache_write_tokens: self.usage_seen.then_some(self.usage.cache_write),
                usage_complete: self.usage_seen.then_some(true),
                ..ProviderStreamFinishedMetadata::default()
            }),
        }
    }
}

fn patterns(list: &[&str]) -> Vec<Regex> {
    list.iter().filter_map(|p| Regex::new(p).ok()).collect()
}
static OVERFLOW: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"^Context window exhausted: ",
        r"^The conversation is too long to resend \(about \d+ tokens, limit \d+\)",
        r"(?i)prompt (?:is )?too long",
        r"(?i)request_too_large",
        r"(?i)exceeds (?:(?:the|this) )?(?:model'?s )?context window",
        r"(?i)context[_ ]length[_ ]exceeded",
        r"(?i)too many tokens",
        r"(?i)token limit exceeded",
        r"(?i)(?:request[ _])?(?:body|entity|payload)[_ ]too[_ ]large",
    ])
});
static NON_OVERFLOW: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"(?i)^(Throttling error|Service unavailable):",
        r"(?i)rate limit",
        r"(?i)too many requests",
        r"(?i)tokens per (?:min|minute|hour|day)",
        r"(?i)\bTPM\b",
        r"(?i)\bRPM\b",
        r"(?i)quota exceeded",
        r"(?i)retry (?:after|in) \d",
        r"^429\b",
        r"(?i)status code 429",
        r"(?i)overloaded",
    ])
});

/// senpi `isContextOverflow` for an error message.
pub fn is_context_overflow(message: &str) -> bool {
    OVERFLOW.iter().any(|re| re.is_match(message))
        && !NON_OVERFLOW.iter().any(|re| re.is_match(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_blocks_map_names_and_arguments_and_keep_tool_use_stop() {
        let mut state = StreamState::new();
        let custom = BTreeMap::new();
        state.apply(&json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "t1", "name": "Read", "input": {}}}), &custom);
        state.apply(&json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"file_path\":\"/x\",\"limit\":5}"}}), &custom);
        let events = state.apply(&json!({"type": "content_block_stop", "index": 0}), &custom);
        assert!(
            matches!(&events[..], [Event::ToolCallComplete { function_name, arguments_json, .. }]
            if function_name == "read" && arguments_json == r#"{"filePath":"/x","limit":5}"#)
        );
        state.apply(&json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 7}}), &custom);
        state.apply_success_result(
            &json!({"stop_reason": "end_turn", "usage": {"input_tokens": 9, "output_tokens": 8}}),
        );
        assert_eq!(state.stop_reason, "toolUse");
        assert_eq!(state.usage.output, 8);
    }
}
