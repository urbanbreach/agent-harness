//! Reducer-facing facts about a finished tool result.
use crate::item::ToolFacts;
use serde_json::Value;

/// What a finished tool call reported, as the coordinator holds it.
#[derive(Debug, Clone, Copy)]
pub struct ResultFacts<'a> {
    pub tool: &'a str,
    pub is_error: bool,
    /// The tool flagged the result as carrying no information.
    pub useless: bool,
    /// The result is an internal recovery artifact.
    pub source_artifact: bool,
    pub structured: Option<&'a Value>,
}

/// Builds [`ToolFacts`] from a finished call. Errors are never useless and never complete.
pub fn tool_facts(result: ResultFacts<'_>) -> ToolFacts {
    ToolFacts {
        is_error: result.is_error,
        useless: result.useless && !result.is_error,
        pruned: false,
        read_complete: (result.tool == "read" && !result.is_error)
            .then(|| result.structured.is_some_and(read_complete)),
        internal_artifact: result.source_artifact,
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// Whether a read result's structured output proves the whole file came back.
///
/// Any truncation flag, a summary, or a column-truncated line makes it incomplete. Positive
/// proof is a reported total line count, or an image. Notices, directories and binary
/// results without that proof are incomplete.
pub fn read_complete(structured: &Value) -> bool {
    let meta = structured.get("meta");
    let truncated = truthy(structured.get("truncated"))
        || truthy(structured.get("summary"))
        || truthy(structured.pointer("/truncation/truncated"))
        || truthy(meta.and_then(|meta| meta.get("truncation")))
        || truthy(meta.and_then(|meta| meta.pointer("/limits/columnTruncated")))
        || truthy(structured.get("byte_truncated"));
    if truncated {
        return false;
    }
    let total = |key: &str| structured.get(key).is_some_and(|value| !value.is_null());
    total("totalLines")
        || total("total_lines")
        || structured.get("kind").and_then(Value::as_str) == Some("image")
}
