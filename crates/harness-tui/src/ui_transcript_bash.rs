// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use std::path::{Path, PathBuf};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::app::{ToolCallDisplayStatus, ToolCallEntry};
use crate::text::{
    collapse_inline_whitespace, replace_control_chars_except_tabs, trimmed_json_string_field,
};
use crate::theme::Theme;

use super::ui_chrome::{display_width, take_width_prefix};
use super::ui_transcript_surface::{
    append_prebuilt_surface_lines, surface_prefix_width, surface_span,
    transcript_surface_content_width,
};

pub(super) const TRANSCRIPT_COMMAND_TOOL_INDENT: &str =
    super::ui_transcript_surface::TRANSCRIPT_ENTRY_CONTENT_PREFIX;
const HARNESS_BLOCK_TOOL_PADDING_LEFT: usize = 0;
const HARNESS_BLOCK_TOOL_GAP: usize = 1;

pub(super) struct HarnessBashPanel<'a> {
    pub(super) command: &'a str,
    pub(super) output: &'a str,
    pub(super) description: Option<&'a str>,
    pub(super) expanded: bool,
}

pub(super) fn append_harness_bash_panel(
    lines: &mut Vec<Line<'static>>,
    panel: HarnessBashPanel<'_>,
    theme: &Theme,
    width: u16,
    surface: Color,
) {
    let available_width = transcript_surface_content_width(width, false);
    let prefix_width = surface_prefix_width(TRANSCRIPT_COMMAND_TOOL_INDENT);
    let panel_width = usize::from(available_width)
        .saturating_sub(prefix_width)
        .max(HARNESS_BLOCK_TOOL_PADDING_LEFT + 1);
    let card_lines = harness_bash_card_lines(panel, theme, panel_width, surface);
    append_prebuilt_surface_lines(
        lines,
        TRANSCRIPT_COMMAND_TOOL_INDENT,
        surface,
        card_lines,
        available_width,
    );
}

pub(super) fn shell_tool_command(tool_call: &ToolCallEntry) -> Option<String> {
    trimmed_json_string_field(tool_call.output_json.as_ref(), &["command"])
        .or_else(|| shell_tool_command_from_value(tool_call.output_json.as_ref()))
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| {
                    trimmed_json_string_field(Some(&value), &["command"])
                        .or_else(|| shell_tool_command_from_value(Some(&value)))
                })
        })
}

fn shell_tool_command_from_value(value: Option<&serde_json::Value>) -> Option<String> {
    let cmd = trimmed_json_string_field(value, &["cmd"])?;
    let args = value
        .and_then(|value| value.get("args"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(cmd)
    } else {
        Some(format!("{cmd} {}", args.join(" ")))
    }
}

pub(super) fn shell_tool_title_description(
    tool_call: &ToolCallEntry,
    _session_path: Option<&Path>,
) -> Option<String> {
    trimmed_json_string_field(tool_call.output_json.as_ref(), &["description"])
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| trimmed_json_string_field(Some(&value), &["description"]))
        })
        .map(|description| {
            let description =
                collapse_inline_whitespace(&super::ui_tool_output::safe_tool_text(&description));
            description
                .strip_prefix("Running ")
                .or_else(|| description.strip_prefix("Run "))
                .unwrap_or(&description)
                .to_string()
        })
        .filter(|description| !description.is_empty())
}

pub(super) fn shell_tool_workdir_display(
    tool_call: &ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<String> {
    let workdir = trimmed_json_string_field(tool_call.output_json.as_ref(), &["workdir", "cwd"])
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| trimmed_json_string_field(Some(&value), &["workdir", "cwd"]))
        })?;
    if workdir == "." {
        return None;
    }

    let base = session_path?;
    let absolute = if Path::new(&workdir).is_absolute() {
        PathBuf::from(&workdir)
    } else {
        base.join(&workdir)
    };
    if absolute == base {
        return None;
    }

    Some(super::ui_tool_paths::tool_header_path(
        &home_collapsed_path_display(&absolute),
        true,
    ))
}

fn home_collapsed_path_display(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return path.display().to_string();
    };
    let home = PathBuf::from(home);
    if path == home {
        return "~".to_string();
    }
    path.strip_prefix(&home)
        .ok()
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|| path.display().to_string())
}

pub(super) fn shell_tool_output(tool_call: &ToolCallEntry) -> Option<String> {
    shell_tool_structured_output(tool_call.output_json.as_ref())
        .or_else(|| tool_call.output_summary.clone())
}

