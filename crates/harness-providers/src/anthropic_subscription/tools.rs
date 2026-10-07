//! Tool exposure (senpi `tools.ts`, `custom-tools.ts`, `custom-tools-schema.ts`).
//!
//! Claude Code plans; the harness executes. `read`, `bash`, `grep` and `glob` are served as
//! Claude Code's built-ins; every other tool (including `write`/`edit`, whose built-in
//! read-before-write validator would answer the call itself) goes through the
//! `custom-tools` MCP server. Every execution is denied Claude-Code-side.
use super::protocol::CustomTool;
use crate::ToolDef;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Claude Code built-in (lowercased) -> harness tool.
pub const SDK_TO_HOST_TOOL_NAME: [(&str, &str); 6] = [
    ("read", "read"),
    ("write", "write"),
    ("edit", "edit"),
    ("bash", "bash"),
    ("grep", "grep"),
    ("glob", "glob"),
];
/// Harness tools served to Claude Code as its built-ins.
pub const HOST_TO_SDK_TOOL_NAME: [(&str, &str); 5] = [
    ("read", "Read"),
    ("bash", "Bash"),
    ("grep", "Grep"),
    ("find", "Glob"),
    ("glob", "Glob"),
];
/// Versioned host-tool denial policy; bump when denial copy or hooks change so resident
/// sessions re-fingerprint (`toolset_changed`) instead of keeping the old reason.
pub const HOST_TOOL_POLICY_FINGERPRINT: &str = "host-tool-denial-v3";
pub const BUILTIN_SDK_TOOLS: [&str; 4] = ["Read", "Bash", "Grep", "Glob"];
pub const TOOL_EXECUTION_DENIED_MESSAGE: &str = "Harness executes this tool on the host and returns its result as the next user message. Wait for that result; this denial is not a failure.";
pub const CUSTOM_TOOLS_MCP_SERVER_NAME: &str = "custom-tools";
pub const CUSTOM_TOOLS_MCP_PREFIX: &str = "mcp__custom-tools__";
pub const HOST_CAPTURED_SDK_TOOL_MATCHER: &str =
    "Bash|Write|Edit|Read|Grep|Glob|mcp__custom-tools__.*";

#[derive(Debug, Clone, Default)]
pub struct ResolvedSdkTools {
    pub sdk_tools: Vec<String>,
    pub custom_tools: Vec<ToolDef>,
    pub custom_tool_name_to_sdk: BTreeMap<String, String>,
    pub custom_tool_name_to_host: BTreeMap<String, String>,
}

fn lookup<'a>(table: &'a [(&str, &str)], name: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

fn pascal_case(value: &str) -> String {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

pub fn map_host_tool_name_to_sdk(name: &str, custom: Option<&BTreeMap<String, String>>) -> String {
    let normalized = name.to_lowercase();
    custom
        .and_then(|map| map.get(name).or_else(|| map.get(&normalized)))
        .cloned()
        .or_else(|| lookup(&HOST_TO_SDK_TOOL_NAME, &normalized).map(str::to_owned))
        .unwrap_or_else(|| pascal_case(name))
}

pub fn map_sdk_tool_name_to_host(name: &str, custom: Option<&BTreeMap<String, String>>) -> String {
    let normalized = name.to_lowercase();
    lookup(&SDK_TO_HOST_TOOL_NAME, &normalized)
        .map(str::to_owned)
        .or_else(|| custom.and_then(|map| map.get(name).or_else(|| map.get(&normalized)).cloned()))
        .unwrap_or_else(|| {
            if normalized.starts_with(CUSTOM_TOOLS_MCP_PREFIX) {
                name[CUSTOM_TOOLS_MCP_PREFIX.len()..].to_owned()
            } else {
                name.to_owned()
            }
        })
}

/// Translate Claude Code's built-in-tool inputs into the harness tool schemas. Keys a
/// harness schema would reject are left out instead of sent as `null`.
pub fn map_tool_args(tool_name: &str, args: &Map<String, Value>) -> Map<String, Value> {
    let get = |key: &str| args.get(key).filter(|v| !v.is_null()).cloned();
    let first = |keys: &[&str]| keys.iter().find_map(|key| get(key));
    let mut out = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            out.insert(key.into(), value);
        }
    };
    match tool_name.to_lowercase().as_str() {
        "read" => {
            put("filePath", first(&["file_path", "filePath", "path"]));
            put("offset", get("offset"));
            put("limit", get("limit"));
        }
        "write" => {
            put("filePath", first(&["file_path", "filePath", "path"]));
            put("content", get("content"));
        }
        "edit" => {
            put("filePath", first(&["file_path", "filePath", "path"]));
            if args.get("edits").is_some_and(Value::is_array) {
                put("edits", get("edits"));
            } else {
                put("oldString", first(&["old_string", "oldString", "old_text"]));
                put("newString", first(&["new_string", "newString", "new_text"]));
            }
            put("replaceAll", first(&["replace_all", "replaceAll"]));
        }
        // The harness `bash` timeout is already in milliseconds, like Claude Code's.
        "bash" => {
            put("command", get("command"));
            put("timeout", get("timeout"));
        }
        // The harness `grep` has no ignore-case flag; an inline `(?i)` carries `-i`.
        "grep" => {
            let insensitive = first(&["-i", "ignoreCase"]).is_some_and(|v| v == true);
            let pattern = get("pattern").map(|p| match (insensitive, p.as_str()) {
                (true, Some(text)) => json!(format!("(?i){text}")),
                _ => p,
            });
            put("pattern", pattern);
            put("path", get("path"));
            put("include", first(&["glob", "include"]));
            put("context", first(&["context", "-C"]));
            put("limit", first(&["head_limit", "limit"]));
        }
        "glob" | "find" => {
            put("pattern", get("pattern"));
            put("path", get("path"));
            put("limit", get("limit"));
        }
        _ => return args.clone(),
    }
    out
}

