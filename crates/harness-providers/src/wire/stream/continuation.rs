use super::*;
use serde_json::json;

impl StreamDecoder {
    pub(super) fn add_continuation_bytes(&mut self, bytes: usize) -> Result<(), &'static str> {
        self.continuation_bytes = self.continuation_bytes.saturating_add(bytes);
        if self.continuation_bytes > 4 * 1024 * 1024 {
            return Err("provider continuation exceeds 4 MiB");
        }
        Ok(())
    }

    pub(super) fn chat_tool(&mut self, call: &Value) -> Result<&mut ToolCall, &'static str> {
        let index = if let Some(index) = call["index"].as_u64() {
            index
        } else {
            // Gemini may send a complete call without an OpenAI stream index.
            let id = text(call, "id")?;
            text(&call["function"], "name")?;
            if !serde_json::from_str::<Value>(text(&call["function"], "arguments")?)
                .is_ok_and(|v| v.is_object())
            {
                return Err("fragmented tool call requires an index");
            }
            self.tools
                .iter()
                .find(|(_, tool)| tool.id == id)
                .map(|(index, _)| *index)
                .or_else(|| {
                    self.tools
                        .last_key_value()
                        .map_or(Some(0), |(index, _)| index.checked_add(1))
                })
                .ok_or("tool index overflow")?
        };
        let extra = call.get("extra_content").filter(|v| !v.is_null());
        if let Some(extra) = extra {
            self.add_continuation_bytes(extra.to_string().len())?;
        }
        let tool = self.tool(index)?;
        if let Some(extra) = extra {
            if tool
                .extra_content
                .as_ref()
                .is_some_and(|prior| prior != extra)
            {
                return Err("tool continuation changed during streaming");
            }
            tool.extra_content = Some(extra.clone());
        }
        Ok(tool)
    }

    pub(super) fn capture_anthropic(&mut self, event: &Value) -> Result<(), &'static str> {
        let Some(index) = event["index"].as_u64() else {
            return Ok(());
        };
        match event["type"].as_str() {
            Some("content_block_start") => {
                let content = &event["content_block"];
                if !content.is_object()
                    || self.anthropic_content.len() >= 128
                    || self.anthropic_content.contains_key(&index)
                {
                    return Err("invalid or duplicate content block");
                }
                self.continuation_bytes = self
                    .continuation_bytes
                    .saturating_add(content.to_string().len());
                self.anthropic_content.insert(index, content.clone());
            }
            Some("content_block_delta") => {
                let field = match event["delta"]["type"].as_str() {
                    Some("text_delta") => "text",
                    Some("thinking_delta") => "thinking",
                    Some("signature_delta") => "signature",
                    _ => return Ok(()),
                };
                let fragment = text(&event["delta"], field)?;
                self.continuation_bytes = self.continuation_bytes.saturating_add(fragment.len());
                let block = self
                    .anthropic_content
                    .entry(index)
                    .or_insert_with(|| json!({"type":"text","text":""}));
                let mut value = block[field].as_str().unwrap_or_default().to_owned();
                append(&mut value, fragment)?;
                block[field] = value.into();
            }
            _ => {}
        }
        if self.continuation_bytes > 4 * 1024 * 1024 {
            return Err("provider continuation exceeds 4 MiB");
        }
        Ok(())
    }

    pub(super) fn finish_continuation(&mut self) -> Result<(), &'static str> {
        if self.metadata.output_limit_reached {
            return Ok(());
        }
        let data = match self.protocol {
            Protocol::Anthropic
                if self.anthropic_content.values().any(|block| {
                    matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    )
                }) =>
            {
                for (index, tool) in &self.tools {
                    let Some(block) = self.anthropic_content.get_mut(index) else {
                        continue;
                    };
                    block["input"] = if tool.arguments.is_empty() {
                        tool.initial_input.clone().unwrap_or_else(|| json!({}))
                    } else {
                        serde_json::from_str(&tool.arguments)
                            .map_err(|_| "invalid tool arguments")?
                    };
                }
                Value::Array(
                    std::mem::take(&mut self.anthropic_content)
                        .into_values()
                        .collect(),
                )
            }
            Protocol::Chat => {
                let extras: serde_json::Map<String, Value> = self
                    .tools
                    .values()
                    .filter_map(|tool| {
                        tool.extra_content
                            .clone()
                            .map(|extra| (tool.id.clone(), extra))
                    })
                    .collect();
                if extras.is_empty() && self.chat_extra_content.is_none() {
                    return Ok(());
                }
                json!({"tool_calls":extras, "extra_content":self.chat_extra_content.take()})
            }
            _ => return Ok(()),
        };
        self.metadata.continuation = Some(Box::new(crate::ProviderContinuation::new(
            self.protocol,
            data,
        )?));
        Ok(())
    }
}
