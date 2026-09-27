use harness_core::tool::ToolError;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Edit {
    op: String,
    #[serde(default, alias = "start")]
    pos: Option<String>,
    #[serde(default)]
    end: Option<String>,
    lines: Lines,
}
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum Lines {
    Text(String),
    List(Vec<String>),
    Empty(()),
}
fn invalid(message: &str) -> ToolError {
    ToolError::InvalidArguments(message.into())
}
pub(crate) fn hash(line: &str) -> String {
    blake3::hash(line.as_bytes()).to_hex()[..8].to_ascii_uppercase()
}

pub(crate) fn apply(source: &str, edits: Vec<Edit>) -> Result<String, ToolError> {
    if edits.is_empty() || edits.len() > 1000 {
        return Err(invalid("provide between 1 and 1000 line edits"));
    }
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    // Offsets avoid copying the file into one allocation per line.
    let starts: Vec<_> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .filter(|&index| index < source.len())
        .collect();
    let anchor = |reference: &str| -> Result<(usize, usize), ToolError> {
        let (line, expected) = reference
            .split_once('#')
            .ok_or_else(|| invalid("anchor must be LINE#HASH; read the file first"))?;
        let index = line
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(|| invalid("anchor line must be positive"))?;
        let start = *starts
            .get(index)
            .ok_or_else(|| invalid("anchor line is outside the file"))?;
        let end = starts.get(index + 1).copied().unwrap_or(source.len());
        let text = source[start..end]
            .strip_suffix('\n')
            .unwrap_or(&source[start..end]);
        let text = text.strip_suffix('\r').unwrap_or(text);
        if !hash(text).eq_ignore_ascii_case(expected) {
            return Err(invalid(
                "anchor changed; read the current file before editing",
            ));
        }
        Ok((start, end))
    };
    let mut changes = Vec::new();
    for edit in edits {
        let lines = match edit.lines {
            Lines::Text(text) if text.is_empty() => Vec::new(),
            Lines::Text(text) => {
                let text = text.replace("\r\n", "\n");
                text.strip_suffix('\n')
                    .unwrap_or(&text)
                    .split('\n')
                    .map(str::to_owned)
                    .collect()
            }
            Lines::List(lines) => lines,
            Lines::Empty(()) => Vec::new(),
        };
        if lines.iter().any(|line| line.contains(['\n', '\r'])) {
            return Err(invalid("each array item must contain one line"));
        }
        if edit.end.is_some() && edit.op != "replace" {
            return Err(invalid("end is only valid for replace"));
        }
        let (start, end) = match edit.op.as_str() {
            "replace" => {
                let reference = edit
                    .pos
                    .as_deref()
                    .or(edit.end.as_deref())
                    .ok_or_else(|| invalid("replace requires an anchor"))?;
                let start = anchor(reference)?.0;
                let (last, end) = anchor(edit.end.as_deref().unwrap_or(reference))?;
                if last < start {
                    return Err(invalid("end anchor precedes start anchor"));
                }
                (start, end)
            }
            "append" | "prepend" => {
                let offset = match edit.pos.as_deref() {
                    Some("bof") => 0,
                    Some("eof") => source.len(),
                    None if edit.op == "append" => source.len(),
                    None => 0,
                    Some(reference) => {
                        let (start, end) = anchor(reference)?;
                        if edit.op == "append" {
                            end
                        } else {
                            start
                        }
                    }
                };
                (offset, offset)
            }
            _ => return Err(invalid("op must be replace, append, or prepend")),
        };
        let mut replacement = lines.join("\n");
        if !lines.is_empty() {
            if start == end && start == source.len() && start > 0 && !source.ends_with('\n') {
                replacement.insert(0, '\n');
            }
            if end < source.len() || source[..end].ends_with('\n') || source.is_empty() {
                replacement.push('\n');
            }
        }
        changes.push((start, end, replacement));
    }
    changes.sort_by_key(|change| (change.0, change.1));
    if changes
        .windows(2)
        .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
    {
        return Err(invalid(
            "line edits overlap; combine them into one replacement",
        ));
    }
    let mut output = String::with_capacity(source.len());
    let mut offset = 0;
    for (start, end, replacement) in changes {
        output.push_str(&source[offset..start]);
        output.push_str(&replacement);
        offset = end;
    }
    output.push_str(&source[offset..]);
    Ok(output)
}
