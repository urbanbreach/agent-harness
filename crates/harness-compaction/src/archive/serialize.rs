//! Scope-tagged, bounded archive transcript serialization.
use super::normalize::elide_data_urls;
use crate::item::{Item, ItemPart};
use harness_providers::MessageRole;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Character caps count UTF-16 code units; `None` disables a cap.
#[derive(Debug, Clone)]
pub struct SerializeOptions {
    pub tool_result_max_chars: Option<usize>,
    pub tool_arg_max_chars: Option<usize>,
    pub tool_call_max_chars: Option<usize>,
    pub truncate_head_ratio: f64,
    pub dim_tool_results: bool,
    pub include_thinking: bool,
}
impl Default for SerializeOptions {
    fn default() -> Self {
        Self {
            tool_result_max_chars: Some(2000),
            tool_arg_max_chars: Some(500),
            tool_call_max_chars: Some(2000),
            truncate_head_ratio: 0.6,
            dim_tool_results: true,
            include_thinking: true,
        }
    }
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{e}' | '\u{f}'))
        .collect()
}

fn truncate(text: &str, cap: Option<usize>, ratio: f64) -> String {
    let Some(cap) = cap else {
        return text.to_owned();
    };
    let length = text.encode_utf16().count();
    if length <= cap {
        return text.to_owned();
    }
    let ratio = if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    };
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "UTF-16 cap split follows the specified JavaScript rounding"
    )]
    let head = ((cap as f64 * ratio).round() as usize).min(cap);
    let tail = cap - head;
    // Rust strings cannot retain lone surrogates created by a JavaScript slice.
    let mut start = 0;
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units + ch.len_utf16() > head {
            break;
        }
        units += ch.len_utf16();
        start = index + ch.len_utf8();
    }
    let mut end = text.len();
    units = 0;
    for (index, ch) in text.char_indices().rev() {
        if units + ch.len_utf16() > tail {
            break;
        }
        units += ch.len_utf16();
        end = index;
    }
    format!(
        "{} […{}ch elided…] {}",
        &text[..start],
        length - cap,
        &text[end..]
    )
}

fn emit(output: &mut String, previous: &mut &'static str, scope: &'static str, body: &str) {
    if body.is_empty() {
        return;
    }
    if *previous == scope {
        if !output.ends_with('\n') && !body.starts_with('\n') {
            output.push('\n');
        }
    } else {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(scope);
        *previous = scope;
    }
    output.push_str(body);
}

fn result_body(text: &str, opts: &SerializeOptions) -> String {
    let body = truncate(
        &elide_data_urls(&clean(text), false),
        opts.tool_result_max_chars,
        opts.truncate_head_ratio,
    );
    if opts.dim_tool_results {
        format!("<out>\n\u{e}{body}\u{f}\n</out>")
    } else {
        format!("<out>\n{body}\n</out>")
    }
}

