//! Kimi turns with `functions.NAME:INDEX` call identifiers.
use super::transcript::{ecma_trim, think_fence, Entry};

/// Reuse an identifier that already carries the `functions.` namespace.
fn identifier(id: &str, name: &str, index: usize) -> String {
    let trimmed = ecma_trim(id);
    if trimmed.starts_with("functions.") {
        trimmed.to_owned()
    } else {
        format!("functions.{name}:{index}")
    }
}

fn turn(role: &str, name: &str, body: &str) -> String {
    format!("<|im_{role}|>{name}<|im_middle|>{body}<|im_end|>")
}

pub(super) fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            Entry::User(text) => out.push_str(&turn("user", "user", text)),
            Entry::Developer(text) => out.push_str(&turn("system", "system", text)),
            Entry::Assistant(assistant) => {
                let mut body = format!("{}{}", think_fence(assistant.thinking()), assistant.text);
                if !assistant.calls.is_empty() {
                    body.push_str("<|tool_calls_section_begin|>");
                    for (index, call) in assistant.calls.iter().enumerate() {
                        body.push_str(&format!(
                            "<|tool_call_begin|>{}<|tool_call_argument_begin|>{}<|tool_call_end|>",
                            identifier(&call.id, &call.name, index),
                            call.args.compact()
                        ));
                    }
                    body.push_str("<|tool_calls_section_end|>");
                }
                out.push_str(&turn("assistant", "assistant", &body));
            }
            Entry::Results(results) => {
                for (index, result) in results.iter().enumerate() {
                    let id = identifier(&result.id, &result.name, index);
                    out.push_str(&turn(
                        "system",
                        &result.name,
                        &format!("## Return of {id}\n{}", result.text),
                    ));
                }
            }
        }
    }
    out
}
