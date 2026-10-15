//! File tracking and compact, deterministic summary trees.
use crate::Item;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOps {
    pub read: BTreeSet<String>,
    pub written: BTreeSet<String>,
    pub edited: BTreeSet<String>,
}
fn scheme_path(path: &str) -> bool {
    path.match_indices("://").any(|(end, _)| {
        let prefix = &path[..end];
        let segment = prefix
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
            .next()
            .unwrap_or_default();
        segment.bytes().any(|byte| byte.is_ascii_alphabetic())
    })
}
/// The tool path argument: a nonempty string `path` wins, then `filePath`, then `file_path`.
fn argument_path(arguments: &Value) -> Option<&str> {
    ["path", "filePath", "file_path"].iter().find_map(|key| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
    })
}
/// Selector reads have their own supersession identity; whole-file reads use the path.
pub fn read_supersede_key(arguments_json: &str) -> Option<String> {
    let arguments: Value = serde_json::from_str(arguments_json).ok()?;
    let path = argument_path(&arguments).filter(|path| !path.contains("://"))?;
    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(1);
    let limit = arguments.get("limit");
    if offset > 1 || limit.is_some() {
        Some(format!(
            "{path}\0{offset}-{}",
            limit.map_or_else(String::new, Value::to_string)
        ))
    } else {
        Some(path.to_owned())
    }
}
pub fn collect_file_ops(items: &[Item<'_>], ops: &mut FileOps) {
    for item in items.iter().filter(|item| item.is_assistant()) {
        for call in item.message.assistant_tool_calls.iter().flatten() {
            let set = match call.function_name.as_str() {
                "read" => &mut ops.read,
                "write" => &mut ops.written,
                "edit" => &mut ops.edited,
                _ => continue,
            };
            let Ok(arguments) = serde_json::from_str::<Value>(&call.arguments_json) else {
                continue;
            };
            if let Some(path) = argument_path(&arguments).filter(|path| !scheme_path(path)) {
                set.insert(path.to_owned());
            }
        }
    }
}
/// Whether one comma-separated chunk is a line range: `L?digits`, optionally followed by
/// `-`, `+` or `..` and then either `L?digits` or, for `-` and `..`, nothing.
fn range_chunk(chunk: &str) -> bool {
    let chunk = chunk.strip_prefix(['L', 'l']).unwrap_or(chunk);
    let digits = chunk.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let rest = &chunk[digits..];
    if rest.is_empty() {
        return true;
    }
    let (rest, open_ended) = if let Some(rest) = rest.strip_prefix("..") {
        (rest, true)
    } else if let Some(rest) = rest.strip_prefix('-') {
        (rest, true)
    } else if let Some(rest) = rest.strip_prefix('+') {
        (rest, false)
    } else {
        return false;
    };
    if rest.is_empty() {
        return open_ended;
    }
    let rest = rest.strip_prefix(['L', 'l']).unwrap_or(rest);
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}
fn range(value: &str) -> bool {
    value.split(',').all(range_chunk)
}
/// Split a trailing read selector off `path`.
///
/// Returns the base path and the removed selector text with its original spelling, for
/// example `src/a:100-170:raw` gives `("src/a", Some("100-170:raw"))`. The last colon counts
/// only when it is not the first character. An archive member or table name stays part of
/// the path unless the text after the colon is `raw`, `conflicts` or a line range. A second
/// suffix is removed as well only for a `raw` and range pair in either order.
pub fn strip_selector(path: &str) -> (&str, Option<&str>) {
    let Some((base, suffix)) = path.rsplit_once(':').filter(|(base, _)| !base.is_empty()) else {
        return (path, None);
    };
    let raw = suffix.eq_ignore_ascii_case("raw");
    let is_range = range(suffix);
    if !(raw || is_range || suffix.eq_ignore_ascii_case("conflicts")) {
        return (path, None);
    }
    let base = match base.rsplit_once(':').filter(|(inner, _)| !inner.is_empty()) {
        Some((inner, previous))
            if (raw && range(previous)) || (is_range && previous.eq_ignore_ascii_case("raw")) =>
        {
            inner
        }
        _ => base,
    };
    (base, Some(&path[base.len() + 1..]))
}
/// Whether any colon-delimited part of a selector is `raw` or `conflicts`.
pub fn is_alternate_selector(selector: &str) -> bool {
    selector
        .split(':')
        .any(|part| part.eq_ignore_ascii_case("raw") || part.eq_ignore_ascii_case("conflicts"))
}
// Prior detail selectors retain the legacy range/raw spelling; live reads use offset/limit.
pub fn merge_previous(ops: &mut FileOps, read: &[String], modified: &[String]) {
    ops.read
        .extend(read.iter().map(|path| strip_selector(path).0.to_owned()));
    ops.edited.extend(modified.iter().cloned());
}
/// Order by UTF-16 code units, matching how lists sort in the reference format.
fn utf16_order(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}
pub fn file_lists(ops: &FileOps) -> (Vec<String>, Vec<String>) {
    let mut modified: Vec<String> = ops
        .edited
        .union(&ops.written)
        .filter(|path| !scheme_path(path))
        .cloned()
        .collect();
    let mut read: Vec<String> = ops
        .read
        .iter()
        .filter(|path| !ops.edited.contains(*path) && !ops.written.contains(*path))
        .filter(|path| !scheme_path(path))
        .cloned()
        .collect();
    read.sort_by(|left, right| utf16_order(left, right));
    modified.sort_by(|left, right| utf16_order(left, right));
    (read, modified)
}
#[derive(Default)]
struct Tree {
    files: Vec<(String, &'static str)>,
    children: Vec<(String, Tree)>,
}
impl Tree {
    /// A path ending in `/` names a directory: its node exists but it gains no file entry.
    fn insert(&mut self, path: &str, mode: &'static str) {
        let Some((first, rest)) = path.split_once('/') else {
            if !self.files.iter().any(|(name, _)| name == path) {
                self.files.push((path.to_owned(), mode));
            }
            return;
        };
        let index = self
            .children
            .iter()
            .position(|(name, _)| name == first)
            .unwrap_or_else(|| {
                self.children.push((first.to_owned(), Self::default()));
                self.children.len() - 1
            });
        if !rest.is_empty() {
            self.children[index].1.insert(rest, mode);
        }
    }
    fn render(&self, lines: &mut Vec<String>, depth: usize) {
        lines.extend(
            self.files
                .iter()
                .map(|(name, mode)| format!("{name} ({mode})")),
        );
        for (name, child) in &self.children {
            let mut name = name.clone();
            let mut child = child;
            while child.files.is_empty() && child.children.len() == 1 {
                name.push('/');
                name.push_str(&child.children[0].0);
                child = &child.children[0].1;
            }
            lines.push(format!("{} {name}/", "#".repeat(depth + 1)));
            child.render(lines, depth + 1);
        }
    }
}
pub fn render_files_block(
    read: &[String],
    modified: &[String],
    read_set: &BTreeSet<String>,
) -> String {
    let mut modes = BTreeMap::new();
    for path in read {
        modes.insert(path, "Read");
    }
    for path in modified {
        modes.insert(
            path,
            if read_set.contains(path) {
                "RW"
            } else {
                "Write"
            },
        );
    }
    if modes.is_empty() {
        return String::new();
    }
    let mut paths: Vec<&String> = modes.keys().copied().collect();
    paths.sort_by(|left, right| utf16_order(left, right));
    let mut tree = Tree::default();
    for path in paths.iter().take(20) {
        let mode = modes[*path];
        if path.contains('\\') {
            tree.insert(&path.replace('\\', "/"), mode);
        } else {
            tree.insert(path, mode);
        }
    }
    let mut lines = vec!["<files>".to_owned()];
    tree.render(&mut lines, 0);
    if paths.len() > 20 {
        lines.push(format!("[…{} files elided…]", paths.len() - 20));
    }
    lines.push("</files>".to_owned());
    lines.join("\n")
}
/// ECMAScript whitespace: unlike Rust's `char::is_whitespace`, it excludes U+0085 and
/// includes U+FEFF.
fn ecma_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}
pub fn upsert_files_block(summary: &str, block: &str) -> String {
    let mut cleaned = summary.to_owned();
    for tag in ["files", "read-files", "modified-files"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut from = 0;
        while let Some(start) = cleaned[from..].find(&open).map(|index| index + from) {
            let Some(end) = cleaned[start + open.len()..]
                .find(&close)
                .map(|index| start + open.len() + index + close.len())
            else {
                break;
            };
            let whitespace = cleaned[end..]
                .chars()
                .take_while(|c| ecma_space(*c))
                .map(char::len_utf8)
                .sum::<usize>();
            cleaned.replace_range(start..end + whitespace, "");
            from = start;
        }
    }
    let cleaned = cleaned.trim_end_matches(ecma_space);
    if block.is_empty() {
        cleaned.to_owned()
    } else if cleaned.is_empty() {
        block.to_owned()
    } else {
        format!("{cleaned}\n\n{block}")
    }
}
#[cfg(test)]
mod tests;
