//! Gemma turns with unescaped string-fenced tool values.
use super::{
    json::Json,
    transcript::{Assistant, Call, Entry, ToolResult},
};

const FENCE: &str = "<|\"|>";

/// Gemma value text: raw keys and no escaping inside fenced strings.
fn value(json: &Json) -> String {
    match json {
        Json::Null => "null".into(),
        Json::Bool(flag) => flag.to_string(),
        Json::Number(text) => text.clone(),
        Json::Str(text) => format!("{FENCE}{text}{FENCE}"),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(value).collect::<Vec<_>>().join(",")
        ),
        Json::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, item)| format!("{key}:{}", value(item)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn call(call: &Call) -> String {
    let arguments = call
        .args
        .entries()
        .iter()
        .map(|(key, item)| format!("{key}:{}", value(item)))
        .collect::<Vec<_>>()
        .join(",");
    format!("<|tool_call>call:{}{{{arguments}}}<tool_call|>", call.name)
}

fn responses(results: &[ToolResult]) -> String {
    let mut out = String::new();
    for result in results {
        let parsed = Json::parse(&result.text).unwrap_or_else(|| Json::Str(result.text.clone()));
        out.push_str(&format!(
            "<|tool_response>response:{}{{output:{}}}<tool_response|>",
            result.name,
            value(&parsed)
        ));
    }
    out
}

fn assistant(assistant: &Assistant) -> String {
    let thinking = if assistant.thinking().is_empty() {
        String::new()
    } else {
        format!("<|channel>thought\n{}<channel|>", assistant.thinking())
    };
    let calls: String = assistant.calls.iter().map(call).collect();
    format!("{thinking}{}{calls}", assistant.text)
}

pub(super) fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("<bos>");
    let mut turn = |role: &str, body: &str| out.push_str(&format!("<|turn>{role}\n{body}<turn|>"));
    let mut index = 0;
    while let Some(entry) = entries.get(index) {
        match entry {
            Entry::User(text) => turn("user", text),
            Entry::Developer(text) => turn("system", text),
            Entry::Assistant(body) => {
                let mut text = assistant(body);
                if let Some(Entry::Results(results)) = entries.get(index + 1) {
                    text.push_str(&responses(results));
                    index += 1;
                }
                turn("model", &text);
            }
            Entry::Results(results) => turn("model", &responses(results)),
        }
        index += 1;
    }
    out
}
