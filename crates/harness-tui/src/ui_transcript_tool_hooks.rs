//! Hook metadata is presentation only; viewing it never executes a hook.
use harness_core::event::{HookExecutionMetadata, HookExecutionStatus};

use super::*;

pub(super) fn summary_spans<'a>(
    hooks: impl Iterator<Item = &'a HookExecutionMetadata>,
    grouped: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut counts = [0usize; 4];
    for hook in hooks {
        match hook.status {
            HookExecutionStatus::Succeeded => counts[0] += 1,
            HookExecutionStatus::Failed => counts[1] += 1,
            HookExecutionStatus::Unknown => counts[2] += 1,
            HookExecutionStatus::Blocked => counts[3] += 1,
            HookExecutionStatus::Skipped => {}
        }
    }
    if counts.iter().all(|count| *count == 0) {
        return Vec::new();
    }
    let muted = Style::default().fg(theme.text.secondary);
    let mut spans = vec![Span::styled("  [hooks: ", muted)];
    let labeled = (grouped && counts[1] + counts[2] + counts[3] > 0) || counts[2] > 0;
    if !labeled {
        counts[0] += counts[3];
        counts[3] = 0;
    }
    let mut first = true;
    for (count, label, color) in [
        (counts[0], "ok", theme.status.success),
        (counts[3], "blocked", theme.text.accent),
        (counts[1], "failed", theme.terminal_colors.error),
        (counts[2], "unknown", theme.status.warning),
    ] {
        if count == 0 {
            continue;
        }
        if !first {
            spans.push(Span::styled(if labeled { ", " } else { "/" }, muted));
        }
        first = false;
        spans.push(Span::styled(
            if labeled {
                format!("{count} {label}")
            } else {
                count.to_string()
            },
            Style::default().fg(color).add_modifier(Modifier::DIM),
        ));
    }
    spans.push(Span::styled("]", muted));
    spans
}

pub(super) fn append(
    render: &mut ToolSectionRender,
    tool: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    surface: Color,
) {
    let hooks = &tool.hook_executions;
    if hooks
        .iter()
        .all(|hook| hook.status == HookExecutionStatus::Skipped)
    {
        return;
    }
    if !tool.details_visible() {
        if let Some(header) = render.lines.first_mut() {
            header
                .spans
                .extend(summary_spans(hooks.iter(), false, theme));
            super::ui_transcript_render::truncate_line_to_width(
                header,
                usize::from(transcript_surface_content_width(width, false)),
            );
        }
        return;
    }
    let muted = Style::default().fg(theme.text.secondary);
    let mut lines = vec![Line::from(Span::styled("───", muted))];
    let mut phases = Vec::new();
    for hook in hooks {
        let phase = hook.hook_event.as_deref().unwrap_or("hooks");
        if !phases.contains(&phase) {
            phases.push(phase);
        }
    }
    for phase in phases {
        let runs = hooks
            .iter()
            .filter(|hook| hook.hook_event.as_deref().unwrap_or("hooks") == phase);
        if runs
            .clone()
            .all(|hook| hook.status == HookExecutionStatus::Skipped)
        {
            continue;
        }
        lines.push(Line::from(Span::styled(
            super::super::ui_tool_output::safe_tool_text(phase),
            muted.add_modifier(Modifier::BOLD),
        )));
        for hook in runs {
            append_run(&mut lines, hook, theme, muted);
        }
    }
    let start = render.lines.len();
    append_prebuilt_surface_lines(
        &mut render.lines,
        &format!("{TRANSCRIPT_TOOL_BODY_PREFIX}    "),
        surface,
        lines,
        transcript_surface_content_width(width, false),
    );
    append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
}

fn append_run(
    lines: &mut Vec<Line<'static>>,
    hook: &HookExecutionMetadata,
    theme: &Theme,
    muted: Style,
) {
    let ascii = theme.glyph_mode() == crate::theme::GlyphMode::Ascii;
    let (glyph, color) = match hook.status {
        HookExecutionStatus::Succeeded => (if ascii { "v" } else { "✓" }, theme.status.success),
        HookExecutionStatus::Blocked => (if ascii { "<" } else { "↩" }, theme.text.accent),
        HookExecutionStatus::Failed => (if ascii { "x" } else { "✗" }, theme.terminal_colors.error),
        HookExecutionStatus::Skipped => ("-", theme.text.secondary),
        HookExecutionStatus::Unknown => ("?", theme.status.warning),
    };
    let name = super::super::ui_tool_output::safe_tool_text(&hook.hook_name);
    let timing = if hook.status == HookExecutionStatus::Skipped {
        " skipped".to_string()
    } else {
        hook.duration_ms
            .map(|duration| format!(" ({duration}ms)"))
            .unwrap_or_default()
    };
    lines.push(Line::from(vec![
        Span::styled("  ", muted),
        Span::styled(format!("{glyph} "), Style::default().fg(color)),
        Span::styled(name, muted),
        Span::styled(timing, muted),
    ]));
    if let Some(output) = &hook.output_summary {
        let output = super::super::ui_tool_output::safe_tool_text(output);
        let text = truncate_plain_text(&output, 120);
        for line in text.lines().take(3) {
            lines.push(Line::from(vec![
                Span::styled("      ", muted),
                Span::styled(
                    line.to_string(),
                    if hook.status == HookExecutionStatus::Failed {
                        Style::default().fg(theme.terminal_colors.error)
                    } else if hook.status == HookExecutionStatus::Blocked {
                        Style::default().fg(theme.text.accent)
                    } else {
                        muted
                    },
                ),
            ]));
        }
    }
}
