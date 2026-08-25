//! Compaction transcript surface rendering, inspired by Pi's
//! `[compaction]` transcript component.
//!
//! Renders a `[compaction]` badge, a "Compacted from X tokens" line,
//! the summary text (collapsed by default), and read/modified file lists
//! as small bullet lines. Uses `transcript_emphasized_surface` for visual
//! consistency with tool-call surfaces.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::ui_transcript_style::transcript_emphasized_surface;
use super::ui_transcript_surface::transcript_surface_content_width;
use super::ui_transcript_types::{TranscriptCompactionKind, TranscriptCompactionSection};
use crate::theme::Theme;
use crate::ui::ui_chrome::display_width;

const COMPACTION_BADGE: &str = "[compaction]";
const BRANCH_SUMMARY_BADGE: &str = "[branch-summary]";
const DISCLOSURE_COLLAPSED: &str = "\u{25b6} "; // ▶
const DISCLOSURE_EXPANDED: &str = "\u{25bc} "; // ▼
const FILE_BULLET: &str = "  ";
const COMPACTION_PREFIX: &str = "   ";

pub(super) struct ResolvedCompactionContent {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) surface: Color,
}

pub(super) fn resolve_compaction_content(
    compaction: &TranscriptCompactionSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> ResolvedCompactionContent {
    let surface = transcript_emphasized_surface(theme, base_surface);
    let content_width = transcript_surface_content_width(width, false);
    let mut lines = Vec::new();

    // Badge line: [compaction] or [branch-summary]
    let badge = match compaction.kind {
        TranscriptCompactionKind::SessionCompaction => COMPACTION_BADGE,
        TranscriptCompactionKind::BranchSummary => BRANCH_SUMMARY_BADGE,
    };
    let badge_color = match compaction.kind {
        TranscriptCompactionKind::SessionCompaction => theme.text.accent,
        TranscriptCompactionKind::BranchSummary => theme.status.info,
    };

    lines.push(Line::from(vec![
        Span::styled(
            DISCLOSURE_COLLAPSED.to_string(),
            Style::default().fg(theme.text.secondary),
        ),
        Span::styled(
            badge.to_string(),
            Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // "Compacted from X tokens" line (only for SessionCompaction)
    if let Some(tokens_before) = compaction.tokens_before {
        if tokens_before > 0 {
            let token_str = format_token_count(tokens_before);
            let label = match compaction.kind {
                TranscriptCompactionKind::SessionCompaction => {
                    format!("Compacted from {token_str} tokens")
                }
                TranscriptCompactionKind::BranchSummary => {
                    format!("Summarized {token_str} tokens of branch history")
                }
            };
            lines.push(Line::from(vec![Span::styled(
                format!("{COMPACTION_PREFIX}{label}"),
                Style::default().fg(theme.text.secondary),
            )]));
        }
    }

    // Summary text (collapsed: first line only, truncated)
    if !compaction.summary.is_empty() {
        let summary_first_line = compaction
            .summary
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        if !summary_first_line.is_empty() {
            let max_summary_width = usize::from(content_width)
                .saturating_sub(display_width(COMPACTION_PREFIX))
                .saturating_sub(display_width(DISCLOSURE_COLLAPSED));
            let truncated = truncate_to_width(summary_first_line, max_summary_width);
            lines.push(Line::from(vec![
                Span::styled(
                    DISCLOSURE_COLLAPSED.to_string(),
                    Style::default().fg(theme.text.secondary),
                ),
                Span::styled(
                    format!("{COMPACTION_PREFIX}{truncated}"),
                    Style::default().fg(theme.text.primary),
                ),
            ]));
        }
    }

    // File lists as small bullet lines
    append_file_list(
        &mut lines,
        "read:",
        &compaction.read_files,
        theme,
        content_width,
    );
    append_file_list(
        &mut lines,
        "modified:",
        &compaction.modified_files,
        theme,
        content_width,
    );

    ResolvedCompactionContent { surface, lines }
}

fn append_file_list(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    files: &[String],
    theme: &Theme,
    content_width: u16,
) {
    if files.is_empty() {
        return;
    }

    let max_width = usize::from(content_width).saturating_sub(display_width(FILE_BULLET));
    let label_span = Span::styled(
        format!("{FILE_BULLET}{label} "),
        Style::default().fg(theme.text.tertiary),
    );

    let files_text = files.join(", ");
    let truncated = truncate_to_width(&files_text, max_width.saturating_sub(display_width(label)));
    lines.push(Line::from(vec![
        label_span,
        Span::styled(truncated, Style::default().fg(theme.text.secondary)),
    ]));
}

fn format_token_count(count: u32) -> String {
    if count < 1000 {
        return count.to_string();
    }
    if count < 10000 {
        return format!("{:.1}k", f64::from(count) / 1000.0);
    }
    if count < 1000000 {
        return format!("{}k", count / 1000);
    }
    format!("{:.1}M", f64::from(count) / 1000000.0)
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = display_width(text);
    if text_width <= max_width {
        return text.to_string();
    }

    let ellipsis = "\u{2026}"; // …
    let ellipsis_width = display_width(ellipsis);
    if max_width <= ellipsis_width {
        return ellipsis.to_string();
    }

    let target = max_width.saturating_sub(ellipsis_width);
    let mut used = 0usize;
    let mut split_at = text.len();
    for (index, ch) in text.char_indices() {
        let ch_width = display_width(&ch.to_string());
        if used.saturating_add(ch_width) > target {
            split_at = index;
            break;
        }
        used = used.saturating_add(ch_width);
    }

    format!("{}{ellipsis}", &text[..split_at])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ActivityEntry, ActivityStatus, AppState};
    use crate::theme::Theme;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, SessionCompactionEvent,
    };

    #[test]
    fn format_token_count_small() {
        // arrange
        // act
        // assert
        assert_eq!(format_token_count(42), "42");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn format_token_count_thousands() {
        // arrange
        // act
        // assert
        assert_eq!(format_token_count(1500), "1.5k");
        assert_eq!(format_token_count(9999), "10.0k");
    }

    #[test]
    fn format_token_count_large() {
        // arrange
        // act
        // assert
        assert_eq!(format_token_count(50000), "50k");
        assert_eq!(format_token_count(2500000), "2.5M");
    }

    #[test]
    fn truncate_to_width_short_text_unchanged() {
        // arrange
        // act
        // assert
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_long_text_truncated() {
        // arrange
        // act
        // assert
        let result = truncate_to_width("hello world this is long", 10);
        assert!(result.ends_with('\u{2026}'));
        assert!(display_width(&result) <= 10);
    }

    #[test]
    fn truncate_to_width_cjk_text() {
        // arrange
        // act
        // assert
        // CJK characters are double-width
        let result = truncate_to_width("\u{4f60}\u{597d}\u{4e16}\u{754c}", 5);
        assert!(display_width(&result) <= 5);
    }

    #[test]
    fn compaction_surface_contains_badge() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: "Summary of work done".to_string(),
            tokens_before: Some(50000),
            read_files: vec!["src/main.rs".to_string()],
            modified_files: vec!["src/lib.rs".to_string()],
        };
        let surface = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        let badge_line = &surface.lines[0];
        let badge_text = badge_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(badge_text.contains("[compaction]"));
    }

    #[test]
    fn compaction_surface_shows_token_count() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: "Summary".to_string(),
            tokens_before: Some(50000),
            read_files: vec![],
            modified_files: vec![],
        };
        let surface = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        let token_line = &surface.lines[1];
        let token_text = token_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(token_text.contains("50k"));
        assert!(token_text.contains("Compacted from"));
    }

    #[test]
    fn branch_summary_surface_uses_branch_badge() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            kind: TranscriptCompactionKind::BranchSummary,
            summary: "Branch summary".to_string(),
            tokens_before: None,
            read_files: vec![],
            modified_files: vec![],
        };
        let surface = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        let badge_line = &surface.lines[0];
        let badge_text = badge_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(badge_text.contains("[branch-summary]"));
    }

    #[test]
    fn compaction_surface_shows_file_lists() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: "Summary".to_string(),
            tokens_before: Some(1000),
            read_files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
            modified_files: vec!["src/c.rs".to_string()],
        };
        let surface = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        let all_text: String = surface
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(all_text.contains("read:"));
        assert!(all_text.contains("src/a.rs"));
        assert!(all_text.contains("modified:"));
        assert!(all_text.contains("src/c.rs"));
    }

    fn make_test_activity(request_id: &str, seq: u64) -> ActivityEntry {
        ActivityEntry {
            request_id: request_id.to_string(),
            profile_label: "default".to_string(),
            model_id: "test-model".to_string(),
            provider_id: "test-provider".to_string(),
            status: ActivityStatus::Done,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: "assistant reply".to_string(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: seq,
            last_seq: seq,
            first_mono_ms: seq,
            last_mono_ms: seq,
            request_started_mono_ms: None,
            revision: 0,
        }
    }

    fn make_session_compaction_event(seq: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: format!("event-{seq}"),
            seq,
            run_id: "test-run".into(),
            mono_ms: seq * 100,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("test-agent".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::SessionCompaction(SessionCompactionEvent {
                agent_id: "test-agent".to_string(),
                summary:
                    "Compacted context: discussed the auth module and fixed a bug in login flow."
                        .to_string(),
                first_kept_event_seq: 1,
                first_kept_request_id: None,
                first_kept_entry_id: None,
                tokens_before: 50000,
                tokens_after: None,
                summary_usage: None,
                summary_provider_id: None,
                summary_model_id: None,
                read_files: vec!["src/auth.rs".to_string()],
                modified_files: vec!["src/login.rs".to_string()],
                current_intent: None,
                trigger_reason: "threshold".to_string(),
                from_hook: false,
            }),
        }
    }

    #[test]
    fn compaction_event_injected_into_transcript_sections() {
        // arrange
        // act
        // assert
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![make_test_activity("request-1", 1)]);
        app.events = vec![make_session_compaction_event(2)];

        let sections = super::super::ui_transcript_sections::build_transcript_sections(&app);

        assert_eq!(sections.len(), 1);
        let turn = &sections[0];
        assert!(
            turn.assistant_parts
                .iter()
                .any(|part| matches!(part, super::super::TranscriptAssistantPart::Compaction(_))),
            "expected a Compaction part in assistant_parts"
        );
    }

    #[test]
    fn compaction_event_renders_badge_and_summary() {
        // arrange
        // act
        // assert
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![make_test_activity("request-1", 1)]);
        app.events = vec![make_session_compaction_event(2)];

        let theme = Theme::default();
        let sections = super::super::ui_transcript_sections::build_transcript_sections(&app);
        let turn = &sections[0];

        let compaction_part = turn
            .assistant_parts
            .iter()
            .find_map(|part| match part {
                super::super::TranscriptAssistantPart::Compaction(c) => Some(c),
                _ => None,
            })
            .expect("compaction part should exist");

        let surface = resolve_compaction_content(compaction_part, &theme, 80, theme.surface.shell);

        let all_text: String = surface
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref().to_string())
            .collect::<Vec<_>>()
            .join("");

        assert!(
            all_text.contains("[compaction]"),
            "rendered surface should contain [compaction] badge"
        );
        assert!(
            all_text.contains("Compacted from"),
            "rendered surface should contain 'Compacted from' text"
        );
        assert!(
            all_text.contains("50k"),
            "rendered surface should contain token count"
        );
        assert!(
            all_text.contains("auth.rs"),
            "rendered surface should contain read file"
        );
    }
}