pub fn resolve_sdk_tools(tools: Option<&[ToolDef]>) -> ResolvedSdkTools {
    let Some(tools) = tools else {
        return ResolvedSdkTools {
            sdk_tools: BUILTIN_SDK_TOOLS.map(str::to_owned).into(),
            ..ResolvedSdkTools::default()
        };
    };
    let mut resolved = ResolvedSdkTools::default();
    for tool in tools {
        let name = &tool.function_name;
        let normalized = name.to_lowercase();
        if let Some(sdk) = lookup(&HOST_TO_SDK_TOOL_NAME, &normalized) {
            if !resolved.sdk_tools.iter().any(|t| t == sdk) {
                resolved.sdk_tools.push(sdk.into());
            }
            continue;
        }
        let sdk_name = format!("{CUSTOM_TOOLS_MCP_PREFIX}{name}");
        resolved.custom_tools.push(tool.clone());
        resolved
            .custom_tool_name_to_sdk
            .insert(name.clone(), sdk_name.clone());
        resolved
            .custom_tool_name_to_sdk
            .insert(normalized, sdk_name.clone());
        resolved
            .custom_tool_name_to_host
            .insert(sdk_name.to_lowercase(), name.clone());
        resolved
            .custom_tool_name_to_host
            .insert(sdk_name, name.clone());
    }
    resolved
}

pub fn custom_tool_servers(tools: &[ToolDef]) -> Vec<CustomTool> {
    tools
        .iter()
        .map(|tool| CustomTool {
            name: tool.function_name.clone(),
            description: tool.description.clone(),
            input_schema: input_schema(&tool.parameters),
        })
        .collect()
}

/// Inlines local `$ref`s: harness tool schemas come from schemars (`$defs`), while the
/// conversion below, like senpi's, reads inline schemas only.
fn inline_refs(schema: &Value, root: &Value, depth: usize) -> Value {
    match schema {
        Value::Object(map) => {
            let target = map
                .get("$ref")
                .and_then(Value::as_str)
                .filter(|_| depth < 16)
                .and_then(|reference| {
                    let path = reference
                        .strip_prefix("#/$defs/")
                        .map(|name| ("$defs", name))
                        .or_else(|| {
                            reference
                                .strip_prefix("#/definitions/")
                                .map(|name| ("definitions", name))
                        })?;
                    root.get(path.0)?.get(path.1)
                });
            let mut out = match target {
                Some(target) => inline_refs(target, root, depth + 1)
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
                None => Map::new(),
            };
            for (key, value) in map {
                if key != "$ref" && key != "$defs" && key != "definitions" {
                    out.insert(key.clone(), inline_refs(value, root, depth));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| inline_refs(item, root, depth))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// JSON Schema exactly as the MCP SDK emits it for senpi's zod shape conversion.
pub fn input_schema(parameters: &Value) -> Value {
    let mut schema = object_schema(&inline_refs(parameters, parameters, 0));
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "$schema".into(),
            json!("http://json-schema.org/draft-07/schema#"),
        );
    }
    schema
}

fn object_schema(schema: &Value) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    let wanted: Vec<&str> = schema["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(input) = schema["properties"].as_object() {
        for (key, value) in input {
            properties.insert(key.clone(), convert(value));
            if wanted.contains(&key.as_str()) {
                required.push(json!(key));
            }
        }
    }
    let mut out = json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        out["required"] = Value::Array(required);
    }
    out
}

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

