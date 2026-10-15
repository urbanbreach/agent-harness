//! Literal, two-cell-tall context partitions with progress-driven dissolution.
use super::ui_compaction_motion::{compact_count, remaining};
use crate::theme::{GlyphMode, Theme};
use harness_core::event::{CompactionPhase, CompactionStage};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

struct Map {
    width: u16,
    window: u32,
    fixed: u64,
    history: u64,
    kept: u64,
    completed: u16,
    head: u16,
    elapsed: u64,
    dissolve: bool,
    moving: bool,
}

impl Map {
    fn new(stage: &CompactionStage, width: u16, elapsed: u64, moving: bool) -> Self {
        let before = stage.tokens_before.unwrap_or(0);
        let window = stage.window_tokens.unwrap_or(before).max(1);
        // Eighth-cell coordinates preserve small fixed/retained partitions.
        let scale =
            |tokens: u32| u64::from(tokens.min(window)) * u64::from(width) * 8 / u64::from(window);
        let fixed = scale(stage.fixed_tokens.unwrap_or(0));
        let kept = scale(stage.kept_tokens.unwrap_or(0));
        let original = scale(stage.summarized_tokens.unwrap_or_else(|| {
            before
                .saturating_sub(stage.fixed_tokens.unwrap_or(0))
                .saturating_sub(stage.kept_tokens.unwrap_or(0))
        }));
        let packed = stage.tokens_after.map_or(8, |after| {
            scale(
                after
                    .saturating_sub(stage.fixed_tokens.unwrap_or(0))
                    .saturating_sub(stage.kept_tokens.unwrap_or(0)),
            )
            .max(8)
        });
        let history = if matches!(
            stage.phase,
            CompactionPhase::Committing | CompactionPhase::Done
        ) {
            u64::from(remaining(
                u32::try_from(original).unwrap_or(u32::MAX),
                u32::try_from(packed.min(original)).unwrap_or(u32::MAX),
                elapsed,
                moving,
            ))
        } else {
            original
        };
        let cells = u16::try_from(original / 8).unwrap_or(width);
        let total = stage.windows_total.max(1);
        let completed = u16::try_from(
            u64::from(cells) * u64::from(stage.windows_done.min(total)) / u64::from(total),
        )
        .unwrap_or(cells);
        let next = u16::try_from(
            u64::from(cells) * u64::from(stage.windows_done.saturating_add(1).min(total))
                / u64::from(total),
        )
        .unwrap_or(cells);
        let advance = if moving {
            u16::try_from(elapsed / 35).unwrap_or(cells)
        } else {
            0
        };
        Self {
            width,
            window,
            fixed,
            history,
            kept,
            completed,
            head: completed.saturating_add(advance).min(next),
            elapsed,
            dissolve: matches!(
                stage.phase,
                CompactionPhase::Summarizing
                    | CompactionPhase::Remote
                    | CompactionPhase::Handoff
                    | CompactionPhase::TurnPrefix
                    | CompactionPhase::ShortSummary
            ),
            moving,
        }
    }

    fn cell(&self, x: u16, theme: &Theme, ascii: bool) -> (&'static str, Color) {
        let position = u64::from(x) * 8;
        let fixed_end = self.fixed;
        let history_end = fixed_end.saturating_add(self.history);
        let kept_end = history_end.saturating_add(self.kept);
        if position < fixed_end {
            return (
                block(fixed_end.saturating_sub(position).min(8), ascii),
                theme.text.tertiary,
            );
        }
        if position < history_end {
            let offset = u16::try_from((position - fixed_end) / 8).unwrap_or(self.width);
            let glyph = if self.dissolve && offset < self.completed {
                if self.moving {
                    " "
                } else if ascii {
                    "."
                } else {
                    "░"
                }
            } else if self.dissolve && self.moving && offset < self.head {
                decay(
                    self.elapsed
                        .saturating_sub(u64::from(offset.saturating_sub(self.completed)) * 35),
                    ascii,
                )
            } else {
                block(history_end.saturating_sub(position).min(8), ascii)
            };
            let color = if self.dissolve && self.moving && offset == self.head {
                theme.text.primary
            } else {
                theme.text.accent
            };
            return (glyph, color);
        }
        if position < kept_end {
            return (
                block(kept_end.saturating_sub(position).min(8), ascii),
                theme.status.info,
            );
        }
        (if ascii { "." } else { "░" }, theme.text.tertiary)
    }
}

fn block(eighths: u64, ascii: bool) -> &'static str {
    if ascii {
        if eighths >= 6 {
            "#"
        } else if eighths >= 3 {
            "="
        } else {
            "-"
        }
    } else {
        match eighths {
            0 => " ",
            1 => "▏",
            2 => "▎",
            3 => "▍",
            4 => "▌",
            5 => "▋",
            6 => "▊",
            7 => "▉",
            _ => "█",
        }
    }
}

fn decay(age: u64, ascii: bool) -> &'static str {
    match age / 100 {
        0 => {
            if ascii {
                "#"
            } else {
                "█"
            }
        }
        1 => {
            if ascii {
                "="
            } else {
                "▓"
            }
        }
        2 => {
            if ascii {
                "-"
            } else {
                "▒"
            }
        }
        3 => {
            if ascii {
                "."
            } else {
                "░"
            }
        }
        _ => " ",
    }
}

