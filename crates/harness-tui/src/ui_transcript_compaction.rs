//! Senpi-style collapsed compaction checkpoint and expandable Markdown summary.
use super::ui_transcript_style::transcript_emphasized_surface;
use super::ui_transcript_surface::transcript_surface_content_width;
use super::ui_transcript_types::{
    TranscriptCompactionKind, TranscriptCompactionSection, TRANSCRIPT_ASSISTANT_BODY_PREFIX,
};
use crate::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

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
    let badge = match compaction.kind {
        TranscriptCompactionKind::SessionCompaction => "[compaction]",
        TranscriptCompactionKind::BranchSummary => "[branch-summary]",
    };
    let mut lines = vec![
        Line::from(Span::styled(
            badge,
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    let action = if compaction.expanded {
        "collapse"
    } else {
        "expand"
    };
    let label = compaction.tokens_before.map_or_else(
        || "Branch summary".to_string(),
        |tokens| format!("Compacted from {} tokens", format_token_count(tokens)),
    );
    lines.extend(
        super::ui_transcript_surface::wrap_surface_spans(
            vec![Span::styled(
                format!("{label} (ctrl+alt+o to {action})"),
                Style::default().fg(theme.text.secondary),
            )],
            usize::from(content_width.saturating_sub(3)),
        )
        .into_iter()
        .map(Line::from),
    );
    if compaction.expanded {
        lines.push(Line::default());
        let summary = crate::text::strip_ansi_escapes(&compaction.summary);
        super::ui_streaming_markdown::append_streaming_rich_text_block(
            &mut lines,
            &summary,
            theme.text.primary,
            "",
            theme,
            content_width.saturating_sub(3),
        );
    }
    for line in &mut lines {
        line.spans
            .insert(0, Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX));
    }
    ResolvedCompactionContent { surface, lines }
}

fn format_token_count(count: u32) -> String {
    let digits = count.to_string();
    let mut formatted = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
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
    fn compaction_surface_contains_badge() {
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            expanded: false,
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
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            expanded: false,
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: "Summary".to_string(),
            tokens_before: Some(50000),
            read_files: vec![],
            modified_files: vec![],
        };
        let surface = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        let token_line = &surface.lines[2];
        let token_text = token_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(token_text.contains("50,000"));
        assert!(token_text.contains("Compacted from"));
    }

    #[test]
    fn branch_summary_surface_uses_branch_badge() {
        let theme = Theme::default();
        let compaction = TranscriptCompactionSection {
            expanded: false,
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
    fn compaction_summary_is_collapsed_until_requested_and_sanitizes_controls() {
        let theme = Theme::default();
        let mut compaction = TranscriptCompactionSection {
            expanded: false,
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: "## Retained work\n\n\u{1b}[31mSafe summary\u{1b}[0m\n\n- Continue testing"
                .to_string(),
            tokens_before: Some(128_000),
            read_files: vec![],
            modified_files: vec![],
        };
        let collapsed = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        assert_eq!(collapsed.lines.len(), 3);
        assert!(!collapsed
            .lines
            .iter()
            .any(|line| line.to_string().contains("Safe summary")));
        compaction.expanded = true;
        let expanded = resolve_compaction_content(&compaction, &theme, 80, theme.surface.shell);
        assert!(expanded
            .lines
            .iter()
            .any(|line| line.to_string().contains("Safe summary")));
        assert!(expanded
            .lines
            .iter()
            .all(|line| !line.to_string().contains('\u{1b}')));
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
                task_intent: None,
                current_intent: None,
                trigger_reason: "threshold".to_string(),
                from_hook: false,
            }),
        }
    }

    #[test]
    fn compaction_event_injected_into_transcript_sections() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![make_test_activity("request-1", 1)]);
        app.events = vec![make_session_compaction_event(2)].into();

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
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![make_test_activity("request-1", 1)]);
        app.events = vec![make_session_compaction_event(2)].into();

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
            all_text.contains("50,000"),
            "rendered surface should contain token count"
        );
        assert!(
            !all_text.contains("auth.rs"),
            "collapsed surface hides summary details"
        );
        for width in [100, 48] {
            for expanded in [false, true] {
                if app.transcript_view.compaction_details_expanded != expanded {
                    app.handle_key(crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char('o'),
                        crossterm::event::KeyModifiers::CONTROL
                            | crossterm::event::KeyModifiers::ALT,
                    ));
                }
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 32))
                        .expect("test terminal");
                terminal
                    .draw(|frame| crate::ui::render_app(frame, &app))
                    .expect("draw completed compaction");
                let buffer = terminal.backend().buffer();
                let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
                capture_compaction_frame(buffer, width, expanded);
                assert!(text.contains("[compaction]"));
                assert_eq!(text.contains("discussed"), expanded);
                assert!(
                    text.contains("ctrl+alt+o"),
                    "{width} columns, expanded={expanded}: {text}"
                );
                assert!(text.contains(if expanded { "collapse)" } else { "expand)" }));
            }
        }
    }
    fn capture_compaction_frame(buffer: &ratatui::buffer::Buffer, width: u16, expanded: bool) {
        if let Ok(dir) = std::env::var("HARNESS_COMPACTION_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).expect("capture directory");
            let cells: Vec<_> = (0..32).flat_map(|y| (0..width).map(move |x| {
                        let cell = &buffer[(x,y)];
                        serde_json::json!({"x": x, "y": y, "text": cell.symbol(), "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg)})
                    })).collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("completed-{expanded}-{width}.json")),
                serde_json::to_vec(
                    &serde_json::json!({"width": width, "height": 32, "cells": cells}),
                )
                .expect("capture JSON"),
            )
            .expect("write capture");
        }
    }
}
