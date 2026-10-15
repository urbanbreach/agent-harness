//! Human/assistant transcripts: XML, Minimax and Anthropic.
use super::{
    transcript::{thinking_block, tool_responses, Assistant, Call, Entry, ToolResult},
    Dialect,
};

fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn text_node(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn invocation(call: &Call) -> String {
    let mut out = format!("<invoke name=\"{}\">", attribute(&call.name));
    for (key, value) in call.args.entries() {
        out.push_str(&format!(
            "<parameter name=\"{}\">{}</parameter>",
            attribute(key),
            value.compact()
        ));
    }
    out.push_str("</invoke>");
    out
}

fn calls(dialect: Dialect, calls: &[Call]) -> String {
    let joined = calls.iter().map(invocation).collect::<Vec<_>>().join("\n");
    match dialect {
        Dialect::Minimax if !calls.is_empty() => {
            format!("<minimax:tool_call>\n{joined}\n</minimax:tool_call>")
        }
        Dialect::Anthropic if !calls.is_empty() => {
            format!("<function_calls>\n{joined}\n</function_calls>")
        }
        _ => joined,
    }
}

fn function_results(results: &[ToolResult]) -> String {
    let records = results
        .iter()
        .map(|result| {
            let (tag, stream) = if result.is_error {
                ("error", "stderr")
            } else {
                ("result", "stdout")
            };
            format!(
                "<{tag}>\n<tool_name>{}</tool_name>\n<{stream}>{}</{stream}>\n</{tag}>",
                text_node(&result.name),
                result.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<function_results>\n{records}\n</function_results>")
}

fn assistant(dialect: Dialect, assistant: &Assistant) -> String {
    format!(
        "{}{}{}",
        thinking_block("<thinking>", "</thinking>", assistant.thinking()),
        assistant.text,
        calls(dialect, &assistant.calls)
    )
}

pub(super) fn render(dialect: Dialect, entries: &[Entry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            Entry::Assistant(body) => {
                out.push_str("\n\nAssistant: ");
                out.push_str(&assistant(dialect, body));
            }
            Entry::User(text) => {
                out.push_str("\n\nHuman: ");
                out.push_str(text);
            }
            Entry::Results(results) => {
                out.push_str("\n\nHuman: ");
                out.push_str(&if dialect == Dialect::Xml {
                    tool_responses(results)
                } else {
                    function_results(results)
                });
            }
            Entry::Developer(text) if text.is_empty() => {}
            Entry::Developer(text) => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(text);
            }
        }
    }
    out
}
