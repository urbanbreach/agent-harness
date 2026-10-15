//! The expanded body of a compaction card.
use super::super::ui_transcript_types::{TranscriptCompactionKind, TranscriptCompactionSection};
use crate::theme::{GlyphMode, Theme};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// Shown instead of a summary when a notes rollover replaced the context window.
pub(super) const ROLLOVER_NOTICE: &str =
    "The context window rolled over, and the notes carry the work forward into the new window.";

/// Cells of padding on each side of the box content.
const BOX_PADDING: u16 = 1;

pub(super) fn heading(before: Option<u32>, after: Option<u32>) -> String {
    match (before.filter(|before| *before > 0), after) {
        (Some(before), Some(after)) => format!(
            "Compacted from {} to {} tokens",
            thousands(before),
            thousands(after)
        ),
        (Some(before), None) => format!("Compacted from {} tokens", thousands(before)),
        (None, Some(after)) => format!("Compacted to {} tokens", thousands(after)),
        (None, None) => "Compacted context".to_string(),
    }
}

fn thousands(count: u32) -> String {
    let digits = count.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

/// The detail rows inside a background box with one cell of padding on every side.
pub(super) fn boxed_detail(
    compaction: &TranscriptCompactionSection,
    theme: &Theme,
    width: u16,
    surface: Color,
) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(BOX_PADDING * 2).max(1);
    let content = content_lines(compaction, theme, inner);
    let pad = " ".repeat(usize::from(BOX_PADDING));
    let blank = Line::from(Span::styled(
        " ".repeat(usize::from(width)),
        Style::default().bg(surface),
    ));
    let mut rows = Vec::with_capacity(content.len() + 2);
    rows.push(blank.clone());
    for line in content {
        let used = line
            .spans
            .iter()
            .map(|span| span.content.width())
            .sum::<usize>();
        let fill = usize::from(width).saturating_sub(used + pad.len() * 2);
        let mut spans = vec![Span::styled(pad.clone(), Style::default().bg(surface))];
        spans.extend(line.spans.into_iter().map(|mut span| {
            span.style = span.style.bg(surface);
            span
        }));
        spans.push(Span::styled(
            " ".repeat(fill + pad.len()),
            Style::default().bg(surface),
        ));
        rows.push(Line::from(spans));
    }
    rows.push(blank);
    rows
}

fn content_lines(
    compaction: &TranscriptCompactionSection,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let text = Style::default().fg(theme.text.primary);
    let mut lines = vec![Line::from(Span::styled(
        if compaction.kind == TranscriptCompactionKind::BranchSummary {
            "Branch summary".to_string()
        } else {
            heading(compaction.tokens_before, compaction.tokens_after)
        },
        text.add_modifier(Modifier::BOLD),
    ))];
    if let Some(warning) = compaction
        .warning
        .as_deref()
        .filter(|warning| !warning.is_empty())
    {
        lines.push(Line::default());
        let warning = crate::text::strip_ansi_escapes(warning);
        let color = Style::default().fg(theme.status.warning);
        lines.extend(
            crate::ui::ui_transcript_surface::wrap_surface_spans(
                vec![
                    Span::styled(if ascii { "[!] " } else { "⚠ " }, color),
                    Span::styled("Warning:", color.add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {warning}"), color),
                ],
                usize::from(width),
            )
            .into_iter()
            .map(Line::from),
        );
    }
    lines.push(Line::default());
    let full_summary = !compaction.superseded && !compaction.rollover;
    let summary = if compaction.rollover {
        ROLLOVER_NOTICE.to_string()
    } else {
        crate::text::strip_ansi_escapes(&compaction.summary)
    };
    crate::ui::ui_streaming_markdown::append_streaming_rich_text_block(
        &mut lines,
        &summary,
        theme.text.primary,
        "",
        theme,
        width,
    );
    if full_summary {
        let read_set = compaction.read_files.iter().cloned().collect();
        let files = harness_compaction::files::render_files_block(
            &compaction.read_files,
            &compaction.modified_files,
            &read_set,
        );
        lines.extend(
            files
                .lines()
                .filter(|line| !matches!(*line, "<files>" | "</files>"))
                .map(|file| {
                    Line::from(Span::styled(
                        crate::text::strip_ansi_escapes(file),
                        Style::default().fg(theme.text.secondary),
                    ))
                }),
        );
    }
    if let Some(archive) = &compaction.archive {
        super::blocks::append_archive_blocks(&mut lines, archive, theme, width);
    }
    if compaction.archive_frames > 0 {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(
                "{} archive frame{} attached",
                compaction.archive_frames,
                if compaction.archive_frames == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            text.add_modifier(Modifier::ITALIC),
        )));
    }
    lines
}
