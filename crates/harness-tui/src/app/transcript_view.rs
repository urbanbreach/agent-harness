use std::cell::Cell;
use std::collections::BTreeSet;

use super::transcript_cache::TranscriptRenderCache;
use super::TranscriptScrollbarDragState;
use crate::ui::{TranscriptMouseTarget, TranscriptSelection};

#[derive(Debug)]
pub(crate) struct TranscriptViewState {
    pub(crate) transcript_scroll: usize,
    pub(crate) follow_mode: bool,
    pub(crate) transcript_scrollbar_visible: bool,
    pub(crate) transcript_scrollbar_drag: Option<TranscriptScrollbarDragState>,
    pub(crate) transcript_selection: Option<TranscriptSelection>,
    pub(crate) transcript_selection_dragging: bool,
    pub(crate) transcript_click_activated_on_down: bool,
    pub(crate) selected_diff_hunk_row: Option<usize>,
    pub(crate) hovered_transcript_target: Option<TranscriptMouseTarget>,
    pub(crate) show_transcript_thinking: bool,
    pub(crate) show_transcript_timestamps: bool,
    pub(crate) show_tool_details: bool,
    pub(crate) show_generic_tool_output: bool,
    pub(crate) stacked_transcript_diffs: bool,
    pub(crate) expanded_tool_outputs: BTreeSet<String>,
    pub(crate) expanded_patch_file_outputs: BTreeSet<String>,
    pub(crate) transcript_cache: TranscriptRenderCache,
    pub(crate) transcript_animation_phase: usize,
    pub(crate) last_transcript_max_scroll: Cell<usize>,
    pub(crate) selected_activity_index: usize,
}

impl Default for TranscriptViewState {
    fn default() -> Self {
        Self {
            transcript_scroll: 0,
            follow_mode: true,
            transcript_scrollbar_visible: true,
            transcript_scrollbar_drag: None,
            transcript_selection: None,
            transcript_selection_dragging: false,
            transcript_click_activated_on_down: false,
            selected_diff_hunk_row: None,
            hovered_transcript_target: None,
            show_transcript_thinking: true,
            show_transcript_timestamps: false,
            show_tool_details: true,
            show_generic_tool_output: false,
            stacked_transcript_diffs: false,
            expanded_tool_outputs: BTreeSet::new(),
            expanded_patch_file_outputs: BTreeSet::new(),
            transcript_cache: TranscriptRenderCache::default(),
            transcript_animation_phase: 0,
            last_transcript_max_scroll: Cell::new(0),
            selected_activity_index: 0,
        }
    }
}
