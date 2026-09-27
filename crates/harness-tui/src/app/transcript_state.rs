// allow: SIZE_OK — TUI app state (session projection + interaction)
use crate::prompt_queue_actions::{QueueLifecycle, QueueState};
use crate::transcript_scroll::PageFlipState;
use crate::UnwrapOrAbort;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::session_projection::ProjectionDelta;
use super::transcript_viewport::TranscriptViewport;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastVariant {
    Rewind,
    Info,
    Error,
    Mode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToastState {
    pub message: String,
    pub variant: ToastVariant,
    expires_at: Instant,
    paused_remaining: Option<Duration>,
}

impl ToastState {
    pub(crate) fn fade_alpha(&self, now: Instant) -> f32 {
        let remaining = self
            .paused_remaining
            .unwrap_or_else(|| self.expires_at.saturating_duration_since(now));
        if self.variant != ToastVariant::Mode || remaining > Duration::from_millis(297) {
            1.0
        } else {
            (remaining.as_secs_f32() / 0.297).clamp(0.0, 1.0)
        }
    }
}

impl AppState {
    pub fn transcript_screen_mode(&self) -> Option<TranscriptScreenMode> {
        self.transcript_outline.as_ref().map(|_| {
            if self.transcript_viewer.is_some() {
                TranscriptScreenMode::SelectedBlockViewer
            } else {
                TranscriptScreenMode::InPlace(crate::transcript_identity::InPlaceMode::Transcript)
            }
        })
    }

    pub(crate) fn transcript_viewer(&self) -> Option<&crate::transcript_block_viewer::ViewerState> {
        self.transcript_viewer.as_ref()
    }

    pub(crate) fn sync_transcript_state(&mut self, animate_tool_transitions: bool) {
        let projection_delta = self.projection.take_transcript_delta();
        match projection_delta {
            ProjectionDelta::Activity { index } | ProjectionDelta::RetainedPrefix { index } => {
                self.transcript_view.prepared.invalidate_from(index)
            }
            ProjectionDelta::None => self.transcript_view.prepared.invalidate_from(usize::MAX),
            ProjectionDelta::FullRebuild | ProjectionDelta::ReplayPending => {
                self.transcript_view.prepared.invalidate_from(0)
            }
        }
        let now = self.now();
        let running_tool_ids = self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| tool_call.status == ToolCallDisplayStatus::Running)
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect::<Vec<_>>();
        let terminal_tool_ids = self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| {
                matches!(
                    tool_call.status,
                    ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed
                )
            })
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect::<Vec<_>>();
        self.transcript_view.tool_motion.sync_terminal_ids(
            terminal_tool_ids,
            now,
            animate_tool_transitions && !self.replay_mode && !self.reduced_motion,
        );
        self.transcript_view
            .tool_motion
            .sync_running_ids(running_tool_ids, now);
        let lifecycle = if self.active_turn_in_progress() {
            QueueLifecycle::Streaming
        } else {
            QueueLifecycle::Idle
        };
        let state = QueueState::new(lifecycle).with_draft(self.composer.editor_text());
        let _ = self.composer.slice.set_queue_state(state);
        let frame_area = self
            .last_frame_area
            .unwrap_or(ratatui::layout::Rect::new(0, 0, 80, 24));
        let viewport = crate::layout::FrameLayoutPlan::for_app(self, frame_area)
            .transcript
            .unwrap_or(frame_area);
        if self.transcript_outline.is_none() && viewport.width > 0 && viewport.height > 0 {
            self.transcript_outline = Some(TranscriptOutline::new(viewport));
        }
        if let Some(outline) = self.transcript_outline.as_mut() {
            let from = match (animate_tool_transitions, projection_delta) {
                (false, ProjectionDelta::ReplayPending) => None,
                (true, ProjectionDelta::None) => Some(usize::MAX),
                (
                    true,
                    ProjectionDelta::Activity { index } | ProjectionDelta::RetainedPrefix { index },
                ) => Some(index),
                _ => Some(0),
            };
            if let Some(from) = from {
                outline.update(&self.projection.activities, viewport, from);
            }
        }
        if self.transcript_viewer().is_some() {
            if let Some(entry) = self.selected_transcript_entry() {
                let content = self.selected_entry_content(&entry);
                if let Some(viewer) = self.transcript_viewer.as_mut() {
                    let _ = viewer.update_content(content);
                }
            }
        }
    }

    pub fn transcript_following(&self) -> bool {
        if self.transcript_view.page_flip.scroll_top().is_some() {
            return false;
        }
        self.transcript_view.measured_viewport().is_following()
    }

    pub fn transcript_viewer_mode(&self) -> Option<crate::transcript_block_viewer::ViewerMode> {
        self.transcript_viewer().map(|viewer| viewer.mode())
    }

    pub(crate) fn set_transcript_following(&mut self, following: bool) {
        self.cancel_transcript_page_flip();
        let viewport = self.transcript_view.measured_viewport();
        let next = if following {
            viewport.jump_to_bottom()
        } else {
            viewport.scroll_up(1)
        };
        self.transcript_view.set_measured_viewport(next);
    }

    pub(in crate::app) fn begin_transcript_page_flip(&mut self, activity_first_seq: u64) {
        let current = self.transcript_view.page_flip;
        let next = current.begin(activity_first_seq);
        if next == current {
            return;
        }
        self.set_transcript_following(true);
        self.transcript_view.page_flip = next;
    }

    pub(crate) fn transcript_page_flip_preserving(&self) -> bool {
        self.transcript_view.page_flip.is_preserving()
    }

    pub(crate) fn transcript_page_flip_state(&self) -> PageFlipState {
        self.transcript_view.page_flip
    }

    pub(crate) fn transcript_page_flip_scroll_top(&self) -> Option<usize> {
        self.transcript_view.page_flip.scroll_top()
    }

    pub(crate) fn set_transcript_page_flip_state(&mut self, state: PageFlipState) {
        self.transcript_view.page_flip = state;
    }

    pub(in crate::app) fn cancel_transcript_page_flip(&mut self) {
        self.transcript_view.page_flip = self.transcript_view.page_flip.cancel();
    }

    pub fn select_transcript_turn_at(&mut self, index: usize) -> bool {
        if !self
            .transcript_outline
            .as_mut()
            .is_some_and(|outline| outline.select(index))
        {
            return false;
        }
        self.transcript_view.selected_activity_index = index;
        self.cancel_transcript_page_flip();
        true
    }

    pub fn toggle_selected_transcript_fold(&mut self) -> bool {
        self.fold_selected_entry()
    }

    pub(crate) fn open_selected_transcript_viewer(&mut self) -> bool {
        use super::transcript_outline::{activity_blocks, replay_turn, rows_as_f64};
        use crate::transcript_block_viewer::{ViewerReturnSnapshot, ViewerState};
        use crate::transcript_blocks::default_fold;
        use crate::transcript_identity::{FocusFollowState, TranscriptFocus};

        let Some(entry) = self.selected_transcript_entry() else {
            return false;
        };
        self.transcript_view.selected_entry = Some(entry.id);
        let content = self.selected_entry_content(&entry);
        let Some((index, activity)) = self
            .activities
            .iter()
            .enumerate()
            .find(|(_, activity)| activity.first_seq == entry.activity_first_seq)
        else {
            return false;
        };
        let Some(outline) = self.transcript_outline.as_ref() else {
            return false;
        };
        let Some(layout) = outline.layout() else {
            return false;
        };
        let Ok(anchor) = layout.capture_anchor(rows_as_f64(outline.scroll_top)) else {
            return false;
        };
        let Some(block) = activity_blocks(activity).next() else {
            return false;
        };
        let focus = if outline.selected_index().is_some() {
            TranscriptFocus::Timeline
        } else {
            TranscriptFocus::Transcript
        };
        let snapshot = ViewerReturnSnapshot::new(
            default_fold(block.kind, block.lifecycle),
            FocusFollowState::new(focus, focus == TranscriptFocus::Transcript),
            anchor,
        );
        let Ok(viewer) = ViewerState::open(
            replay_turn(index, activity, 0).block_id(0),
            content,
            snapshot,
        ) else {
            return false;
        };
        self.transcript_viewer = Some(viewer);
        self.resize_transcript_viewer(self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)));
        true
    }

    pub(crate) fn close_transcript_viewer(&mut self) -> bool {
        let Some(viewer) = self.transcript_viewer.take() else {
            return false;
        };
        if let Some(outline) = self.transcript_outline.as_mut() {
            outline.restore_viewer_anchor(viewer.close().return_snapshot().anchor);
        }
        true
    }

    pub(crate) fn select_transcript_turn(&mut self, turn_id: TurnId) -> bool {
        let Some(index) = self
            .transcript_outline
            .as_ref()
            .and_then(|outline| outline.index_of(turn_id))
        else {
            return false;
        };
        // Timeline clicks preserve the entry selection; message commands move it explicitly.
        let selected = self
            .transcript_outline
            .as_mut()
            .is_some_and(|outline| outline.select(index));
        if selected {
            self.cancel_transcript_page_flip();
        }
        selected
    }

    pub(crate) fn handle_transcript_timeline_key(&mut self, key: KeyEvent) -> bool {
        let Some(jump) = crate::transcript_timeline::navigation::key_jump(key) else {
            return false;
        };
        self.jump_transcript_timeline(jump)
    }

    pub(crate) fn jump_transcript_timeline(
        &mut self,
        jump: crate::transcript_timeline::TimelineJump,
    ) -> bool {
        let response_jump = matches!(
            jump,
            crate::transcript_timeline::TimelineJump::NextResponse
                | crate::transcript_timeline::TimelineJump::PreviousResponse
        );
        if response_jump {
            return self.jump_transcript_response(
                jump == crate::transcript_timeline::TimelineJump::NextResponse,
            );
        }
        let Some(outline) = self.transcript_outline.as_mut() else {
            return false;
        };
        let scroll_top = outline.jump(jump);
        if let Some(index) = outline.selected_index() {
            self.transcript_view.selected_activity_index = index;
        }
        let max_scroll = self.transcript_view.viewport.max_scroll();
        self.transcript_view
            .set_measured_viewport(TranscriptViewport::detached(scroll_top, max_scroll));
        self.cancel_transcript_page_flip();
        true
    }

    pub(crate) fn transcript_thinking_visible(&self) -> bool {
        self.transcript_view.show_transcript_thinking
    }

    pub(crate) fn transcript_timestamps_visible(&self) -> bool {
        self.transcript_view.show_transcript_timestamps
    }

    pub(crate) fn transcript_animation_phase(&self) -> usize {
        self.transcript_view.transcript_animation_phase
    }

    pub(crate) fn shell_mode(&self) -> bool {
        self.composer.shell_mode
    }

    pub(crate) fn hovered_transcript_target(&self) -> Option<&TranscriptMouseTarget> {
        self.transcript_view.hovered_transcript_target.as_ref()
    }

    pub(crate) fn transcript_render_cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash_transcript_render_settings(&mut hasher);
        self.transcript_view.prepared.epoch().hash(&mut hasher);
        hasher.finish()
    }

    pub(crate) fn transcript_settings_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash_transcript_render_settings(&mut hasher);
        hasher.finish()
    }

    fn hash_transcript_render_settings(&self, hasher: &mut impl Hasher) {
        (self.focus == super::Focus::Details).hash(hasher);
        self.todo_pane_focused().hash(hasher);
        self.transcript_view.selected_entry.hash(hasher);
        self.replay_mode.hash(hasher);
        if self.focus == super::Focus::Details && !self.todo_pane_focused() {
            self.transcript_view.selected_activity_index.hash(hasher);
        }
        self.transcript_view.show_transcript_thinking.hash(hasher);
        self.transcript_view.show_transcript_timestamps.hash(hasher);
        self.transcript_view.show_tool_details.hash(hasher);
        self.transcript_view
            .compaction_details_expanded
            .hash(hasher);
        self.transcript_view.show_generic_tool_output.hash(hasher);
        self.transcript_view.stacked_transcript_diffs.hash(hasher);
        self.transcript_view.hovered_transcript_target.hash(hasher);
        self.active_profile().hash(hasher);
        self.session_path.hash(hasher);
    }

    pub(crate) fn advance_transcript_animation_phase(&mut self) {
        self.transcript_view.transcript_animation_phase = self
            .transcript_view
            .transcript_animation_phase
            .wrapping_add(1);
        let now = self.now();
        self.clear_expired_interrupt_confirmation();
        self.refresh_toast_motion(now);
    }

    pub fn advance_animation_tick(&mut self) {
        let elapsed = Duration::from_millis(crate::scheduling::active_animation_period_ms());
        let now = self.now() + elapsed;
        self.now_fn = std::sync::Arc::new(move || now);
        self.sampled_motion_elapsed = self.sampled_motion_elapsed.saturating_add(elapsed);
        self.advance_transcript_animation_phase();
    }

    pub fn animation_phase(&self) -> usize {
        self.transcript_animation_phase()
    }

    pub fn has_active_animations(&self) -> bool {
        !self.motion_plan().is_none()
    }

    pub(crate) fn starting_session_seed_visible(&self) -> bool {
        self.starting_session_seed
    }

    pub(crate) fn set_starting_session_seed(&mut self, visible: bool) {
        self.starting_session_seed = visible && !self.active_turn_in_progress();
    }

    pub(crate) fn tool_finish_elapsed(&self, tool_call_id: &str) -> Option<Duration> {
        if self.replay_mode || self.reduced_motion {
            return None;
        }
        self.transcript_view
            .tool_motion
            .finish_elapsed(tool_call_id, self.now())
    }

    pub(crate) fn record_visible_running_tool_motion(&mut self, visible: bool) {
        self.transcript_view.visible_running_tool_motion = visible;
    }

    pub(crate) fn active_turn_tool_motion_demand(&self) -> bool {
        if self.replay_mode || self.interrupt_requested() || !self.active_turn_in_progress() {
            return false;
        }
        let mut has_active_tool = false;
        let mut has_running_tool = false;
        for tool_call in self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
        {
            match tool_call.status {
                ToolCallDisplayStatus::Running => {
                    has_active_tool = true;
                    has_running_tool = true;
                }
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
                    has_active_tool = true;
                    has_running_tool |= tool_call.has_execution_motion();
                }
                ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => {}
            }
        }
        super::transcript_view::active_turn_motion_demand(
            has_active_tool,
            has_running_tool,
            self.transcript_view.visible_running_tool_motion,
        )
    }

    pub(in crate::app) fn invalidate_transcript_after_durable_event(&mut self) {
        // Queued badges and tool rows depend on other turns. Only terminal text
        // turns can keep their prepared layout when a durable suffix changes.
        let retained = match self.projection.transcript_delta {
            ProjectionDelta::RetainedPrefix { index }
                if self
                    .projection
                    .activities
                    .iter()
                    .take(index)
                    .all(|activity| {
                        matches!(
                            activity.status,
                            ActivityStatus::Done | ActivityStatus::Error
                        ) && activity.tool_calls.is_empty()
                            && activity.permissions.is_empty()
                    }) =>
            {
                index
            }
            _ => 0,
        };
        self.transcript_view.prepared.invalidate_from(retained);
    }

    pub(in crate::app) fn bump_transcript_render_epoch(&mut self) {
        self.transcript_view.prepared.invalidate_from(0);
    }

    pub(crate) fn tool_details_visible(&self) -> bool {
        self.transcript_view.show_tool_details
    }

    pub(crate) fn reasoning_expanded(&self, request_id: &str) -> bool {
        self.transcript_view
            .expanded_reasoning_requests
            .contains(request_id)
    }

    pub(in crate::app) fn toggle_reasoning_expansion(&mut self, request_id: &str) {
        if !self
            .transcript_view
            .expanded_reasoning_requests
            .insert(request_id.to_string())
        {
            self.transcript_view
                .expanded_reasoning_requests
                .remove(request_id);
        }
        self.bump_transcript_render_epoch();
    }

    pub(crate) fn generic_tool_output_visible(&self) -> bool {
        self.transcript_view.show_generic_tool_output
    }

    pub(crate) fn stacked_transcript_diffs(&self) -> bool {
        self.transcript_view.stacked_transcript_diffs
    }

    pub(crate) fn tool_output_expanded(&self, tool_call: &ToolCallEntry) -> bool {
        if ui::tool_output_is_viewer_only(tool_call) {
            return false;
        }
        if self
            .transcript_view
            .collapsed_tool_outputs
            .contains(&tool_call.tool_call_id)
        {
            return false;
        }
        self.transcript_view
            .expanded_tool_outputs
            .contains(&tool_call.tool_call_id)
            || self.tool_output_previewed(&tool_call.tool_call_id)
            || self
                .transcript_view
                .expanded_patch_file_outputs
                .iter()
                .any(|key| key.starts_with(&format!("{}\u{1f}", tool_call.tool_call_id)))
            || (tool_call.status == ToolCallDisplayStatus::Succeeded
                && matches!(
                    tool_call.effective_tool_id(),
                    "edit" | "write" | "fs.write" | "edit.hashline_apply"
                ))
    }

    pub(crate) fn patch_file_output_expanded(&self, tool_call_id: &str, file_path: &str) -> bool {
        self.transcript_view
            .expanded_patch_file_outputs
            .contains(&Self::patch_file_disclosure_key(tool_call_id, file_path))
    }

    pub(crate) fn seed_patch_file_expansions(&mut self, event: &EventEnvelopeV1) {
        let EventV1::ToolCallFinished(data) = &event.payload else {
            return;
        };
        if data.status != ToolCallStatus::Succeeded {
            return;
        }
        let Some(edits) = data
            .output_json
            .as_ref()
            .and_then(|output| output.get("edits").or_else(|| output.get("applied")))
            .and_then(serde_json::Value::as_array)
        else {
            return;
        };
        for edit in edits {
            if edit.get("deleted").and_then(serde_json::Value::as_bool) == Some(true)
                || edit.get("type").and_then(serde_json::Value::as_str) == Some("delete")
            {
                continue;
            }
            let Some(path) = edit
                .get("path")
                .or_else(|| edit.get("resource"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            self.transcript_view.expanded_patch_file_outputs.insert(
                Self::patch_file_disclosure_key(data.tool_call_id.as_str(), path),
            );
        }
    }

    fn patch_file_disclosure_key(tool_call_id: &str, file_path: &str) -> String {
        format!("{tool_call_id}\u{1f}{file_path}")
    }

    pub(in crate::app) fn toggle_tool_output(&mut self, tool_call_id: &str) {
        let expanded = self
            .tool_call_entry(tool_call_id)
            .is_some_and(|tool_call| self.tool_output_expanded(tool_call));
        self.set_tool_output_expanded(tool_call_id, !expanded);
        if !expanded
            && self.tool_call_entry(tool_call_id).is_some_and(|tool| {
                matches!(tool.effective_tool_id(), "read" | "fs.read")
                    && tool_call_has_expandable_output(tool)
                    && !ui::tool_output_is_viewer_only(tool)
            })
        {
            self.transcript_view
                .expanded_tool_outputs
                .remove(tool_call_id);
            self.transcript_view
                .previewed_tool_outputs
                .insert(tool_call_id.to_string());
        }
    }

    pub(crate) fn tool_output_previewed(&self, tool_call_id: &str) -> bool {
        self.transcript_view
            .previewed_tool_outputs
            .contains(tool_call_id)
    }

    pub(in crate::app) fn set_tool_output_expanded(&mut self, tool_call_id: &str, expanded: bool) {
        self.transcript_view
            .previewed_tool_outputs
            .remove(tool_call_id);
        if self
            .tool_call_entry(tool_call_id)
            .is_some_and(ui::tool_output_is_viewer_only)
        {
            return;
        }
        if expanded {
            self.transcript_view
                .collapsed_tool_outputs
                .remove(tool_call_id);
            self.transcript_view
                .expanded_tool_outputs
                .insert(tool_call_id.to_string());
        } else {
            self.transcript_view
                .expanded_tool_outputs
                .remove(tool_call_id);
            self.transcript_view
                .collapsed_tool_outputs
                .insert(tool_call_id.to_string());
        }
        self.bump_transcript_render_epoch();
    }

    fn toggle_patch_file_output(&mut self, tool_call_id: &str, file_path: &str) {
        let disclosure_key = Self::patch_file_disclosure_key(tool_call_id, file_path);
        if !self
            .transcript_view
            .expanded_patch_file_outputs
            .insert(disclosure_key.clone())
        {
            self.transcript_view
                .expanded_patch_file_outputs
                .remove(&disclosure_key);
        }
        self.bump_transcript_render_epoch();
    }

    pub(super) fn set_tool_group_outputs_expanded(
        &mut self,
        tool_call_ids: &[String],
        expanded: bool,
    ) {
        let Some(first) = tool_call_ids.first() else {
            return;
        };
        let area = self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let select_member = expanded && self.selected_transcript_entry().is_some_and(|entry| {
            matches!(&entry.target, Some(TranscriptMouseTarget::ToolGroup { tool_call_ids: ids })
                if ids.first() == Some(first))
        });
        let select_tail = expanded && ui::transcript_navigation_entries(self, area)
            .iter()
            .any(|entry| !entry.context_group && matches!(&entry.target,
                Some(TranscriptMouseTarget::ToolGroup { tool_call_ids: ids }) if ids.first() == Some(first)));
        if expanded {
            self.transcript_view
                .expanded_tool_groups
                .insert(first.clone());
        } else {
            self.transcript_view.expanded_tool_groups.remove(first);
        }
        self.bump_transcript_render_epoch();
        if select_tail {
            // The reference clears dense-group selection and its active pane
            // resumes at the final member. Context groups retain member zero.
            if let Some(entry) = ui::transcript_navigation_entries(self, area)
                .into_iter()
                .rev()
                .find(|entry| matches!(&entry.target,
                    Some(TranscriptMouseTarget::Tool { tool_call_id }) if tool_call_ids.contains(tool_call_id)))
            {
                self.select_transcript_entry(&entry);
            }
        } else if select_member {
            // Grok's expanded context header selects member zero. That member
            // can be a Thought, whose next fold key must reveal its own body.
            let entries = ui::transcript_navigation_entries(self, area);
            if let Some([_, member]) = entries.windows(2).find(|pair| {
                pair[0].context_group && matches!(&pair[0].target,
                    Some(TranscriptMouseTarget::ToolGroup { tool_call_ids: ids }) if ids.first() == Some(first))
            }) {
                self.transcript_view.selected_entry = Some(member.id);
            }
        }
    }

    pub(crate) fn tool_group_expanded(&self, first_tool_call_id: &str) -> bool {
        self.transcript_view
            .expanded_tool_groups
            .contains(first_tool_call_id)
    }

    pub(in crate::app) fn tool_call_entry(&self, tool_call_id: &str) -> Option<&ToolCallEntry> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .find(|tool_call| tool_call.tool_call_id == tool_call_id)
    }

    #[cfg(test)]
    pub(crate) fn set_patch_file_output_expanded_for_test(
        &mut self,
        tool_call_id: &str,
        file_path: &str,
        expanded: bool,
    ) {
        let disclosure_key = Self::patch_file_disclosure_key(tool_call_id, file_path);
        if expanded {
            self.transcript_view
                .expanded_patch_file_outputs
                .insert(disclosure_key);
        } else {
            self.transcript_view
                .expanded_patch_file_outputs
                .remove(&disclosure_key);
        }
        self.bump_transcript_render_epoch();
    }

    pub(in crate::app) fn activate_transcript_mouse_target(
        &mut self,
        target: TranscriptMouseTarget,
    ) {
        match target {
            TranscriptMouseTarget::UserTimestamp { .. } => {}
            TranscriptMouseTarget::Reasoning { request_id } => {
                self.toggle_reasoning_expansion(&request_id);
            }
            TranscriptMouseTarget::SubagentSession { session_id } => {
                self.navigate_to_child_session_id(session_id);
            }
            TranscriptMouseTarget::Tool { tool_call_id } => {
                if let Some(child_session_id) = self.task_tool_child_session_id(&tool_call_id) {
                    self.navigate_to_child_session_id(child_session_id);
                    return;
                }
                if self
                    .tool_call_entry(&tool_call_id)
                    .is_some_and(Self::tool_call_is_task_spawn)
                {
                    self.set_status_banner(Some(
                        "subagent session is not available for this task yet".to_string(),
                    ));
                    return;
                }
                self.toggle_tool_output(&tool_call_id);
            }
            TranscriptMouseTarget::ToolGroup { tool_call_ids } => {
                let expand_group = !tool_call_ids
                    .first()
                    .is_some_and(|id| self.tool_group_expanded(id));
                self.set_tool_group_outputs_expanded(&tool_call_ids, expand_group);
            }
            TranscriptMouseTarget::PatchFile {
                tool_call_id,
                file_path,
            } => {
                self.toggle_patch_file_output(&tool_call_id, &file_path);
            }
        }
    }

    pub(in crate::app) fn close_subagent_actions_dialog(&mut self) {
        self.subagent_actions_session_id = None;
    }

    pub(in crate::app) fn open_selected_subagent_session(&mut self) {
        if let Some(session_id) = self.subagent_actions_session_id.take() {
            self.navigate_to_child_session_id(session_id);
        }
    }

    fn task_tool_child_session_id(&self, tool_call_id: &str) -> Option<String> {
        let tool_call = self.tool_call_entry(tool_call_id)?;
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        task_child_session_id_from_output(tool_call.output_json.as_ref())
            .or_else(|| {
                tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.clone())
            })
            .or_else(|| {
                self.transcript_task_row_for_tool_call(tool_call)
                    .and_then(|row| row.effective_child_session_id().map(str::to_string))
            })
    }

    pub(in crate::app) fn selected_activity_expandable_tool_ids(&self) -> Vec<String> {
        self.activities
            .get(self.transcript_view.selected_activity_index)
            .into_iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| tool_call_has_expandable_output(tool_call))
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect()
    }

    pub(in crate::app) fn set_selected_activity_expandable_outputs(&mut self, expanded: bool) {
        for tool_call_id in self.selected_activity_expandable_tool_ids() {
            self.set_tool_output_expanded(&tool_call_id, expanded);
        }
    }
}

