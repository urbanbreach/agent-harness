use super::*;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static REFERENCE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"\{env:([A-Za-z_][A-Za-z_0-9]*)\}|\$\{([A-Za-z_][A-Za-z_0-9]*)(?::-([^}]*))?\}|\{file:([^}]+)\}",
    )
});

pub(super) fn expand(
    value: &mut Value,
    base: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<(), ConfigError> {
    expand_bounded(value, base, lookup, &mut (4 * 1024 * 1024))
}
fn expand_bounded(
    value: &mut Value,
    base: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
    remaining: &mut usize,
) -> Result<(), ConfigError> {
    match value {
        Value::Array(values) => {
            for value in values {
                expand_bounded(value, base, lookup, remaining)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                expand_bounded(value, base, lookup, remaining)?;
            }
        }
        Value::String(text) => {
            let regex = REFERENCE.as_ref().map_err(normalize::parse_error)?;
            let mut captures = regex.captures_iter(text).peekable();
            if captures.peek().is_none() {
                charge(text.len(), remaining)?;
                return Ok(());
            }
            let mut output = String::new();
            let mut last = 0;
            for capture in captures {
                let Some(full) = capture.get(0) else { continue };
                charge(full.start() - last, remaining)?;
                output.push_str(&text[last..full.start()]);
                let replacement = if let Some(key) = capture.get(1) {
                    lookup(key.as_str()).unwrap_or_default()
                } else if let Some(key) = capture.get(2) {
                    lookup(key.as_str())
                        .filter(|v| capture.get(3).is_none() || !v.is_empty())
                        .or_else(|| capture.get(3).map(|m| m.as_str().into()))
                        .ok_or_else(|| {
                            ConfigError(format!("environment variable {} is not set", key.as_str()))
                        })?
                } else if let Some(file) = capture.get(4) {
                    loader::read_text(&base.join(file.as_str()))?
                        .trim_end()
                        .into()
                } else {
                    continue;
                };
                charge(replacement.len(), remaining)?;
                output.push_str(&replacement);
                last = full.end();
            }
            charge(text.len() - last, remaining)?;
            output.push_str(&text[last..]);
            *text = output;
        }
        _ => {}
    }
    Ok(())
}

fn charge(bytes: usize, remaining: &mut usize) -> Result<(), ConfigError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| ConfigError("expanded configuration exceeds 4 MiB".into()))?;
    Ok(())
}
