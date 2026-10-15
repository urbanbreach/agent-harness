//! Compaction divider and its lazily built detail box.
//!
//! The divider is three rows (blank, rule row, blank) that span the viewport. Expanding a
//! card adds a padded background box under it; nothing of the box is built while the card
//! is collapsed.
#[path = "ui_transcript_compaction/blocks.rs"]
mod blocks;
#[path = "ui_transcript_compaction/detail.rs"]
mod detail;
#[path = "ui_transcript_compaction/notice.rs"]
mod notice;
#[path = "ui_transcript_compaction/placement.rs"]
pub(super) mod placement;
#[path = "ui_transcript_compaction/plan.rs"]
pub(super) mod plan;
#[cfg(test)]
#[path = "ui_transcript_compaction/tests.rs"]
mod tests;

use super::ui_transcript_interaction::{
    full_width_interaction_row, TranscriptInteractionRow, TranscriptMouseTarget,
};
use super::ui_transcript_style::transcript_emphasized_surface;
use super::ui_transcript_types::{
    TranscriptCompactionKind, TranscriptCompactionSection, TranscriptNoticeSection,
};
use crate::theme::{GlyphMode, Theme};
use harness_core::event::CompactionMethod;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

pub(super) struct ResolvedCompactionContent {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) surface: Color,
    pub(super) interaction_rows: Vec<Option<TranscriptInteractionRow>>,
}

/// Name shown in the divider label. Notes rollovers have no label of their own.
pub(super) fn method_name(
    kind: TranscriptCompactionKind,
    method: Option<CompactionMethod>,
) -> &'static str {
    if kind == TranscriptCompactionKind::BranchSummary {
        return "branch summary";
    }
    match method {
        Some(CompactionMethod::Remote) => "remote-compacted",
        Some(CompactionMethod::Soft) => "soft-compacted",
        Some(CompactionMethod::Handoff) => "handed-off",
        Some(CompactionMethod::Archive) => "archived",
        Some(CompactionMethod::Shake) => "shaken",
        None => "compacted",
    }
}

/// `before→after`, only when the amount after is known and the amount before is positive.
fn amount(compaction: &TranscriptCompactionSection, ascii: bool) -> Option<String> {
    let before = compaction.tokens_before.filter(|before| *before > 0)?;
    let after = compaction.tokens_after?;
    Some(format!(
        "{}{}{}",
        crate::ui::ui_compaction_motion::compact_count(before),
        if ascii { "->" } else { "→" },
        crate::ui::ui_compaction_motion::compact_count(after),
    ))
}

fn divider_line(
    compaction: &TranscriptCompactionSection,
    theme: &Theme,
    width: u16,
    ascii: bool,
) -> Line<'static> {
    let glyph = crate::ui::ui_compaction_motion::method_glyph(compaction.method, ascii);
    let dot = if ascii { "-" } else { "·" };
    let mut label = format!(
        "{glyph} {}",
        method_name(compaction.kind, compaction.method)
    );
    if let Some(amount) = amount(compaction, ascii) {
        label.push_str(&format!(" {dot} {amount}"));
    }
    let badge = compaction
        .warning
        .as_deref()
        .is_some_and(|warning| !warning.is_empty())
        .then_some(if ascii { " [!]" } else { " ⚠" });
    let hint = format!("{dot} {}", compaction.expand_hint);
    let label_width = label.width() + badge.map_or(0, UnicodeWidthStr::width);
    let remaining = usize::from(width.max(1)).saturating_sub(label_width + 1 + hint.width() + 2);
    let label_style = Style::default().fg(theme.text.secondary);
    let mut spans = Vec::new();
    let rule_style = Style::default().fg(theme.text.tertiary);
    let compose = remaining >= 4;
    if compose {
        let rule = if ascii { "-" } else { "─" };
        spans.push(Span::styled(rule.repeat(remaining / 2) + " ", rule_style));
    }
    spans.push(Span::styled(label, label_style));
    if let Some(badge) = badge {
        spans.push(Span::styled(
            badge,
            Style::default().fg(theme.status.warning),
        ));
    }
    if compose {
        let rule = if ascii { "-" } else { "─" };
        spans.push(Span::styled(format!(" {hint} "), rule_style));
        spans.push(Span::styled(
            rule.repeat(remaining - remaining / 2),
            rule_style,
        ));
    }
    Line::from(spans)
}

/// Divider row of a pass that just finished while its amount is still easing from
/// `before` toward the final amount; it shares the composition of the settled divider.
pub(in crate::ui) fn settling_divider(
    method: Option<CompactionMethod>,
    (before, current): (u32, u32),
    warning: bool,
    expand_hint: &str,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let section = TranscriptCompactionSection {
        seq: 0,
        expanded: false,
        kind: TranscriptCompactionKind::SessionCompaction,
        summary: String::new(),
        short_summary: None,
        at_ms: None,
        superseded: false,
        rollover: false,
        tokens_before: Some(before),
        tokens_after: Some(current),
        method,
        warning: warning.then(|| "warning".to_owned()),
        archive: None,
        archive_frames: 0,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        expand_hint: expand_hint.to_owned(),
    };
    divider_line(
        &section,
        theme,
        width,
        theme.glyph_mode() == GlyphMode::Ascii,
    )
}

pub(super) fn resolve_compaction_content(
    compaction: &TranscriptCompactionSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> ResolvedCompactionContent {
    let ascii = theme.glyph_mode() == GlyphMode::Ascii;
    let mut lines = vec![
        Line::default(),
        divider_line(compaction, theme, width, ascii),
        Line::default(),
    ];
    let mut interaction_rows = vec![
        None,
        Some(full_width_interaction_row(
            TranscriptMouseTarget::Compaction {
                seq: compaction.seq,
            },
        )),
        None,
    ];
    if compaction.expanded {
        let surface = transcript_emphasized_surface(theme, base_surface);
        let boxed = detail::boxed_detail(compaction, theme, width.max(1), surface);
        interaction_rows.resize(lines.len() + boxed.len(), None);
        lines.extend(boxed);
    }
    ResolvedCompactionContent {
        lines,
        surface: base_surface,
        interaction_rows,
    }
}

pub(super) fn resolve_notice_content(
    notice: &TranscriptNoticeSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> ResolvedCompactionContent {
    let lines = notice::notice_lines(notice, theme, width.max(1));
    ResolvedCompactionContent {
        interaction_rows: vec![None; lines.len()],
        lines,
        surface: base_surface,
    }
}
