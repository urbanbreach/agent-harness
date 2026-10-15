use super::*;

impl StreamDecoder {
    pub(super) fn anthropic(&mut self, value: &Value) -> Result<Vec<Event>, &'static str> {
        self.capture_anthropic(value)?;
        let mut events = Vec::new();
        match value["type"].as_str().ok_or("missing message event type")? {
            "message_start" => {
                self.metadata.assistant_message_id =
                    value["message"]["id"].as_str().map(str::to_owned);
                self.record_usage(&value["message"]["usage"], "input_tokens", "output_tokens")?;
            }
            "content_block_start" => match value["content_block"]["type"].as_str() {
                Some("thinking") => {
                    self.start_anthropic_thinking(value["index"].as_u64(), &value["content_block"]);
                }
                Some("redacted_thinking") => {}
                Some("tool_use") => {
                    let tool = self.tool(value["index"].as_u64().ok_or("missing tool index")?)?;
                    label(&mut tool.id, Some(text(&value["content_block"], "id")?))?;
                    label(&mut tool.name, Some(text(&value["content_block"], "name")?))?;
                    tool.initial_input = value["content_block"].get("input").cloned();
                }
                _ => {}
            },
            "content_block_delta" => match value["delta"]["type"].as_str() {
                Some("text_delta") => {
                    if let Some(index) = value["index"].as_u64() {
                        self.anthropic_text.insert(index);
                    }
                    events.push(Event::TextDelta(text(&value["delta"], "text")?.into()))
                }
                Some("thinking_delta") => {
                    let fragment = text(&value["delta"], "thinking")?;
                    self.append_anthropic_reasoning(value["index"].as_u64(), fragment);
                    events.push(Event::ReasoningDelta(fragment.into()));
                }
                Some("signature_delta") => {}
                Some("input_json_delta") => {
                    let tool = self.tool(value["index"].as_u64().ok_or("missing tool index")?)?;
                    let fragment = text(&value["delta"], "partial_json")?;
                    append(&mut tool.arguments, fragment)?;
                    events.push(tool.delta(fragment));
                }
                _ => {}
            },
            "content_block_stop" => {
                if value["index"]
                    .as_u64()
                    .is_some_and(|index| self.anthropic_text.remove(&index))
                {
                    events.push(Event::TextBlockEnd);
                }
                if let Some(index) = value["index"].as_u64()
                    && let Some(block) = self.anthropic_thinking.get_mut(&index)
                {
                    block.finished = true;
                }
            }
            "message_delta" => {
                self.metadata.provider_stop_reason =
                    value["delta"]["stop_reason"].as_str().map(str::to_owned);
                if self.metadata.provider_stop_reason.as_deref() == Some("max_tokens") {
                    self.metadata.output_limit_reached = true;
                    self.anthropic_reasoning_unavailable = true;
                }
                self.stopped = self
                    .metadata
                    .provider_stop_reason
                    .as_deref()
                    .is_some_and(|r| {
                        matches!(r, "end_turn" | "tool_use" | "max_tokens" | "stop_sequence")
                    });
                self.record_usage(&value["usage"], "input_tokens", "output_tokens")?;
            }
            "message_stop" => {
                if !self.stopped {
                    return Err("message ended without a stop reason");
                }
                return self.finish();
            }
            _ => {}
        }
        Ok(events)
    }

    fn start_anthropic_thinking(&mut self, index: Option<u64>, content: &Value) {
        let Some(index) = index else {
            self.anthropic_reasoning_unavailable = true;
            return;
        };
        if self.anthropic_thinking.len() >= 128 && !self.anthropic_thinking.contains_key(&index) {
            self.anthropic_reasoning_unavailable = true;
            return;
        }
        if self
            .anthropic_thinking
            .insert(index, ThinkingBlock::default())
            .is_some()
        {
            self.anthropic_reasoning_unavailable = true;
            return;
        }
        if let Some(thinking) = content.get("thinking") {
            if let Some(thinking) = thinking.as_str() {
                self.append_anthropic_reasoning(Some(index), thinking);
            } else {
                self.anthropic_reasoning_unavailable = true;
            }
        }
    }

    fn append_anthropic_reasoning(&mut self, index: Option<u64>, fragment: &str) {
        let Some(index) = index else {
            self.anthropic_reasoning_unavailable = true;
            return;
        };
        if self
            .anthropic_thinking
            .get(&index)
            .is_some_and(|block| block.finished)
        {
            self.anthropic_reasoning_unavailable = true;
            return;
        }
        let Some(block) = self.anthropic_thinking.get_mut(&index) else {
            self.anthropic_reasoning_unavailable = true;
            return;
        };
        if self
            .anthropic_reasoning_bytes
            .saturating_add(fragment.len())
            > 1_048_576
            || append(&mut block.text, fragment).is_err()
        {
            self.anthropic_reasoning_unavailable = true;
            return;
        }
        self.anthropic_reasoning_bytes += fragment.len();
    }
}
