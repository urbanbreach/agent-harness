//! Context maintenance motion stays within the live card and latest divider.
pub(super) fn fresh_divider(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let Some(card) = app.displayed_compaction() else {
        return;
    };
    let Some(stage) = card
        .stage
        .as_ref()
        .filter(|stage| stage.phase == CompactionPhase::Done)
    else {
        return;
    };
    if !app.transcript_motion_enabled() || area.width == 0 {
        return;
    }
    let elapsed = u64::try_from(
        app.now()
            .saturating_duration_since(card.updated_at)
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let (Some(before), Some(after)) = (
        stage.tokens_before.filter(|before| *before > 0),
        stage.tokens_after,
    ) else {
        return;
    };
    let current = remaining(before, after, elapsed, true);
    let warning = app
        .events()
        .rev()
        .find_map(|event| match &event.payload {
            harness_core::event::EventV1::SessionCompaction(data)
                if data.agent_id == card.agent_id =>
            {
                Some(data.warning.is_some())
            }
            harness_core::event::EventV1::ContextPruned(data)
                if data.agent_id == card.agent_id
                    && data.kind == harness_core::event::PruneKindRecord::Shake =>
            {
                Some(data.warning.is_some())
            }
            _ => None,
        })
        .unwrap_or(false);
    frame.render_widget(
        Paragraph::new(super::ui_transcript::settling_divider(
            stage.method,
            (before, current),
            warning,
            &app.compaction_expand_hint(),
            theme,
            area.width,
        ))
        .style(Style::default().bg(theme.surface.canvas)),
        area,
    );
    let scan = u16::try_from(elapsed.min(899) * u64::from(area.width) / 900).unwrap_or(0);
    for x in scan.saturating_sub(2)..scan.saturating_add(2).min(area.width) {
        if let Some(cell) = frame.buffer_mut().cell_mut((area.x + x, area.y))
            && cell.fg != theme.status.warning
        {
            cell.set_fg(theme.status.success);
        }
    }
}
use crate::{
    app::AppState,
    theme::{GlyphMode, Theme},
};
use harness_core::event::{CompactionMethod, CompactionPhase, CompactionStage};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub(super) fn method_glyph(method: Option<CompactionMethod>, ascii: bool) -> &'static str {
    match (method, ascii) {
        (Some(CompactionMethod::Remote), false) => "☁",
        (Some(CompactionMethod::Archive), false) => "▦",
        (Some(CompactionMethod::Handoff), false) => "✎",
        (Some(CompactionMethod::Shake), false) => "≋",
        (_, false) => "◇",
        (Some(CompactionMethod::Remote), true) => "@",
        (Some(CompactionMethod::Archive), true) => "#",
        (Some(CompactionMethod::Handoff), true) => ">",
        (Some(CompactionMethod::Shake), true) => "~",
        (_, true) => "*",
    }
}

pub(super) fn phase_label(phase: CompactionPhase) -> &'static str {
    match phase {
        CompactionPhase::Preparing => "preparing",
        CompactionPhase::Summarizing => "distilling",
        CompactionPhase::TurnPrefix => "retaining turn context",
        CompactionPhase::ShortSummary => "writing short summary",
        CompactionPhase::Archiving => "printing frames",
        CompactionPhase::Remote => "server compaction",
        CompactionPhase::Handoff => "writing handoff",
        CompactionPhase::Shaking => "shaking blocks",
        CompactionPhase::Committing => "committing",
        CompactionPhase::Done => "completed",
        CompactionPhase::Failed => "failed",
        CompactionPhase::Cancelled => "cancelled",
        CompactionPhase::Skipped => "skipped",
    }
}

