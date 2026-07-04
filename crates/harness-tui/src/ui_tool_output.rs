use super::ui_secondary::format_detail_payload;

pub(super) struct CollapsibleOutputPreview {
    pub(super) output: String,
    pub(super) expand_hint: Option<&'static str>,
}

pub(super) fn collapsible_output_preview(
    output: &str,
    max_lines: usize,
    expanded: bool,
) -> CollapsibleOutputPreview {
    let formatted = format_detail_payload(output);
    collapsible_preview_for_text(&formatted, max_lines, expanded)
}

pub(super) fn collapsible_bash_panel_preview(
    output: &str,
    max_lines: usize,
    expanded: bool,
) -> CollapsibleOutputPreview {
    collapsible_preview_for_text(output, max_lines, expanded)
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
