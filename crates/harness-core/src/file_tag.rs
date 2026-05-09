use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const READ_DEFAULT_LIMIT: usize = 2_000;

type FileTagChars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

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

        let (end, name) = consume_file_tag_name(&mut indices, idx + ch.len_utf8());

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
        append_materialized_file_tag(
            &mut sections,
            &mut seen,
            workspace_root,
            &tag.filename,
            &tag.path,
            tag.line_range,
        );
    }

    for file_match in files(prompt) {
        let (path_name, line_range) = split_line_range(&file_match.name);
        append_materialized_file_tag(
            &mut sections,
            &mut seen,
            workspace_root,
            &file_match.name,
            path_name,
            line_range,
        );
    }

    join_context_sections(sections)
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
    sections.extend(selected_agents.iter().map(selected_agent_section));
    sections.extend(selected_resources.iter().map(selected_resource_section));
    join_context_sections(sections)
}

fn join_context_sections(sections: Vec<String>) -> Option<String> {
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

fn selected_agent_section(agent: &SelectedAgentTag) -> String {
    format!(
        "Selected agent mention: @{}. Use the task tool with subagent `{}` if delegation is appropriate.",
        agent.name, agent.name
    )
}

fn selected_resource_section(resource: &SelectedResourceTag) -> String {
    let description = resource
        .description
        .as_deref()
        .map(|description| format!("\nDescription: {description}"))
        .unwrap_or_default();
    format!(
        "Selected MCP resource: {}\nURI: {}\nMIME: {}{}",
        resource.name, resource.uri, resource.mime, description
    )
}

fn is_forbidden_file_tag_prefix(ch: char) -> bool {
    ch == '`' || ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_file_tag_separator(ch: char) -> bool {
    ch.is_whitespace() || ch == '`' || ch == ','
}

fn consume_file_tag_name(indices: &mut FileTagChars<'_>, initial_end: usize) -> (usize, String) {
    let mut end = initial_end;
    let mut name = String::new();

    if let Some(&(_, '.')) = indices.peek() {
        let (_, dot) = indices.next().expect("peeked dot");
        end += dot.len_utf8();
        name.push(dot);
    }

    consume_file_tag_segment(indices, &mut end, &mut name);

    while should_continue_after_dot(indices) {
        let (_, dot) = indices.next().expect("peeked dot");
        end += dot.len_utf8();
        name.push(dot);

        consume_file_tag_segment(indices, &mut end, &mut name);
    }

    (end, name)
}

fn should_continue_after_dot(indices: &FileTagChars<'_>) -> bool {
    let mut lookahead = indices.clone();
    let Some((_, '.')) = lookahead.next() else {
        return false;
    };
    let Some(&(_, after_dot)) = lookahead.peek() else {
        return false;
    };
    !is_file_tag_separator(after_dot) && after_dot != '.'
}

fn consume_file_tag_segment(indices: &mut FileTagChars<'_>, end: &mut usize, name: &mut String) {
    while let Some(&(next_idx, next)) = indices.peek() {
        if is_file_tag_separator(next) || next == '.' {
            *end = next_idx;
            break;
        }
        let (_, consumed) = indices.next().expect("peeked char");
        *end += consumed.len_utf8();
        name.push(consumed);
    }
}

fn append_materialized_file_tag(
    sections: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    workspace_root: &Path,
    dedupe_key: &str,
    path_name: &str,
    line_range: Option<FileTagLineRange>,
) {
    if !seen.insert(dedupe_key.to_string()) {
        return;
    }
    let resolved = resolve_tag_path(workspace_root, path_name);
    sections.extend(materialized_file_section(
        workspace_root,
        &resolved,
        line_range,
    ));
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
        FileTagReadOutcome::Output(output) => Some(format!(
            "Called the Read tool with the following input: {args}\n{output}"
        )),
        FileTagReadOutcome::Failure(reason) => Some(format!(
            "Read tool failed to read {} with the following error: {reason}",
            resolved.display()
        )),
        FileTagReadOutcome::Missing => None,
    }
}

enum FileTagReadOutcome {
    Missing,
    Output(String),
    Failure(String),
}

fn materialize_resolved_path(
    workspace_root: &Path,
    resolved: &Path,
    line_range: Option<FileTagLineRange>,
) -> FileTagReadOutcome {
    let canonical = match canonicalize_file_tag_path(workspace_root, resolved) {
        Ok(CanonicalFileTagPath::Path(canonical)) => canonical,
        Ok(CanonicalFileTagPath::Missing) => return FileTagReadOutcome::Missing,
        Err(reason) => return FileTagReadOutcome::Failure(reason),
    };
    match read_canonical_file_tag_path(&canonical, line_range) {
        Ok(output) => FileTagReadOutcome::Output(output),
        Err(reason) => FileTagReadOutcome::Failure(reason),
    }
}

fn canonicalize_file_tag_path(
    workspace_root: &Path,
    resolved: &Path,
) -> Result<CanonicalFileTagPath, String> {
    let workspace = workspace_root
        .canonicalize()
        .map_err(|err| format!("failed to resolve workspace root: {err}"))?;
    let canonical = if resolved == workspace {
        workspace.clone()
    } else {
        match resolved.canonicalize() {
            Ok(canonical) => canonical,
            Err(_) => return Ok(CanonicalFileTagPath::Missing),
        }
    };
    if !canonical.starts_with(&workspace) {
        return Err(format!(
            "path escapes workspace root {}: {}",
            workspace.display(),
            canonical.display()
        ));
    }
    Ok(CanonicalFileTagPath::Path(canonical))
}

enum CanonicalFileTagPath {
    Missing,
    Path(PathBuf),
}

fn read_canonical_file_tag_path(
    canonical: &Path,
    line_range: Option<FileTagLineRange>,
) -> Result<String, String> {
    let metadata = std::fs::metadata(canonical).map_err(|err| err.to_string())?;
    if metadata.is_dir() {
        return read_directory_entries(canonical).map(|entries| entries.join("\n"));
    }
    read_text_file(canonical, line_range)
}

fn read_directory_entries(directory: &Path) -> Result<Vec<String>, String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|err| format!("failed to list directory: {err}"))?
        .map(directory_entry_display_name)
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort();
    truncate_directory_entries(&mut entries);
    Ok(entries)
}

