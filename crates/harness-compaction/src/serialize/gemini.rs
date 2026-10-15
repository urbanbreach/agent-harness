//! Gemini turns with Python-style tool code.
use super::{
    json::Json,
    transcript::{Assistant, Call, Entry},
};

/// Python literal text of a JSON value.
fn python(value: &Json) -> String {
    match value {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Number(text) => text.clone(),
        Json::Str(text) => string(text),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(python).collect::<Vec<_>>().join(", ")
        ),
        Json::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", string(key), python(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn string(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn call(call: &Call) -> String {
    let arguments = call
        .args
        .entries()
        .iter()
        .map(|(key, value)| format!("{key}={}", python(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("default_api.{}({arguments})", call.name)
}

fn assistant(assistant: &Assistant) -> String {
    let mut out = String::new();
    if !assistant.thinking().is_empty() {
        out.push_str(&format!("```thinking\n{}\n```", assistant.thinking()));
    }
    out.push_str(&assistant.text);
    let calls: Vec<String> = assistant.calls.iter().map(call).collect();
    let body = match calls.as_slice() {
        [] => "[]".to_owned(),
        [only] => only.clone(),
        _ => format!("[{}]", calls.join(", ")),
    };
    out.push_str(&format!("```tool_code\n{body}\n```"));
    out
}

/// Join developer and user text with one LF unless either side is empty.
fn join(left: &str, right: &str) -> String {
    match (left.is_empty(), right.is_empty()) {
        (true, _) => right.to_owned(),
        (_, true) => left.to_owned(),
        _ => format!("{left}\n{right}"),
    }
}

pub(super) fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("<bos>");
    let mut pending = String::new();
    let turn = |out: &mut String, role: &str, body: &str| {
        out.push_str(&format!("<start_of_turn>{role}\n{body}<end_of_turn>\n"));
    };
    for entry in entries {
        if !matches!(entry, Entry::Developer(_) | Entry::User(_)) && !pending.is_empty() {
            turn(&mut out, "user", &std::mem::take(&mut pending));
        }
        match entry {
            Entry::Developer(text) => pending = join(&pending, text),
            Entry::User(text) => turn(&mut out, "user", &join(&std::mem::take(&mut pending), text)),
            Entry::Assistant(body) => turn(&mut out, "model", &assistant(body)),
            Entry::Results(results) => {
                let outputs = results
                    .iter()
                    .map(|result| format!("```tool_outputs\n{}\n```", result.text))
                    .collect::<Vec<_>>()
                    .join("\n");
                turn(&mut out, "user", &outputs);
            }
        }
    }
    if !pending.is_empty() {
        turn(&mut out, "user", &pending);
    }
    out
}
