use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use crate::overlay::OverlayState;
use crate::ui::{TranscriptMouseTarget, TranscriptSelection};

use super::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use super::prompt_editor::{
    ComposerMode, ComposerSnapshot, PendingQueuedPromptCancellation, PromptStashEntry,
    QueuedPromptEntry,
};
use super::prompt_history::PromptHistoryDraft;
use super::transcript_cache::TranscriptRenderCache;
use super::SubagentFooterTarget;

const LEADER_KEY_TIMEOUT: Duration = Duration::from_millis(1_000);

#[derive(Debug, Default)]
pub(crate) struct KeySequenceState {
    pending_leader_deadline: Option<Instant>,
}

impl KeySequenceState {
    pub(crate) fn begin_leader_sequence(&mut self, now: Instant) {
        self.pending_leader_deadline = Some(now + LEADER_KEY_TIMEOUT);
    }

    pub(crate) fn clear_leader_sequence(&mut self) {
        self.pending_leader_deadline = None;
    }

    pub(crate) fn leader_sequence_pending(&self, now: Instant) -> bool {
        self.pending_leader_deadline
            .is_some_and(|deadline| deadline > now)
    }

    pub(crate) fn clear_expired_leader_sequence(&mut self, now: Instant) {
        if self
            .pending_leader_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.clear_leader_sequence();
        }
    }

    #[cfg(test)]
    pub(crate) fn force_leader_timeout(&mut self) {
        self.pending_leader_deadline = Some(Instant::now());
    }
}

#[derive(Debug, Default)]
pub struct ComposerState {
    pub prompt_buffer: String,
    pub prompt_cursor: usize,
    pub prompt_history: Vec<String>,
    pub prompt_history_index: Option<usize>,
    pub(crate) prompt_history_path: Option<PathBuf>,
    pub(crate) prompt_stash_path: Option<PathBuf>,
    pub(in crate::app) prompt_history_draft: Option<PromptHistoryDraft>,
    pub(in crate::app) mode: ComposerMode,
    pub(in crate::app) selection_anchor: Option<usize>,
    pub(in crate::app) undo_stack: Vec<ComposerSnapshot>,
    pub(in crate::app) redo_stack: Vec<ComposerSnapshot>,
    pub(in crate::app) prompt_stash: Vec<PromptStashEntry>,
    pub(in crate::app) prompt_stash_selected: usize,
    pub(in crate::app) stash_dialog_visible: bool,
    pub(in crate::app) queued_prompt_count: usize,
    pub(in crate::app) queued_prompts: Vec<QueuedPromptEntry>,
    pub(in crate::app) pending_queued_prompt_cancellations: Vec<PendingQueuedPromptCancellation>,
    pub(in crate::app) queued_prompt_selected: usize,
    pub(in crate::app) queued_prompt_dialog_visible: bool,
}

#[derive(Debug, Default)]
pub struct OverlayStateHolder {
    pub slash_visible: bool,
    pub file_mention_visible: bool,
    pub palette_visible: bool,
    pub status_dialog_visible: bool,
    pub session_history_visible: bool,
    pub model_switcher_visible: bool,
    pub toggles_menu_visible: bool,
    pub lineage_browser_visible: bool,
    pub fork_selector_visible: bool,
    pub error_details_visible: bool,
    pub child_sessions_visible: bool,
    pub onboarding_visible: bool,
}

impl OverlayStateHolder {
    pub fn to_overlay_state(
        &self,
        details_drawer_open: bool,
        prompt_stash_visible: bool,
        queued_prompts_visible: bool,
        permission_pending: bool,
    ) -> OverlayState {
        OverlayState {
            details_drawer_open,
            slash_visible: self.slash_visible,
            file_mention_visible: self.file_mention_visible,
            palette_visible: self.palette_visible,
            status_dialog_visible: self.status_dialog_visible,
            prompt_stash_visible,
            queued_prompts_visible,
            session_history_visible: self.session_history_visible,
            model_switcher_visible: self.model_switcher_visible,
            toggles_menu_visible: self.toggles_menu_visible,
            lineage_browser_visible: self.lineage_browser_visible,
            fork_selector_visible: self.fork_selector_visible,
            error_details_visible: self.error_details_visible,
            child_sessions_visible: self.child_sessions_visible,
            permission_pending,
        }
    }

    pub fn command_palette_channel_visible(&self) -> bool {
        self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.toggles_menu_visible
            || self.lineage_browser_visible
            || self.fork_selector_visible
            || self.child_sessions_visible
    }
}

#[derive(Debug, Default)]
pub(crate) struct PermissionPromptState {
    pub(crate) dismissed_permissions: BTreeSet<String>,
    pub(crate) submitted_permission_id: Option<String>,
    pub(crate) permission_modal_permission_id: Option<String>,
    pub(crate) permission_modal_stage: PermissionModalStage,
    pub(crate) permission_modal_selection: PermissionModalSelection,
    pub(crate) permission_modal_confirm_selection: PermissionConfirmSelection,
}

impl PermissionPromptState {
    pub(crate) fn permission_modal_is_active(&self, permission_id: &str) -> bool {
        self.permission_modal_permission_id.as_deref() == Some(permission_id)
    }

    pub(crate) fn permission_modal_selection(
        &self,
        permission_id: &str,
    ) -> PermissionModalSelection {
        if self.permission_modal_is_active(permission_id) {
            self.permission_modal_selection
        } else {
            PermissionModalSelection::AllowOnce
        }
    }

    pub(crate) fn permission_modal_stage(&self, permission_id: &str) -> PermissionModalStage {
        if self.permission_modal_is_active(permission_id) {
            self.permission_modal_stage
        } else {
            PermissionModalStage::Decision
        }
    }