/// Compact token count: below 1,000 as is, then K/M/B with one decimal under ten units
/// (a trailing `.0` is dropped) and whole units above. Rounding is half-up.
pub(super) fn compact_count(tokens: u32) -> String {
    let tokens = u64::from(tokens);
    if tokens < 1000 {
        return tokens.to_string();
    }
    let (unit, suffix) = if tokens < 1_000_000 {
        (1_000, "K")
    } else if tokens < 1_000_000_000 {
        (1_000_000, "M")
    } else {
        (1_000_000_000, "B")
    };
    if tokens < unit * 10 {
        let tenths = (tokens * 10 + unit / 2) / unit;
        let (whole, tenth) = (tenths / 10, tenths % 10);
        return if tenth == 0 {
            format!("{whole}{suffix}")
        } else {
            format!("{whole}.{tenth}{suffix}")
        };
    }
    format!("{}{suffix}", (tokens + unit / 2) / unit)
}

pub(super) fn reduction_percent(before: u32, after: u32) -> u32 {
    u32::try_from(
        (u64::from(before.saturating_sub(after)) * 100 + u64::from(before) / 2)
            .checked_div(u64::from(before))
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

// Integer cubic ease-out avoids floating-point drift in deterministic captures.
pub(super) fn remaining(before: u32, after: u32, elapsed: u64, motion: bool) -> u32 {
    let left = if motion {
        700_u64.saturating_sub(elapsed.min(700))
    } else {
        0
    };
    let difference = u64::from(before.saturating_sub(after));
    after.saturating_add(
        u32::try_from(difference * left * left * left / 343_000_000).unwrap_or(u32::MAX),
    )
}

pub(super) fn context_bar(
    stage: &CompactionStage,
    theme: &Theme,
    width: u16,
    elapsed: u64,
    motion: bool,
) -> Line<'static> {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let before = stage.tokens_before.unwrap_or(0);
    let done = stage.phase == CompactionPhase::Done;
    let tokens = if done {
        remaining(
            before,
            stage.tokens_after.unwrap_or(before),
            elapsed,
            motion,
        )
    } else {
        before
    };
    let window = stage
        .window_tokens
        .or(stage.threshold_tokens)
        .unwrap_or(before)
        .max(1);
    let cell = |amount: u32| {
        u16::try_from(u64::from(amount.min(window)) * u64::from(width) / u64::from(window))
            .unwrap_or(width)
    };
    let fixed = cell(stage.fixed_tokens.unwrap_or(0));
    let kept = cell(stage.kept_tokens.unwrap_or(0));
    let mut used = cell(tokens);
    let mut summarized_end = used.saturating_sub(kept).max(fixed);
    let original_summary = summarized_end.saturating_sub(fixed);
    if stage.phase == CompactionPhase::Committing && motion {
        let packed = u16::try_from(remaining(
            u32::from(original_summary),
            u32::from(original_summary.min(6)),
            elapsed,
            true,
        ))
        .unwrap_or(original_summary);
        summarized_end = fixed.saturating_add(packed);
        used = summarized_end.saturating_add(kept).min(width);
    }
    let tick = cell(stage.threshold_tokens.unwrap_or(window)).min(width.saturating_sub(1));
    let scan_width = summarized_end.saturating_sub(fixed).max(1);
    let scan = fixed + u16::try_from((elapsed / 35) % u64::from(scan_width)).unwrap_or(0);
    let capsule = if motion {
        u16::try_from((elapsed / 450).min(4)).unwrap_or(0) + 2
    } else {
        6
    };
    let capsule = capsule.min(scan_width);
    let mut spans = Vec::with_capacity(usize::from(width));
    for x in 0..width {
        let (glyph, color) = if x == width.saturating_sub(1) {
            ("]", theme.text.primary)
        } else if x == tick {
            (if ascii { "|" } else { "┃" }, theme.text.primary)
        } else if x < fixed {
            (if ascii { "=" } else { "━" }, theme.text.tertiary)
        } else if x < summarized_end {
            let head = motion && !done && x == scan;
            let distance = scan.saturating_sub(x);
            let tail = motion && !done && distance <= 4 && x < scan;
            let particle =
                motion && !done && (u64::from(x - fixed) + elapsed / 85).is_multiple_of(11);
            let glyph = capsule_cell(x - fixed, capsule, ascii).unwrap_or_else(|| {
                if tail {
                    trail_cell(distance, ascii)
                } else {
                    summary_cell(head, particle, ascii)
                }
            });
            (
                glyph,
                if head {
                    theme.text.accent
                } else {
                    theme.text.secondary
                },
            )
        } else if x < used {
            (if ascii { "=" } else { "━" }, theme.status.info)
        } else if done && elapsed < 250 && motion && x < cell(before) {
            (if ascii { "+" } else { "─" }, theme.status.success)
        } else {
            (if ascii { "." } else { "·" }, theme.text.tertiary)
        };
        spans.push(Span::styled(glyph, Style::default().fg(color)));
    }
    Line::from(spans)
}

fn capsule_cell(offset: u16, width: u16, ascii: bool) -> Option<&'static str> {
    match (offset, width, ascii) {
        (0, _, true) => Some("["),
        (0, _, false) => Some("⟦"),
        (offset, width, true) if offset == width.saturating_sub(1) => Some("]"),
        (offset, width, false) if offset == width.saturating_sub(1) => Some("⟧"),
        (offset, width, true) if offset < width => Some("#"),
        (offset, width, false) if offset < width => Some("▰"),
        _ => None,
    }
}

