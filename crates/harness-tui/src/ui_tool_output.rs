use super::ui_secondary::format_detail_payload;

pub(super) fn safe_tool_text(text: &str) -> String {
    use crate::transcript_blocks::{RawDisclosure, RawPayload};
    let text = crate::text::strip_ansi_escapes(text);
    let text = text
        .split('\n')
        .map(crate::text::replace_control_chars_except_tabs)
        .collect::<Vec<_>>()
        .join("\n");
    match RawDisclosure::from_text(&text).payload {
        RawPayload::Text(text) => text,
        RawPayload::Json(value) => value.to_string(),
    }
}

pub(super) struct CollapsibleOutputPreview {
    pub(super) output: String,
    pub(super) expand_hint: Option<&'static str>,
}

pub(super) fn collapsible_output_preview(
    output: &str,
    max_lines: usize,
    expanded: bool,
) -> CollapsibleOutputPreview {
    let formatted = safe_tool_text(&format_detail_payload(output));
    collapsible_preview_for_text(&formatted, max_lines, expanded)
}

pub(super) fn measured_output_preview(
    mut rows: Vec<ratatui::text::Line<'static>>,
    (first, last): (usize, usize),
    expanded: bool,
) -> (Vec<ratatui::text::Line<'static>>, Option<&'static str>) {
    let overflow = rows.len() > first.saturating_add(last);
    if overflow && !expanded {
        let end = rows.len().saturating_sub(last);
        rows.splice(first..end, [ratatui::text::Line::from("…")]);
    }
    (
        rows,
        overflow.then_some(if expanded {
            "Click to collapse"
        } else {
            "Click to expand"
        }),
    )
}

fn collapsible_preview_for_text(
    output: &str,
    max_lines: usize,
    expanded: bool,
) -> CollapsibleOutputPreview {
    let overflow = line_count_exceeds(output, max_lines);
    let output = if overflow && !expanded {
        first_lines_with_ellipsis(output, max_lines)
    } else {
        output.to_string()
    };

    CollapsibleOutputPreview {
        output,
        expand_hint: overflow.then_some(if expanded {
            "Click to collapse"
        } else {
            "Click to expand"
        }),
    }
}

pub(super) fn line_count_exceeds(text: &str, max_lines: usize) -> bool {
    text.lines().nth(max_lines).is_some()
}

pub(super) fn first_lines_with_ellipsis(text: &str, max_lines: usize) -> String {
    let mut preview = String::new();
    for (index, line) in text.lines().take(max_lines).enumerate() {
        if index > 0 {
            preview.push('\n');
        }
        preview.push_str(line);
    }
    preview.push_str("\n…");
    preview
}
