//! Ordered archive blocks of the full-history transcript: head text, frames, tail text.
//!
//! Frames are shown as descriptors; their pixels are read back by the model, not drawn.
use crate::theme::Theme;
use harness_core::event::{ArchiveFrameRecord, ArchiveRecord};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// A text block longer than this is cut with a count of the rows left out.
const MAX_TEXT_ROWS: usize = 40;

pub(super) fn append_archive_blocks(
    lines: &mut Vec<Line<'static>>,
    archive: &ArchiveRecord,
    theme: &Theme,
    width: u16,
) {
    append_text_block(lines, "Archive head", &archive.text_head, theme, width);
    if !archive.frames.is_empty() {
        lines.push(Line::default());
        let total = archive.frames.len();
        for (index, frame) in archive.frames.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                frame_descriptor(index + 1, total, frame),
                Style::default().fg(theme.text.secondary),
            )));
        }
    }
    append_text_block(lines, "Archive tail", &archive.text_tail, theme, width);
}

fn append_text_block(
    lines: &mut Vec<Line<'static>>,
    caption: &str,
    text: &str,
    theme: &Theme,
    width: u16,
) {
    let text = crate::text::strip_ansi_escapes(text);
    if text.trim().is_empty() {
        return;
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        caption.to_string(),
        Style::default()
            .fg(theme.text.secondary)
            .add_modifier(Modifier::ITALIC),
    )));
    let style = Style::default().fg(theme.text.secondary);
    let rows = text
        .lines()
        .flat_map(|line| {
            if line.is_empty() {
                return vec![Line::default()];
            }
            crate::ui::ui_transcript_surface::wrap_surface_spans(
                vec![Span::styled(line.to_string(), style)],
                usize::from(width),
            )
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let hidden = rows.len().saturating_sub(MAX_TEXT_ROWS);
    lines.extend(rows.into_iter().take(MAX_TEXT_ROWS));
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("… {hidden} more rows"),
            style.add_modifier(Modifier::ITALIC),
        )));
    }
}

fn frame_descriptor(number: usize, total: usize, frame: &ArchiveFrameRecord) -> String {
    let name = std::path::Path::new(&frame.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(frame.path.as_str());
    let mut parts = vec![format!("frame {number}/{total}")];
    if frame.width > 0 && frame.height > 0 {
        parts.push(format!("{}×{}", frame.width, frame.height));
    }
    if frame.bytes > 0 {
        parts.push(byte_size(frame.bytes));
    }
    if !name.is_empty() {
        parts.push(name.to_string());
    }
    parts.join(" · ")
}

fn byte_size(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        format!("{}.{} KB", bytes / 1_024, bytes % 1_024 * 10 / 1_024)
    } else {
        format!(
            "{}.{} MB",
            bytes / 1_048_576,
            bytes % 1_048_576 * 10 / 1_048_576
        )
    }
}
