use super::{CompletionUsage, NativeCompactionError, NativeCompactionLane};
use crate::{envelope, ProviderErrorEnvelope};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// One native event may carry opaque ciphertext, so the compaction lanes allow far more than
/// an ordinary turn's event.
const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Parsed {
    pub item: Value,
    pub usage: Option<CompletionUsage>,
    pub usage_details: Option<Value>,
}
#[derive(Default)]
struct State {
    item: Option<Value>,
    count: usize,
    complete: bool,
    stop: String,
    usage: Option<CompletionUsage>,
    usage_details: Option<Value>,
    /// Response headers, kept on any failure the stream reports.
    headers: BTreeMap<String, String>,
}
impl State {
    fn event(&mut self, frame: &[u8]) -> Result<(), NativeCompactionError> {
        let frame = std::str::from_utf8(frame)
            .map_err(|_| NativeCompactionError::new("native stream parse failed"))?;
        let mut event = "";
        let mut data = String::new();
        for line in frame.lines() {
            if let Some(name) = line.strip_prefix("event:") {
                event = name.trim();
            }
            if let Some(value) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value.trim_start());
            }
        }
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&data)
            .map_err(|_| NativeCompactionError::new("native stream parse failed"))?;
        let kind = value["type"].as_str().unwrap_or(event);
        match kind {
            "response.output_item.done" if value["item"]["type"] == "compaction" => {
                self.count += 1;
                self.item = Some(value["item"].clone());
            }
            "content_block_start" if value["content_block"]["type"] == "compaction" => {
                self.count += 1;
                self.item = Some(value["content_block"].clone());
            }
            "response.completed" | "response.done" => {
                self.complete = true;
                self.usage = read_usage(&value["response"]["usage"]);
                self.usage_details = self.usage.as_ref().map(usage_details);
            }
            "message_start" => self.usage = read_anthropic_usage(&value["message"]["usage"]),
            "message_delta" => {
                if let Some(stop) = value["delta"]["stop_reason"].as_str() {
                    self.stop = stop.into();
                }
                if let Some(next) = read_anthropic_usage(&value["usage"]) {
                    let old = self.usage.get_or_insert_with(CompletionUsage::default);
                    old.output_tokens = next.output_tokens;
                    old.completion_tokens = next.completion_tokens;
                    old.prompt_tokens = old.prompt_tokens.max(next.prompt_tokens);
                    old.input_tokens = old.input_tokens.max(next.input_tokens);
                    old.cache_read_tokens = old.cache_read_tokens.max(next.cache_read_tokens);
                    old.cache_write_tokens = old.cache_write_tokens.max(next.cache_write_tokens);
                    old.total_tokens = old.prompt_tokens.saturating_add(old.completion_tokens);
                }
            }
            "message_stop" => self.complete = true,
            "error" | "response.failed" | "response.incomplete" => {
                return Err(self.failure(kind, &value))
            }
            _ => {}
        }
        Ok(())
    }

    /// A standalone error keeps a finite numeric top-level status and its classification;
    /// the message names the event, the code or type, and the provider's words.
    fn failure(&self, kind: &str, value: &Value) -> NativeCompactionError {
        let error = value
            .get("error")
            .or_else(|| value["response"].get("error"))
            .unwrap_or(value);
        let message = format!(
            "{kind} {} {}",
            error["code"]
                .as_str()
                .or_else(|| error["type"].as_str())
                .unwrap_or(""),
            error["message"].as_str().unwrap_or("")
        );
        let status = finite(&value["status"]).and_then(|n| u16::try_from(n).ok());
        match status {
            Some(status) => NativeCompactionError::http(
                message.clone(),
                ProviderErrorEnvelope {
                    status: Some(status),
                    headers: self.headers.clone(),
                    body: Some(envelope::redact(&message, &[])),
                    usage: None,
                },
            ),
            None => {
                let mut error = NativeCompactionError::new(message);
                error.headers.clone_from(&self.headers);
                error
            }
        }
    }
}

fn usage_details(usage: &CompletionUsage) -> Value {
    let mut details = json!({"inputTokens":usage.prompt_tokens,"outputTokens":usage.completion_tokens,"totalTokens":usage.total_tokens});
    if usage.cache_read_tokens > 0 {
        details["cachedInputTokens"] = usage.cache_read_tokens.into();
    }
    if let Some(reasoning) = usage.reasoning_tokens {
        details["reasoningOutputTokens"] = reasoning.into();
    }
    details
}

