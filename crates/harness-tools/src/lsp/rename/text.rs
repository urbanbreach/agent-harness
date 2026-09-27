use super::*;
#[derive(Deserialize)]
pub(super) struct Edit {
    range: Range,
    #[serde(rename = "newText")]
    text: String,
}
#[derive(Deserialize)]
struct Range {
    start: Position,
    end: Position,
}
#[derive(Deserialize, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: usize,
    character: usize,
}

pub(super) fn apply(source: &str, mut edits: Vec<Edit>) -> Result<String, ToolError> {
    if edits.len() > 10_000 {
        return Err(failure("rename exceeds 10000 text edits"));
    }
    // One forward scan translates all UTF-16 positions without a per-line index or rescans.
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut characters = source.char_indices().peekable();
    let mut position = Position::default();
    let mut byte = 0;
    let mut offset = |target| {
        while position < target {
            let (index, ch) = characters
                .next()
                .ok_or_else(|| failure("rename position is outside the file"))?;
            match ch {
                '\n' => {
                    position.line += 1;
                    position.character = 0;
                }
                '\r' if characters.peek().is_none_or(|(_, next)| *next == '\n') => {}
                _ => position.character += ch.len_utf16(),
            }
            byte = index + ch.len_utf8();
        }
        if position == target {
            Ok(byte)
        } else {
            Err(failure(
                "rename position is outside the line or splits a UTF-16 surrogate pair",
            ))
        }
    };
    let mut result = String::new();
    let mut cursor = 0;
    let mut previous = Position::default();
    for edit in edits {
        if edit.range.start < previous || edit.range.end < edit.range.start {
            return Err(failure("overlapping or reversed rename ranges"));
        }
        let start = offset(edit.range.start)?;
        let end = offset(edit.range.end)?;
        if result.len() + (start - cursor) + edit.text.len() > 8 * 1024 * 1024 {
            return Err(failure("renamed file exceeds 8 MiB"));
        }
        result.push_str(&source[cursor..start]);
        result.push_str(&edit.text);
        cursor = end;
        previous = edit.range.end;
    }
    if result.len() + source.len() - cursor > 8 * 1024 * 1024 {
        return Err(failure("renamed file exceeds 8 MiB"));
    }
    result.push_str(&source[cursor..]);
    Ok(result)
}
