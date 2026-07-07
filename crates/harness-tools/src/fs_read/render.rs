use std::borrow::Cow;

use harness_core::edit::hashline::{compute_line_hash, LineAnchor};
use serde_json::json;

const MAX_LINE_LENGTH: usize = 2000;
const MAX_LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";

#[derive(Debug, Clone, Copy)]
pub(super) struct FsReadRenderOptions {
    pub(super) line_numbers: bool,
    pub(super) hashline_anchors: bool,
}

pub(super) fn format_fs_read_output_line(
    line: &str,
    line_number: usize,
    render: FsReadRenderOptions,
) -> String {
    if render.hashline_anchors {
        let anchor = build_fs_read_line_anchor(line_number, line);
        format_fs_read_hashline_line(&anchor, line)
    } else {
        format_fs_read_line(line, line_number, render.line_numbers)
    }
}

pub(super) fn build_fs_read_anchor_payload(
    anchors: &[LineAnchor],
    lines: &[String],
) -> serde_json::Value {
    debug_assert_eq!(anchors.len(), lines.len());

    json!(anchors
        .iter()
        .zip(lines)
        .map(|(anchor, line)| {
            json!({
                "line": anchor.line,
                "hash": anchor.hash,
                "text": fs_read_hashline_text(line),
            })
        })
        .collect::<Vec<_>>())
}

pub(super) fn append_truncation_marker(display_text: &mut String, marker: &str) {
    if display_text.is_empty() {
        display_text.push_str(marker);
        return;
    }

    if !display_text.ends_with('\n') {
        display_text.push('\n');
    }
    display_text.push_str(marker);
}

pub(super) fn format_fs_read_hashline_line(anchor: &LineAnchor, line: &str) -> String {
    format!(
        "{}#{}|{}",
        anchor.line,
        anchor.hash,
        fs_read_hashline_text(line)
    )
}

pub(super) fn truncate_fs_read_line(line: &str) -> Cow<'_, str> {
    if line.len() <= MAX_LINE_LENGTH || line.chars().count() <= MAX_LINE_LENGTH {
        return Cow::Borrowed(line);
    }

    Cow::Owned(
        line.chars()
            .take(MAX_LINE_LENGTH)
            .chain(MAX_LINE_SUFFIX.chars())
            .collect(),
    )
}

pub(super) fn build_fs_read_line_anchor(line_number: usize, line: &str) -> LineAnchor {
    LineAnchor {
        line: u32::try_from(line_number).unwrap_or(u32::MAX),
        hash: compute_line_hash(line),
    }
}

fn format_fs_read_line(line: &str, line_number: usize, line_numbers: bool) -> String {
    if line_numbers {
        format!("{line_number}: {line}")
    } else {
        line.to_string()
    }
}

fn fs_read_hashline_text(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}
