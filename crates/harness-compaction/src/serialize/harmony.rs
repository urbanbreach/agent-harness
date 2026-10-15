//! Harmony role/channel records.
use super::transcript::Entry;

const MARKERS: [&str; 7] = [
    "start",
    "end",
    "message",
    "channel",
    "constrain",
    "return",
    "call",
];

fn recipient(name: &str) -> String {
    if name.starts_with("functions.") {
        name.to_owned()
    } else {
        format!("functions.{name}")
    }
}

pub(super) fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            Entry::User(text) => out.push_str(&format!("<|start|>user<|message|>{text}<|end|>")),
            Entry::Developer(text) => {
                out.push_str(&format!("<|start|>developer<|message|>{text}<|end|>"))
            }
            Entry::Assistant(assistant) => {
                let mut emitted = false;
                if !assistant.thinking().is_empty() {
                    out.push_str(&format!(
                        "<|start|>assistant<|channel|>analysis<|message|>{}<|end|>",
                        assistant.thinking()
                    ));
                    emitted = true;
                }
                if !assistant.text.is_empty() {
                    out.push_str(&format!(
                        "<|start|>assistant<|channel|>final<|message|>{}<|end|>",
                        assistant.text
                    ));
                    emitted = true;
                }
                for call in &assistant.calls {
                    out.push_str(&format!(
                        "<|start|>assistant<|channel|>commentary to={}<|message|>{}<|call|>",
                        recipient(&call.name),
                        call.args.compact()
                    ));
                    emitted = true;
                }
                if !emitted {
                    out.push_str("<|start|>assistant<|channel|>final<|message|><|end|>");
                }
            }
            Entry::Results(results) => {
                for result in results {
                    out.push_str(&format!(
                        "<|start|>{} to=assistant<|channel|>commentary<|message|>{}<|end|>",
                        recipient(&result.name),
                        result.text
                    ));
                }
            }
        }
    }
    out
}

/// Defuse reserved control markers so summarized history cannot forge records.
pub(super) fn neutralize(text: &str) -> String {
    MARKERS.iter().fold(text.to_owned(), |text, marker| {
        text.replace(&format!("<|{marker}|>"), &format!("<\\|{marker}\\|>"))
    })
}