fn shell_tool_structured_output(output_json: Option<&serde_json::Value>) -> Option<String> {
    let value = output_json?;
    let stdout = value.get("stdout").and_then(serde_json::Value::as_str);
    let stderr = value.get("stderr").and_then(serde_json::Value::as_str);
    let output = match (stdout, stderr) {
        (Some(stdout), Some(stderr)) if !stdout.is_empty() && !stderr.is_empty() => {
            format!("{stdout}\n{stderr}")
        }
        (Some(stdout), _) if !stdout.is_empty() => stdout.to_string(),
        (_, Some(stderr)) if !stderr.is_empty() => stderr.to_string(),
        (Some(stdout), _) => stdout.to_string(),
        (_, Some(stderr)) => stderr.to_string(),
        _ => return None,
    };
    Some(output)
}

fn harness_bash_card_lines(
    panel: HarnessBashPanel<'_>,
    theme: &Theme,
    panel_width: usize,
    surface: Color,
) -> Vec<Line<'static>> {
    let HarnessBashPanel {
        command,
        output,
        description,
        expanded,
    } = panel;
    let mut lines = Vec::new();
    let body_padding_left = HARNESS_BLOCK_TOOL_PADDING_LEFT;

    if let Some(title) = harness_bash_title(description) {
        append_harness_bash_rows(
            &mut lines,
            &title,
            Style::default().fg(theme.text.secondary),
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
            surface,
        );

        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(surface));
        }
    }

    if !command.trim().is_empty() {
        let command = super::ui_tool_output::safe_tool_text(command);
        let highlighted = super::ui_syntax_highlight::render_highlighted_code_block(
            Some("bash"),
            &command,
            &command,
            "",
            theme.text.primary,
            theme,
        );
        // The reference's content width already excludes the two-cell bullet.
        let content_width = panel_width.saturating_sub(body_padding_left + 4).max(1);
        let mut first = true;
        for spans in super::ui_tool_wrapping::shell(highlighted, content_width) {
            let mut row = vec![
                Span::raw(" ".repeat(body_padding_left)),
                Span::styled(
                    if first { "$ " } else { "  " },
                    Style::default().fg(theme.terminal_colors.muted),
                ),
            ];
            row.extend(spans.into_iter().map(|mut span| {
                span.style = span.style.bg(surface);
                span
            }));
            lines.push(harness_bash_line(row, surface));
            first = false;
        }
    }

    let output = output.trim_end_matches('\n');
    let content_width = panel_width.saturating_sub(body_padding_left + 4).max(20);
    let output_background = theme.markdown.code_background;
    let mut output_rows = Vec::new();
    for line in
        super::ui_terminal_output::render(output, Style::default().fg(theme.text.primary), theme)
    {
        for spans in super::ui_tool_wrapping::words(
            super::ui_transcript_surface::expand_preformatted_tabs(line.spans),
            content_width,
        ) {
            let mut row = vec![Span::raw(" ".repeat(body_padding_left))];
            let used = spans
                .iter()
                .map(|span| display_width(&span.content))
                .sum::<usize>();
            row.extend(spans.into_iter().map(|mut span| {
                if span.style.bg.is_none() || span.style.bg == Some(surface) {
                    span.style.bg = Some(output_background);
                }
                span
            }));
            row.push(Span::styled(
                " ".repeat(panel_width.saturating_sub(body_padding_left + used)),
                Style::default().bg(output_background),
            ));
            output_rows.push(Line::from(row));
        }
    }
    let output_rows = super::ui_tool_output::measured_output_preview(
        output_rows,
        (2, 3),
        expanded,
        Style::default(),
    );
    if !output.is_empty() {
        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(surface));
        }
        for row in output_rows {
            if row.spans.len() == 1 && row.spans[0].content == "…" {
                append_harness_bash_rows(
                    &mut lines,
                    "…",
                    theme_muted_style(theme),
                    panel_width,
                    body_padding_left,
                    surface,
                );
            } else {
                lines.push(row);
            }
        }
    }

    lines
}

fn harness_bash_title(description: Option<&str>) -> Option<String> {
    let description = description
        .map(collapse_inline_whitespace)
        .filter(|value| !value.is_empty())?;
    Some(if description.starts_with("# ") {
        description
    } else {
        format!("# {description}")
    })
}

fn theme_muted_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

