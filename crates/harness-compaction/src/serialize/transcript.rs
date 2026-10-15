//! History preparation and primitives shared by every summary transcript renderer.
use super::{json::Json, truncate_units};
use crate::{Item, ItemPart};
use harness_providers::MessageRole;
use std::collections::HashSet;

/// Tool results keep this many UTF-16 units in a summary transcript.
const RESULT_UNITS: usize = 2000;

pub(super) struct Call {
    pub id: String,
    pub name: String,
    pub args: Json,
}

pub(super) struct ToolResult {
    pub id: String,
    pub name: String,
    pub text: String,
    pub is_error: bool,
}

pub(super) struct Assistant {
    /// Thinking blocks joined by LF; `None` when the assistant has no thinking block.
    pub thinking: Option<String>,
    pub text: String,
    /// A text block exists, possibly empty; `text` alone cannot tell it from no block.
    pub has_text: bool,
    pub calls: Vec<Call>,
}

impl Assistant {
    pub(super) fn thinking(&self) -> &str {
        self.thinking.as_deref().unwrap_or_default()
    }
}

/// Thinking, text and text-block presence for one assistant. Ordered parts supply the
/// blocks when present; text blocks join by LF in the default transcript and concatenate
/// in native ones. A flat message has one text block when its content is nonempty.
fn assistant_blocks(
    item: &Item<'_>,
    keep_thinking: bool,
    native: bool,
) -> (Option<String>, String, bool) {
    let mut texts: Vec<&str> = Vec::new();
    let mut reasoning: Vec<&str> = Vec::new();
    if item.parts.is_empty() {
        if !item.text().is_empty() {
            texts.push(item.text());
        }
    } else {
        for part in item.parts {
            match part {
                ItemPart::Text(text) => texts.push(text),
                ItemPart::Reasoning(text) => reasoning.push(text),
                ItemPart::RedactedReasoning | ItemPart::Call(_) => {}
            }
        }
    }
    if reasoning.is_empty() {
        reasoning.extend(item.reasoning.iter().map(String::as_str));
    }
    let thinking = (keep_thinking && !reasoning.is_empty()).then(|| reasoning.join("\n"));
    let text = texts.join(if native { "" } else { "\n" });
    (thinking, text, !texts.is_empty())
}

/// One prepared history record. Adjacent tool results form one run.
pub(super) enum Entry {
    User(String),
    Developer(String),
    Assistant(Assistant),
    Results(Vec<ToolResult>),
}

/// Remove useless call pairs, truncate and drop empty results, drop assistants left with
/// nothing, and group adjacent results. `native` is true for every dialect but the default
/// transcript: it renders attached images as `[Image]` in user and developer text and
/// concatenates assistant text blocks without a separator. Summary transcripts never carry
/// image data.
pub(super) fn prepare(items: &[Item<'_>], keep_thinking: bool, native: bool) -> Vec<Entry> {
    let useless: HashSet<&str> = items
        .iter()
        .filter(|item| item.is_tool_result() && item.tool.useless && !item.tool.is_error)
        .filter_map(|item| item.message.tool_call_id.as_deref())
        .collect();
    let body = |item: &Item<'_>| {
        let mut text = item.text().to_owned();
        if native {
            // Known images carry their MIME; images only counted (no metadata) stay bare.
            for image in item.images {
                if image.mime.is_empty() {
                    text.push_str("[Image]");
                } else {
                    text.push_str(&format!("[Image: {}]", image.mime));
                }
            }
            for _ in item.images.len()..usize::try_from(item.image_count).unwrap_or(0) {
                text.push_str("[Image]");
            }
        }
        text
    };
    let mut entries: Vec<Entry> = Vec::new();
    for item in items {
        let message = item.message;
        match message.role {
            MessageRole::User => entries.push(Entry::User(body(item))),
            MessageRole::System => entries.push(Entry::Developer(body(item))),
            MessageRole::Assistant => {
                let (thinking, text, has_text) = assistant_blocks(item, keep_thinking, native);
                let calls: Vec<Call> = message
                    .assistant_tool_calls
                    .iter()
                    .flatten()
                    .filter(|call| !useless.contains(call.tool_call_id.as_str()))
                    .map(|call| Call {
                        id: call.tool_call_id.clone(),
                        name: call.function_name.clone(),
                        args: Json::parse(&call.arguments_json)
                            .unwrap_or_else(|| Json::Object(Vec::new())),
                    })
                    .collect();
                if thinking.is_some() || has_text || !calls.is_empty() {
                    entries.push(Entry::Assistant(Assistant {
                        thinking,
                        text,
                        has_text,
                        calls,
                    }));
                }
            }
            MessageRole::Tool => {
                let hidden = message
                    .tool_call_id
                    .as_deref()
                    .is_some_and(|id| useless.contains(id));
                if hidden || item.text().is_empty() {
                    continue;
                }
                let result = ToolResult {
                    id: message.tool_call_id.clone().unwrap_or_default(),
                    name: message.name.clone().unwrap_or_default(),
                    text: truncate_units(item.text(), RESULT_UNITS),
                    is_error: item.tool.is_error,
                };
                match entries.last_mut() {
                    Some(Entry::Results(run)) => run.push(result),
                    _ => entries.push(Entry::Results(vec![result])),
                }
            }
        }
    }
    entries
}

/// ECMAScript `String.prototype.trim` whitespace.
pub(super) fn ecma_trim(text: &str) -> &str {
    text.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

fn ascii_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ')
}

/// `<tool_response>` records joined by LF.
pub(super) fn tool_responses(results: &[ToolResult]) -> String {
    results
        .iter()
        .map(|result| format!("<tool_response>\n{}\n</tool_response>", result.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Thinking wrapped in `open`/`close`, unwrapping envelopes the text already carries.
pub(super) fn thinking_block(open: &str, close: &str, thinking: &str) -> String {
    if thinking.is_empty() {
        return String::new();
    }
    let body = unwrap_envelopes(open, close, thinking).unwrap_or_else(|| thinking.to_owned());
    format!("{open}\n{body}\n{close}")
}

/// `<think>` wrapping that keeps existing envelopes untouched.
pub(super) fn think_fence(thinking: &str) -> String {
    if thinking.is_empty() {
        String::new()
    } else {
        format!("<think>\n{thinking}\n</think>")
    }
}

/// The inner bodies of text made only of complete envelopes, joined by LF.
fn unwrap_envelopes(open: &str, close: &str, text: &str) -> Option<String> {
    let mut rest = text.trim_matches(ascii_space);
    if rest.is_empty() {
        return None;
    }
    let mut bodies = Vec::new();
    while !rest.is_empty() {
        let after = rest.strip_prefix(open)?;
        let end = envelope_end(after, open, close)?;
        let inner = after[..end].trim_matches(ascii_space);
        bodies.push(unwrap_envelopes(open, close, inner).unwrap_or_else(|| inner.to_owned()));
        rest = after[end + close.len()..].trim_start_matches(ascii_space);
    }
    Some(bodies.join("\n"))
}

/// Offset of the close delimiter matching an already consumed open delimiter.
fn envelope_end(text: &str, open: &str, close: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut position = 0;
    loop {
        let rest = &text[position..];
        let closing = rest.find(close)?;
        match rest.find(open) {
            Some(opening) if opening < closing => {
                depth += 1;
                position += opening + open.len();
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    return Some(position + closing);
                }
                position += closing + close.len();
            }
        }
    }
}
