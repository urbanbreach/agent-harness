use super::Protocol;
use crate::{CompletionUsage, ProviderStreamEvent as Event, ProviderStreamFinishedMetadata};
use serde_json::Value;
use std::collections::BTreeMap;
mod anthropic;
mod continuation;

#[derive(Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
    initial_input: Option<Value>,
    extra_content: Option<Value>,
}

#[derive(Default)]
struct ThinkingBlock {
    text: String,
    finished: bool,
}

pub(crate) struct StreamDecoder {
    protocol: Protocol,
    tools: BTreeMap<u64, ToolCall>,
    usage: Option<CompletionUsage>,
    usage_prompt_tokens_seen: bool,
    usage_completion_tokens_seen: bool,
    metadata: ProviderStreamFinishedMetadata,
    chat_reasoning: String,
    chat_reasoning_seen: bool,
    chat_reasoning_available: bool,
    anthropic_thinking: BTreeMap<u64, ThinkingBlock>,
    anthropic_content: BTreeMap<u64, Value>,
    chat_extra_content: Option<Value>,
    continuation_bytes: usize,
    anthropic_reasoning_bytes: usize,
    anthropic_reasoning_unavailable: bool,
    pub done: bool,
    stopped: bool,
}

impl StreamDecoder {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            tools: BTreeMap::new(),
            usage: None,
            usage_prompt_tokens_seen: false,
            usage_completion_tokens_seen: false,
            metadata: ProviderStreamFinishedMetadata::default(),
            chat_reasoning: String::new(),
            chat_reasoning_seen: false,
            chat_reasoning_available: true,
            anthropic_thinking: BTreeMap::new(),
            anthropic_content: BTreeMap::new(),
            chat_extra_content: None,
            continuation_bytes: 0,
            anthropic_reasoning_bytes: 0,
            anthropic_reasoning_unavailable: false,
            done: false,
            stopped: false,
        }
    }

    pub fn frame(&mut self, frame: &str) -> Result<Vec<Event>, &'static str> {
        if self.done {
            return Ok(Vec::new());
        }
        if frame == "[DONE]" {
            if self.protocol != Protocol::Chat || !self.stopped {
                return Err("stream ended before completion");
            }
            return self.finish();
        }
        let value: Value = serde_json::from_str(frame).map_err(|_| "invalid provider JSON")?;
        if !value.is_object() {
            return Err("provider event must be an object");
        }
        if value.get("error").is_some() || value["type"] == "error" {
            return Err("provider reported a stream error");
        }
        match self.protocol {
            Protocol::Chat => self.chat(&value),
            Protocol::Responses => self.responses(&value),
            Protocol::Anthropic => self.anthropic(&value),
        }
    }

    fn chat(&mut self, value: &Value) -> Result<Vec<Event>, &'static str> {
        self.record_usage(&value["usage"], "prompt_tokens", "completion_tokens")?;
        let mut events = Vec::new();
        let choices = value["choices"].as_array().ok_or("missing chat choices")?;
        if choices.len() > 1 {
            return Err("multiple completion choices are unsupported");
        }
        for choice in choices {
            let delta = &choice["delta"];
            if let Some(extra) = delta.get("extra_content").filter(|v| !v.is_null()) {
                self.add_continuation_bytes(extra.to_string().len())?;
                self.chat_extra_content = Some(extra.clone());
            }
            if let Some(text) = delta["content"].as_str() {
                events.push(Event::TextDelta(text.into()));
            }
            let reasoning_content = delta.get("reasoning_content");
            let reasoning = delta.get("reasoning");
            if reasoning_content.is_some_and(|value| !value.is_null() && value.as_str().is_none())
                || reasoning.is_some_and(|value| !value.is_null() && value.as_str().is_none())
                || (reasoning_content.is_some_and(Value::is_string)
                    && reasoning.is_some_and(Value::is_string))
            {
                self.chat_reasoning_available = false;
            }
            if let Some(text) = reasoning_content
                .and_then(Value::as_str)
                .or_else(|| reasoning.and_then(Value::as_str))
            {
                self.chat_reasoning_seen = true;
                if append(&mut self.chat_reasoning, text).is_err() {
                    self.chat_reasoning_available = false;
                }
                events.push(Event::ReasoningDelta(text.into()));
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for call in calls {
                    let tool = self.chat_tool(call)?;
                    label(&mut tool.id, call["id"].as_str())?;
                    label(&mut tool.name, call["function"]["name"].as_str())?;
                    let fragment = call["function"]["arguments"].as_str().unwrap_or("");
                    append(&mut tool.arguments, fragment)?;
                    events.push(tool.delta(fragment));
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                if !matches!(reason, "stop" | "tool_calls" | "length") {
                    return Err("provider did not complete the request");
                }
                self.metadata.provider_stop_reason = Some(reason.into());
                if reason == "length" {
                    self.metadata.output_limit_reached = true;
                    self.chat_reasoning_available = false;
                }
                self.stopped = true;
            }
        }
        Ok(events)
    }

    fn responses(&mut self, value: &Value) -> Result<Vec<Event>, &'static str> {
        let mut events = Vec::new();
        match value["type"]
            .as_str()
            .ok_or("missing response event type")?
        {
            "response.output_text.delta" => {
                events.push(Event::TextDelta(text(value, "delta")?.into()))
            }
            "response.reasoning_summary_text.delta" => {
                events.push(Event::ReasoningDelta(text(value, "delta")?.into()))
            }
            "response.output_item.added" | "response.output_item.done"
                if value["item"]["type"] == "function_call" =>
            {
                let tool =
                    self.tool(value["output_index"].as_u64().ok_or("missing tool index")?)?;
                let item = &value["item"];
                label(&mut tool.id, Some(text(item, "call_id")?))?;
                label(&mut tool.name, Some(text(item, "name")?))?;
                if let Some(args) = item["arguments"].as_str().filter(|s| !s.is_empty()) {
                    if !tool.arguments.is_empty() && tool.arguments != args {
                        return Err("tool arguments changed at completion");
                    }
                    tool.arguments = args.into();
                }
            }
            "response.function_call_arguments.delta" => {
                let tool =
                    self.tool(value["output_index"].as_u64().ok_or("missing tool index")?)?;
                let fragment = text(value, "delta")?;
                append(&mut tool.arguments, fragment)?;
                events.push(tool.delta(fragment));
            }
            "response.completed" | "response.incomplete" => {
                let response = &value["response"];
                if value["type"] == "response.incomplete"
                    && response["status"] == "incomplete"
                    && response["incomplete_details"]["reason"] == "max_output_tokens"
                {
                    self.metadata.output_limit_reached = true;
                    self.metadata.provider_stop_reason = Some("max_output_tokens".into());
                } else if value["type"] != "response.completed" || response["status"] != "completed"
                {
                    return Err("response is incomplete");
                }
                self.metadata.provider_response_id = response["id"].as_str().map(str::to_owned);
                self.record_usage(&response["usage"], "input_tokens", "output_tokens")?;
                return self.finish();
            }
            "response.failed" => return Err("response is incomplete"),
            _ => {}
        }
        Ok(events)
    }

    fn record_usage(
        &mut self,
        value: &Value,
        input: &str,
        output: &str,
    ) -> Result<(), &'static str> {
        if value.is_null() {
            return Ok(());
        }
        let usage = self.usage.get_or_insert_with(CompletionUsage::default);
        if let Some(n) = value.get(input) {
            usage.prompt_tokens = count(n)?;
            self.usage_prompt_tokens_seen = true;
        }
        if let Some(n) = value.get(output) {
            usage.completion_tokens = count(n)?;
            self.usage_completion_tokens_seen = true;
        }
        usage.total_tokens = usage
            .prompt_tokens
            .checked_add(usage.completion_tokens)
            .ok_or("usage overflow")?;
        if let Some(n) = value
            .get("cache_read_input_tokens")
            .or_else(|| value.pointer("/input_tokens_details/cached_tokens"))
            .or_else(|| value.pointer("/prompt_tokens_details/cached_tokens"))
        {
            self.metadata.cache_read_tokens = Some(count(n)?);
        }
        if let Some(n) = value.get("cache_creation_input_tokens") {
            self.metadata.cache_write_tokens = Some(count(n)?);
        }
        if let Some(n) = value.pointer("/cache_creation/ephemeral_1h_input_tokens") {
            self.metadata.cache_write_1h_tokens = count(n)?;
        }
        Ok(())
    }

    fn tool(&mut self, index: u64) -> Result<&mut ToolCall, &'static str> {
        if !self.tools.contains_key(&index) && self.tools.len() >= 128 {
            return Err("too many tool calls");
        }
        Ok(self.tools.entry(index).or_default())
    }

    fn finish(&mut self) -> Result<Vec<Event>, &'static str> {
        self.finish_continuation()?;
        let mut events = Vec::new();
        if self.metadata.output_limit_reached {
            self.tools.clear();
        }
        let mut ids = std::collections::BTreeSet::new();
        for (_, mut tool) in std::mem::take(&mut self.tools) {
            if tool.arguments.is_empty()
                && let Some(input) = tool.initial_input
            {
                tool.arguments = input.to_string();
            }
            if tool.id.is_empty()
                || tool.name.is_empty()
                || !ids.insert(tool.id.clone())
                || !serde_json::from_str::<Value>(&tool.arguments).is_ok_and(|v| v.is_object())
            {
                return Err("incomplete or invalid tool call");
            }
            events.push(Event::ToolCallComplete {
                tool_call_id: tool.id,
                function_name: tool.name,
                arguments_json: tool.arguments,
            });
        }
        self.metadata.usage_complete = self
            .usage
            .as_ref()
            .map(|_| self.usage_prompt_tokens_seen && self.usage_completion_tokens_seen);
        if self.protocol == Protocol::Anthropic
            && let Some(usage) = self.usage.as_mut()
        {
            // Anthropic reports `input_tokens` without cache reads and writes.
            let cached = [
                self.metadata.cache_read_tokens,
                self.metadata.cache_write_tokens,
            ];
            for tokens in cached.into_iter().flatten() {
                usage.prompt_tokens = usage
                    .prompt_tokens
                    .checked_add(tokens)
                    .ok_or("usage overflow")?;
            }
            usage.total_tokens = usage
                .prompt_tokens
                .checked_add(usage.completion_tokens)
                .ok_or("usage overflow")?;
        }
        self.metadata.settled_reasoning = match self.protocol {
            Protocol::Chat if self.chat_reasoning_available => Some(
                if self.chat_reasoning_seen && !self.chat_reasoning.is_empty() {
                    vec![std::mem::take(&mut self.chat_reasoning)]
                } else {
                    Vec::new()
                },
            ),
            Protocol::Anthropic
                if !self.anthropic_reasoning_unavailable
                    && self.anthropic_thinking.values().all(|block| block.finished) =>
            {
                Some(
                    std::mem::take(&mut self.anthropic_thinking)
                        .into_values()
                        .map(|block| block.text)
                        .collect(),
                )
            }
            Protocol::Chat | Protocol::Responses | Protocol::Anthropic => None,
        };
        events.push(Event::DoneWithMetadata {
            usage: self.usage.take(),
            metadata: Some(std::mem::take(&mut self.metadata)),
        });
        self.done = true;
        Ok(events)
    }
}

impl ToolCall {
    fn delta(&self, fragment: &str) -> Event {
        Event::ToolCallDelta {
            tool_call_id: self.id.clone(),
            function_name: (!self.name.is_empty()).then(|| self.name.clone()),
            arguments_delta: fragment.into(),
        }
    }
}

fn append(target: &mut String, text: &str) -> Result<(), &'static str> {
    if target.len().saturating_add(text.len()) > 1_048_576 {
        return Err("tool input exceeds 1 MiB");
    }
    target.push_str(text);
    Ok(())
}

fn label(target: &mut String, value: Option<&str>) -> Result<(), &'static str> {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        if value.len() > 256 || (!target.is_empty() && target != value) {
            return Err("invalid or conflicting tool identifier");
        }
        if target.is_empty() {
            target.push_str(value);
        }
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key]
        .as_str()
        .ok_or("provider event is missing a string field")
}

fn count(value: &Value) -> Result<u32, &'static str> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("invalid provider token count")
}
