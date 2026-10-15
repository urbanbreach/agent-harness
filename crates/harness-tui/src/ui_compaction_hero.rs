//! Bounded live card; the expanded surface makes context partitions the hero.
use super::ui_compaction_motion::{
    compact_count, context_bar, method_glyph, phase_label, reduction_percent, remaining,
};
use crate::{
    app::AppState,
    theme::{GlyphMode, Theme},
};
use harness_core::event::{CompactionMethod, CompactionPhase, CompactionStage};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn render(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) -> bool {
    let Some(card) = app.displayed_compaction() else {
        return false;
    };
    let Some(stage) = &card.stage else {
        return false;
    };
    let elapsed = millis(app.now().saturating_duration_since(card.updated_at));
    let moving = app.transcript_motion_enabled()
        && !matches!(
            stage.phase,
            CompactionPhase::Failed | CompactionPhase::Cancelled | CompactionPhase::Skipped
        );
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel)),
        area,
    );
    for y in area.y..area.bottom() {
        ink(
            frame,
            area.x,
            y,
            if ascii { "|" } else { "▎" },
            Style::default().fg(theme.text.accent),
        );
    }
    let body = Rect::new(
        area.x.saturating_add(2),
        area.y,
        area.width.saturating_sub(3),
        area.height,
    );
    let total_elapsed = millis(app.now().saturating_duration_since(card.started_at));
    // The stop control owns the right end of the header row while the pass runs.
    let stop_width = if card.finished {
        0
    } else {
        u16::try_from(UnicodeWidthStr::width("[stop]") + 1).unwrap_or(0)
    };
    header(
        frame,
        app,
        Rect::new(body.x, body.y, body.width.saturating_sub(stop_width), 1),
        stage,
        card.finished,
        (total_elapsed, elapsed),
        moving,
        theme,
    );
    let expanded = body.height == 7 || body.height >= 11;
    if expanded {
        super::ui_compaction_map::render(
            frame,
            Rect::new(body.x, body.y + 1, body.width, 4),
            stage,
            theme,
            elapsed,
            moving,
        );
    } else {
        frame.render_widget(
            Paragraph::new(context_bar(stage, theme, body.width, elapsed, moving)),
            Rect::new(body.x, body.y + 1, body.width, 1),
        );
    }
    let source = stage
        .message
        .as_deref()
        .unwrap_or_else(|| card.preview.as_deref().unwrap_or(""));
    let count = if moving
        && !matches!(
            stage.phase,
            CompactionPhase::Done | CompactionPhase::Committing
        ) {
        usize::try_from(elapsed / 12 + 1).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    let visible_end = source
        .grapheme_indices(true)
        .nth(count)
        .map_or(source.len(), |(index, _)| index);
    let wrapped = super::wrap_completion_text(
        &source[..visible_end],
        usize::from(body.width.saturating_sub(2)),
    );
    let first = wrapped.len().saturating_sub(2);
    let stale = card.finished
        || millis(app.now().saturating_duration_since(card.preview_updated_at)) > 1400;
    let preview_style = Style::default().fg(if stale {
        theme.text.secondary
    } else {
        theme.text.primary
    });
    for (row, text) in wrapped.iter().skip(first).take(2).enumerate() {
        let cursor = !card.finished
            && !stale
            && (!moving || (elapsed / 400).is_multiple_of(2))
            && row + first + 1 == wrapped.len();
        let line = Line::from(vec![
            Span::styled(text.as_str(), preview_style),
            Span::styled(
                if cursor { "_" } else { "" },
                Style::default().fg(theme.text.accent),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(
                body.x,
                body.y + if expanded { 5 } else { 2 } + u16::try_from(row).unwrap_or(0),
                body.width,
                1,
            ),
        );
    }
    let flourish = Rect::new(body.x, body.y + if expanded { 7 } else { 4 }, body.width, 4);
    match stage.method {
        Some(CompactionMethod::Archive) => filmstrip(frame, app, flourish, theme, moving),
        Some(CompactionMethod::Remote) => uplink(frame, flourish, theme, elapsed, moving),
        Some(CompactionMethod::Handoff) => document(
            frame,
            flourish,
            theme,
            elapsed,
            moving && stage.phase != CompactionPhase::Done,
            source,
        ),
        Some(CompactionMethod::Shake) => shake(frame, flourish, theme, stage, elapsed, moving),
        _ if !expanded => capsule(frame, flourish, theme, stage, elapsed, moving),
        _ => {}
    }
    true
}

fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn header(
    frame: &mut Frame,
    app: &AppState,
    body: Rect,
    stage: &CompactionStage,
    finished: bool,
    timing: (u64, u64),
    moving: bool,
    theme: &Theme,
) {
    let (total_elapsed, elapsed) = timing;
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let counter = match (stage.tokens_before, stage.tokens_after) {
        (Some(before), Some(after)) if stage.phase == CompactionPhase::Done => format!(
            "{} {} {}  -{}%",
            compact_count(before),
            if ascii { "->" } else { "→" },
            compact_count(remaining(before, after, elapsed, moving)),
            reduction_percent(before, after)
        ),
        (Some(before), _) => format!("{} tokens", compact_count(before)),
        (_, Some(after)) => format!("{} tokens", compact_count(after)),
        _ => String::new(),
    };
    let count_width = counter.width().min(usize::from(body.width));
    let available = usize::from(body.width).saturating_sub(count_width + 1);
    let title = format!(
        "{} {}",
        method_glyph(stage.method, ascii),
        if finished {
            "Context maintenance".to_owned()
        } else {
            let full = super::ui_compaction_labels::running_label(app, ascii);
            if full.width() + 2 <= available {
                full
            } else {
                super::ui_compaction_labels::running_label_short(app)
            }
        }
    );
    let title = super::ui_chrome::take_width_prefix(&title, available);
    let frames = if stage.method == Some(CompactionMethod::Archive) {
        format!(" {}/{}", stage.frames_done, stage.frames_total)
    } else {
        String::new()
    };
    let detail = format!(
        " [{} / {}{frames}] {} {}",
        stage.method.map_or("summary", CompactionMethod::as_str),
        stage.reason,
        phase_label(stage.phase),
        super::ui_live_turn_status::format_elapsed_ms(total_elapsed)
    );
    let detail =
        super::ui_chrome::take_width_prefix(&detail, available.saturating_sub(title.width()));
    let gap = usize::from(body.width).saturating_sub(title.width() + detail.width() + count_width);
    let color = if stage.phase == CompactionPhase::Done {
        theme.status.success
    } else if stage.phase == CompactionPhase::Failed {
        theme.status.error
    } else {
        theme.text.primary
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(detail, Style::default().fg(theme.text.secondary)),
            Span::raw(" ".repeat(gap)),
            Span::styled(
                counter,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])),
        Rect::new(body.x, body.y, body.width, 1),
    );
}