fn directory_entry_display_name(
    entry: std::io::Result<std::fs::DirEntry>,
) -> Result<String, String> {
    let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
    let file_type = entry
        .file_type()
        .map_err(|err| format!("failed to read directory entry type: {err}"))?;
    let mut name = entry.file_name().to_string_lossy().to_string();
    if file_type.is_dir() {
        name.push('/');
    }
    Ok(name)
}

fn truncate_directory_entries(entries: &mut Vec<String>) {
    let total = entries.len();
    if total > READ_DEFAULT_LIMIT {
        let truncated = total - READ_DEFAULT_LIMIT;
        entries.truncate(READ_DEFAULT_LIMIT);
        entries.push(format!("... (truncated, {truncated} more)"));
    }
}

fn read_text_file(path: &Path, line_range: Option<FileTagLineRange>) -> Result<String, String> {
    let content = match read_text_file_content(path)? {
        TextFileContent::Utf8(content) => content,
        TextFileContent::Binary => return Ok(binary_file_omission(path)),
    };
    let total = content.lines().count();
    let selection = line_selection(total, line_range);
    Ok(render_selected_lines(&content, selection))
}

enum TextFileContent {
    Utf8(String),
    Binary,
}

fn read_text_file_content(path: &Path) -> Result<TextFileContent, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read file: {err}"))?;
    if bytes.contains(&0) {
        return Ok(TextFileContent::Binary);
    }
    String::from_utf8(bytes)
        .map(TextFileContent::Utf8)
        .map_err(|_| {
            format!(
                "binary or non-UTF-8 file omitted: MIME {}",
                attachment_mime(path)
            )
        })
}

fn binary_file_omission(path: &Path) -> String {
    format!("[binary file omitted: MIME {}]", attachment_mime(path))
}

fn render_selected_lines(content: &str, selection: LineSelection) -> String {
    let mut lines = content
        .lines()
        .skip(selection.skip_lines)
        .take(selection.requested_lines.min(READ_DEFAULT_LIMIT))
        .enumerate()
        .map(|(idx, line)| format!("{}: {line}", idx + selection.start_line))
        .collect::<Vec<_>>();
    if selection.requested_lines > READ_DEFAULT_LIMIT {
        let requested_lines = selection.requested_lines;
        let start_line = selection.start_line;
        lines.push(format!(
            "... [truncated: showing {READ_DEFAULT_LIMIT} of {requested_lines} requested lines from line {start_line}]",
        ));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineSelection {
    start_line: usize,
    skip_lines: usize,
    requested_lines: usize,
}

fn line_selection(total_lines: usize, line_range: Option<FileTagLineRange>) -> LineSelection {
    let start_line = line_range.map(|range| range.start.max(1)).unwrap_or(1);
    let end_line = line_selection_end_line(total_lines, start_line, line_range);
    let skip_lines = start_line.saturating_sub(1);
    LineSelection {
        start_line,
        skip_lines,
        requested_lines: end_line.saturating_sub(skip_lines),
    }
}

fn line_selection_end_line(
    total_lines: usize,
    start_line: usize,
    line_range: Option<FileTagLineRange>,
) -> usize {
    let requested_end_line = match line_range {
        Some(range) => range.end.unwrap_or(start_line).max(start_line),
        None => total_lines,
    };
    requested_end_line.min(total_lines)
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
