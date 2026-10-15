//! Tool arguments as ordered JSON values with ECMAScript-compatible rendering.
//!
//! Provider arguments are JSON text, but dialects render them the way a JavaScript runtime
//! would: object keys in enumeration order (integer-like keys first), numbers through
//! `Number::toString`, and compact `JSON.stringify` string escaping.
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use std::{collections::HashMap, fmt};

/// Largest integer magnitude a JavaScript number keeps exactly.
const EXACT_INTEGER: u64 = 1 << 53;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Json {
    Null,
    Bool(bool),
    /// The ECMAScript `String(number)` text of a finite number.
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub(super) fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    /// Entries of an object in enumeration order; empty for every other value.
    pub(super) fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Object(entries) => entries,
            _ => &[],
        }
    }

    /// Compact `JSON.stringify` text.
    pub(super) fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Number(text) => out.push_str(text),
            Json::Str(text) => quote_into(text, out),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    quote_into(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// `JSON.stringify` of one string.
pub(super) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    quote_into(text, &mut out);
    out
}

fn quote_into(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number::toString` for a double.
pub(super) fn es_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let width = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let point = exponent.parse::<i32>().unwrap_or(0) + 1;
    let body = if width <= point && point <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(point - width).unwrap_or(0))
        )
    } else if 0 < point && point <= 21 {
        let split = usize::try_from(point).unwrap_or(0);
        format!("{}.{}", &digits[..split], &digits[split..])
    } else if -6 < point && point <= 0 {
        format!(
            "0.{}{digits}",
            "0".repeat(usize::try_from(-point).unwrap_or(0))
        )
    } else {
        let exponent = point - 1;
        let tail = format!(
            "e{}{}",
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        );
        if width == 1 {
            format!("{digits}{tail}")
        } else {
            format!("{}.{}{tail}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// A canonical array index: decimal digits without a leading zero, below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    let canonical = key == "0" || !key.starts_with('0');
    if key.is_empty() || !canonical || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

/// Apply object semantics: a repeated key keeps its first position with the last value,
/// and integer-like keys enumerate first in ascending order.
fn enumeration_order(entries: Vec<(String, Json)>) -> Vec<(String, Json)> {
    let mut positions: HashMap<String, usize> = HashMap::with_capacity(entries.len());
    let mut unique: Vec<(String, Json)> = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        match positions.get(&key) {
            Some(position) => unique[*position].1 = value,
            None => {
                positions.insert(key.clone(), unique.len());
                unique.push((key, value));
            }
        }
    }
    let (mut indexed, named): (Vec<_>, Vec<_>) = unique
        .into_iter()
        .partition(|(key, _)| array_index(key).is_some());
    indexed.sort_by_key(|(key, _)| array_index(key));
    indexed.extend(named);
    indexed
}

fn integer<E: serde::de::Error>(magnitude: u64, negative: bool) -> Result<Json, E> {
    let text = if magnitude <= EXACT_INTEGER {
        let sign = if negative && magnitude != 0 { "-" } else { "" };
        format!("{sign}{magnitude}")
    } else {
        // Larger integers lose precision exactly as a JavaScript number does.
        let value = magnitude.to_string().parse::<f64>().unwrap_or(f64::MAX);
        es_number(if negative { -value } else { value })
    };
    Ok(Json::Number(text))
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Json, E> {
        integer(value.unsigned_abs(), value < 0)
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Json, E> {
        integer(value, false)
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Json, E> {
        Ok(Json::Number(es_number(value)))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Json, E> {
        Ok(Json::Str(value.to_owned()))
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry::<String, Json>()? {
            entries.push(entry);
        }
        Ok(Json::Object(enumeration_order(entries)))
    }
}

/// Top-level arguments of a tool call as `(key, compact JSON)` pairs in enumeration
/// order; `None` when the arguments are not a JSON object.
pub(crate) fn ordered_arguments(arguments_json: &str) -> Option<Vec<(String, String)>> {
    match Json::parse(arguments_json)? {
        Json::Object(entries) => Some(
            entries
                .into_iter()
                .map(|(key, value)| (key, value.compact()))
                .collect(),
        ),
        _ => None,
    }
}
