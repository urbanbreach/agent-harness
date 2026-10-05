use super::*;
use serde_json::{Map, Value};

pub(super) fn populate(row: &mut TranscriptToolCallSection, tool: &ToolCallEntry) {
    let mut source = tool.args_summary.as_str();
    let (input, active) = partial_value(&mut source, 0).unwrap_or_default();
    let input = match crate::transcript_blocks::RawDisclosure::from_json(&input).payload {
        crate::transcript_blocks::RawPayload::Json(value) => value,
        crate::transcript_blocks::RawPayload::Text(_) => Value::Null,
    };
    let id = tool.effective_tool_id();
    let path = input["path"]
        .as_str()
        .or_else(|| input["filePath"].as_str());
    let (title, field, language) = match id {
        "eval" => (
            input["summary"].as_str().unwrap_or("Eval"),
            "code",
            input["language"].as_str().unwrap_or(""),
        ),
        "bash" | "shell.run" => ("Run command", "command", "bash"),
        "write" | "fs.write" => ("Write", "content", path.unwrap_or("")),
        "edit" => ("Edit", "newString", path.unwrap_or("")),
        "apply_patch" => ("Apply patch", "patchText", "diff"),
        "spawn_subagent" | "agent.spawn" | "task" => ("Subagent", "prompt", ""),
        "read" | "fs.read" => ("Read", "", ""),
        "grep" | "fs.grep" | "glob" | "fs.glob" => ("Search", "", ""),
        _ => (id, "", ""),
    };
    row.header.title = if id == "eval" && title != "Eval" {
        format!("Eval {title}")
    } else if let Some(path) = path.filter(|_| matches!(id, "write" | "fs.write" | "edit")) {
        format!(
            "{title} {}",
            crate::ui::ui_tool_paths::tool_header_path(path, true)
        )
    } else if is_mcp_tool_id(id) {
        mcp_tool_title(tool, id)
    } else {
        title.to_owned()
    };
    row.header.subtitle = Some(if id == "eval" {
        format!(
            "{} · writing",
            match language {
                "js" => "JavaScript",
                "py" => "Python",
                "rb" => "Ruby",
                "jl" => "Julia",
                _ => "Eval",
            }
        )
    } else {
        "writing".to_owned()
    });
    row.header.visual_style = TranscriptToolCallVisualStyle::Inline;
    row.header.disclosure_state = Some(if row.expanded {
        TranscriptToolCallDisclosureState::Expanded
    } else {
        TranscriptToolCallDisclosureState::Collapsed
    });
    row.details_preview_visible = true;
    let text = if let Some(text) = input[field].as_str() {
        text.to_owned()
    } else {
        let mut lines = Vec::new();
        input_lines(&input, "", &active, &mut lines);
        lines.join("\n")
    };
    row.detail_blocks
        .push(TranscriptToolCallDetailBlock::InputPreview {
            text,
            language: language.to_owned(),
        });
}

fn input_lines(value: &Value, label: &str, active: &[String], lines: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            // Keep the field currently arriving at the bottom, even when JSON's
            // sorted map order would put it above an older, longer argument.
            let current = active.first();
            for (key, value) in object
                .iter()
                .filter(|(key, _)| Some(*key) != current)
                .chain(current.and_then(|key| object.get_key_value(key)))
            {
                input_lines(
                    value,
                    &if label.is_empty() {
                        key.clone()
                    } else {
                        format!("{label}.{key}")
                    },
                    if Some(key) == current {
                        &active[1..]
                    } else {
                        &[]
                    },
                    lines,
                );
            }
        }
        Value::Array(values) => {
            for value in values {
                input_lines(value, label, active, lines);
            }
        }
        Value::Null => {}
        value => {
            let text = value.as_str().map_or_else(
                || value.to_string(),
                |text| {
                    if matches!(
                        label.rsplit('.').next(),
                        Some("path" | "filePath" | "file_path" | "cwd")
                    ) {
                        crate::ui::ui_tool_paths::tool_header_path(text, true)
                    } else {
                        text.to_owned()
                    }
                },
            );
            lines.push(format!("{label}: {text}"));
        }
    }
}

// Decode completed JSON tokens with serde, retaining the unfinished final value.
// This is presentation only; the coordinator still validates the complete call.
fn partial_value(source: &mut &str, depth: usize) -> Option<(Value, Vec<String>)> {
    *source = source.trim_start();
    if depth >= 32 {
        return None;
    }
    match source.as_bytes().first()? {
        b'{' => {
            *source = &source[1..];
            let mut object = Map::new();
            let mut active = Vec::new();
            while let Some((key, true)) = take_string(source) {
                let Some(rest) = source.trim_start().strip_prefix(':') else {
                    break;
                };
                *source = rest;
                let Some((value, nested)) = partial_value(source, depth + 1) else {
                    break;
                };
                active = std::iter::once(key.clone()).chain(nested).collect();
                object.insert(key, value);
                *source = source.trim_start();
                let Some(rest) = source.strip_prefix(',') else {
                    break;
                };
                *source = rest;
            }
            *source = source.trim_start().strip_prefix('}').unwrap_or(source);
            Some((Value::Object(object), active))
        }
        b'[' => {
            *source = &source[1..];
            let mut values = Vec::new();
            let mut active = Vec::new();
            while let Some((value, nested)) = partial_value(source, depth + 1) {
                active = nested;
                values.push(value);
                *source = source.trim_start();
                let Some(rest) = source.strip_prefix(',') else {
                    break;
                };
                *source = rest;
            }
            *source = source.trim_start().strip_prefix(']').unwrap_or(source);
            Some((Value::Array(values), active))
        }
        b'"' => take_string(source).map(|(value, _)| (Value::String(value), Vec::new())),
        _ => {
            let mut stream = serde_json::Deserializer::from_str(source).into_iter::<Value>();
            let value = stream.next()?.ok()?;
            *source = &source[stream.byte_offset()..];
            Some((value, Vec::new()))
        }
    }
}

fn take_string(source: &mut &str) -> Option<(String, bool)> {
    *source = source.trim_start();
    if !source.starts_with('"') {
        return None;
    }
    let bytes = source.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let value = serde_json::from_str(&source[..=index]).ok()?;
                *source = &source[index + 1..];
                return Some((value, true));
            }
            b'\\' => {
                let size = match bytes.get(index + 1) {
                    Some(b'u') => {
                        let Some(hex) = source.get(index + 2..index + 6) else {
                            break;
                        };
                        let code = u16::from_str_radix(hex, 16).ok()?;
                        if (0xd800..=0xdbff).contains(&code) {
                            12
                        } else {
                            6
                        }
                    }
                    Some(_) => 2,
                    None => break,
                };
                if index + size > bytes.len() {
                    break;
                }
                index += size;
            }
            _ => index += 1,
        }
    }
    let value = serde_json::from_str(&format!("{}\"", &source[..index])).ok()?;
    *source = "";
    Some((value, false))
}
