use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::transcript_cache::TranscriptRenderCache;
use super::transcript_viewport::MeasuredTranscriptViewport;
use super::{AppState, TranscriptScrollbarDragState};
use crate::transcript_scroll::PageFlipState;
use crate::ui::{TranscriptContentAnchor, TranscriptMouseTarget, TranscriptSelection};

pub(crate) const TOOL_FINISH_FLASH_DURATION: Duration = Duration::from_millis(400);

pub(super) const fn active_turn_motion_demand(
    has_active_tool: bool,
    has_running_tool: bool,
    running_tool_visible: bool,
) -> bool {
    if has_active_tool {
        has_running_tool && running_tool_visible
    } else {
        true
    }
}

#[derive(Debug, Default)]
pub(crate) struct ToolMotionTracker {
    seen_terminal: BTreeSet<String>,
    running_since: BTreeMap<String, Instant>,
    finish_flashes: BTreeMap<String, Instant>,
}

impl ToolMotionTracker {
    pub(crate) fn sync_running_ids(
        &mut self,
        running_ids: impl IntoIterator<Item = String>,
        now: Instant,
    ) {
        let running = running_ids.into_iter().collect::<BTreeSet<_>>();
        self.running_since
            .retain(|tool_call_id, _| running.contains(tool_call_id));
        for tool_call_id in running {
            self.running_since.entry(tool_call_id).or_insert(now);
        }
    }

    pub(crate) fn sync_terminal_ids(
        &mut self,
        terminal_ids: impl IntoIterator<Item = String>,
        animate_new_terminal: bool,
        now: Instant,
    ) {
        for tool_call_id in terminal_ids {
            self.running_since.remove(&tool_call_id);
            if self.seen_terminal.insert(tool_call_id.clone()) && animate_new_terminal {
                self.finish_flashes.insert(tool_call_id, now);
            }
        }
    }

    pub(crate) fn advance(&mut self, now: Instant, motion_enabled: bool) -> bool {
        if !motion_enabled {
            let changed = !self.finish_flashes.is_empty();
            self.finish_flashes.clear();
            return changed;
        }
        let previous_len = self.finish_flashes.len();
        self.finish_flashes.retain(|_, started_at| {
            now.saturating_duration_since(*started_at) < TOOL_FINISH_FLASH_DURATION
        });
        self.finish_flashes.len() != previous_len
    }

    pub(crate) fn finish_flash_elapsed(
        &self,
        tool_call_id: &str,
        now: Instant,
    ) -> Option<Duration> {
        self.finish_flashes
            .get(tool_call_id)
            .map(|started_at| now.saturating_duration_since(*started_at))
    }

    pub(crate) fn running_elapsed(&self, tool_call_id: &str, now: Instant) -> Duration {
        self.running_since
            .get(tool_call_id)
            .map_or(Duration::ZERO, |started_at| {
                now.saturating_duration_since(*started_at)
            })
    }

    pub(crate) fn has_finish_flash(&self) -> bool {
        !self.finish_flashes.is_empty()
    }

    pub(crate) fn clear_finish_flashes(&mut self) {
        self.finish_flashes.clear();
    }

    pub(crate) fn earliest_finish_flash_remaining(
        &self,
        now: Instant,
        duration: Duration,
    ) -> Option<Duration> {
        self.finish_flashes
            .values()
            .map(|started_at| duration.saturating_sub(now.saturating_duration_since(*started_at)))
            .min()
    }
}

#[derive(Debug)]
pub(crate) struct TranscriptViewState {
    pub(crate) transcript_scroll: usize,
    pub(crate) follow_mode: bool,
    pub(crate) transcript_scrollbar_visible: bool,
    pub(crate) transcript_scrollbar_drag: Option<TranscriptScrollbarDragState>,
    pub(crate) transcript_selection: Option<TranscriptSelection>,
    pub(crate) transcript_selection_anchors:
        Cell<Option<(TranscriptContentAnchor, TranscriptContentAnchor)>>,
    pub(crate) transcript_selection_dragging: bool,
    pub(crate) transcript_click_activated_on_down: bool,
    pub(crate) selected_diff_hunk_row: Option<usize>,
    pub(crate) hovered_transcript_target: Option<TranscriptMouseTarget>,
    pub(crate) show_transcript_thinking: bool,
    pub(crate) show_transcript_timestamps: bool,
    pub(crate) show_tool_details: bool,
    pub(crate) show_generic_tool_output: bool,
    pub(crate) stacked_transcript_diffs: bool,
    pub(crate) expanded_reasoning_requests: BTreeSet<String>,
    pub(crate) expanded_tool_outputs: BTreeSet<String>,
    pub(crate) expanded_patch_file_outputs: BTreeSet<String>,
    pub(crate) transcript_cache: TranscriptRenderCache,
    pub(crate) transcript_animation_phase: usize,
    pub(crate) last_transcript_max_scroll: Cell<usize>,
    pub(crate) last_transcript_viewport_height: Cell<usize>,
    pub(crate) measured_viewport: Cell<MeasuredTranscriptViewport>,
    pub(crate) legacy_scroll_snapshot: Cell<(bool, usize)>,
    pub(crate) measured_anchor: Cell<Option<TranscriptContentAnchor>>,
    pub(crate) selected_activity_index: usize,
    pub(crate) page_flip: Cell<PageFlipState>,
    pub(crate) tool_motion: ToolMotionTracker,
    pub(crate) visible_running_tool_motion: Cell<bool>,
}

impl Default for TranscriptViewState {
    fn default() -> Self {
        Self {
            transcript_scroll: 0,
            follow_mode: true,
            transcript_scrollbar_visible: true,
            transcript_scrollbar_drag: None,
            transcript_selection: None,
            transcript_selection_anchors: Cell::new(None),
            transcript_selection_dragging: false,
            transcript_click_activated_on_down: false,
            selected_diff_hunk_row: None,
            hovered_transcript_target: None,
            show_transcript_thinking: true,
            show_transcript_timestamps: false,
            show_tool_details: true,
            show_generic_tool_output: false,
            stacked_transcript_diffs: false,
            expanded_reasoning_requests: BTreeSet::new(),
            expanded_tool_outputs: BTreeSet::new(),
            expanded_patch_file_outputs: BTreeSet::new(),
            transcript_cache: TranscriptRenderCache::default(),
            transcript_animation_phase: 0,
            last_transcript_max_scroll: Cell::new(0),
            last_transcript_viewport_height: Cell::new(0),
            measured_viewport: Cell::new(MeasuredTranscriptViewport::following(0)),
            legacy_scroll_snapshot: Cell::new((true, 0)),
            measured_anchor: Cell::new(None),
            selected_activity_index: 0,
            page_flip: Cell::new(PageFlipState::Idle),
            tool_motion: ToolMotionTracker::default(),
            visible_running_tool_motion: Cell::new(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{active_turn_motion_demand, ToolMotionTracker};

    #[test]
    fn active_turn_motion_demand_requires_a_visible_running_tool() {
        assert!(active_turn_motion_demand(true, true, true));
        assert!(!active_turn_motion_demand(true, true, false));
        assert!(!active_turn_motion_demand(true, false, true));
        assert!(active_turn_motion_demand(false, false, false));
    }

    #[test]
    fn reduced_motion_finish_flash_settles_after_one_transition_tick() {
        let mut tracker = ToolMotionTracker::default();
        let now = std::time::Instant::now();
        tracker.sync_terminal_ids(["tool-finished".to_string()], true, now);
        assert!(tracker.has_finish_flash());

        tracker.advance(now, false);

        assert!(!tracker.has_finish_flash());
    }
}