impl AppState {
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, variant: ToastVariant) {
        let now = self.now();
        self.toast = Some(ToastState {
            message: message.into(),
            variant,
            expires_at: now
                + Duration::from_secs(if variant == ToastVariant::Rewind {
                    3
                } else {
                    2
                }),
            paused_remaining: None,
        });
        self.motion_revision = self.motion_revision.wrapping_add(1);
    }

    pub(in crate::app) fn show_mode_banner(&mut self, message: impl Into<String>) {
        let now = self.now();
        self.toast = Some(ToastState {
            message: message.into(),
            variant: ToastVariant::Mode,
            expires_at: now + Duration::from_millis(2_277),
            paused_remaining: None,
        });
        self.motion_revision = self.motion_revision.wrapping_add(1);
    }

    pub(crate) fn toast_fade_alpha(&self) -> Option<f32> {
        self.toast
            .as_ref()
            .map(|toast| toast.fade_alpha(self.now()))
    }

    pub(in crate::app) fn refresh_toast_motion(&mut self, now: Instant) -> bool {
        let occluded = self.overlay_stack().top().is_some();
        let Some(toast) = self.toast.as_mut() else {
            return false;
        };
        match (occluded, toast.paused_remaining) {
            (true, None) => {
                toast.paused_remaining = Some(toast.expires_at.saturating_duration_since(now));
            }
            (false, Some(remaining)) => {
                toast.expires_at = now + remaining;
                toast.paused_remaining = None;
            }
            _ => {}
        }
        if !occluded && now >= toast.expires_at {
            self.toast = None;
            return true;
        }
        false
    }

    pub(in crate::app) fn toast_motion_remaining(&self, now: Instant) -> Option<Duration> {
        self.toast.as_ref().and_then(|toast| {
            toast
                .paused_remaining
                .is_none()
                .then(|| toast.expires_at.saturating_duration_since(now))
        })
    }

    pub(in crate::app) fn toast_requires_fade(&self, now: Instant) -> bool {
        self.toast.as_ref().is_some_and(|toast| {
            toast.variant == ToastVariant::Mode
                && toast.paused_remaining.is_none()
                && toast.expires_at.saturating_duration_since(now) <= Duration::from_millis(297)
        })
    }

    pub(crate) fn toast(&self) -> Option<&ToastState> {
        self.toast.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_toast_for_test(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.show_toast(message, variant);
    }

    pub(in crate::app) fn navigate_diff_hunk(&mut self, reverse: bool) -> bool {
        let Some(frame_area) = self.last_frame_area else {
            return false;
        };
        let hunk_rows = ui::transcript_diff_hunk_rows(self, frame_area);
        if hunk_rows.is_empty() {
            return false;
        }

        let max_scroll = self.transcript_view.viewport.max_scroll();
        let current_top = self
            .transcript_page_flip_scroll_top()
            .unwrap_or_else(|| max_scroll.saturating_sub(self.transcript_scroll_offset()));
        let anchor = self
            .transcript_view
            .selected_diff_hunk_row
            .unwrap_or(current_top);
        let target = if reverse {
            hunk_rows
                .iter()
                .rev()
                .copied()
                .find(|row| *row < anchor)
                .unwrap_or_else(|| hunk_rows[0])
        } else {
            hunk_rows
                .iter()
                .copied()
                .find(|row| *row > anchor)
                .unwrap_or_else(|| *hunk_rows.last().unwrap_or_abort())
        };

        self.transcript_view.selected_diff_hunk_row = Some(target);
        let target_top = target.min(max_scroll);
        self.set_transcript_scroll_from_top_with_max(target_top, max_scroll);
        true
    }

    #[cfg(test)]
    pub(crate) fn selected_diff_hunk_row_for_test(&self) -> Option<usize> {
        self.transcript_view.selected_diff_hunk_row
    }

    pub(in crate::app) fn scroll_transcript_up(&mut self, amount: u16) {
        if let Some(scroll_top) = self.transcript_page_flip_scroll_top() {
            let max_scroll = self.transcript_view.viewport.max_scroll();
            let next = TranscriptViewport::detached(scroll_top, max_scroll)
                .scroll_up(usize::from(amount.max(1)));
            self.transcript_view.set_measured_viewport(next);
            self.transcript_view.page_flip = self.transcript_view.page_flip.detach_at(next.top());
            return;
        }
        self.cancel_transcript_page_flip();
        let next = self
            .transcript_view
            .measured_viewport()
            .scroll_up(usize::from(amount.max(1)));
        self.transcript_view.set_measured_viewport(next);
    }

    pub(in crate::app) fn transcript_page_scroll_rows(&self) -> u16 {
        u16::try_from(
            self.transcript_view
                .last_transcript_viewport_height
                .saturating_sub(2)
                .max(1),
        )
        .unwrap_or(u16::MAX)
    }

    pub(in crate::app) fn scroll_transcript_down(&mut self, amount: u16) {
        if let Some(scroll_top) = self.transcript_page_flip_scroll_top() {
            let max_scroll = self.transcript_view.viewport.max_scroll();
            let next = TranscriptViewport::detached(scroll_top, max_scroll)
                .scroll_down(usize::from(amount.max(1)));
            self.transcript_view.set_measured_viewport(next);
            if next.is_following() {
                self.cancel_transcript_page_flip();
            } else {
                self.transcript_view.page_flip =
                    self.transcript_view.page_flip.detach_at(next.top());
            }
            return;
        }
        self.cancel_transcript_page_flip();
        let next = self
            .transcript_view
            .measured_viewport()
            .scroll_down(usize::from(amount.max(1)));
        self.transcript_view.set_measured_viewport(next);
    }

    pub(in crate::app) fn release_transcript_page_flip(&mut self) {
        let Some(scroll_top) = self.transcript_page_flip_scroll_top() else {
            self.cancel_transcript_page_flip();
            return;
        };
        let max_scroll = self.transcript_view.viewport.max_scroll();
        self.set_transcript_scroll_from_top_with_max(scroll_top, max_scroll);
    }

    pub(in crate::app) fn set_transcript_scroll_from_top_with_max(
        &mut self,
        scroll_top: usize,
        max_scroll: usize,
    ) {
        let clamped = scroll_top.min(max_scroll);
        let page_flip = self.transcript_view.page_flip;
        self.transcript_view.record_measured_max_scroll(max_scroll);
        let next = self.transcript_view.measured_viewport().detach_at(clamped);
        self.transcript_view.set_measured_viewport(next);
        if next.is_following() {
            self.cancel_transcript_page_flip();
            return;
        }

        self.transcript_view.page_flip = page_flip.detach_at(clamped);
    }

    pub fn transcript_interaction_snapshot(&self) -> TranscriptInteractionSnapshot {
        let viewport = self.transcript_view.measured_viewport();
        TranscriptInteractionSnapshot {
            scroll: viewport.offset_from_bottom(),
            follow_mode: viewport.is_following(),
            selected_activity_index: self.transcript_view.selected_activity_index,
            show_tool_details: self.transcript_view.show_tool_details,
            expanded_tool_call_ids: self
                .transcript_view
                .expanded_tool_outputs
                .iter()
                .cloned()
                .collect(),
        }
    }

    pub fn set_transcript_scroll_for_test(&mut self, scroll: usize) {
        self.transcript_view.set_offset(scroll);
        self.transcript_view.set_following(scroll == 0);
    }

    pub fn set_selected_activity_index_for_test(&mut self, index: usize) {
        self.transcript_view.selected_activity_index = index;
        if self.transcript_view.viewport.max_scroll() == 0 {
            self.transcript_view.record_measured_max_scroll(1);
        }
        self.set_transcript_following(false);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptInteractionSnapshot {
    pub scroll: usize,
    pub follow_mode: bool,
    pub selected_activity_index: usize,
    pub show_tool_details: bool,
    pub expanded_tool_call_ids: Vec<String>,
}

#[cfg(test)]
mod toast_tests {
    use super::*;

    #[test]
    fn ambient_toast_uses_wall_clock_and_pauses_behind_overlays() {
        let mut app = AppState::new_live(None, false, None);
        app.freeze_animation_clock();
        app.show_toast("Saved", ToastVariant::Info);
        assert!(app.toast().is_some());

        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(990));
        app.refresh_motion_state();
        app.palette_visible = true;
        app.refresh_motion_state();
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(990));
        app.refresh_motion_state();
        assert!(app.toast().is_some());

        app.palette_visible = false;
        app.refresh_motion_state();
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(1023));
        app.refresh_motion_state();
        assert!(app.toast().is_none());
    }
}