fn ink(frame: &mut Frame, x: u16, y: u16, glyph: &str, style: Style) {
    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
        cell.set_symbol(glyph).set_style(style);
    }
}

fn caption(frame: &mut Frame, rect: Rect, text: &str, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(theme.text.secondary)),
        Rect::new(rect.x, rect.y, rect.width, 1),
    );
}

fn filmstrip(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme, moving: bool) {
    let Some(card) = app.displayed_compaction() else {
        return;
    };
    let Some(stage) = &card.stage else {
        return;
    };
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    if stage.thumbnails.is_empty() {
        caption(frame, area, "Archive frames are preparing", theme);
        return;
    }
    for (index, (thumbnail, (bits, born))) in
        stage.thumbnails.iter().zip(&card.thumbnails).enumerate()
    {
        if bits.is_empty() || thumbnail.width == 0 || thumbnail.height == 0 {
            continue;
        }
        let age = millis(app.now().saturating_duration_since(*born));
        let slide = if moving {
            u16::try_from(10 * 350_u64.saturating_sub(age.min(350)) / 350).unwrap_or(0)
        } else {
            0
        };
        let offset = u16::try_from(index)
            .unwrap_or(u16::MAX)
            .saturating_mul(12)
            .saturating_add(slide);
        let x = area.x.saturating_add(offset);
        if x >= area.right() {
            break;
        }
        let style = Style::default().fg(if moving && age < 250 {
            theme.status.success
        } else {
            theme.text.accent
        });
        for row in 0..4_u16 {
            film_row(frame, area, (x, row), thumbnail, bits, ascii, style);
        }
    }
}

