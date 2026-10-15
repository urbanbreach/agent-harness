//! ChatML records: Hermes and Qwen3.
use super::{
    json::quote,
    transcript::{think_fence, thinking_block, tool_responses, Call, Entry},
    Dialect,
};

fn call(call: &Call) -> String {
    format!(
        "<tool_call>\n{{\"name\":{},\"arguments\":{}}}\n</tool_call>",
        quote(&call.name),
        call.args.compact()
    )
}

pub(super) fn render(dialect: Dialect, entries: &[Entry]) -> String {
    let mut out = String::new();
    let mut record = |role: &str, body: &str| {
        out.push_str(&format!("<|im_start|>{role}\n{body}<|im_end|>\n"));
    };
    for entry in entries {
        match entry {
            Entry::User(text) => record("user", text),
            Entry::Developer(text) => record("system", text),
            Entry::Assistant(assistant) => {
                let thinking = if dialect == Dialect::Hermes {
                    thinking_block("<think>", "</think>", assistant.thinking())
                } else {
                    think_fence(assistant.thinking())
                };
                let calls = assistant.calls.iter().map(call).collect::<Vec<_>>();
                record(
                    "assistant",
                    &format!("{thinking}{}{}", assistant.text, calls.join("\n")),
                );
            }
            Entry::Results(results) => record(
                if dialect == Dialect::Hermes {
                    "tool"
                } else {
                    "user"
                },
                &tool_responses(results),
            ),
        }
    }
    out
}