fn ink(frame: &mut Frame, point: (u16, u16), glyph: &str, color: Color) {
    if let Some(cell) = frame.buffer_mut().cell_mut(point) {
        cell.set_symbol(glyph).set_style(Style::default().fg(color));
    }
}

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    stage: &CompactionStage,
    theme: &Theme,
    elapsed: u64,
    moving: bool,
) {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let map = Map::new(stage, area.width.saturating_sub(1), elapsed, moving);
    for x in 0..map.width {
        let (glyph, color) = map.cell(x, theme, ascii);
        for y in 1..=2 {
            ink(frame, (area.x + x, area.y + y), glyph, color);
        }
    }
    for y in 1..=2 {
        ink(
            frame,
            (area.right().saturating_sub(1), area.y + y),
            if ascii { "]" } else { "▕" },
            theme.text.primary,
        );
    }
    particles(frame, area, &map, theme, ascii);
    capsule(frame, area, stage, theme, elapsed, moving, ascii);
    threshold(frame, area, stage, &map, theme, ascii);
    legend(frame, area, stage, theme, ascii);
}

fn particles(frame: &mut Frame, area: Rect, map: &Map, theme: &Theme, ascii: bool) {
    if !map.moving || !map.dissolve {
        return;
    }
    const PARTICLES: [&str; 8] = ["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"];
    let fixed = u16::try_from(map.fixed / 8).unwrap_or(map.width);
    for offset in map.completed..map.head {
        let age = map
            .elapsed
            .saturating_sub(u64::from(offset.saturating_sub(map.completed)) * 35);
        if !(100..1000).contains(&age) || !offset.is_multiple_of(3) {
            continue;
        }
        let origin = fixed.saturating_add(offset);
        let drift = u16::try_from(age / 25).unwrap_or(origin);
        let x = origin.saturating_sub(drift).max(1);
        if x < map.width {
            let index = usize::try_from((age / 70 + u64::from(offset)) % 8).unwrap_or(0);
            ink(
                frame,
                (area.x + x, area.y),
                if ascii { "." } else { PARTICLES[index] },
                theme.text.accent,
            );
        }
    }
}

fn capsule(
    frame: &mut Frame,
    area: Rect,
    stage: &CompactionStage,
    theme: &Theme,
    elapsed: u64,
    moving: bool,
    ascii: bool,
) {
    let total = stage.windows_total.max(1);
    let completed = stage.windows_done.min(total);
    let glyph = if ascii {
        if completed == total {
            "#"
        } else {
            "+"
        }
    } else if completed == total
        || matches!(
            stage.phase,
            CompactionPhase::Committing | CompactionPhase::Done
        )
    {
        "◆"
    } else if completed > 0 || moving && (elapsed / 250).is_multiple_of(2) {
        "◈"
    } else {
        "◇"
    };
    let color = if stage.phase == CompactionPhase::Done {
        theme.status.success
    } else {
        theme.text.accent
    };
    ink(frame, (area.x, area.y), glyph, color);
}

fn threshold(
    frame: &mut Frame,
    area: Rect,
    stage: &CompactionStage,
    map: &Map,
    theme: &Theme,
    ascii: bool,
) {
    let Some(tokens) = stage.threshold_tokens else {
        return;
    };
    let x = u16::try_from(
        u64::from(tokens.min(map.window)) * u64::from(map.width) / u64::from(map.window),
    )
    .unwrap_or(map.width)
    .min(map.width.saturating_sub(1));
    for y in 1..=2 {
        ink(
            frame,
            (area.x + x, area.y + y),
            if ascii { "|" } else { "┃" },
            theme.text.primary,
        );
    }
    let value = compact_count(tokens);
    let value_width = u16::try_from(value.len()).unwrap_or(area.width);
    let label_x = x.min(area.width.saturating_sub(value_width));
    frame.render_widget(
        Paragraph::new(value).style(Style::default().fg(theme.text.primary)),
        Rect::new(area.x + label_x, area.y, value_width, 1),
    );
}

fn legend(frame: &mut Frame, area: Rect, stage: &CompactionStage, theme: &Theme, ascii: bool) {
    let swatch = if ascii { "# " } else { "■ " };
    let mut spans = Vec::with_capacity(8);
    for (label, tokens, color) in [
        ("system", stage.fixed_tokens, theme.text.tertiary),
        ("summarizing", stage.summarized_tokens, theme.text.accent),
        ("kept", stage.kept_tokens, theme.status.info),
    ] {
        spans.push(Span::styled(swatch, Style::default().fg(color)));
        spans.push(Span::styled(
            format!(
                "{label} {}  ",
                tokens.map_or_else(|| "?".into(), compact_count)
            ),
            Style::default().fg(theme.text.secondary),
        ));
    }
    if let Some(tokens) = stage.threshold_tokens {
        spans.push(Span::styled(
            if ascii { "| " } else { "┃ " },
            Style::default().fg(theme.text.primary),
        ));
        spans.push(Span::styled(
            format!("threshold {}", compact_count(tokens)),
            Style::default().fg(theme.text.secondary),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(area.x, area.y + 3, area.width, 1),
    );
}