fn film_row(
    frame: &mut Frame,
    area: Rect,
    position: (u16, u16),
    thumbnail: &harness_core::event::FrameThumbnail,
    bits: &[u8],
    ascii: bool,
    style: Style,
) {
    let (x, row) = position;
    const QUADRANTS: [&str; 16] = [
        " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
    ];
    ink(frame, x, area.y + row, if ascii { "[" } else { "▕" }, style);
    for column in 0..8_u16 {
        let mut pattern = 0_usize;
        for (dx, dy, bit) in [(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3)] {
            let px = u32::from(column * 2 + dx) * u32::from(thumbnail.width) / 16;
            let py = u32::from(row * 2 + dy) * u32::from(thumbnail.height) / 8;
            let index = usize::try_from(py * u32::from(thumbnail.width) + px).unwrap_or(usize::MAX);
            if bits
                .get(index / 8)
                .is_some_and(|byte| byte & (0x80 >> (index % 8)) != 0)
            {
                pattern |= 1 << bit;
            }
        }
        let cell_x = x.saturating_add(1 + column);
        if cell_x < area.right() {
            ink(
                frame,
                cell_x,
                area.y + row,
                if ascii {
                    if pattern == 0 {
                        " "
                    } else {
                        "#"
                    }
                } else {
                    QUADRANTS[pattern]
                },
                style,
            );
        }
    }
    if x.saturating_add(9) < area.right() {
        ink(
            frame,
            x + 9,
            area.y + row,
            if ascii { "]" } else { "▏" },
            style,
        );
    }
}

fn uplink(frame: &mut Frame, area: Rect, theme: &Theme, elapsed: u64, moving: bool) {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    caption(frame, area, "UPLINK - server-side context exchange", theme);
    let width = area.width.saturating_sub(5).max(1);
    let phase = if moving {
        u16::try_from((elapsed / 45) % u64::from(width)).unwrap_or(0)
    } else {
        width / 2
    };
    for x in 0..width {
        let out = x == phase
            || u32::from(x) == (u32::from(phase) + u32::from(width) / 3) % u32::from(width);
        let back = x == width.saturating_sub(phase + 1);
        ink(
            frame,
            area.x + x,
            area.y + 1,
            if out {
                if ascii {
                    "o"
                } else {
                    "•"
                }
            } else {
                "."
            },
            Style::default().fg(if out {
                theme.text.accent
            } else {
                theme.text.tertiary
            }),
        );
        ink(
            frame,
            area.x + x,
            area.y + 3,
            if back {
                if ascii {
                    "#"
                } else {
                    "▪"
                }
            } else {
                "."
            },
            Style::default().fg(if back {
                theme.status.success
            } else {
                theme.text.tertiary
            }),
        );
    }
    ink(
        frame,
        area.x + width + 1,
        area.y + 1,
        if ascii { "@" } else { "☁" },
        Style::default()
            .fg(theme.status.info)
            .add_modifier(Modifier::BOLD),
    );
    for (index, glyph) in if ascii {
        ["[", "#", "]"]
    } else {
        ["⟦", "▣", "⟧"]
    }
    .into_iter()
    .enumerate()
    {
        ink(
            frame,
            area.x + u16::try_from(index).unwrap_or(0),
            area.y + 3,
            glyph,
            Style::default().fg(theme.status.success),
        );
    }
}

