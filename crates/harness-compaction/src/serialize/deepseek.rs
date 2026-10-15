//! DeepSeek records; the markers use fullwidth bars and lower one-eighth blocks.
use super::transcript::{think_fence, Call, Entry};

fn call(call: &Call) -> String {
    format!(
        "<｜tool▁call▁begin｜>{}<｜tool▁sep｜>{}<｜tool▁call▁end｜>",
        call.name,
        call.args.compact()
    )
}

pub(super) fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("<｜begin▁of▁sentence｜>");
    for entry in entries {
        match entry {
            Entry::Developer(text) => out.push_str(text),
            Entry::User(text) => out.push_str(&format!("<｜User｜>{text}")),
            Entry::Assistant(assistant) => {
                out.push_str("<｜Assistant｜>");
                out.push_str(&think_fence(assistant.thinking()));
                out.push_str(&assistant.text);
                if !assistant.calls.is_empty() {
                    out.push_str("<｜tool▁calls▁begin｜>");
                    out.extend(assistant.calls.iter().map(call));
                    out.push_str("<｜tool▁calls▁end｜>");
                }
                out.push_str("<｜end▁of▁sentence｜>");
            }
            Entry::Results(results) => out.push_str(
                &results
                    .iter()
                    .map(|result| {
                        format!(
                            "<｜tool▁output▁begin｜>{}<｜tool▁output▁end｜>",
                            result.text
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }
    out
}