fn summary_cell(head: bool, particle: bool, ascii: bool) -> &'static str {
    match (head, particle, ascii) {
        (true, _, true) => ">",
        (true, _, false) => "▰",
        (_, true, true) => ".",
        (_, true, false) => "⠂",
        (_, _, true) => "-",
        (_, _, false) => "─",
    }
}

fn trail_cell(distance: u16, ascii: bool) -> &'static str {
    match (distance, ascii) {
        (1, false) => "▓",
        (2, false) => "▒",
        (3, false) => "░",
        (_, false) => "·",
        (1, true) => "#",
        (2, true) => "*",
        (3, true) => ":",
        (_, true) => ".",
    }
}

fn thumbnail_line(
    stage: &CompactionStage,
    bits: &[(Vec<u8>, std::time::Instant)],
    ascii: bool,
    width: u16,
    now: std::time::Instant,
    motion: bool,
) -> String {
    let mut text = String::new();
    let mut cells = 0_u16;
    for (frame, (bitmap, started_at)) in stage.thumbnails.iter().zip(bits) {
        if bitmap.is_empty() || frame.width == 0 || frame.height == 0 {
            continue;
        }
        let elapsed = u64::try_from(now.saturating_duration_since(*started_at).as_millis())
            .unwrap_or(u64::MAX);
        if cells.saturating_add(frame.width / 2 + 3) > width {
            break;
        }
        text.push('[');
        for x in (0..frame.width).step_by(2) {
            let mut dots = 0_u8;
            for (dx, dy, bit) in [
                (0, 0, 0),
                (0, 1, 1),
                (0, 2, 2),
                (1, 0, 3),
                (1, 1, 4),
                (1, 2, 5),
                (0, 3, 6),
                (1, 3, 7),
            ] {
                let px = x + dx;
                let py = dy * frame.height / 4;
                let index = usize::from(py) * usize::from(frame.width) + usize::from(px);
                if px < frame.width
                    && py < frame.height
                    && bitmap
                        .get(index / 8)
                        .is_some_and(|byte| byte & (0x80 >> (index % 8)) != 0)
                {
                    dots |= 1 << bit;
                }
            }
            if motion && u64::from(x) > elapsed / 45 {
                text.push(' ');
            } else if ascii {
                text.push(if dots == 0 { ' ' } else { '#' });
            } else {
                text.push(char::from_u32(0x2800 + u32::from(dots)).unwrap_or(' '));
            }
        }
        text.push_str("] ");
        cells = cells.saturating_add(frame.width / 2 + 3);
    }
    text
}

