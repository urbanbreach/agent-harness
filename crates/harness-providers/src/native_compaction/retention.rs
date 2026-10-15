use serde_json::{json, Value};
fn tokens(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count().div_ceil(4)).unwrap_or(u32::MAX)
}
fn contextual(item: &Value) -> bool {
    item["content"].as_array().into_iter().flatten().any(|p| {
        p["type"] == "input_text"
            && p["text"].as_str().is_some_and(|s| {
                let s = s.trim_start();
                [
                    "<environment_context>",
                    "<user_instructions>",
                    "<additional_context>",
                    "<skills",
                    "<token_budget>",
                    "<model_switch>",
                ]
                .iter()
                .any(|tag| {
                    s.get(..tag.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag))
                })
            })
    })
}
fn truncate(text: &str, budget: u32) -> String {
    let max = usize::try_from(budget)
        .unwrap_or(usize::MAX)
        .saturating_mul(4);
    let marker = format!(
        "…{} tokens truncated…",
        tokens(text).saturating_sub(budget).max(1)
    );
    let marker_len = marker.encode_utf16().count();
    if max <= marker_len + 2 {
        return edge_slice(text, max, false).to_owned();
    }
    let edge = (max - marker_len) / 2;
    format!(
        "{}{}{}",
        edge_slice(text, edge, false),
        marker,
        edge_slice(text, edge, true)
    )
}
fn edge_slice(text: &str, units: usize, reverse: bool) -> &str {
    let mut used = 0;
    if reverse {
        for (index, ch) in text.char_indices().rev() {
            used += ch.len_utf16();
            if used > units {
                return &text[index + ch.len_utf8()..];
            }
        }
    } else {
        for (index, ch) in text.char_indices() {
            used += ch.len_utf16();
            if used > units {
                return &text[..index];
            }
        }
    }
    text
}
pub(super) fn retain(candidates: &[Value], budget: u32, compaction: Value) -> Vec<Value> {
    let mut remaining = budget.clamp(1, 64000);
    let mut result = Vec::new();
    for item in candidates.iter().rev().filter(|v| {
        v["role"] == "user" && (v["type"].is_null() || v["type"] == "message") && !contextual(v)
    }) {
        if remaining == 0 {
            break;
        }
        let parts: std::borrow::Cow<'_, [Value]> = item["content"].as_array().map_or_else(
            || {
                std::borrow::Cow::Owned(item["content"].as_str().map_or_else(Vec::new, |text| {
                    vec![json!({"type":"input_text","text":text})]
                }))
            },
            |parts| std::borrow::Cow::Borrowed(parts.as_slice()),
        );
        let cost: u32 = parts
            .iter()
            .map(|p| match p["type"].as_str() {
                Some("input_image") => 765,
                Some("input_text" | "output_text") => p["text"].as_str().map_or(0, tokens),
                _ => 0,
            })
            .fold(0, u32::saturating_add)
            .max(1);
        if cost <= remaining {
            remaining -= cost;
            result.push(item.clone());
            continue;
        }
        let mut kept = Vec::new();
        let mut available = remaining;
        for mut part in parts.iter().cloned() {
            match part["type"].as_str() {
                Some("input_image") if available >= 765 => {
                    available -= 765;
                    kept.push(part);
                }
                Some("input_text" | "output_text") if available > 0 => {
                    let text = part["text"].as_str().unwrap_or("");
                    let cost = tokens(text);
                    if cost > available {
                        part["text"] = truncate(text, available).into();
                        kept.push(part);
                        available = 0;
                        break;
                    }
                    available -= cost;
                    kept.push(part);
                }
                _ => {}
            }
        }
        if !kept.is_empty() {
            let mut value = item.clone();
            value["content"] = kept.into();
            result.push(value);
            remaining = available;
        }
    }
    result.reverse();
    result.push(compaction);
    result
}