fn literal(value: &Value) -> Option<Value> {
    Some(match value {
        Value::Null => json!({"type": "null"}),
        Value::String(_) => json!({"type": "string", "const": value}),
        Value::Number(_) => json!({"type": "number", "const": value}),
        Value::Bool(_) => json!({"type": "boolean", "const": value}),
        _ => return None,
    })
}

fn union(variants: Vec<Value>) -> Value {
    let simple: Option<Vec<Value>> = variants
        .iter()
        .map(|v| {
            v.as_object()
                .filter(|o| o.len() == 1)
                .and_then(|o| o.get("type"))
                .filter(|t| t.is_string())
                .cloned()
        })
        .collect();
    match simple {
        Some(types) => json!({"type": types}),
        None => json!({"anyOf": variants}),
    }
}

fn convert(schema: &Value) -> Value {
    let mut converted = None;
    if let Some(constant) = schema.get("const") {
        converted = literal(constant);
    } else if let Some(values) = schema["enum"].as_array().filter(|v| !v.is_empty()) {
        let literals: Vec<Value> = values
            .iter()
            .filter(|v| v.is_string() || v.is_number() || v.is_boolean())
            .filter_map(literal)
            .collect();
        converted = match literals.len() {
            0 => None,
            1 => literals.into_iter().next(),
            _ => Some(union(literals)),
        };
    }
    let mut converted = converted.unwrap_or_else(|| {
        let variants = schema["anyOf"]
            .as_array()
            .or_else(|| schema["oneOf"].as_array())
            .filter(|v| !v.is_empty());
        if let Some(variants) = variants {
            let mut converted: Vec<Value> = variants.iter().map(convert).collect();
            if converted.len() == 1 {
                return converted.remove(0);
            }
            return union(converted);
        }
        let kind = match &schema["type"] {
            Value::Array(types) => types.first().and_then(Value::as_str),
            other => other.as_str(),
        };
        match kind {
            Some("string") => json!({"type": "string"}),
            Some("number") => json!({"type": "number"}),
            Some("integer") => json!({"type": "integer", "minimum": -MAX_SAFE_INTEGER, "maximum": MAX_SAFE_INTEGER}),
            Some("boolean") => json!({"type": "boolean"}),
            Some("null") => json!({"type": "null"}),
            Some("array") => json!({"type": "array", "items": match schema.get("items") {
                Some(items) if items.is_object() => convert(items),
                _ => json!({}),
            }}),
            Some("object") => object_schema(schema),
            _ => json!({}),
        }
    });
    if let Some(description) = schema["description"].as_str()
        && let Some(object) = converted.as_object_mut()
    {
        object.insert("description".into(), json!(description));
    }
    converted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_conversion_matches_the_sdk_manifest() {
        let schema = json!({"type": "object", "properties": {
            "int": {"type": "integer", "description": "an int"},
            "e_mixed": {"enum": ["x", 1, true]},
            "e_one": {"enum": ["only"]},
            "any_sn": {"anyOf": [{"type": "string"}, {"type": "number"}, {"type": "null"}]},
            "one_of": {"oneOf": [{"type": "integer"}, {"type": "boolean"}]},
            "t_arr": {"type": ["string", "null"]},
            "arr": {"type": "array"},
            "fmt": {"type": "string", "format": "uri", "default": "x"},
            "c_null": {"const": null},
            "nested": {"type": "object", "properties": {"k": {"type": "boolean"}}, "required": ["k"]},
            "mode": {"$ref": "#/$defs/Mode", "description": "how"},
        }, "required": ["int", "any_sn"], "$defs": {"Mode": {"enum": ["a", "b"]}}});
        let int =
            json!({"type": "integer", "minimum": -MAX_SAFE_INTEGER, "maximum": MAX_SAFE_INTEGER});
        assert_eq!(
            input_schema(&schema),
            json!({"$schema": "http://json-schema.org/draft-07/schema#", "type": "object", "properties": {
                "int": {"type": "integer", "minimum": -MAX_SAFE_INTEGER, "maximum": MAX_SAFE_INTEGER, "description": "an int"},
                "e_mixed": {"anyOf": [{"type": "string", "const": "x"}, {"type": "number", "const": 1}, {"type": "boolean", "const": true}]},
                "e_one": {"type": "string", "const": "only"},
                "any_sn": {"type": ["string", "number", "null"]},
                "one_of": {"anyOf": [int, {"type": "boolean"}]},
                "t_arr": {"type": "string"},
                "arr": {"type": "array", "items": {}},
                "fmt": {"type": "string"},
                "c_null": {"type": "null"},
                "nested": {"type": "object", "properties": {"k": {"type": "boolean"}}, "required": ["k"]},
                "mode": {"anyOf": [{"type": "string", "const": "a"}, {"type": "string", "const": "b"}], "description": "how"},
            }, "required": ["any_sn", "int"]})
        );
    }
}
