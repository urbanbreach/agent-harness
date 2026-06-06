use std::path::Path;

use crate::text::has_trimmed_content;

#[derive(Debug, Clone, Default)]
pub(super) struct ParsedSkillFrontmatter {
    pub(super) name: Option<String>,
    pub(super) description: Option<String>,
    pub(super) argument_hint: Option<String>,
    pub(super) allowed_tools: Vec<String>,
    pub(super) target_agent: Option<String>,
    pub(super) target_category: Option<String>,
    pub(super) deferred_mcp: Option<String>,
    pub(super) deferred_resources: Option<String>,
}

struct FrontmatterFieldLine<'a> {
    key: &'a str,
    raw_value: &'a str,
}

pub(super) fn read_frontmatter_text(skill_file: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(skill_file)
        .map_err(|err| format!("failed to read skill file {}: {err}", skill_file.display()))?;
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Err("skill frontmatter must start with `---`".to_string());
    }
    let mut frontmatter = String::from("---\n");
    let mut found_closing = false;
    for line in lines {
        frontmatter.push_str(line);
        frontmatter.push('\n');
        if line == "---" {
            found_closing = true;
            break;
        }
    }
    if !found_closing {
        return Err("skill frontmatter must end with `---`".to_string());
    }
    Ok(frontmatter)
}

pub(super) fn parse_skill_frontmatter(content: &str) -> Result<ParsedSkillFrontmatter, String> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Err("skill frontmatter must start with `---`".to_string());
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in lines {
        if line == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_closing {
        return Err("skill frontmatter must end with `---`".to_string());
    }

    parse_frontmatter_fields(&frontmatter_lines)
}

fn parse_frontmatter_fields(lines: &[&str]) -> Result<ParsedSkillFrontmatter, String> {
    let mut frontmatter = ParsedSkillFrontmatter::default();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if !has_trimmed_content(line) {
            index += 1;
            continue;
        }
        if leading_indent(line)? != 0 {
            return Err(format!(
                "frontmatter line `{}` must not be indented",
                line.trim()
            ));
        }

        let field = parse_frontmatter_field_line(line, |trimmed| {
            format!("frontmatter line `{trimmed}` must use `key: value` syntax")
        })?;

        match field.key {
            "name" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.name = Some(value);
                index = next_index;
            }
            "description" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.description = Some(value);
                index = next_index;
            }
            "argument_hint" | "argumentHint" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.argument_hint = non_empty(value);
                index = next_index;
            }
            "allowed_tools" | "allowedTools" | "expected_tools" | "expectedTools" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.allowed_tools = parse_list_field(&value);
                index = next_index;
            }
            "target_agent" | "targetAgent" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.target_agent = non_empty(value);
                index = next_index;
            }
            "target_category" | "targetCategory" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.target_category = non_empty(value);
                index = next_index;
            }
            "mcp" | "deferred_mcp" | "deferredMcp" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.deferred_mcp = non_empty(value);
                index = next_index;
            }
            "resources" | "deferred_resources" | "deferredResources" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.deferred_resources = non_empty(value);
                index = next_index;
            }
            "license" | "compatibility" => {
                let (_, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                index = next_index;
            }
            "metadata" => {
                index = parse_metadata_field(lines, index, field.raw_value)?;
            }
            _ => {
                return Err(format!(
                    "unsupported V1 skill frontmatter field `{}`",
                    field.key
                ));
            }
        }
    }

    Ok(frontmatter)
}

fn parse_frontmatter_field_line<'a>(
    line: &'a str,
    syntax_error: impl FnOnce(&str) -> String,
) -> Result<FrontmatterFieldLine<'a>, String> {
    let trimmed = line.trim_end();
    let (key, raw_value) = trimmed
        .split_once(':')
        .ok_or_else(|| syntax_error(trimmed))?;
    Ok(FrontmatterFieldLine {
        key: key.trim(),
        raw_value: raw_value.trim_start(),
    })
}

fn parse_scalar_field(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(String, usize), String> {
    match raw_value {
        ">" => {
            let (value, next_index) = collect_block_scalar(lines, index + 1, 0)?;
            Ok((
                value
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                next_index,
            ))
        }
        "|" => collect_block_scalar(lines, index + 1, 0),
        "" => {
            if next_line_is_indented(lines, index + 1)? {
                return Err("frontmatter scalar fields must not use nested mappings".to_string());
            }
            Ok((String::new(), index + 1))
        }
        value => Ok((parse_inline_scalar(value), index + 1)),
    }
}

fn parse_metadata_field(lines: &[&str], index: usize, raw_value: &str) -> Result<usize, String> {
    if !raw_value.is_empty() {
        return Err("frontmatter `metadata` must be a string-to-string map".to_string());
    }

    let mut cursor = index + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }

        let field = parse_frontmatter_field_line(line.trim(), |trimmed| {
            format!("frontmatter `metadata` entry `{trimmed}` must use `key: value` syntax")
        })?;
        if !has_trimmed_content(field.key) {
            return Err("frontmatter `metadata` keys must not be empty".to_string());
        }

        match field.raw_value {
            ">" | "|" => {
                let (_, next_index) = collect_block_scalar(lines, cursor + 1, indent)?;
                cursor = next_index;
            }
            "" => {
                if next_line_has_deeper_indent(lines, cursor + 1, indent)? {
                    return Err("frontmatter `metadata` must be a string-to-string map".to_string());
                }
                cursor += 1;
            }
            _ => {
                cursor += 1;
            }
        }
    }

    Ok(cursor)
}

fn collect_block_scalar(
    lines: &[&str],
    start_index: usize,
    parent_indent: usize,
) -> Result<(String, usize), String> {
    let mut cursor = start_index;
    let mut block_indent = None;
    let mut values = Vec::new();

    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            values.push(String::new());
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent <= parent_indent {
            break;
        }

        let block_indent = *block_indent.get_or_insert(indent);
        if indent < block_indent {
            break;
        }
        values.push(line[block_indent..].to_string());
        cursor += 1;
    }

    Ok((values.join("\n"), cursor))
}

fn next_line_is_indented(lines: &[&str], start_index: usize) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?.is_some_and(|indent| indent > 0))
}

fn next_line_has_deeper_indent(
    lines: &[&str],
    start_index: usize,
    current_indent: usize,
) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?
        .is_some_and(|indent| indent > current_indent))
}

fn next_non_empty_line_indent(lines: &[&str], start_index: usize) -> Result<Option<usize>, String> {
    for line in &lines[start_index..] {
        if !has_trimmed_content(line) {
            continue;
        }
        return leading_indent(line).map(Some);
    }
    Ok(None)
}

fn leading_indent(line: &str) -> Result<usize, String> {
    let indent = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    if line[..indent].contains('\t') {
        return Err("frontmatter indentation must use spaces".to_string());
    }
    Ok(indent)
}

fn parse_inline_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn parse_list_field(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|item| non_empty(item.trim().trim_matches('"').trim_matches('\'').to_string()))
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
