use super::super::ui_transcript_sections::prepare_transcript_sections;
use super::super::ui_transcript_test_helpers::transcript_section_model_test_activity;
use super::super::ui_transcript_types::{
    TranscriptAssistantPart, TranscriptCompactionKind, TranscriptCompactionSection,
    TranscriptNoticeSection, TranscriptTurnSection,
};
use super::*;
use crate::app::notices::TranscriptNoticeKind;
use crate::app::{ActivityStatus, AppState};
use crate::theme::{ColorLevel, Theme};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, SessionCompactionEvent,
};
use std::sync::Arc;

fn section(
    method: Option<CompactionMethod>,
    before: u32,
    after: Option<u32>,
) -> TranscriptCompactionSection {
    TranscriptCompactionSection {
        seq: 3,
        expanded: false,
        kind: TranscriptCompactionKind::SessionCompaction,
        summary: "Retained decisions".into(),
        short_summary: None,
        at_ms: None,
        superseded: false,
        rollover: false,
        tokens_before: Some(before),
        tokens_after: after,
        method,
        warning: None,
        archive: None,
        archive_frames: 0,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        expand_hint: "Ctrl+O".into(),
    }
}

fn text(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(ToString::to_string).collect()
}

fn theme(ascii: bool) -> Theme {
    Theme::default()
        .with_glyph_mode(if ascii {
            GlyphMode::Ascii
        } else {
            GlyphMode::Preferred
        })
        .for_color_level(ColorLevel::Ansi256)
}

fn resolve(section: &TranscriptCompactionSection, width: u16, ascii: bool) -> Vec<String> {
    let theme = theme(ascii);
    text(&resolve_compaction_content(section, &theme, width, theme.surface.canvas).lines)
}

#[test]
fn divider_is_three_rows_spanning_the_viewport_with_the_specified_label() {
    // (method, before, after, label fragments, absent fragments)
    let cases: [(Option<CompactionMethod>, u32, Option<u32>, &[&str], &[&str]); 5] = [
        (None, 84_000, None, &["compacted"], &["→", "%"]),
        (
            Some(CompactionMethod::Remote),
            256_000,
            Some(20_000),
            &["remote-compacted", "256K→20K"],
            &["%"],
        ),
        (
            Some(CompactionMethod::Handoff),
            84_000,
            None,
            &["handed-off"],
            &["→"],
        ),
        (
            Some(CompactionMethod::Archive),
            0,
            Some(48_573),
            &["archived"],
            &["→", "from 0"],
        ),
        (
            Some(CompactionMethod::Soft),
            10_500,
            Some(6_900),
            &["soft-compacted", "11K→6.9K"],
            &[],
        ),
    ];
    for (method, before, after, present, absent) in cases {
        for ascii in [false, true] {
            let rows = resolve(&section(method, before, after), 80, ascii);
            assert_eq!(rows.len(), 3);
            assert!(rows[0].is_empty() && rows[2].is_empty());
            assert_eq!(rows[1].width(), 80, "{}", rows[1]);
            assert!(rows[1].contains("Ctrl+O"));
            assert!(!rows.concat().contains("Retained decisions"));
            for fragment in present {
                let fragment = if ascii {
                    fragment.replace('→', "->")
                } else {
                    (*fragment).to_string()
                };
                assert!(rows[1].contains(&fragment), "{} / {fragment}", rows[1]);
            }
            for fragment in absent {
                let fragment = if ascii {
                    fragment.replace('→', "->")
                } else {
                    (*fragment).to_string()
                };
                assert!(!rows[1].contains(&fragment), "{} / {fragment}", rows[1]);
            }
        }
    }
}

#[test]
fn narrow_viewport_keeps_only_the_bare_label() {
    let rows = resolve(&section(None, 84_000, None), 10, false);
    assert_eq!(rows.len(), 3);
    assert!(rows[1].contains("compacted"));
    assert!(!rows[1].contains('─') && !rows[1].contains("Ctrl+O"));
}

#[test]
fn warning_badge_needs_a_non_empty_warning() {
    for (warning, badge) in [
        (None, false),
        (Some(""), false),
        (Some("freed little"), true),
    ] {
        let mut card = section(None, 84_000, None);
        card.warning = warning.map(str::to_owned);
        assert_eq!(
            resolve(&card, 80, false)[1].contains('⚠'),
            badge,
            "{warning:?}"
        );
    }
}

#[test]
fn compact_counts_round_half_up() {
    for (tokens, expected) in [
        (999, "999"),
        (1_000, "1K"),
        (6_900, "6.9K"),
        (10_500, "11K"),
        (48_573, "49K"),
        (1_500_000, "1.5M"),
        (12_500_000, "13M"),
    ] {
        assert_eq!(
            crate::ui::ui_compaction_motion::compact_count(tokens),
            expected
        );
    }
}

