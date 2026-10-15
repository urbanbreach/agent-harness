//! Puts compaction cards and notices at their journal position among the turn sections.
//!
//! An item that falls inside a turn's span becomes a part of that turn, ahead of the first
//! part journaled after it. Any other item goes into a marker section of its own between
//! two turns, or alone when there are no turns.
use super::super::ui_transcript_types::{
    TranscriptAssistantPartSourceId, TranscriptTurnHeader, TranscriptTurnSection,
};
use super::plan::TimelineItem;
use crate::app::{ActivityEntry, ActivityStatus, AppState};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(in crate::ui::ui_transcript) fn merge_timeline(
    app: &AppState,
    activities: &[(usize, &ActivityEntry)],
    mut sections: Vec<Arc<TranscriptTurnSection>>,
    fresh: &[bool],
    items: Vec<TimelineItem>,
    previous: &[Arc<TranscriptTurnSection>],
) -> Vec<Arc<TranscriptTurnSection>> {
    let mut gaps: BTreeMap<usize, Vec<TimelineItem>> = BTreeMap::new();
    for item in items {
        let host = activities.iter().rposition(|(_, activity)| {
            activity.first_seq <= item.seq && item.seq < activity.last_seq
        });
        match host {
            // A reused turn already holds the items that were placed into it.
            Some(index) if !fresh[index] => {}
            Some(index) => insert_part(Arc::make_mut(&mut sections[index]), item),
            None => {
                let gap = activities
                    .iter()
                    .filter(|(_, activity)| activity.last_seq <= item.seq)
                    .count();
                gaps.entry(gap).or_default().push(item);
            }
        }
    }
    if gaps.is_empty() {
        return sections;
    }
    let marker = |items: Vec<TimelineItem>| {
        let section = marker_section(app, items);
        previous
            .iter()
            .find(|previous| {
                previous.activity_first_seq == section.activity_first_seq
                    && previous.request_id == section.request_id
                    && ***previous == section
            })
            .map_or_else(|| Arc::new(section), Arc::clone)
    };
    let mut merged = Vec::with_capacity(sections.len() + gaps.len());
    for (index, section) in sections.into_iter().enumerate() {
        if let Some(items) = gaps.remove(&index) {
            merged.push(marker(items));
        }
        merged.push(section);
    }
    merged.extend(gaps.into_values().map(marker));
    merged
}

fn insert_part(turn: &mut TranscriptTurnSection, item: TimelineItem) {
    let at = if turn.assistant_part_source_ids.len() == turn.assistant_parts.len() {
        turn.assistant_part_source_ids
            .iter()
            .position(|id| id.0 > item.seq)
            .unwrap_or(turn.assistant_parts.len())
    } else {
        turn.assistant_parts.len()
    };
    for index in &mut turn.raw_parts {
        if *index >= at {
            *index += 1;
        }
    }
    turn.assistant_parts.insert(at, item.part);
    if turn.assistant_part_source_ids.len() + 1 == turn.assistant_parts.len() {
        turn.assistant_part_source_ids
            .insert(at, TranscriptAssistantPartSourceId(item.seq));
    }
}

fn marker_section(app: &AppState, items: Vec<TimelineItem>) -> TranscriptTurnSection {
    let first_seq = items.first().map_or(0, |item| item.seq);
    let (assistant_parts, assistant_part_source_ids) = items
        .into_iter()
        .map(|item| (item.part, TranscriptAssistantPartSourceId(item.seq)))
        .unzip();
    TranscriptTurnSection {
        raw_parts: Vec::new(),
        child_view: app.current_subagent_session_present(),
        activity_first_seq: first_seq,
        request_id: format!("compaction-marker:{first_seq}"),
        user_message: None,
        show_footer: false,
        footer_timestamp: None,
        animation_phase: 0,
        motion_enabled: false,
        reasoning_expanded: false,
        header: TranscriptTurnHeader {
            status: ActivityStatus::Done,
            is_selected: false,
            is_hovered: false,
            provider_request_open: false,
            profile_label: String::new(),
            model_id: String::new(),
            duration_ms: None,
            thinking_duration_ms: None,
            responding_duration_ms: None,
            total_tokens: None,
            retry: None,
            retry_elapsed_ms: None,
        },
        assistant_parts,
        assistant_part_source_ids,
    }
}
