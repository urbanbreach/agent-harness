use serde_json::Value;

use crate::app::ActivePermissionView;

#[derive(Debug, Default)]
struct ParsedPermissionSummary {
    path: Option<String>,
    dir: Option<String>,
    pattern: Option<String>,
    url: Option<String>,
    command: Option<String>,
    diff: Option<String>,
    selectors: Vec<String>,
}

impl ParsedPermissionSummary {
    fn parse(summary: &str) -> Self {
        let trimmed = summary.trim();
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return Self::default();
        }

        let Some(value) = serde_json::from_str::<Value>(trimmed).ok() else {
            return Self::default();
        };

        Self {
            path: find_string(&value, &["path", "file", "filepath"]),
            dir: find_string(&value, &["dir", "directory", "cwd"]),
            pattern: find_string(&value, &["pattern", "query", "glob", "regex"]),
            url: find_string(&value, &["url", "href"]),
            command: find_string(&value, &["command", "cmd"]),
            diff: find_string(&value, &["diff", "patch"]),
            selectors: find_strings(&value, &["selectors", "selector"]),
        }
    }

    fn path_or_selector_path(&self) -> Option<&str> {
        self.path.as_deref().or_else(|| {
            self.selectors
                .iter()
                .find_map(|selector| selector.strip_prefix("workspace:"))
                .filter(|path| !path.is_empty())
        })
    }
}

pub(in crate::ui) fn permission_modal_typed_subject_line(
    permission: &ActivePermissionView,
) -> Option<String> {
    let parsed = ParsedPermissionSummary::parse(&permission.summary);
    let kind = permission.kind.to_ascii_lowercase();
    let tool = permission
        .tool_label
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches_tool(&kind, &tool, &["edit", "edit_fs"]) {
        return parsed
            .path_or_selector_path()
            .map(|path| format!("Edit {path}"));
    }
    if matches_tool(&kind, &tool, &["read", "fs.read"]) {
        return parsed
            .path_or_selector_path()
            .map(|path| format!("Read {path}"));
    }
    if matches_tool(&kind, &tool, &["list", "ls", "fs.list"]) {
        return parsed
            .dir
            .as_deref()
            .or_else(|| parsed.path_or_selector_path())
            .map(|dir| format!("List {dir}"));
    }
    if matches_tool(&kind, &tool, &["glob"]) {
        return parsed
            .pattern
            .as_deref()
            .map(|pattern| format!("Glob \"{pattern}\""));
    }
    if matches_tool(&kind, &tool, &["grep", "codesearch"]) {
        return parsed
            .pattern
            .as_deref()
            .map(|pattern| format!("Grep \"{pattern}\""));
    }
    if matches_tool(&kind, &tool, &["webfetch"]) {
        return parsed.url.as_deref().map(|url| format!("Fetch {url}"));
    }
    if matches_tool(&kind, &tool, &["websearch"]) {
        return parsed
            .pattern
            .as_deref()
            .map(|pattern| format!("Search web \"{pattern}\""));
    }
    if matches_tool(&kind, &tool, &["bash", "shell"]) {
        return parsed
            .command
            .as_deref()
            .map(short_command_subject)
            .or_else(|| Some("Run shell command".to_string()));
    }
    if matches_tool(&kind, &tool, &["task"]) {
        return Some("Run task".to_string());
    }

    None
}

pub(in crate::ui) fn permission_modal_diff_preview_lines(
    permission: &ActivePermissionView,
) -> Vec<String> {
    let parsed = ParsedPermissionSummary::parse(&permission.summary);
    let Some(diff) = parsed.diff else {
        return Vec::new();
    };

    let changed_lines = diff
        .lines()
        .filter(|line| {
            (line.starts_with('+') && !line.starts_with("+++"))
                || (line.starts_with('-') && !line.starts_with("---"))
        })
        .take(8)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if changed_lines.is_empty() {
        let mut lines = vec!["Diff preview".to_string()];
        lines.extend(diff.lines().take(8).map(str::to_string));
        return lines;
    }

    let mut changed_lines = changed_lines.into_iter();
    let Some(first_line) = changed_lines.next() else {
        return Vec::new();
    };
    let mut lines = vec![format!("Diff preview · {first_line}")];
    lines.extend(changed_lines);
    lines
}

pub(in crate::ui) fn permission_modal_always_scope_line(
    permission: &ActivePermissionView,
) -> String {
    let parsed = ParsedPermissionSummary::parse(&permission.summary);
    let selectors = durable_selectors(&parsed, &permission.request_digest);
    if selectors.iter().any(|selector| selector == "*") {
        return "This creates a run-scoped durable grant for * until restart.".to_string();
    }

    format!(
        "This creates a run-scoped durable grant for {}.",
        selectors.join(", ")
    )
}

fn durable_selectors(parsed: &ParsedPermissionSummary, digest: &str) -> Vec<String> {
    if !parsed.selectors.is_empty() {
        return parsed.selectors.clone();
    }
    if let Some(path) = parsed.path_or_selector_path() {
        return vec![path.to_string()];
    }

    vec![format!("digest:{}", abbreviated_digest(digest))]
}

fn abbreviated_digest(digest: &str) -> String {
    let mut short = digest.chars().take(12).collect::<String>();
    if digest.chars().count() > 12 {
        short.push('…');
    }
    short
}

fn matches_tool(kind: &str, tool: &str, names: &[&str]) -> bool {
    names.iter().any(|name| kind == *name || tool == *name)
}

fn short_command_subject(command: &str) -> String {
    let command = command.trim();
    if command.is_empty() {
        return "Run shell command".to_string();
    }

    let prefix = command.chars().take(42).collect::<String>();
    if command.chars().count() > 42 {
        format!("Run `{prefix}…`")
    } else {
        format!("Run `{prefix}`")
    }
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map
                    .get(*key)
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    return Some(text.to_string());
                }
            }

            map.get("args")
                .or_else(|| map.get("input"))
                .and_then(|nested| find_string(nested, keys))
        }
        Value::Array(values) => values.iter().find_map(|item| find_string(item, keys)),
        _ => None,
    }
}

fn find_strings(value: &Value, keys: &[&str]) -> Vec<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(strings) = strings_from_value(map.get(*key)) {
                    return strings;
                }
            }

            map.get("args")
                .or_else(|| map.get("input"))
                .map(|nested| find_strings(nested, keys))
                .unwrap_or_default()
        }
        Value::Array(values) => values
            .iter()
            .flat_map(|item| find_strings(item, keys))
            .collect(),
        _ => Vec::new(),
    }
}

fn strings_from_value(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::String(text)) if !text.is_empty() => Some(vec![text.to_string()]),
        Some(Value::Array(values)) => {
            let strings = values
                .iter()
                .filter_map(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            Some(strings)
        }
        _ => None,
    }
}