#[test]
fn expanded_detail_follows_the_specified_order_and_branches() {
    // (before, after, heading)
    for (before, after, heading) in [
        (
            84_000,
            Some(20_000),
            "Compacted from 84,000 to 20,000 tokens",
        ),
        (84_000, None, "Compacted from 84,000 tokens"),
        (0, Some(48_573), "Compacted to 48,573 tokens"),
        (0, None, "Compacted context"),
    ] {
        let mut card = section(None, before, after);
        card.expanded = true;
        card.warning = Some("freed little".into());
        card.archive_frames = 1;
        card.read_files = vec!["src/a.rs".into()];
        let rows = resolve(&card, 80, false);
        let detail = rows[3..].iter().map(|row| row.trim()).collect::<Vec<_>>();
        let at = |needle: &str| detail.iter().position(|row| row.contains(needle));
        assert!(!rows.concat().contains("from 0"));
        let (heading_at, warning_at, summary_at, files_at, caption_at) = (
            at(heading),
            at("Warning: freed little"),
            at("Retained decisions"),
            at("a.rs (Read)"),
            at("1 archive frame attached"),
        );
        assert!(
            heading_at < warning_at && warning_at < summary_at,
            "{detail:?}"
        );
        assert!(summary_at < files_at && files_at < caption_at, "{detail:?}");
        // blank row between warning, summary and caption blocks
        assert!(detail[warning_at.unwrap_or(0) - 1].is_empty());
        assert!(detail[caption_at.unwrap_or(0) - 1].is_empty());
    }
}

#[test]
fn rollover_and_superseded_cards_expand_without_summary_or_file_rows() {
    let mut rollover = section(None, 84_000, Some(20_000));
    rollover.expanded = true;
    rollover.rollover = true;
    rollover.read_files = vec!["src/a.rs".into()];
    let mut superseded = rollover.clone();
    superseded.rollover = false;
    superseded.superseded = true;
    superseded.summary = "[Superseded compaction summary elided after a newer compaction]".into();
    let rollover_rows = resolve(&rollover, 80, false);
    assert!(rollover_rows.iter().any(|row| row.contains("rolled over")));
    assert!(rollover_rows
        .iter()
        .all(|row| !row.contains("Retained decisions")));
    // The divider keeps the generic label.
    assert!(rollover_rows[1].contains("compacted"));
    let superseded_rows = resolve(&superseded, 80, false);
    assert!(superseded_rows
        .iter()
        .any(|row| row.contains("Superseded compaction summary elided")));
    for rows in [rollover_rows, superseded_rows] {
        assert!(rows.iter().all(|row| !row.contains("src/a.rs")));
    }
}

fn compaction_event(seq: u64, agent: &str) -> EventEnvelopeV1 {
    compaction_event_before(seq, agent, 50_000)
}

fn compaction_event_before(seq: u64, agent: &str, before: u32) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: 1,
        event_id: format!("event-{seq}"),
        seq,
        run_id: "run".into(),
        mono_ms: seq * 100,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some(agent.to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::SessionCompaction(SessionCompactionEvent {
            agent_id: agent.to_string(),
            summary: format!("summary {seq}"),
            first_kept_event_seq: 1,
            first_kept_request_id: None,
            first_kept_entry_id: None,
            tokens_before: before,
            tokens_after: Some(20_000),
            summary_usage: None,
            summary_provider_id: None,
            summary_model_id: None,
            read_files: Vec::new(),
            modified_files: Vec::new(),
            task_intent: None,
            current_intent: None,
            trigger_reason: "threshold".into(),
            from_hook: false,
            short_summary: None,
            method: None,
            warning: None,
            provider_replay_through_seq: None,
            native: None,
            archive: None,
            details: Default::default(),
            operation_id: Default::default(),
            preserve_data: Default::default(),
            supersedes_seq: Default::default(),
        }),
    }
}

fn spawned_event(seq: u64, child: &str) -> EventEnvelopeV1 {
    let mut event = compaction_event(seq, child);
    event.payload = EventV1::AgentSpawned(AgentSpawnedEvent {
        agent_id: child.into(),
        profile: "worker".into(),
        parent_agent_id: Some("root".into()),
    });
    event
}

fn app_with(spans: &[(u64, u64)], events: Vec<EventEnvelopeV1>, collapse: bool) -> AppState {
    let mut app = AppState::default();
    app.set_collapsed_history_config_for_test(std::path::PathBuf::new(), collapse);
    app.activities = spans
        .iter()
        .enumerate()
        .map(|(index, (first, last))| {
            let mut activity = transcript_section_model_test_activity(
                &format!("request-{index}"),
                ActivityStatus::Done,
                "reply",
            );
            activity.first_seq = *first;
            activity.last_seq = *last;
            activity
        })
        .collect();
    app.inspection_events = Some(events);
    app
}

fn compaction_parts(sections: &[Arc<TranscriptTurnSection>]) -> Vec<&TranscriptCompactionSection> {
    sections
        .iter()
        .flat_map(|section| &section.assistant_parts)
        .filter_map(|part| match part {
            TranscriptAssistantPart::Compaction(card) => Some(card),
            _ => None,
        })
        .collect()
}

fn layout(sections: &[Arc<TranscriptTurnSection>]) -> Vec<String> {
    sections
        .iter()
        .map(|section| {
            if section.user_message.is_none() && section.request_id.starts_with("compaction") {
                "card".to_string()
            } else {
                section.request_id.clone()
            }
        })
        .collect()
}

