// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use std::path::{Path, PathBuf};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::app::{ToolCallDisplayStatus, ToolCallEntry};
use crate::text::{
    collapse_inline_whitespace, has_trimmed_content, replace_control_chars_except_tabs,
    strip_ansi_escapes, trimmed_json_string_field,
};
use crate::theme::Theme;

use super::ui_chrome::{display_width, take_width_prefix};
use super::ui_transcript::TranscriptToolCallDetailTone;
use super::ui_transcript_surface::{
    append_prebuilt_surface_lines, surface_prefix_width, surface_span,
    transcript_surface_content_width,
};

pub(super) const TRANSCRIPT_COMMAND_TOOL_INDENT: &str = "";
pub(super) const HARNESS_BASH_OUTPUT_LINE_CLAMP: usize = 15;
const HARNESS_BLOCK_TOOL_MARGIN_TOP: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_TOP: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_BOTTOM: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_LEFT: usize = 2;
const HARNESS_BLOCK_TOOL_GAP: usize = 1;
pub(super) const HARNESS_SPLIT_RAIL_GLYPH: &str = "┃";
const HARNESS_SPLIT_RAIL_WIDTH: usize = 1;

pub(super) struct HarnessBashPanel<'a> {
    pub(super) command: &'a str,
    pub(super) output: &'a str,
    pub(super) description: Option<&'a str>,
    pub(super) expand_hint: Option<&'a str>,
    pub(super) tone: TranscriptToolCallDetailTone,
}

pub(super) fn append_harness_bash_panel(
    lines: &mut Vec<Line<'static>>,
    panel: HarnessBashPanel<'_>,
    theme: &Theme,
    width: u16,
) {
    let available_width = transcript_surface_content_width(width, false);
    let prefix_width = surface_prefix_width(TRANSCRIPT_COMMAND_TOOL_INDENT);
    let panel_width = usize::from(available_width)
        .saturating_sub(prefix_width)
        .max(HARNESS_SPLIT_RAIL_WIDTH + HARNESS_BLOCK_TOOL_PADDING_LEFT + 1);
    for _ in 0..HARNESS_BLOCK_TOOL_MARGIN_TOP {
        lines.push(Line::default());
    }
    let card_lines = harness_bash_card_lines(
        panel.command,
        panel.output,
        panel.description,
        panel.expand_hint,
        panel.tone,
        theme,
        panel_width,
    );
    append_prebuilt_surface_lines(
        lines,
        TRANSCRIPT_COMMAND_TOOL_INDENT,
        theme.surface.panel,
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
    session_path: Option<&Path>,
) -> Option<String> {
    shell_tool_workdir_display(tool_call, session_path)
        .map(|workdir| format!("# Running in {workdir}"))
}

fn shell_tool_workdir_display(
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

    Some(home_collapsed_path_display(&absolute))
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
    let structured = shell_tool_structured_output(tool_call.output_json.as_ref());
    if tool_call.status == ToolCallDisplayStatus::Failed {
        return structured;
    }
    structured.or_else(|| {
        tool_call
            .output_summary
            .as_deref()
            .map(strip_ansi_escapes)
            .map(|output| output.trim().to_string())
    })
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
    let stripped = strip_ansi_escapes(&output);
    Some(stripped.trim().to_string())
}

fn harness_bash_card_lines(
    command: &str,
    output: &str,
    description: Option<&str>,
    expand_hint: Option<&str>,
    tone: TranscriptToolCallDetailTone,
    theme: &Theme,
    panel_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for _ in 0..HARNESS_BLOCK_TOOL_PADDING_TOP {
        lines.push(harness_bash_padding_line(theme));
    }

    if let Some(title) = harness_bash_title(description) {
        append_harness_bash_rows(
            &mut lines,
            &title,
            Style::default().fg(theme.text.secondary),
            theme,
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
        );

        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(theme));
        }
    }

    let command_style = Style::default().fg(theme.text.primary);
    append_harness_bash_rows(
        &mut lines,
        &format!("$ {command}"),
        command_style,
        theme,
        panel_width,
        HARNESS_BLOCK_TOOL_PADDING_LEFT,
    );

    let output = output.trim();
    if !output.is_empty() {
        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(theme));
        }
        append_harness_bash_rows(
            &mut lines,
            output,
            harness_bash_output_style(tone, theme),
            theme,
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
        );
    }

    if let Some(expand_hint) = expand_hint.filter(|hint| has_trimmed_content(hint)) {
        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(theme));
        }
        append_harness_bash_rows(
            &mut lines,
            expand_hint.trim(),
            Style::default().fg(theme.text.secondary),
            theme,
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
        );
    }

    for _ in 0..HARNESS_BLOCK_TOOL_PADDING_BOTTOM {
        lines.push(harness_bash_padding_line(theme));
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

fn harness_bash_output_style(_tone: TranscriptToolCallDetailTone, theme: &Theme) -> Style {
    Style::default().fg(theme.text.primary)
}

fn append_harness_bash_rows(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    style: Style,
    theme: &Theme,
    panel_width: usize,
    padding_left: usize,
) {
    let content_width = panel_width
        .saturating_sub(HARNESS_SPLIT_RAIL_WIDTH)
        .saturating_sub(padding_left)
        .max(1);
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
            theme,
            padding_left,
            content_width,
        ));
    }
}

fn harness_bash_content_line(
    text: &str,
    style: Style,
    theme: &Theme,
    padding_left: usize,
    content_width: usize,
) -> Line<'static> {
    let content = sanitize_harness_bash_text(text);
    let remaining = content_width.saturating_sub(display_width(&content));
    harness_bash_line(
        vec![
            harness_split_rail_span(theme),
            Span::styled(" ".repeat(padding_left), Style::default()),
            Span::styled(content, style),
            Span::styled(" ".repeat(remaining), Style::default()),
        ],
        theme.surface.panel,
    )
}

fn harness_bash_padding_line(theme: &Theme) -> Line<'static> {
    harness_bash_line(vec![harness_split_rail_span(theme)], theme.surface.panel)
}

fn harness_split_rail_span(theme: &Theme) -> Span<'static> {
    Span::styled(
        HARNESS_SPLIT_RAIL_GLYPH.to_string(),
        Style::default().fg(theme.surface.shell),
    )
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
