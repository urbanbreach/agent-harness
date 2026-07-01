use harness_core::tool::ToolError;

use super::matching::seek;

#[derive(Clone)]
pub(super) enum Hunk {
    Add {
        path: String,
        contents: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_path: Option<String>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Clone)]
pub(super) struct UpdateChunk {
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    change_context: Option<String>,
    end_of_file: bool,
}

pub(super) fn parse_patch(patch_text: &str) -> Result<Vec<Hunk>, ToolError> {
    let text = strip_heredoc(patch_text.trim());
    let lines = text.split('\n').collect::<Vec<_>>();
    let begin = lines
        .iter()
        .position(|line| line.trim() == "*** Begin Patch");
    let end = lines.iter().position(|line| line.trim() == "*** End Patch");
    let (Some(begin), Some(end)) = (begin, end) else {
        return Err(ToolError::Execution(
            "Invalid patch format: missing Begin/End markers".to_string(),
        ));
    };
    if begin >= end {
        return Err(ToolError::Execution(
            "Invalid patch format: missing Begin/End markers".to_string(),
        ));
    }

    let mut hunks = Vec::new();
    let mut index = begin + 1;
    while index < end {
        let line = lines[index];
        if let Some(path) = line.strip_prefix("*** Add File:") {
            let (contents, next) = parse_add(&lines, index + 1)?;
            hunks.push(Hunk::Add {
                path: require_path(path, "Invalid add file path")?,
                contents,
            });
            index = next;
        } else if let Some(path) = line.strip_prefix("*** Delete File:") {
            hunks.push(Hunk::Delete {
                path: require_path(path, "Invalid delete file path")?,
            });
            index += 1;
        } else if let Some(path) = line.strip_prefix("*** Update File:") {
            let mut next = index + 1;
            let move_path = lines
                .get(next)
                .and_then(|line| line.strip_prefix("*** Move to:"))
                .map(str::trim)
                .map(str::to_string);
            if move_path.is_some() {
                next += 1;
            }
            let (chunks, parsed_next) = parse_update(&lines, next)?;
            if chunks.is_empty() {
                return Err(ToolError::Execution(format!(
                    "Invalid update hunk for {}: expected at least one @@ chunk",
                    path.trim()
                )));
            }
            hunks.push(Hunk::Update {
                path: require_path(path, "Invalid update file path")?,
                move_path,
                chunks,
            });
            index = parsed_next;
        } else {
            return Err(ToolError::Execution(format!("Invalid patch line: {line}")));
        }
    }
    Ok(hunks)
}

fn parse_add(lines: &[&str], start: usize) -> Result<(String, usize), ToolError> {
    let mut content = Vec::new();
    let mut index = start;
    while index < lines.len() && !lines[index].starts_with("***") {
        let Some(line) = lines[index].strip_prefix('+') else {
            return Err(ToolError::Execution(format!(
                "Invalid add file line: {}",
                lines[index]
            )));
        };
        content.push(line.to_string());
        index += 1;
    }
    Ok((content.join("\n"), index))
}

fn parse_update(lines: &[&str], start: usize) -> Result<(Vec<UpdateChunk>, usize), ToolError> {
    let mut chunks = Vec::new();
    let mut index = start;
    while index < lines.len() && !lines[index].starts_with("***") {
        if !lines[index].starts_with("@@") {
            return Err(ToolError::Execution(format!(
                "Invalid update file line: {}",
                lines[index]
            )));
        }
        let change_context = Some(
            lines[index]
                .strip_prefix("@@")
                .unwrap_or("")
                .trim()
                .to_string(),
        )
        .filter(|value| !value.is_empty());
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();
        let mut end_of_file = false;
        index += 1;
        while index < lines.len() && !lines[index].starts_with("@@") {
            let line = lines[index];
            if line == "*** End of File" {
                end_of_file = true;
                index += 1;
                break;
            }
            if line.starts_with("***") {
                break;
            }
            if let Some(value) = line.strip_prefix(' ') {
                old_lines.push(value.to_string());
                new_lines.push(value.to_string());
            } else if let Some(value) = line.strip_prefix('-') {
                old_lines.push(value.to_string());
            } else if let Some(value) = line.strip_prefix('+') {
                new_lines.push(value.to_string());
            } else {
                return Err(ToolError::Execution(format!(
                    "Invalid update chunk line: {line}"
                )));
            }
            index += 1;
        }
        chunks.push(UpdateChunk {
            old_lines,
            new_lines,
            change_context,
            end_of_file,
        });
    }
    Ok((chunks, index))
}

pub(super) fn derive_update(
    path: &str,
    chunks: &[UpdateChunk],
    original: &str,
) -> Result<String, String> {
    let mut lines = original.split('\n').map(str::to_string).collect::<Vec<_>>();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut replacements = Vec::new();
    let mut line_index = 0;
    for chunk in chunks {
        if let Some(context) = &chunk.change_context {
            let context_index = seek(&lines, std::slice::from_ref(context), line_index, false)
                .ok_or_else(|| format!("Failed to find context '{context}' in {path}"))?;
            line_index = context_index + 1;
        }
        if chunk.old_lines.is_empty() {
            replacements.push((lines.len(), 0, chunk.new_lines.clone()));
            continue;
        }
        let (found, old_lines, new_lines) = find_update_lines(&lines, chunk, line_index)
            .ok_or_else(|| {
                format!(
                    "Failed to find expected lines in {path}:\n{}",
                    chunk.old_lines.join("\n")
                )
            })?;
        replacements.push((found, old_lines.len(), new_lines.to_vec()));
        line_index = found + old_lines.len();
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    for (start, remove, insert) in replacements.into_iter().rev() {
        lines.splice(start..start + remove, insert);
    }
    if lines.last().is_none_or(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

fn find_update_lines<'a>(
    lines: &[String],
    chunk: &'a UpdateChunk,
    line_index: usize,
) -> Option<(usize, &'a [String], &'a [String])> {
    let mut old_lines = chunk.old_lines.as_slice();
    let mut new_lines = chunk.new_lines.as_slice();
    let mut found = seek(lines, old_lines, line_index, chunk.end_of_file);
    if found.is_none() && old_lines.last().is_some_and(String::is_empty) {
        old_lines = &old_lines[..old_lines.len() - 1];
        if new_lines.last().is_some_and(String::is_empty) {
            new_lines = &new_lines[..new_lines.len() - 1];
        }
        found = seek(lines, old_lines, line_index, chunk.end_of_file);
    }
    found.map(|index| (index, old_lines, new_lines))
}

pub(super) fn add_file_content(content: &str) -> String {
    if content.ends_with('\n') || content.is_empty() {
        content.to_string()
    } else {
        format!("{content}\n")
    }
}

fn require_path(path: &str, message: &str) -> Result<String, ToolError> {
    let value = path.trim();
    if value.is_empty() {
        Err(ToolError::Execution(message.to_string()))
    } else {
        Ok(value.to_string())
    }
}

fn strip_heredoc(input: &str) -> &str {
    input
}