fn document(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    elapsed: u64,
    moving: bool,
    source: &str,
) {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    caption(
        frame,
        area,
        "HANDOFF - writing the continuation document",
        theme,
    );
    let title = source
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Continuation handoff")
        .trim_start_matches('#')
        .trim();
    let available = usize::from(area.width.saturating_sub(3));
    let limit = if moving {
        usize::try_from(elapsed / 45)
            .unwrap_or(available)
            .min(available)
    } else {
        available
    };
    let written = super::ui_chrome::take_width_prefix(title, limit);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(written, Style::default().fg(theme.text.primary)),
            Span::styled(
                if ascii { ">" } else { "✎" },
                Style::default().fg(theme.text.accent),
            ),
        ])),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
    let width = u16::try_from(written.width())
        .unwrap_or(area.width)
        .min(area.width);
    for x in 0..width {
        ink(
            frame,
            area.x + x,
            area.y + 2,
            if ascii { "-" } else { "─" },
            Style::default().fg(theme.text.accent),
        );
    }
}

fn shake(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    stage: &CompactionStage,
    elapsed: u64,
    moving: bool,
) {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let saved = stage
        .tokens_before
        .zip(stage.tokens_after)
        .map_or(0, |(before, after)| before.saturating_sub(after));
    let freed = if stage.phase == CompactionPhase::Done {
        saved.saturating_sub(remaining(saved, 0, elapsed, moving))
    } else {
        0
    };
    caption(
        frame,
        area,
        &format!("SHAKE - {} tokens freed", compact_count(freed)),
        theme,
    );
    let jitter = if moving && (stage.phase != CompactionPhase::Done || elapsed < 250) {
        u16::try_from((elapsed / 50) % 3).unwrap_or(0)
    } else {
        0
    };
    let blocks = if stage.phase == CompactionPhase::Done {
        u16::try_from(remaining(14, 0, elapsed, moving)).unwrap_or(0)
    } else {
        14
    };
    for x in 0..blocks.min(area.width.saturating_sub(jitter)) {
        ink(
            frame,
            area.x + x + jitter,
            area.y + 1,
            if ascii { "#" } else { "█" },
            Style::default().fg(theme.text.accent),
        );
    }
    let falling = if moving {
        u16::try_from((elapsed / 70) % 3).unwrap_or(0)
    } else {
        2
    };
    for x in (1..area.width.min(32)).step_by(5) {
        ink(
            frame,
            area.x + x,
            area.y + 1 + falling,
            if ascii { "." } else { "⠂" },
            Style::default().fg(theme.text.secondary),
        );
    }
}

fn capsule(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    stage: &CompactionStage,
    elapsed: u64,
    moving: bool,
) {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let fixed = stage.fixed_tokens.map_or_else(|| "?".into(), compact_count);
    let kept = stage.kept_tokens.map_or_else(|| "?".into(), compact_count);
    caption(
        frame,
        area,
        &format!("system {fixed}   kept {kept}   -> summary"),
        theme,
    );
    let width = area.width.saturating_sub(9).max(1);
    let phase = if moving {
        u16::try_from((elapsed / 55) % u64::from(width)).unwrap_or(0)
    } else {
        width / 2
    };
    let size = if moving {
        u16::try_from((elapsed / 450).min(4)).unwrap_or(0) + 1
    } else {
        5
    };
    ink(
        frame,
        area.x,
        area.y + 2,
        if ascii { "[" } else { "⟦" },
        Style::default().fg(theme.text.accent),
    );
    for x in 1..=size {
        ink(
            frame,
            area.x + x,
            area.y + 2,
            if ascii { "#" } else { "▰" },
            Style::default().fg(if stage.phase == CompactionPhase::Done {
                theme.status.success
            } else {
                theme.text.accent
            }),
        );
    }
    ink(
        frame,
        area.x + size + 1,
        area.y + 2,
        if ascii { "]" } else { "⟧" },
        Style::default().fg(theme.text.accent),
    );
    for x in 8..area.width {
        ink(
            frame,
            area.x + x,
            area.y + 2,
            if moving && (u32::from(x) + u32::from(phase)).is_multiple_of(7) {
                if ascii {
                    ":"
                } else {
                    "⠂"
                }
            } else {
                "."
            },
            Style::default().fg(theme.text.secondary),
        );
    }
}
