//! The labeled `[Role]: text` transcript, used when no native dialect applies.
use super::transcript::{Call, Entry};

fn call(call: &Call) -> String {
    let arguments = call
        .args
        .entries()
        .iter()
        .map(|(key, value)| format!("{key}={}", value.compact()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({arguments})", call.name)
}

pub(super) fn render(entries: &[Entry]) -> String {
    let mut segments = Vec::new();
    for entry in entries {
        match entry {
            Entry::User(text) if !text.is_empty() => segments.push(format!("[User]: {text}")),
            Entry::Assistant(assistant) => {
                if let Some(thinking) = &assistant.thinking {
                    segments.push(format!("[Think]: {thinking}"));
                }
                if assistant.has_text {
                    segments.push(format!("[Assistant]: {}", assistant.text));
                }
                if !assistant.calls.is_empty() {
                    let calls: Vec<String> = assistant.calls.iter().map(call).collect();
                    segments.push(format!("[Tool Call]: {}", calls.join("; ")));
                }
            }
            Entry::Results(results) => segments.extend(
                results
                    .iter()
                    .map(|result| format!("[Tool Result]: {}", result.text)),
            ),
            Entry::User(_) | Entry::Developer(_) => {}
        }
    }
    segments.join("\n\n")
}