pub(super) async fn parse(
    mut response: reqwest::Response,
    lane: NativeCompactionLane,
) -> Result<Parsed, NativeCompactionError> {
    let mut pending = Vec::new();
    let mut state = State {
        headers: envelope::header_map(response.headers()),
        ..State::default()
    };
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| NativeCompactionError::new("provider connection closed"))?
    {
        pending.extend_from_slice(&chunk);
        while let Some((end, delimiter)) = boundary(&pending) {
            state.event(&pending[..end])?;
            pending.drain(..end + delimiter);
        }
        if pending.len() > MAX_EVENT_BYTES {
            return Err(NativeCompactionError::new("native event exceeds 16 MiB"));
        }
    }
    if !pending.is_empty() {
        state.event(&pending)?;
    }
    if !state.complete {
        return Err(NativeCompactionError::new(
            "native stream closed-before-completed",
        ));
    }
    if state.count != 1 {
        return Err(NativeCompactionError::new(
            "native compaction output count mismatch",
        ));
    }
    let item = state
        .item
        .ok_or_else(|| NativeCompactionError::new("native compaction missing item"))?;
    if lane == NativeCompactionLane::Anthropic
        && (state.stop != "compaction"
            || item["content"].as_str().is_none_or(str::is_empty)
            || item["signature"].as_str().is_none_or(str::is_empty))
    {
        return Err(NativeCompactionError::new(
            "native compaction returned no signed summary",
        ));
    }
    Ok(Parsed {
        item,
        usage: state.usage,
        usage_details: state.usage_details,
    })
}
fn boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    bytes
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|n| (n, 2))
        .into_iter()
        .chain(
            bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|n| (n, 4)),
        )
        .min_by_key(|(n, _)| *n)
}
/// Any finite JSON number, floored and clamped to `u32`; a negative number reads as zero.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The value is clamped to the u32 range first."
)]
fn finite(value: &Value) -> Option<u32> {
    let number = value.as_f64().filter(|n| n.is_finite())?;
    Some(number.floor().clamp(0.0, f64::from(u32::MAX)) as u32)
}
fn number(value: &Value, key: &str) -> Option<u32> {
    finite(&value[key])
}
/// Usage counts only when input, output and total are all present.
fn read_usage(value: &Value) -> Option<CompletionUsage> {
    let prompt = number(value, "input_tokens")?;
    let completion = number(value, "output_tokens")?;
    let cached = number(&value["input_tokens_details"], "cached_tokens");
    Some(CompletionUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: number(value, "total_tokens")?,
        input_tokens: prompt.saturating_sub(cached.unwrap_or(0)),
        output_tokens: completion,
        cache_read_tokens: cached.unwrap_or(0),
        reasoning_tokens: number(&value["output_tokens_details"], "reasoning_tokens"),
        ..Default::default()
    })
}
fn read_anthropic_usage(value: &Value) -> Option<CompletionUsage> {
    if !value.is_object() {
        return None;
    }
    if let Some(iterations) = value["iterations"]
        .as_array()
        .filter(|a| a.iter().any(|v| v["type"] == "compaction"))
    {
        // Every iteration is billed, so the usage is their sum.
        let mut total = CompletionUsage::default();
        for iteration in iterations {
            if let Some(usage) = read_anthropic_usage(iteration.get("usage").unwrap_or(iteration)) {
                total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
                total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
                total.cache_read_tokens = total
                    .cache_read_tokens
                    .saturating_add(usage.cache_read_tokens);
                total.cache_write_tokens = total
                    .cache_write_tokens
                    .saturating_add(usage.cache_write_tokens);
            }
        }
        return Some(anthropic_totals(total));
    }
    Some(anthropic_totals(CompletionUsage {
        input_tokens: number(value, "input_tokens").unwrap_or(0),
        output_tokens: number(value, "output_tokens").unwrap_or(0),
        cache_read_tokens: number(value, "cache_read_input_tokens").unwrap_or(0),
        cache_write_tokens: number(value, "cache_creation_input_tokens").unwrap_or(0),
        ..Default::default()
    }))
}
/// Fills the prompt, completion and total counts Anthropic leaves for the caller to sum.
fn anthropic_totals(mut usage: CompletionUsage) -> CompletionUsage {
    usage.prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens);
    usage.completion_tokens = usage.output_tokens;
    usage.total_tokens = usage.prompt_tokens.saturating_add(usage.completion_tokens);
    usage
}
