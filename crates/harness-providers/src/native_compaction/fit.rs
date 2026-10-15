use super::{NativeCompactionError, ProviderErrorCategory};
use harness_tokenizer::{image_size_from_data_url, Counter, ImageDetail};
use serde_json::{json, Value};

const TRUNCATED: &str = "Output: exceeded available model context → truncated.";
/// Tokens reserved for framing the request adds around the measured text.
const OVERHEAD: i64 = 256;

/// Measures the request the way the model will read it: ciphertext is dropped and an image
/// costs its price, not its base64. Returns the image tokens.
fn normalize(value: &mut Value, counter: &Counter) -> u64 {
    match value {
        Value::Object(object) => {
            if object
                .get("encrypted_content")
                .is_some_and(Value::is_string)
            {
                object.remove("encrypted_content");
            }
            let image = object.get("image_url").and_then(Value::as_str).map(|url| {
                let detail = match object.get("detail").and_then(Value::as_str) {
                    Some("auto") => Some(ImageDetail::Auto),
                    Some("low") => Some(ImageDetail::Low),
                    Some("high") => Some(ImageDetail::High),
                    Some("original") => Some(ImageDetail::Original),
                    _ => None,
                };
                u64::from(counter.image_tokens(image_size_from_data_url(url), detail))
            });
            if image.is_some() {
                object.insert("image_url".into(), "<image>".into());
            }
            object
                .values_mut()
                .map(|value| normalize(value, counter))
                .fold(image.unwrap_or(0), u64::saturating_add)
        }
        Value::Array(array) => array
            .iter_mut()
            .map(|value| normalize(value, counter))
            .fold(0, u64::saturating_add),
        _ => 0,
    }
}

struct Fit {
    /// Text, images and overhead are within the window.
    fits: bool,
    /// Text and overhead alone are within the window.
    text_fits: bool,
    estimate: u64,
}

fn measure(body: &Value, window: u32, counter: &Counter) -> Fit {
    let mut measurement = json!({"instructions":body["instructions"],"input":body["input"]});
    if let Some(tools) = body.get("tools") {
        measurement["tools"] = tools.clone();
    }
    let images = normalize(&mut measurement, counter);
    let text = measurement.to_string();
    let budget = i64::from(window) - OVERHEAD;
    let text_probe = counter.probe_budget([text.as_str()], budget);
    let fits = if images == 0 {
        text_probe.fits
    } else {
        let after_images = budget.saturating_sub(i64::try_from(images).unwrap_or(i64::MAX));
        counter.probe_budget([text.as_str()], after_images).fits
    };
    Fit {
        fits,
        text_fits: text_probe.fits,
        estimate: text_probe
            .tokens
            .saturating_add(images)
            .saturating_add(OVERHEAD.unsigned_abs()),
    }
}

/// Rewrites trailing tool outputs, newest first, until the request fits. An image-only
/// overflow sends the input untouched; a text overflow no rewrite can cure is rejected
/// before any request.
pub(super) fn protect(
    body: &mut Value,
    window: u32,
    counter: &Counter,
    model: &str,
) -> Result<(), NativeCompactionError> {
    if window == 0 {
        return Ok(());
    }
    let original = measure(body, window, counter);
    if original.fits {
        return Ok(());
    }
    let mut rewritten = body.clone();
    let count = rewritten["input"].as_array().map_or(0, Vec::len);
    for index in (0..count).rev() {
        let item = &mut rewritten["input"][index];
        if item["type"] == "compaction_trigger" {
            continue;
        }
        let attachment = item["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|p| p["type"] == "input_image")
                && parts.iter().any(|p| {
                    p["type"] == "input_text" && p["text"] == "Attached image(s) from tool result:"
                })
        });
        if attachment {
            continue;
        }
        match item["type"].as_str() {
            Some("function_call_output" | "custom_tool_call_output") => {
                item["output"] = TRUNCATED.into();
            }
            Some("tool_search_output") => item["tools"] = json!([]),
            _ => break,
        }
        let next = measure(&rewritten, window, counter);
        if next.fits || !original.text_fits && next.text_fits {
            *body = rewritten;
            return Ok(());
        }
    }
    if original.text_fits {
        return Ok(());
    }
    Err(NativeCompactionError {
        message: format!(
            "native compaction context overflow for {model}: estimate {}, window {window}",
            original.estimate
        ),
        category: Some(ProviderErrorCategory::ContextWindowExceeded),
        status: None,
        auth: false,
        headers: std::collections::BTreeMap::new(),
        cause: None,
    })
}
