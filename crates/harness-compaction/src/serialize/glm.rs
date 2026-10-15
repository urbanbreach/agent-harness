//! GLM records after the `[gMASK]<sop>` prefix.
use super::transcript::{think_fence, tool_responses, Call, Entry};

fn call(call: &Call) -> String {
    let mut out = format!("<tool_call>{}", call.name);
    for (key, value) in call.args.entries() {
        out.push_str(&format!(
            "\n<arg_key>{key}</arg_key>\n<arg_value>{}</arg_value>",
            value.compact()
        ));
    }
    out.push_str("\n</tool_call>");
    out
}

pub(super) fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("[gMASK]<sop>");
    for entry in entries {
        match entry {
            Entry::User(text) => out.push_str(&format!("<|user|>\n{text}")),
            Entry::Developer(text) => out.push_str(&format!("<|system|>\n{text}")),
            Entry::Assistant(assistant) => {
                out.push_str("<|assistant|>\n");
                if !assistant.thinking().is_empty() {
                    out.push('\n');
                    out.push_str(&think_fence(assistant.thinking()));
                }
                out.push_str(&assistant.text);
                out.push_str(
                    &assistant
                        .calls
                        .iter()
                        .map(call)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            Entry::Results(results) => {
                out.push_str("<|observation|>\n");
                out.push_str(&tool_responses(results));
            }
        }
    }
    out
}