fn append_harness_bash_rows(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    style: Style,
    panel_width: usize,
    padding_left: usize,
    surface: Color,
) {
    let content_width = panel_width.saturating_sub(padding_left).max(1);
    let text = super::ui_tool_output::safe_tool_text(text);
    let rows = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n')
            .flat_map(|row| wrap_plain_terminal_row(row, content_width))
            .collect::<Vec<_>>()
    };

    for row in rows {
        lines.push(harness_bash_content_line(
            &row,
            style,
            padding_left,
            content_width,
            surface,
        ));
    }
}

fn harness_bash_content_line(
    text: &str,
    style: Style,
    padding_left: usize,
    content_width: usize,
    surface: Color,
) -> Line<'static> {
    let content = sanitize_harness_bash_text(text);
    let remaining = content_width.saturating_sub(display_width(&content));
    harness_bash_line(
        vec![
            Span::styled(" ".repeat(padding_left), Style::default()),
            Span::styled(content, style),
            Span::styled(" ".repeat(remaining), Style::default()),
        ],
        surface,
    )
}

fn harness_bash_padding_line(surface: Color) -> Line<'static> {
    harness_bash_line(Vec::new(), surface)
}

fn harness_bash_line(spans: Vec<Span<'static>>, surface: Color) -> Line<'static> {
    Line::from(
        spans
            .into_iter()
            .map(|span| surface_span(span.content.into_owned(), span.style, surface))
            .collect::<Vec<_>>(),
    )
}

fn wrap_plain_terminal_row(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let mut chunk = take_width_prefix(remaining, width);
        if chunk.is_empty() {
            chunk = remaining
                .char_indices()
                .nth(1)
                .map(|(index, _)| &remaining[..index])
                .unwrap_or(remaining);
        }
        rows.push(chunk.to_string());
        remaining = &remaining[chunk.len()..];
    }
    rows
}

fn sanitize_harness_bash_text(text: &str) -> String {
    replace_control_chars_except_tabs(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn shell_panel_wraps_operators_without_splitting_quoted_or_heredoc_payload() {
        // These are rendered rows: a wrapping regression changes every tool below them.
        for (command, width, expected) in [
            (
                "printf 'a deliberately long command argument for terminal reflow'",
                30,
                vec![
                    "$ printf",
                    "  'a deliberately long command argument for terminal reflow'",
                ],
            ),
            (
                "git status --short --branch && cargo check --workspace",
                42,
                vec![
                    "$ git status --short --branch &&",
                    "  cargo check --workspace",
                ],
            ),
            (
                "echo \"keep && together\" && echo next",
                30,
                vec!["$ echo \"keep && together\" &&", "  echo next"],
            ),
            (
                "cat <<EOF\nthis long heredoc body keeps its spaces and && payload together\nEOF",
                30,
                vec![
                    "$ cat <<EOF",
                    "  this long heredoc body keeps its spaces and && payload together",
                    "  EOF",
                ],
            ),
        ] {
            let rows = harness_bash_card_lines(
                HarnessBashPanel {
                    command,
                    output: "",
                    description: None,
                    expanded: true,
                },
                &Theme::default(),
                width,
                Color::Reset,
            );
            assert_eq!(
                rows.iter().map(Line::to_string).collect::<Vec<_>>(),
                expected,
                "{command}"
            );
        }
    }

    #[test]
    fn bash_body_can_omit_command_owned_by_header() {
        // arrange
        // act
        let lines = harness_bash_card_lines(
            HarnessBashPanel {
                command: "",
                output: "stdout",
                description: None,
                expanded: false,
            },
            &Theme::default(),
            80,
            Color::Reset,
        );
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        // assert
        assert!(!rendered.contains("$ "));
        assert!(rendered.contains("stdout"));

        let output = (1..=12)
            .map(|line| format!("row-{line:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        for expanded in [false, true] {
            let rows = harness_bash_card_lines(
                HarnessBashPanel {
                    command: "printf 'ready\\n'",
                    output: &output,
                    description: None,
                    expanded,
                },
                &Theme::default(),
                80,
                Color::Reset,
            );
            let text = rows
                .iter()
                .flat_map(|line| &line.spans)
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert!(text.contains("row-02") && text.contains("row-10") && text.contains("row-12"));
            assert_eq!(text.contains("row-03"), expanded, "{text}");
            assert_eq!(text.matches("$ printf 'ready\\n'").count(), 1);
            let string_span = rows
                .iter()
                .flat_map(|line| &line.spans)
                .find(|span| span.content.contains("ready"))
                .unwrap_or_abort();
            assert_ne!(string_span.style.fg, Some(Theme::default().text.primary));
            assert_eq!(string_span.style.bg, Some(Color::Reset));
        }
    }
}
