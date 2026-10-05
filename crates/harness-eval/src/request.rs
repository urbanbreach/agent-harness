use crate::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Option<String>,
    language: Option<String>,
    code: Option<String>,
    #[serde(alias = "title")]
    summary: Option<String>,
    timeout: Option<f64>,
    on_timeout: Option<String>,
    reset: Option<bool>,
    cell_id: Option<String>,
}

/// Validate a public eval request before creating a kernel or scheduling work.
pub fn normalize_request(mut input: Value, languages: &[String]) -> Result<Value> {
    let args: Args = serde_json::from_value(input.clone())?;
    let control = matches!(args.action.as_deref(), Some("peek" | "stop" | "list"));
    if !control
        && (!matches!(args.action.as_deref(), None | Some("run"))
            || args
                .language
                .as_ref()
                .is_none_or(|language| !languages.contains(language))
            || args.code.as_ref().is_none_or(|code| code.trim().is_empty())
            || args
                .summary
                .as_ref()
                .is_none_or(|summary| summary.trim().is_empty()))
    {
        return Err("run requires an available language, code, and summary".into());
    }
    if args
        .timeout
        .is_some_and(|n| !n.is_finite() || !(1.0..=86400.0).contains(&n))
        || args
            .on_timeout
            .as_deref()
            .is_some_and(|value| !matches!(value, "detach" | "error"))
        || matches!(args.action.as_deref(), Some("peek" | "stop"))
            && args
                .cell_id
                .as_ref()
                .is_none_or(|id| id.is_empty() || id.len() > 256)
        || control
            && (args.code.is_some()
                || args.language.is_some()
                || args.summary.is_some()
                || args.timeout.is_some()
                || args.on_timeout.is_some()
                || args.reset.is_some())
        || (!control || args.action.as_deref() == Some("list")) && args.cell_id.is_some()
    {
        return Err("invalid eval control or deadline arguments".into());
    }
    if let Some(summary) = args.summary {
        input["summary"] = summary
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .into();
        if let Some(object) = input.as_object_mut() {
            object.remove("title");
        }
    }
    Ok(input)
}
