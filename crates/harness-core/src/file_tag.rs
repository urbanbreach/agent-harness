use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const READ_DEFAULT_LIMIT: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTagMatch {
    pub start: usize,
    pub end: usize,
    pub value: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedFileTag {
    pub path: String,
    pub filename: String,
    pub url: String,
    pub mime: String,
    pub source: FileTagSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_range: Option<FileTagLineRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedAgentTag {
    pub name: String,
    pub source: FileTagSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedResourceTag {
    pub name: String,
    pub uri: String,
    pub mime: String,
    pub source: FileTagSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedPromptTags {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SelectedFileTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<SelectedAgentTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<SelectedResourceTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTagSource {
    pub start: usize,
    pub end: usize,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTagLineRange {
    pub start: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
}

pub fn files(template: &str) -> Vec<FileTagMatch> {
    let mut matches = Vec::new();
    let mut indices = template.char_indices().peekable();
    let mut previous = None;

    while let Some((idx, ch)) = indices.next() {
        if ch != '@' {
            previous = Some(ch);
            continue;
        }

        if previous.is_some_and(is_forbidden_file_tag_prefix) {
            previous = Some(ch);
            continue;
        }

        let mut end = idx + ch.len_utf8();
        let mut name = String::new();

        if let Some(&(_, '.')) = indices.peek() {
            let (_, dot) = indices.next().expect("peeked dot");
            end += dot.len_utf8();
            name.push(dot);
        }

        while let Some(&(next_idx, next)) = indices.peek() {
            if is_file_tag_separator(next) || next == '.' {
                end = next_idx;
                break;
            }
            let (_, consumed) = indices.next().expect("peeked char");
            end += consumed.len_utf8();
            name.push(consumed);
        }

        loop {
            let Some(&(_, '.')) = indices.peek() else {
                break;
            };
            let mut lookahead = indices.clone();
            let _ = lookahead.next();
            let Some(&(_, after_dot)) = lookahead.peek() else {
                break;
            };
            if is_file_tag_separator(after_dot) || after_dot == '.' {
                break;
            }

            let (_, dot) = indices.next().expect("peeked dot");
            end += dot.len_utf8();
            name.push(dot);

            while let Some(&(next_idx, next)) = indices.peek() {
                if is_file_tag_separator(next) || next == '.' {
                    end = next_idx;
                    break;
                }
                let (_, consumed) = indices.next().expect("peeked char");
                end += consumed.len_utf8();
                name.push(consumed);
            }
        }

        matches.push(FileTagMatch {
            start: idx,
            end,
            value: template[idx..end].to_string(),
            name,
        });
        previous = template[..end].chars().next_back();
    }

    matches
}

pub fn materialize_file_tag_context(workspace_root: &Path, prompt: &str) -> Option<String> {
    materialize_file_tag_context_with_selected(workspace_root, prompt, &[])
}

pub fn materialize_file_tag_context_with_selected(
    workspace_root: &Path,
    prompt: &str,
    selected: &[SelectedFileTag],
) -> Option<String> {
    let mut seen = BTreeSet::new();
    let mut sections = Vec::new();

    for tag in selected {
        if !seen.insert(tag.filename.clone()) {
            continue;
        }
        let resolved = resolve_tag_path(workspace_root, &tag.path);
        sections.extend(materialized_file_section(
            workspace_root,
            &resolved,
            tag.line_range,
        ));
    }

    for file_match in files(prompt) {
        if !seen.insert(file_match.name.clone()) {
            continue;
        }
        let (path_name, line_range) = split_line_range(&file_match.name);
        let resolved = resolve_tag_path(workspace_root, path_name);
        sections.extend(materialized_file_section(
            workspace_root,
            &resolved,
            line_range,
        ));
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

pub fn materialize_prompt_part_context(
    workspace_root: &Path,
    prompt: &str,
    selected_files: &[SelectedFileTag],
    selected_agents: &[SelectedAgentTag],
    selected_resources: &[SelectedResourceTag],
) -> Option<String> {
    let mut sections =
        materialize_file_tag_context_with_selected(workspace_root, prompt, selected_files)
            .into_iter()
            .collect::<Vec<_>>();
    for agent in selected_agents {
        sections.push(format!(
            "Selected agent mention: @{}. Use the task tool with subagent `{}` if delegation is appropriate.",
            agent.name, agent.name
        ));
    }
    for resource in selected_resources {
        let description = resource
            .description
            .as_deref()
            .map(|description| format!("\nDescription: {description}"))
            .unwrap_or_default();
        sections.push(format!(
            "Selected MCP resource: {}\nURI: {}\nMIME: {}{}",
            resource.name, resource.uri, resource.mime, description
        ));
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

fn is_forbidden_file_tag_prefix(ch: char) -> bool {
    ch == '`' || ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_file_tag_separator(ch: char) -> bool {
    ch.is_whitespace() || ch == '`' || ch == ','
}

fn resolve_tag_path(workspace_root: &Path, name: &str) -> PathBuf {
    if let Some(home_relative) = name.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(home_relative);
        }
    }

    let path = Path::new(name);
    if path.is_absolute() {
        return normalize_path(path);
    }
    normalize_path(&workspace_root.join(path))
}

fn materialized_file_section(
    workspace_root: &Path,
    resolved: &Path,
    line_range: Option<FileTagLineRange>,
) -> Option<String> {
    let args = serde_json::json!({ "filePath": resolved.display().to_string() }).to_string();
    match materialize_resolved_path(workspace_root, resolved, line_range) {
        Some(Ok(output)) => Some(format!(
            "Called the Read tool with the following input: {args}\n{output}"
        )),
        Some(Err(reason)) => Some(format!(
            "Read tool failed to read {} with the following error: {reason}",
            resolved.display()
        )),
        None => None,
    }
}

fn materialize_resolved_path(
    workspace_root: &Path,
    resolved: &Path,
    line_range: Option<FileTagLineRange>,
) -> Option<Result<String, String>> {
    let workspace = match workspace_root.canonicalize() {
        Ok(workspace) => workspace,
        Err(err) => return Some(Err(format!("failed to resolve workspace root: {err}"))),
    };
    let canonical = if resolved == workspace {
        workspace.clone()
    } else {
        match resolved.canonicalize() {
            Ok(canonical) => canonical,
            Err(_) => return None,
        }
    };
    if !canonical.starts_with(&workspace) {
        return Some(Err(format!(
            "path escapes workspace root {}: {}",
            workspace.display(),
            canonical.display()
        )));
    }

    let metadata = match std::fs::metadata(&canonical) {
        Ok(metadata) => metadata,
        Err(err) => return Some(Err(err.to_string())),
    };
    if metadata.is_dir() {
        Some(read_directory_entries(&canonical).map(|entries| entries.join("\n")))
    } else {
        Some(read_text_file(&canonical, line_range))
    }
}

fn read_directory_entries(directory: &Path) -> Result<Vec<String>, String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|err| format!("failed to list directory: {err}"))?
        .map(|entry| {
            let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
            let file_type = entry
                .file_type()
                .map_err(|err| format!("failed to read directory entry type: {err}"))?;
            let mut name = entry.file_name().to_string_lossy().to_string();
            if file_type.is_dir() {
                name.push('/');
            }
            Ok(name)
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort();
    let total = entries.len();
    if total > READ_DEFAULT_LIMIT {
        let truncated = total - READ_DEFAULT_LIMIT;
        entries.truncate(READ_DEFAULT_LIMIT);
        entries.push(format!("... (truncated, {truncated} more)"));
    }
    Ok(entries)
}

fn read_text_file(path: &Path, line_range: Option<FileTagLineRange>) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read file: {err}"))?;
    if bytes.contains(&0) {
        return Ok(format!(
            "[binary file omitted: MIME {}]",
            attachment_mime(path)
        ));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        format!(
            "binary or non-UTF-8 file omitted: MIME {}",
            attachment_mime(path)
        )
    })?;
    let total = content.lines().count();
    let start_line = line_range.map(|range| range.start.max(1)).unwrap_or(1);
    let end_line = match line_range {
        Some(range) => range
            .end
            .map(|end| end.max(start_line))
            .unwrap_or(start_line),
        None => total,
    }
    .min(total);
    let skip = start_line.saturating_sub(1);
    let requested = end_line.saturating_sub(skip);
    let mut lines = content
        .lines()
        .skip(skip)
        .take(requested.min(READ_DEFAULT_LIMIT))
        .enumerate()
        .map(|(idx, line)| format!("{}: {line}", idx + start_line))
        .collect::<Vec<_>>();
    if requested > READ_DEFAULT_LIMIT {
        lines.push(format!(
            "... [truncated: showing {} of {requested} requested lines from line {start_line}]",
            READ_DEFAULT_LIMIT,
        ));
    }
    Ok(lines.join("\n"))
}

fn attachment_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        _ => "text/plain",
    }
}

pub fn split_line_range(value: &str) -> (&str, Option<FileTagLineRange>) {
    let Some((path, suffix)) = value.rsplit_once('#') else {
        return (value, None);
    };
    let Some(line_range) = parse_line_range(suffix) else {
        return (path, None);
    };
    (path, Some(line_range))
}

fn parse_line_range(value: &str) -> Option<FileTagLineRange> {
    let (start, end) = match value.split_once('-') {
        Some((start, end)) => (start, (!end.is_empty()).then_some(end)),
        None => (value, None),
    };
    let start = start.parse::<usize>().ok()?;
    let end = end.and_then(|end| end.parse::<usize>().ok());
    Some(FileTagLineRange { start, end })
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        }
    }
    normalized
}
