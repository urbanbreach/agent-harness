use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::Duration;

use super::transcript_cache::TranscriptRenderCache;
use super::transcript_viewport::MeasuredTranscriptViewport;
use super::{AppState, TranscriptScrollbarDragState};
use crate::transcript_scroll::PageFlipState;
use crate::ui::{TranscriptContentAnchor, TranscriptMouseTarget, TranscriptSelection};

const ACTIVE_ANIMATION_TICK_INTERVAL: Duration = Duration::from_millis(1_000 / 30);
const STARTUP_ANIMATION_TICK_INTERVAL: Duration = Duration::from_millis(1_000 / 12);

/// The cadence requested by the currently visible TUI state.
///
/// `None` intentionally leaves no timer armed. `Slow` is reserved for the
/// welcome shimmer; turn status and state-expiry work use the 30 Hz root tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimationTickDemand {
    None,
    Slow,
    Fast,
}

impl AnimationTickDemand {
    const fn interval(self) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Slow => Some(STARTUP_ANIMATION_TICK_INTERVAL),
            Self::Fast => Some(ACTIVE_ANIMATION_TICK_INTERVAL),
        }
    }
}

impl AppState {
    /// Returns the next animation-tick interval, or `None` when the terminal
    /// loop should park its animation timer.
    pub(crate) fn animation_tick_interval(&self) -> Option<Duration> {
        self.animation_tick_interval_with_motion(
            std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_none(),
        )
    }

    pub(crate) fn animation_tick_interval_with_motion(
        &self,
        motion_enabled: bool,
    ) -> Option<Duration> {
        let has_semantic_tick = self.active_turn_in_progress()
            || self.toast.is_some()
            || self.interrupt_confirmation_pending();
        if has_semantic_tick {
            return AnimationTickDemand::Fast.interval();
        }

        if !motion_enabled {
            return AnimationTickDemand::None.interval();
        }

        if self.starting_session_seed_visible() || self.active_background_task_count() > 0 {
            return AnimationTickDemand::Fast.interval();
        }

        AnimationTickDemand::None.interval()
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
    pub(crate) measured_viewport: Cell<MeasuredTranscriptViewport>,
    pub(crate) legacy_scroll_snapshot: Cell<(bool, usize)>,
    pub(crate) measured_anchor: Cell<Option<TranscriptContentAnchor>>,
    pub(crate) selected_activity_index: usize,
    pub(crate) page_flip: Cell<PageFlipState>,
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
            measured_viewport: Cell::new(MeasuredTranscriptViewport::following(0)),
            legacy_scroll_snapshot: Cell::new((true, 0)),
            measured_anchor: Cell::new(None),
            selected_activity_index: 0,
            page_flip: Cell::new(PageFlipState::Idle),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::{AppState, ToastVariant};

    #[test]
    fn animation_tick_demand_uses_fast_cadence_for_semantic_state() {
        // Given: a live shell with a transient state that must expire.
        let mut app = AppState::new_live(None, false, None);
        app.show_toast("Saved", ToastVariant::Info);

        // When: the animation scheduler queries the full-motion demand.
        let interval = app.animation_tick_interval_with_motion(true);

        // Then: expiry/state timing remains at the 30 Hz root cadence.
        assert_eq!(interval, Some(Duration::from_millis(1_000 / 30)));
    }

    #[test]
    fn animation_tick_demand_parks_idle_and_reduced_visual_only_shells() {
        // Given: an idle live shell and a startup shell with visual motion disabled.
        let idle = AppState::new_live(None, false, None);
        let startup = AppState::new_startup(Vec::new(), None);

        // When: each shell reports its timer demand.
        let idle_interval = idle.animation_tick_interval_with_motion(true);
        let disabled_startup_interval = startup.animation_tick_interval_with_motion(false);

        // Then: neither arms a redraw timer.
        assert_eq!(idle_interval, None);
        assert_eq!(disabled_startup_interval, None);
    }

    #[test]
    fn animation_tick_demand_parks_static_startup_shell() {
        // Given: the empty compose-first startup shell.
        let startup = AppState::new_startup(Vec::new(), None);

        // When: the scheduler queries full-motion demand.
        let interval = startup.animation_tick_interval_with_motion(true);

        // Then: decorative startup content never arms a redraw timer.
        assert_eq!(interval, None);
    }
}