pub(super) fn render(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) -> bool {
    let Some(card) = app.displayed_compaction() else {
        return false;
    };
    let Some(stage) = card.stage.as_ref() else {
        return false;
    };
    if area.height >= 7 && area.width >= 48 {
        return super::ui_compaction_hero::render(frame, app, area, theme);
    }
    let elapsed = u64::try_from(
        app.now()
            .saturating_duration_since(card.updated_at)
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let motion = app.transcript_motion_enabled()
        && !matches!(
            stage.phase,
            CompactionPhase::Failed | CompactionPhase::Cancelled | CompactionPhase::Skipped
        );
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let method = stage.method.map_or("summary", CompactionMethod::as_str);
    let title = if card.finished {
        "Context maintenance".to_owned()
    } else {
        super::ui_compaction_labels::running_label(app, ascii)
    };
    let full = format!("{} {title}", method_glyph(stage.method, ascii));
    let basic = if unicode_width::UnicodeWidthStr::width(full.as_str()) > usize::from(area.width) {
        format!(
            "{} {}",
            method_glyph(stage.method, ascii),
            super::ui_compaction_labels::running_label_short(app)
        )
    } else {
        full
    };
    let detail = format!(
        " [{method} / {}] {} {}",
        stage.reason,
        phase_label(stage.phase),
        super::ui_live_turn_status::format_elapsed_ms(
            u64::try_from(
                app.now()
                    .saturating_duration_since(card.started_at)
                    .as_millis()
            )
            .unwrap_or(u64::MAX)
        )
    );
    let available = usize::from(area.width)
        .saturating_sub(unicode_width::UnicodeWidthStr::width(basic.as_str()));
    let header = Line::from(vec![
        Span::styled(
            basic,
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if ascii {
                super::ui_chrome::take_width_prefix(&detail, available).to_string()
            } else {
                super::ui_chrome::truncate_plain_text(&detail, available)
            },
            Style::default().fg(theme.text.secondary),
        ),
    ]);
    let mut lines = vec![
        header,
        context_bar(stage, theme, area.width, elapsed, motion),
    ];
    let activity = if stage.method == Some(CompactionMethod::Archive)
        && !stage.thumbnails.is_empty()
    {
        format!(
            "{}/{} {}",
            stage.frames_done,
            stage.frames_total,
            thumbnail_line(
                stage,
                &card.thumbnails,
                ascii,
                area.width.saturating_sub(9),
                app.now(),
                motion
            )
        )
    } else if stage.phase == CompactionPhase::Done {
        match (stage.tokens_before, stage.tokens_after) {
            (Some(before), Some(after)) => {
                let current = remaining(before, after, elapsed, motion);
                let percent = reduction_percent(before, after);
                format!(
                    "{} {} {} (-{percent}%)",
                    compact_count(before),
                    if ascii { "->" } else { "→" },
                    compact_count(current)
                )
            }
            (_, Some(after)) => format!("Context retained: {} tokens", compact_count(after)),
            (Some(before), _) => format!("Context compacted from {} tokens", compact_count(before)),
            _ => "Context compacted".to_string(),
        }
    } else {
        let preview = stage
            .message
            .as_deref()
            .unwrap_or_else(|| card.preview.as_deref().unwrap_or_default());
        let label = if stage.windows_total > 0 {
            format!(
                "{} {}/{}",
                phase_label(stage.phase),
                stage.windows_done,
                stage.windows_total
            )
        } else {
            phase_label(stage.phase).to_string()
        };
        let available = usize::from(area.width)
            .saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()) + 3);
        format!(
            "{label} {}{}",
            super::ui_live_turn_status::compaction_preview_tail(preview, available),
            if motion && (elapsed / 400).is_multiple_of(2) {
                "_"
            } else {
                ""
            }
        )
    };
    let jitter = motion
        && stage.method == Some(CompactionMethod::Shake)
        && stage.phase == CompactionPhase::Done
        && elapsed < 250
        && (elapsed / 50).is_multiple_of(2);
    lines.push(Line::from(Span::styled(
        format!("{}{activity}", if jitter { " " } else { "" }),
        Style::default().fg(if stage.phase == CompactionPhase::Failed {
            theme.status.error
        } else if stage.phase == CompactionPhase::Done {
            theme.status.success
        } else {
            theme.text.primary
        }),
    )));
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.surface.canvas)),
        area,
    );
    true
}
