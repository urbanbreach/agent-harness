use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn write_json_pretty(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(json_io_error)?;
    file.write_all(b"\n")
}

pub fn write_jsonl(path: &Path, rows: &[Value]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    for row in rows {
        serde_json::to_writer(&mut file, row).map_err(json_io_error)?;
        file.write_all(b"\n")?;
    }
    Ok(())
}

pub fn stable_fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn stable_fingerprint_value(value: &Value) -> String {
    stable_fingerprint_bytes(canonical_json(value).as_bytes())
}

pub fn stable_fingerprint_file(path: &Path) -> Option<String> {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("artifact-index.jsonl") => {
            stable_fingerprint_jsonl_without_embedded_fingerprints(path)
        }
        Some("simulation-report.json") => {
            stable_fingerprint_json_without_embedded_fingerprints(path)
        }
        _ => fs::read(path)
            .ok()
            .map(|bytes| stable_fingerprint_bytes(&bytes)),
    }
}

fn stable_fingerprint_jsonl_without_embedded_fingerprints(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut row = serde_json::from_str::<Value>(line).ok()?;
        normalize_embedded_fingerprints(&mut row);
        rows.push(row);
    }
    Some(stable_fingerprint_value(&Value::Array(rows)))
}

fn stable_fingerprint_json_without_embedded_fingerprints(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut value = serde_json::from_str::<Value>(&text).ok()?;
    normalize_embedded_fingerprints(&mut value);
    Some(stable_fingerprint_value(&value))
}

fn normalize_embedded_fingerprints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(fingerprint) = map.get_mut("fingerprint") {
                *fingerprint = Value::String("<normalized-fingerprint>".to_owned());
            }
            for child in map.values_mut() {
                normalize_embedded_fingerprints(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_embedded_fingerprints(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub(super) fn first_json_diff(
    left: &Value,
    right: &Value,
    path: String,
) -> (String, String, String) {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            let keys = left_map
                .keys()
                .chain(right_map.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                match (left_map.get(key), right_map.get(key)) {
                    (Some(left_value), Some(right_value)) if left_value == right_value => {}
                    (Some(left_value), Some(right_value)) => {
                        return first_json_diff(left_value, right_value, format!("{path}.{key}"))
                    }
                    (Some(left_value), None) => {
                        return (
                            format!("{path}.{key}"),
                            canonical_json(left_value),
                            "<missing>".to_owned(),
                        )
                    }
                    (None, Some(right_value)) => {
                        return (
                            format!("{path}.{key}"),
                            "<missing>".to_owned(),
                            canonical_json(right_value),
                        )
                    }
                    (None, None) => {}
                }
            }
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            for index in 0..left_items.len().max(right_items.len()) {
                match (left_items.get(index), right_items.get(index)) {
                    (Some(left_value), Some(right_value)) if left_value == right_value => {}
                    (Some(left_value), Some(right_value)) => {
                        return first_json_diff(left_value, right_value, format!("{path}[{index}]"))
                    }
                    (Some(left_value), None) => {
                        return (
                            format!("{path}[{index}]"),
                            canonical_json(left_value),
                            "<missing>".to_owned(),
                        )
                    }
                    (None, Some(right_value)) => {
                        return (
                            format!("{path}[{index}]"),
                            "<missing>".to_owned(),
                            canonical_json(right_value),
                        )
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {}
    }
    (path, canonical_json(left), canonical_json(right))
}

pub(super) fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn json_io_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