/// The call line `name(key=value, ...)//intent`. The intent is the derived `intent`
/// argument when it is a string, else the raw `i` argument; neither is printed as an
/// argument.
fn call_line(name: &str, arguments: &str, opts: &SerializeOptions) -> String {
    let mut args = String::new();
    let (mut derived, mut raw) = (None, None);
    if let Some(object) = crate::serialize::ordered_arguments(arguments) {
        for (key, value) in object {
            let text = || serde_json::from_str::<String>(&value).ok();
            if key == "i" {
                raw = text();
                continue;
            }
            if key == "intent"
                && let Some(text) = text()
            {
                derived = Some(text);
                continue;
            }
            if !args.is_empty() {
                args.push_str(", ");
            }
            args.push_str(&key);
            args.push('=');
            args.push_str(&truncate(
                &elide_data_urls(&value, false),
                opts.tool_arg_max_chars,
                opts.truncate_head_ratio,
            ));
        }
    } else {
        args = elide_data_urls(&clean(arguments), false);
    }
    let args = truncate(&args, opts.tool_call_max_chars, opts.truncate_head_ratio);
    let mut line = format!("{name}({args})");
    let intent = derived
        .or(raw)
        .map(|text| {
            clean(&text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if !intent.is_empty() {
        line.push_str("//");
        line.push_str(&intent);
    }
    line
}

/// Grouped reasoning and reply text of the assistant segment being streamed.
#[derive(Default)]
struct Segment {
    reasoning: Vec<String>,
    reply: Vec<String>,
}

impl Segment {
    /// Emit reasoning first, then the reply, each joined by a line feed.
    fn flush(&mut self, output: &mut String, previous: &mut &'static str, opts: &SerializeOptions) {
        let reasoning = std::mem::take(&mut self.reasoning).join("\n");
        let reply = std::mem::take(&mut self.reply).join("\n");
        if opts.include_thinking {
            emit(output, previous, "¶think:", &reasoning);
        }
        emit(output, previous, "¶ai:", &reply);
    }

    fn text(&mut self, text: &str) {
        let text = clean(text);
        if !text.trim().is_empty() {
            self.reply.push(text);
        }
    }

    fn reasoning(&mut self, text: &str) {
        let text = clean(text);
        if !text.trim().is_empty() {
            self.reasoning.push(text);
        }
    }
}

/// Serialize text, reasoning, calls and paired results in chronological call order.
///
/// An assistant message streams in block order: reasoning and reply are buffered apart and
/// flushed (reasoning first) before every surviving call and again at the message end, so
/// each call is a boundary and text after an earlier call stays after it.
pub fn serialize_for_archive(items: &[Item<'_>], opts: &SerializeOptions) -> String {
    let mut results = HashMap::new();
    let mut useless = HashSet::new();
    let mut called = HashSet::new();
    for item in items {
        if let Some(calls) = &item.message.assistant_tool_calls {
            called.extend(calls.iter().map(|call| call.tool_call_id.as_str()));
        }
        if item.is_tool_result()
            && let Some(id) = item.message.tool_call_id.as_deref()
        {
            if item.tool.useless && !item.tool.is_error {
                useless.insert(id);
            }
            if !item.text().is_empty() {
                results.insert(id, item.text());
            }
        }
    }
    let mut output = String::new();
    let mut previous = "";
    for item in items {
        match item.role() {
            MessageRole::User => emit(&mut output, &mut previous, "¶user:", &clean(item.text())),
            MessageRole::Assistant => {
                let calls = item
                    .message
                    .assistant_tool_calls
                    .as_deref()
                    .unwrap_or_default();
                let mut segment = Segment::default();
                let mut stream = |part: ItemPart<'_>, segment: &mut Segment| match part {
                    ItemPart::Text(text) => segment.text(&text),
                    ItemPart::Reasoning(text) => segment.reasoning(&text),
                    ItemPart::RedactedReasoning => {}
                    ItemPart::Call(index) => {
                        let Some(call) = calls.get(index) else {
                            return;
                        };
                        if useless.contains(call.tool_call_id.as_str()) {
                            return;
                        }
                        segment.flush(&mut output, &mut previous, opts);
                        let mut line = call_line(&call.function_name, &call.arguments_json, opts);
                        if let Some(body) = results.get(call.tool_call_id.as_str()) {
                            line.push('\n');
                            line.push_str(&result_body(body, opts));
                        }
                        emit(&mut output, &mut previous, "¶call:", &line);
                    }
                };
                if item.parts.is_empty() {
                    // A flat message: all reasoning, then the reply, then every call.
                    for text in item.reasoning {
                        stream(ItemPart::Reasoning(text.as_str().into()), &mut segment);
                    }
                    stream(ItemPart::Text(item.text().into()), &mut segment);
                    for index in 0..calls.len() {
                        stream(ItemPart::Call(index), &mut segment);
                    }
                } else {
                    for part in item.parts {
                        stream(part.clone(), &mut segment);
                    }
                }
                segment.flush(&mut output, &mut previous, opts);
            }
            MessageRole::Tool => {
                let id = item.message.tool_call_id.as_deref();
                if id.is_some_and(|id| useless.contains(id) || called.contains(id))
                    || (item.tool.useless && !item.tool.is_error)
                {
                    continue;
                }
                emit(
                    &mut output,
                    &mut previous,
                    "¶call:",
                    &format!("\n{}", result_body(item.text(), opts)),
                );
            }
            MessageRole::System => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemKind, Stop, ToolFacts};
    use harness_providers::{AssistantToolCall, CompletionMessage};

    fn item(message: &CompletionMessage) -> Item<'_> {
        Item {
            message,
            kind: ItemKind::Message,
            seq: 0,
            turn: None,
            reasoning: &[],
            tool: ToolFacts::default(),
            image_count: 0,
            image_tokens: 0,
            usage: None,
            stop: Stop::Complete,
            at_ms: None,
            tokens: 0,
            ..Item::new(message)
        }
    }

    #[test]
    fn pairs_outputs_and_drops_only_nonerror_useless_calls() {
        let mut assistant = CompletionMessage::text(MessageRole::Assistant, "reply");
        assistant.assistant_tool_calls = Some(vec![AssistantToolCall {
            tool_call_id: "one".into(),
            function_name: "read".into(),
            arguments_json: "{\"z\":1,\"i\":\" Look\\n here \",\"a\":2}".into(),
        }]);
        let mut tool = CompletionMessage::text(MessageRole::Tool, "body\u{e}");
        tool.tool_call_id = Some("one".into());
        for (useless, is_error, expected) in [
            (
                false,
                false,
                "¶ai:reply\n\n¶call:read(z=1, a=2)//Look here\n<out>\n\u{e}body\u{f}\n</out>",
            ),
            (true, false, "¶ai:reply"),
            (
                true,
                true,
                "¶ai:reply\n\n¶call:read(z=1, a=2)//Look here\n<out>\n\u{e}body\u{f}\n</out>",
            ),
        ] {
            let mut result = item(&tool);
            result.tool.useless = useless;
            result.tool.is_error = is_error;
            assert_eq!(
                serialize_for_archive(&[item(&assistant), result], &SerializeOptions::default()),
                expected
            );
        }
    }

    #[test]
    fn caps_values_arguments_and_results_independently() {
        let mut assistant = CompletionMessage::text(MessageRole::Assistant, "");
        assistant.assistant_tool_calls = Some(vec![AssistantToolCall {
            tool_call_id: "one".into(),
            function_name: "run".into(),
            arguments_json: "{\"x\":\"abcdefghij\"}".into(),
        }]);
        let mut tool = CompletionMessage::text(MessageRole::Tool, "0123456789");
        tool.tool_call_id = Some("one".into());
        let opts = SerializeOptions {
            tool_arg_max_chars: Some(4),
            tool_call_max_chars: None,
            tool_result_max_chars: Some(4),
            truncate_head_ratio: 0.5,
            dim_tool_results: false,
            include_thinking: false,
        };
        assert_eq!(
            serialize_for_archive(&[item(&assistant), item(&tool)], &opts),
            "¶call:run(x=\"a […8ch elided…] j\")\n<out>\n01 […6ch elided…] 89\n</out>"
        );
        let opts = SerializeOptions {
            tool_arg_max_chars: None,
            tool_call_max_chars: Some(4),
            tool_result_max_chars: None,
            truncate_head_ratio: 0.5,
            dim_tool_results: false,
            include_thinking: false,
        };
        assert_eq!(
            serialize_for_archive(&[item(&assistant)], &opts),
            "¶call:run(x= […10ch elided…] j\")"
        );
    }

    #[test]
    fn coalesces_scopes_and_applies_reasoning_policy() {
        let user = CompletionMessage::text(MessageRole::User, "first\u{e}");
        let second = CompletionMessage::text(MessageRole::User, "second");
        let assistant = CompletionMessage::text(MessageRole::Assistant, " reply ");
        let reasoning = vec![" hidden\u{f} ".to_owned()];
        let mut reply = item(&assistant);
        reply.reasoning = &reasoning;
        for (include, expected) in [
            (
                true,
                "¶user:first\nsecond\n\n¶think: hidden \n\n¶ai: reply ",
            ),
            (false, "¶user:first\nsecond\n\n¶ai: reply "),
        ] {
            let opts = SerializeOptions {
                include_thinking: include,
                ..SerializeOptions::default()
            };
            assert_eq!(
                serialize_for_archive(&[item(&user), item(&second), reply], &opts),
                expected
            );
        }
        let orphan = CompletionMessage::text(MessageRole::Tool, "output");
        assert_eq!(
            serialize_for_archive(&[item(&orphan)], &SerializeOptions::default()),
            "¶call:\n<out>\n\u{e}output\u{f}\n</out>"
        );
    }

    #[test]
    fn assistant_blocks_flush_at_each_call_boundary() {
        let mut assistant = CompletionMessage::text(MessageRole::Assistant, "before after");
        assistant.assistant_tool_calls = Some(vec![
            AssistantToolCall {
                tool_call_id: "a".into(),
                function_name: "read".into(),
                arguments_json: "{\"intent\":\"look\",\"i\":\"raw\",\"path\":\"x\"}".into(),
            },
            AssistantToolCall {
                tool_call_id: "b".into(),
                function_name: "skip".into(),
                arguments_json: "{}".into(),
            },
        ]);
        let parts = [
            ItemPart::reasoning("plan"),
            ItemPart::text("before"),
            ItemPart::Call(0),
            ItemPart::text("between"),
            ItemPart::Call(1),
            ItemPart::text("after"),
        ];
        let mut message = item(&assistant);
        message.parts = &parts;
        let mut skipped = CompletionMessage::text(MessageRole::Tool, "none");
        skipped.tool_call_id = Some("b".into());
        let mut result = item(&skipped);
        result.tool.useless = true;
        let opts = SerializeOptions {
            dim_tool_results: false,
            ..SerializeOptions::default()
        };
        // The useless call does not flush, so prose either side of it coalesces.
        assert_eq!(
            serialize_for_archive(&[message, result], &opts),
            "¶think:plan\n\n¶ai:before\n\n¶call:read(path=\"x\")//look\n\n¶ai:between\nafter"
        );
    }
}