    pub(crate) fn permission_modal_confirm_selection(
        &self,
        permission_id: &str,
    ) -> PermissionConfirmSelection {
        if self.permission_modal_is_active(permission_id) {
            self.permission_modal_confirm_selection
        } else {
            PermissionConfirmSelection::Confirm
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct QuestionPromptState {
    pub(crate) question_answer_permission_id: Option<String>,
    pub(crate) question_prompt_tab: usize,
    pub(crate) question_prompt_selection: usize,
    pub(crate) question_prompt_answers: Vec<Vec<String>>,
    pub(crate) question_prompt_custom: Vec<String>,
    pub(crate) question_prompt_editing: bool,
    pub(crate) question_answer_buffer: String,
    pub(crate) question_answer_cursor: usize,
    pub(crate) question_answer_error: Option<String>,
}

impl QuestionPromptState {
    pub(crate) fn question_answer_is_active(&self, permission_id: &str) -> bool {
        self.question_answer_permission_id.as_deref() == Some(permission_id)
    }

    pub(crate) fn question_prompt_tab(&self, permission_id: &str) -> usize {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt_tab
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_selection(&self, permission_id: &str) -> usize {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt_selection
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_editing(&self, permission_id: &str) -> bool {
        self.question_answer_is_active(permission_id) && self.question_prompt_editing
    }

    pub(crate) fn question_prompt_answers(&self, permission_id: &str) -> Vec<Vec<String>> {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt_answers.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn question_prompt_custom(&self, permission_id: &str, index: usize) -> Option<&str> {
        if !self.question_answer_is_active(permission_id) {
            return None;
        }

        self.question_prompt_custom.get(index).map(String::as_str)
    }

    pub(crate) fn question_answer_error(&self, permission_id: &str) -> Option<&str> {
        self.question_answer_is_active(permission_id)
            .then_some(self.question_answer_error.as_deref())
            .flatten()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TranscriptScrollbarDragState {
    pub(crate) track: Rect,
    pub(crate) thumb_height: u16,
    pub(crate) pointer_offset_y: u16,
    pub(crate) max_scroll: usize,
}

#[derive(Debug)]
pub struct TranscriptViewState {
    pub follow_mode: bool,
    pub transcript_scroll: usize,
    pub(crate) last_transcript_max_scroll: Cell<usize>,
    last_transcript_message_top_rows: RefCell<Vec<usize>>,
    pub(crate) transcript_scrollbar_drag: Option<TranscriptScrollbarDragState>,
    pub(crate) selected_diff_hunk_row: Option<usize>,
    pub(crate) hovered_transcript_target: Option<TranscriptMouseTarget>,
    pub(crate) hovered_subagent_footer_target: Option<SubagentFooterTarget>,
    pub(crate) transcript_click_activated_on_down: bool,
    pub(crate) transcript_selection: Option<TranscriptSelection>,
    pub(crate) transcript_selection_dragging: bool,
    pub(in crate::app) transcript_cache: TranscriptRenderCache,
    pub(crate) transcript_animation_phase: usize,
    pub(crate) show_transcript_thinking: bool,
    pub(crate) show_transcript_timestamps: bool,
    pub(crate) show_tool_details: bool,
    pub(crate) show_generic_tool_output: bool,
    pub(crate) stacked_transcript_diffs: bool,
    show_transcript_scrollbar: bool,
    pub(crate) expanded_tool_outputs: BTreeSet<String>,
    pub(crate) expanded_patch_file_outputs: BTreeSet<String>,
}

impl Default for TranscriptViewState {
    fn default() -> Self {
        Self {
            follow_mode: true,
            transcript_scroll: 0,
            last_transcript_max_scroll: Cell::new(0),
            last_transcript_message_top_rows: RefCell::new(Vec::new()),
            transcript_scrollbar_drag: None,
            selected_diff_hunk_row: None,
            hovered_transcript_target: None,
            hovered_subagent_footer_target: None,
            transcript_click_activated_on_down: false,
            transcript_selection: None,
            transcript_selection_dragging: false,
            transcript_cache: TranscriptRenderCache::default(),
            transcript_animation_phase: 0,
            show_transcript_thinking: true,
            show_transcript_timestamps: false,
            show_tool_details: true,
            show_generic_tool_output: false,
            stacked_transcript_diffs: false,
            show_transcript_scrollbar: true,
            expanded_tool_outputs: BTreeSet::new(),
            expanded_patch_file_outputs: BTreeSet::new(),
        }
    }
}

impl TranscriptViewState {
    pub(crate) fn transcript_animation_phase(&self) -> usize {
        self.transcript_animation_phase
    }

    pub(crate) fn hovered_transcript_target(&self) -> Option<&TranscriptMouseTarget> {
        self.hovered_transcript_target.as_ref()
    }

    pub(crate) fn hovered_subagent_footer_target(&self) -> Option<SubagentFooterTarget> {
        self.hovered_subagent_footer_target
    }

    pub(crate) fn transcript_selection(&self) -> Option<TranscriptSelection> {
        self.transcript_selection
    }

    pub(crate) fn stacked_transcript_diffs(&self) -> bool {
        self.stacked_transcript_diffs
    }

    pub(crate) fn transcript_scrollbar_visible(&self) -> bool {
        self.show_transcript_scrollbar
    }

    pub(crate) fn toggle_transcript_scrollbar(&mut self) {
        self.show_transcript_scrollbar = !self.show_transcript_scrollbar;
    }

    pub(crate) fn set_transcript_message_top_rows(&self, rows: Vec<usize>) {
        *self.last_transcript_message_top_rows.borrow_mut() = rows;
    }

    pub(crate) fn transcript_message_top_rows(&self) -> Vec<usize> {
        self.last_transcript_message_top_rows.borrow().clone()
    }
}
