use super::*;

fn format_assistant_error_display(text: &str) -> String {
    let trimmed = text.trim_end();
    let body = trimmed.trim_start();
    if body.starts_with("Retry failed:") || is_cancel_error_message(body) {
        trimmed.to_string()
    } else {
        format!("Retry failed: {body}")
    }
}

fn is_cancel_error_message(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("interrupted")
        || lower.contains("cancelled")
        || lower.contains("canceled")
        || lower.contains("user cancel")
}

pub(in crate::ui::ui_transcript) fn append_assistant_error_box(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let surface = base_surface;
    let style = Style::default().fg(theme.status.error);
    let trimmed = text.trim_end();
    let display = format_assistant_error_display(trimmed);
    for row in display.lines() {
        let content = row.trim_start_matches(' ');
        let indent = format!(
            "{TRANSCRIPT_ASSISTANT_BODY_PREFIX}{}",
            " ".repeat(row.len().saturating_sub(content.len()))
        );
        if content.is_empty() {
            append_surface_row(lines, "", surface, Vec::new(), width);
            continue;
        }
        let first_w = usize::from(width)
            .saturating_sub(surface_prefix_width(&indent))
            .max(1);
        let wrapped = wrap_surface_spans(vec![Span::styled(content.to_string(), style)], first_w);
        match wrapped.as_slice() {
            [] => append_surface_row(lines, &indent, surface, Vec::new(), width),
            [first] => {
                let first_text: String = first.iter().map(|s| s.content.to_string()).collect();
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(first_text, style)],
                    width,
                );
            }
            [first, rest @ ..] => {
                let first_text: String = first.iter().map(|s| s.content.to_string()).collect();
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(first_text, style)],
                    width,
                );
                let rest_text = rest
                    .iter()
                    .map(|visual| {
                        visual
                            .iter()
                            .map(|s| s.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(rest_text, style)],
                    width,
                );
            }
        }
    }
}