#[test]
fn collapsed_transcript_hides_originals_before_the_latest_divider() {
    let events = vec![compaction_event(3, "root")];
    let spans = [(1, 2), (5, 6)];
    let collapsed = prepare_transcript_sections(&app_with(&spans, events.clone(), true), &[], 0);
    assert_eq!(layout(&collapsed), ["card", "request-1"]);
    let full = prepare_transcript_sections(&app_with(&spans, events, false), &[], 0);
    assert_eq!(layout(&full), ["request-0", "card", "request-1"]);
}

#[test]
fn divider_survives_a_transcript_without_turns() {
    let app = app_with(&[], vec![compaction_event(3, "root")], true);
    let sections = prepare_transcript_sections(&app, &[], 0);
    assert_eq!(compaction_parts(&sections).len(), 1);
}

#[test]
fn a_child_compaction_does_not_supersede_the_root_card() {
    let events = vec![
        spawned_event(2, "child"),
        compaction_event(3, "root"),
        compaction_event(4, "child"),
    ];
    let app = app_with(&[(1, 1), (6, 6)], events, false);
    let sections = prepare_transcript_sections(&app, &[], 0);
    let cards = compaction_parts(&sections);
    assert_eq!(cards.len(), 1);
    assert!(!cards[0].superseded);
    assert_eq!(cards[0].summary, "summary 3");
}

#[test]
fn older_cards_keep_their_amounts_but_show_the_superseded_placeholder() {
    let events = vec![
        compaction_event(3, "root"),
        compaction_event_before(5, "root", 60_000),
    ];
    let app = app_with(&[(1, 1), (7, 7)], events, false);
    let sections = prepare_transcript_sections(&app, &[], 0);
    let cards = compaction_parts(&sections);
    assert_eq!(cards.len(), 2);
    assert!(cards[0].superseded && !cards[1].superseded);
    assert_eq!(cards[0].tokens_before, Some(50_000));
    assert_eq!(cards[1].tokens_before, Some(60_000));
    assert!(cards[0].summary.starts_with("[Superseded"));
}

#[test]
fn a_card_inside_a_turn_is_placed_before_the_parts_that_follow_it() {
    let app = app_with(&[(1, 9)], vec![compaction_event(4, "root")], false);
    let sections = prepare_transcript_sections(&app, &[], 0);
    assert_eq!(sections.len(), 1);
    assert_eq!(compaction_parts(&sections).len(), 1);
}

#[test]
fn expansion_is_per_card_and_unchanged_sections_are_reused() {
    let events = vec![
        compaction_event(3, "root"),
        compaction_event_before(5, "root", 60_000),
    ];
    let mut app = app_with(&[(1, 1), (7, 7)], events, false);
    let before = prepare_transcript_sections(&app, &[], 0);
    let again = prepare_transcript_sections(&app, &before, usize::MAX);
    assert!(before.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)));

    app.toggle_compaction_card(5);
    let after = prepare_transcript_sections(&app, &[], 0);
    let expanded = compaction_parts(&after)
        .iter()
        .map(|card| card.expanded)
        .collect::<Vec<_>>();
    assert_eq!(expanded, [false, true]);
}

#[test]
fn notices_follow_the_card_journaled_at_the_same_position() {
    let mut app = app_with(&[(1, 1)], vec![compaction_event(3, "root")], false);
    app.notices
        .push(3, TranscriptNoticeKind::Warning, "careful".into());
    let sections = prepare_transcript_sections(&app, &[], 0);
    let marker = &sections[1];
    assert!(matches!(
        marker.assistant_parts.as_slice(),
        [TranscriptAssistantPart::Compaction(_), TranscriptAssistantPart::Notice(TranscriptNoticeSection { text, .. })]
            if text == "careful"
    ));
}

#[test]
fn rebinding_the_expand_key_changes_the_divider_hint() {
    let mut app = AppState::default();
    assert_eq!(app.compaction_expand_hint(), "Ctrl+Alt+O");
    app.keymap
        .apply_overrides(&std::collections::BTreeMap::from([(
            "toggle_compaction_details".to_string(),
            "ctrl+e".to_string(),
        )]));
    assert_eq!(app.compaction_expand_hint(), "Ctrl+E");
}

#[test]
fn rendered_divider_spans_the_transcript_viewport() {
    use ratatui::{backend::TestBackend, Terminal};
    let app = app_with(&[(1, 2), (5, 6)], vec![compaction_event(3, "root")], true);
    for width in [80_u16, 120] {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).expect("terminal");
        terminal
            .draw(|frame| crate::ui::render_app(frame, &app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..30)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let divider = rows
            .iter()
            .find(|row| row.contains("compacted"))
            .expect("divider");
        // Gutter, rule to rule, gutter: the divider fills the row between the outer spaces.
        let gutter = divider.chars().take_while(|c| *c == ' ').count();
        let trailing = divider.chars().rev().take_while(|c| *c == ' ').count();
        assert_eq!(gutter, trailing, "{width}: {divider:?}");
        assert!(gutter <= 2, "{width}: {divider:?}");
    }
}
