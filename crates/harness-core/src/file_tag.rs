//! Prompt selections carry character offsets used by the existing composer.
use serde::{Deserialize, Serialize};

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
#[serde(default)]
pub struct SelectedPromptTags {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SelectedFileTag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<SelectedAgentTag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<SelectedResourceTag>,
}
pub fn split_line_range(value: &str) -> (&str, Option<FileTagLineRange>) {
    let parsed = (|| {
        let (path, suffix) = value.rsplit_once('#')?;
        if path.is_empty() {
            return None;
        }
        let (start, end) = suffix
            .split_once('-')
            .map_or((suffix, None), |(a, b)| (a, Some(b)));
        if !start.bytes().all(|b| b.is_ascii_digit())
            || end.is_some_and(|end| !end.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        let start = start.parse::<usize>().ok().filter(|n| *n > 0)?;
        let end = end.map(str::parse::<usize>).transpose().ok()?;
        if end.is_some_and(|end| end < start) {
            return None;
        }
        Some((path, FileTagLineRange { start, end }))
    })();
    parsed.map_or((value, None), |(path, range)| (path, Some(range)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_ranges_do_not_consume_literal_hashes_or_reversed_ranges() {
        for (input, expected) in [
            ("src/main.rs#7-12", Some(("src/main.rs", 7, Some(12)))),
            ("notes/文.md#3", Some(("notes/文.md", 3, None))),
            ("name#fragment", None),
            ("name#0", None),
            ("name#12-7", None),
            ("name#99999999999999999999999", None),
            ("name#1-2-3", None),
            ("name#+7", None),
            ("name#7-+9", None),
        ] {
            let (path, range) = split_line_range(input);
            assert_eq!(range.map(|r| (path, r.start, r.end)), expected, "{input}");
            if expected.is_none() {
                assert_eq!(path, input);
            }
        }
    }
}
