// allow: SIZE_OK — TUI app state (session projection + interaction)
use crate::design_contract::LifecycleState;
use crate::prompt_queue_actions::{QueueLifecycle, QueueState};
use crate::scheduling::FrameNow;
use crate::transcript_blocks::{BlockKind, BlockLifecycle, RawDisclosure};
use crate::transcript_identity::ReplayTurn;
use crate::transcript_integration::{
    BlockSeed, TranscriptComposite, TranscriptEvent, TranscriptIntegrationError, TurnSeed,
};
use crate::transcript_pager::{run_pager, PagerCommand, PagerExit, PagerStdio, TerminalControl};
use crate::transcript_scroll::PageFlipState;
use crate::transcript_timeline::TimelineStatus;
use crate::UnwrapOrAbort;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::session_projection::ProjectionDelta;
#[cfg(test)]
use super::transcript_cache::TranscriptRenderCache;
use super::transcript_viewport::MeasuredTranscriptViewport;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastVariant {
    Info,
    Error,
    Mode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToastState {
    pub message: String,
    pub variant: ToastVariant,
    remaining_frames: u16,
}

impl ToastState {
    pub(crate) fn fade_alpha(&self) -> f32 {
        if self.variant != ToastVariant::Mode || self.remaining_frames > 9 {
            1.0
        } else {
            f32::from(self.remaining_frames) / 9.0
        }
    }
}

impl AppState {
    pub fn transcript_view_model(&self) -> Option<&TranscriptViewModel> {
        self.transcript_integration
            .as_ref()
            .map(TranscriptComposite::view)
    }

    pub fn transcript_screen_mode(&self) -> Option<TranscriptScreenMode> {
        self.transcript_integration
            .as_ref()
            .map(|composite| composite.screen().mode())
    }

    pub(crate) fn transcript_viewer(&self) -> Option<&crate::transcript_block_viewer::ViewerState> {
        self.transcript_integration
            .as_ref()
            .and_then(TranscriptComposite::viewer)
    }

    pub(crate) fn sync_transcript_integration(&mut self, animate_tool_transitions: bool) {
        let projection_delta = self.projection.take_transcript_delta();
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
            animate_tool_transitions && !self.replay_mode,
        );
        let lifecycle = if self.active_turn_in_progress() {
            QueueLifecycle::Streaming
        } else {
            QueueLifecycle::Idle
        };
        let state = QueueState::new(lifecycle).with_draft(self.composer.parity_text());
        let _ = self.composer.slice.set_queue_state(state);
        let frame_area = self
            .last_frame_area
            .unwrap_or(ratatui::layout::Rect::new(0, 0, 80, 24));
        let viewport = crate::layout::FrameLayoutPlan::for_app(self, frame_area)
            .transcript
            .unwrap_or(frame_area);
        if self.transcript_integration.is_none() {
            self.transcript_integration = TranscriptComposite::new(viewport).ok();
        }
        let Some(mut composite) = self.transcript_integration.take() else {
            return;
        };
        let _ = composite.resize(viewport);
        match (animate_tool_transitions, projection_delta) {
            (false, ProjectionDelta::ReplayPending) | (true, ProjectionDelta::None) => {}
            (false, _) | (true, ProjectionDelta::FullRebuild | ProjectionDelta::ReplayPending) => {
                let _ = composite.replace_events(transcript_events_for_activities(self));
            }
            (true, ProjectionDelta::Activity { index }) => {
                let incremental = self
                    .activities
                    .get(index)
                    .map(|activity| transcript_events_for_activity(index, activity));
                let incremental_applied =
                    incremental.is_some_and(|events| composite.sync_turn(events).is_ok());
                if !incremental_applied {
                    let _ = composite.replace_events(transcript_events_for_activities(self));
                }
            }
        }
        self.transcript_integration = Some(composite);
    }

    pub fn transcript_following(&self) -> bool {
        if self.transcript_view.page_flip.get().scroll_top().is_some() {
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
        let current = self.transcript_view.page_flip.get();
        let next = current.begin(activity_first_seq);
        if next == current {
            return;
        }
        self.set_transcript_following(true);
        self.transcript_view.page_flip.set(next);
    }

    pub(crate) fn transcript_page_flip_preserving(&self) -> bool {
        self.transcript_view.page_flip.get().is_preserving()
    }

    pub(crate) fn transcript_page_flip_state(&self) -> PageFlipState {
        self.transcript_view.page_flip.get()
    }

    pub(crate) fn transcript_page_flip_scroll_top(&self) -> Option<usize> {
        self.transcript_view.page_flip.get().scroll_top()
    }

    pub(crate) fn set_transcript_page_flip_state(&self, state: PageFlipState) {
        self.transcript_view.page_flip.set(state);
    }

    pub(in crate::app) fn cancel_transcript_page_flip(&self) {
        self.transcript_view
            .page_flip
            .set(self.transcript_view.page_flip.get().cancel());
    }

    pub fn select_transcript_turn_at(&mut self, index: usize) -> bool {
        let Some(turn_id) = self
            .transcript_view_model()
            .and_then(|view| view.turns.get(index).map(|turn| turn.turn_id()))
        else {
            return false;
        };
        self.transcript_view.selected_activity_index = index;
        self.select_transcript_turn(turn_id)
    }

    pub fn toggle_selected_transcript_fold(&mut self) -> bool {
        let Some(request_id) = self
            .activities
            .get(self.transcript_view.selected_activity_index)
            .map(|activity| activity.request_id.clone())
        else {
            return false;
        };
        self.toggle_reasoning_expansion(&request_id);
        true
    }

    pub fn run_transcript_pager<T: TerminalControl>(
        &mut self,
        pager_command: &PagerCommand,
        stdio: PagerStdio,
        terminal: &mut T,
    ) -> Result<PagerExit, TranscriptIntegrationError> {
        let snapshot = self
            .transcript_integration
            .as_mut()
            .ok_or(TranscriptIntegrationError::NoPager)?
            .suspend_pager()?
            .clone();
        let pager_result = run_pager(&snapshot, pager_command, stdio, terminal);
        let restore_result = self
            .transcript_integration
            .as_mut()
            .ok_or(TranscriptIntegrationError::NoPager)?
            .restore_pager();
        restore_result?;
        pager_result.map_err(Into::into)
    }

    pub(crate) fn open_selected_transcript_viewer(&mut self) -> bool {
        let Some(block_id) = self.transcript_integration.as_ref().and_then(|composite| {
            composite
                .view()
                .turns
                .get(self.transcript_view.selected_activity_index)
                .map(|turn| turn.replay_turn().block_id(0))
        }) else {
            return false;
        };
        self.transcript_integration
            .as_mut()
            .is_some_and(|composite| composite.open_viewer(block_id).is_ok())
    }

    pub(crate) fn close_transcript_viewer(&mut self) -> bool {
        self.transcript_integration
            .as_mut()
            .is_some_and(|composite| composite.close_viewer().is_ok())
    }

    pub(crate) fn handle_transcript_viewer_key(&mut self, key: KeyEvent) -> bool {
        if self.transcript_screen_mode() != Some(TranscriptScreenMode::SelectedBlockViewer) {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.close_transcript_viewer(),
            KeyCode::Char('r') | KeyCode::Char('R') => self
                .transcript_integration
                .as_mut()
                .and_then(TranscriptComposite::viewer_mut)
                .is_some_and(|viewer| viewer.toggle_mode().is_ok()),
            KeyCode::Char('n') | KeyCode::Char('N') => self
                .transcript_integration
                .as_mut()
                .and_then(TranscriptComposite::viewer_mut)
                .is_some_and(|viewer| {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        let _ = viewer.search_backward();
                    } else {
                        let _ = viewer.search_forward();
                    }
                    true
                }),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let Some(viewer) = self.transcript_viewer() else {
                    return false;
                };
                let Some(text) = viewer.copy_selection_text().ok() else {
                    return true;
                };
                match clipboard::copy(&text) {
                    Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
                    Err(error) => self.show_toast(
                        format!("clipboard copy failed: {error}"),
                        ToastVariant::Error,
                    ),
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn select_transcript_turn(&mut self, turn_id: TurnId) -> bool {
        let selected = self
            .transcript_integration
            .as_mut()
            .is_some_and(|composite| composite.select_turn(turn_id).is_ok());
        if selected {
            self.cancel_transcript_page_flip();
        }
        selected
    }

    pub(crate) fn handle_transcript_timeline_key(&mut self, key: KeyEvent) -> bool {
        let Some(jump) = crate::transcript_timeline::navigation::key_jump(key) else {
            return false;
        };
        let jumped = self
            .transcript_integration
            .as_mut()
            .is_some_and(|composite| composite.jump(jump).is_ok());
        if jumped {
            self.cancel_transcript_page_flip();
        }
        jumped
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

    pub(crate) fn transcript_cache_instance_id(&self) -> u64 {
        self.transcript_view.transcript_cache.instance_id()
    }

    pub(crate) fn transcript_render_cache_key(&self) -> u64 {
        let stamp = self.transcript_render_cache_stamp();
        self.transcript_view
            .transcript_cache
            .cache_key(stamp, || self.compute_transcript_render_cache_key())
    }

    #[cfg(test)]
    pub(crate) fn transcript_measure_cache_key(&self) -> u64 {
        let stamp = self.transcript_measure_cache_stamp();
        self.transcript_view
            .transcript_cache
            .cache_key(stamp, || self.compute_transcript_measure_cache_key())
    }

    pub(crate) fn transcript_selection_cache_key(&self) -> u64 {
        let stamp = self.transcript_measure_cache_stamp();
        self.transcript_view
            .transcript_cache
            .selection_cache_key(stamp, || self.compute_transcript_measure_cache_key())
    }

    fn transcript_render_cache_stamp(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_render_settings(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        hasher.finish()
    }

    fn transcript_measure_cache_stamp(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_measure_settings(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        hasher.finish()
    }

    fn compute_transcript_render_cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_render_settings(&mut hasher);
        self.hash_transcript_content(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        for (permission_id, summary) in self.transcript_pending_permissions() {
            permission_id.hash(&mut hasher);
            summary.hash(&mut hasher);
        }

        hasher.finish()
    }

    fn compute_transcript_measure_cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_measure_settings(&mut hasher);
        self.hash_transcript_content(&mut hasher);
        self.hash_transcript_render_expansions(&mut hasher);

        for (permission_id, summary) in self.transcript_pending_permissions() {
            permission_id.hash(&mut hasher);
            summary.hash(&mut hasher);
        }

        hasher.finish()
    }

    fn hash_transcript_render_settings(&self, hasher: &mut impl Hasher) {
        self.replay_mode.hash(hasher);
        self.transcript_view.selected_activity_index.hash(hasher);
        self.transcript_view.show_transcript_thinking.hash(hasher);
        self.transcript_view.show_transcript_timestamps.hash(hasher);
        self.transcript_view.show_tool_details.hash(hasher);
        self.transcript_view.show_generic_tool_output.hash(hasher);
        self.transcript_view.stacked_transcript_diffs.hash(hasher);
        self.transcript_view.hovered_transcript_target.hash(hasher);
        self.transcript_view.transcript_cache.epoch().hash(hasher);
        self.active_profile().hash(hasher);
        self.session_path.hash(hasher);
    }

    fn hash_transcript_measure_settings(&self, hasher: &mut impl Hasher) {
        self.replay_mode.hash(hasher);
        self.transcript_view.selected_activity_index.hash(hasher);
        self.transcript_view.show_transcript_thinking.hash(hasher);
        self.transcript_view.show_transcript_timestamps.hash(hasher);
        self.transcript_view.show_tool_details.hash(hasher);
        self.transcript_view.show_generic_tool_output.hash(hasher);
        self.transcript_view.stacked_transcript_diffs.hash(hasher);
        self.transcript_view.transcript_cache.epoch().hash(hasher);
        self.active_profile().hash(hasher);
        self.session_path.hash(hasher);
    }

    fn hash_transcript_content(&self, hasher: &mut impl Hasher) {
        for activity in &self.activities {
            activity.request_id.hash(hasher);
            activity.profile_label.hash(hasher);
            activity.model_id.hash(hasher);
            activity.provider_id.hash(hasher);
            activity.status.hash(hasher);
            activity.user_timestamp.hash(hasher);
            activity.thinking_text.hash(hasher);
            activity.transcript_text.hash(hasher);
            activity.error_message.hash(hasher);
            activity.first_seq.hash(hasher);
            activity.last_seq.hash(hasher);
            activity.revision.hash(hasher);

            if let Some(user_message) = activity.user_message.as_ref() {
                user_message.request_id.hash(hasher);
                user_message.text.hash(hasher);
            }

            for permission in &activity.permissions {
                permission.permission_id.hash(hasher);
                permission.kind.hash(hasher);
                permission.tool_call_id.hash(hasher);
                permission.summary.hash(hasher);
                permission.request_digest.hash(hasher);
                permission.timeout_ms.hash(hasher);
                std::mem::discriminant(&permission.default_decision).hash(hasher);
                permission.resolution_reason.hash(hasher);
                permission.first_seq.hash(hasher);
                permission.last_seq.hash(hasher);
            }

            for tool_call in &activity.tool_calls {
                tool_call.tool_call_id.hash(hasher);
                tool_call.tool_id.hash(hasher);
                tool_call.canonical_tool_id.hash(hasher);
                tool_call.alias_source_tool_id.hash(hasher);
                tool_call.args_digest.hash(hasher);
                tool_call.output_digest.hash(hasher);
                tool_call.output_summary.hash(hasher);
                tool_call.first_seq.hash(hasher);
                tool_call.last_seq.hash(hasher);
                std::mem::discriminant(&tool_call.status).hash(hasher);

                if let Some(edit) = tool_call.edit.as_ref() {
                    edit.edit_id.hash(hasher);
                    edit.path.hash(hasher);
                    std::mem::discriminant(&edit.status).hash(hasher);
                    edit.summary.hash(hasher);
                    edit.patch_digest.hash(hasher);
                    edit.new_file_digest.hash(hasher);
                    edit.diff_rel_path.hash(hasher);
                    edit.diff_digest.hash(hasher);
                    edit.rejection_reason.hash(hasher);
                }

                for artifact in &tool_call.artifact_refs {
                    artifact.path.hash(hasher);
                    artifact.digest.hash(hasher);
                }
            }
        }
    }

    fn hash_transcript_render_expansions(&self, hasher: &mut impl Hasher) {
        for request_id in &self.transcript_view.expanded_reasoning_requests {
            request_id.hash(hasher);
        }
        for tool_call_id in &self.transcript_view.expanded_tool_outputs {
            tool_call_id.hash(hasher);
        }
        for file_key in &self.transcript_view.expanded_patch_file_outputs {
            file_key.hash(hasher);
        }
    }

    #[cfg(test)]
    pub(crate) fn reset_transcript_render_key_metrics_for_test() {
        TranscriptRenderCache::reset_build_metrics_for_test();
    }

    #[cfg(test)]
    pub(crate) fn transcript_render_key_build_count_for_test() -> usize {
        TranscriptRenderCache::build_count_for_test()
    }

    pub(crate) fn advance_transcript_animation_phase(&mut self) {
        self.advance_transcript_animation_phase_with_motion(
            std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_none(),
        );
    }

    fn advance_transcript_animation_phase_with_motion(&mut self, motion_enabled: bool) {
        self.transcript_view.transcript_animation_phase = self
            .transcript_view
            .transcript_animation_phase
            .wrapping_add(1);
        if self.transcript_view.tool_motion.advance(motion_enabled) {
            self.bump_transcript_render_epoch();
        }
        self.clear_expired_interrupt_confirmation();
        let toast_occluded = self.overlay_stack().top().is_some();
        if !toast_occluded {
            if let Some(toast) = self.toast.as_mut() {
                toast.remaining_frames = toast.remaining_frames.saturating_sub(1);
                if toast.remaining_frames == 0 {
                    self.toast = None;
                }
            }
        }
        if let Some(composite) = self.transcript_integration.as_mut() {
            let animation_ms = u64::try_from(self.transcript_view.transcript_animation_phase)
                .unwrap_or(u64::MAX)
                .saturating_mul(crate::scheduling::active_animation_period_ms());
            let _ = composite.tick_at(FrameNow {
                animation_ms,
                flush_ms: animation_ms,
            });
        }
    }

    /// Public fixed-tick advance for A-ANIMATION evidence capture (no wall clock).
    pub fn advance_animation_tick_for_evidence(&mut self) {
        self.advance_transcript_animation_phase();
    }

    pub fn advance_animation_tick_with_motion_for_evidence(&mut self, motion_enabled: bool) {
        self.advance_transcript_animation_phase_with_motion(motion_enabled);
    }

    /// Current transcript animation phase for evidence metadata.
    pub fn animation_phase_for_evidence(&self) -> usize {
        self.transcript_animation_phase()
    }

    pub(crate) fn has_active_animations(&self) -> bool {
        let motion_enabled = std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_none();
        self.has_active_animations_with_motion(motion_enabled)
    }

    fn has_active_animations_with_motion(&self, motion_enabled: bool) -> bool {
        self.animation_tick_interval_with_motion(motion_enabled)
            .is_some()
    }

    pub(crate) fn starting_session_seed_visible(&self) -> bool {
        self.starting_session_seed
    }

    pub(crate) fn set_starting_session_seed(&mut self, visible: bool) {
        self.starting_session_seed = visible;
    }

    /// Whether the shell currently requests animation ticks (evidence / tests).
    pub fn has_active_animations_for_evidence(&self) -> bool {
        self.has_active_animations()
    }

    pub fn has_active_animations_with_motion_for_evidence(&self, motion_enabled: bool) -> bool {
        self.has_active_animations_with_motion(motion_enabled)
    }

    pub(crate) fn tool_finish_flash_remaining(&self, tool_call_id: &str) -> Option<u8> {
        self.transcript_view
            .tool_motion
            .finish_flash_remaining(tool_call_id)
    }

    pub(crate) fn record_visible_running_tool_motion(&self, visible: bool) {
        self.transcript_view
            .visible_running_tool_motion
            .set(visible);
    }

    pub fn record_visible_running_tool_motion_for_evidence(&self, visible: bool) {
        self.record_visible_running_tool_motion(visible);
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
                }
                ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => {}
            }
        }
        super::transcript_view::active_turn_motion_demand(
            has_active_tool,
            has_running_tool,
            self.transcript_view.visible_running_tool_motion.get(),
        )
    }

    pub(in crate::app) fn bump_transcript_render_epoch(&mut self) {
        self.transcript_view.transcript_cache.bump_epoch();
    }

    pub(crate) fn tool_details_visible(&self) -> bool {
        self.transcript_view.show_tool_details
    }

    pub(crate) fn reasoning_expanded(&self, request_id: &str) -> bool {
        self.transcript_view
            .expanded_reasoning_requests
            .contains(request_id)
    }

    fn toggle_reasoning_expansion(&mut self, request_id: &str) {
        if !self
            .transcript_view
            .expanded_reasoning_requests
            .insert(request_id.to_string())
        {
            self.transcript_view
                .expanded_reasoning_requests
                .remove(request_id);
        }
    }

    pub(crate) fn generic_tool_output_visible(&self) -> bool {
        self.transcript_view.show_generic_tool_output
    }

    pub(crate) fn stacked_transcript_diffs(&self) -> bool {
        self.transcript_view.stacked_transcript_diffs
    }

    pub(crate) fn tool_output_expanded(&self, tool_call: &ToolCallEntry) -> bool {
        self.transcript_view
            .expanded_tool_outputs
            .contains(&tool_call.tool_call_id)
            || self
                .transcript_view
                .expanded_patch_file_outputs
                .iter()
                .any(|key| key.starts_with(&format!("{}\u{1f}", tool_call.tool_call_id)))
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
            .and_then(|output| output.get("edits"))
            .and_then(serde_json::Value::as_array)
        else {
            return;
        };
        for edit in edits {
            if edit.get("deleted").and_then(serde_json::Value::as_bool) == Some(true) {
                continue;
            }
            let Some(path) = edit.get("path").and_then(serde_json::Value::as_str) else {
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

    fn toggle_tool_output(&mut self, tool_call_id: &str) {
        if !self
            .transcript_view
            .expanded_tool_outputs
            .insert(tool_call_id.to_string())
        {
            self.transcript_view
                .expanded_tool_outputs
                .remove(tool_call_id);
        }
    }

    fn set_tool_output_expanded(&mut self, tool_call_id: &str, expanded: bool) {
        if expanded {
            self.transcript_view
                .expanded_tool_outputs
                .insert(tool_call_id.to_string());
        } else {
            self.transcript_view
                .expanded_tool_outputs
                .remove(tool_call_id);
        }
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
    }

    pub(super) fn set_tool_group_outputs_expanded(
        &mut self,
        tool_call_ids: &[String],
        expanded: bool,
    ) {
        for tool_call_id in tool_call_ids {
            self.set_tool_output_expanded(tool_call_id, expanded);
        }
    }

    fn tool_call_entry(&self, tool_call_id: &str) -> Option<&ToolCallEntry> {
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
    }

    pub(in crate::app) fn activate_transcript_mouse_target(
        &mut self,
        target: TranscriptMouseTarget,
    ) {
        match target {
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
                let expand_group = tool_call_ids.iter().any(|tool_call_id| {
                    !self
                        .transcript_view
                        .expanded_tool_outputs
                        .contains(tool_call_id)
                });
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
            if expanded {
                self.transcript_view
                    .expanded_tool_outputs
                    .insert(tool_call_id);
            } else {
                self.transcript_view
                    .expanded_tool_outputs
                    .remove(&tool_call_id);
            }
        }
    }
}

fn transcript_events_for_activities(app: &AppState) -> Vec<TranscriptEvent> {
    let mut events = Vec::new();
    for (activity_index, activity) in app.activities.iter().enumerate() {
        events.extend(transcript_events_for_activity(activity_index, activity));
    }
    events
}

fn transcript_events_for_activity(
    activity_index: usize,
    activity: &ActivityEntry,
) -> Vec<TranscriptEvent> {
    let mut blocks = Vec::new();
    if let Some(user_message) = activity.user_message.as_ref() {
        blocks.push((BlockKind::User, user_message.text.clone(), None));
    }
    if !activity.thinking_text.is_empty() {
        blocks.push((BlockKind::Thinking, activity.thinking_text.clone(), None));
    }
    if !activity.transcript_text.is_empty() {
        blocks.push((BlockKind::Assistant, activity.transcript_text.clone(), None));
    }
    for tool in &activity.tool_calls {
        let content = tool
            .output_summary
            .as_deref()
            .unwrap_or(tool.args_summary.as_str())
            .to_owned();
        let raw = tool.output_json.as_ref().map(RawDisclosure::from_json);
        blocks.push((BlockKind::Tool, content, raw));
    }
    if blocks.is_empty() {
        blocks.push((BlockKind::System, activity.status.to_string(), None));
    }

    let turn_index = u64::try_from(activity_index).unwrap_or(u64::MAX);
    let replay = ReplayTurn::event(
        activity.first_seq.max(1),
        turn_index,
        u64::try_from(blocks.len()).unwrap_or(u64::MAX),
    );
    let mut events = Vec::with_capacity(blocks.len().saturating_add(1));
    events.push(TranscriptEvent::TurnStarted(TurnSeed::new(
        replay,
        timeline_status(activity.status),
        lifecycle_state(activity),
    )));
    events.extend(
        blocks
            .into_iter()
            .enumerate()
            .map(|(block_index, (kind, content, raw))| {
                let block_index = u64::try_from(block_index).unwrap_or(u64::MAX);
                TranscriptEvent::BlockCreated(BlockSeed {
                    id: replay.block_id(block_index),
                    turn_id: replay.turn_id(),
                    kind,
                    lifecycle: block_lifecycle(activity.status),
                    content,
                    raw,
                })
            }),
    );
    events
}

fn timeline_status(status: ActivityStatus) -> TimelineStatus {
    match status {
        ActivityStatus::Queued => TimelineStatus::Queued,
        ActivityStatus::Streaming => TimelineStatus::Streaming,
        ActivityStatus::Done => TimelineStatus::Completed,
        ActivityStatus::Error => TimelineStatus::Failed,
    }
}

fn lifecycle_state(activity: &ActivityEntry) -> LifecycleState {
    match activity.status {
        ActivityStatus::Queued => LifecycleState::Queued,
        ActivityStatus::Streaming if !activity.tool_calls.is_empty() => LifecycleState::Tool,
        ActivityStatus::Streaming if !activity.thinking_text.is_empty() => LifecycleState::Thinking,
        ActivityStatus::Streaming => LifecycleState::Streaming,
        ActivityStatus::Done => LifecycleState::Completed,
        ActivityStatus::Error => LifecycleState::Failed,
    }
}

fn block_lifecycle(status: ActivityStatus) -> BlockLifecycle {
    match status {
        ActivityStatus::Queued => BlockLifecycle::Waiting,
        ActivityStatus::Streaming => BlockLifecycle::Streaming,
        ActivityStatus::Done => BlockLifecycle::Completed,
        ActivityStatus::Error => BlockLifecycle::Failed,
    }
}

impl AppState {
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.toast = Some(ToastState {
            message: message.into(),
            variant,
            remaining_frames: 90,
        });
    }

    pub(in crate::app) fn show_mode_banner(&mut self, message: impl Into<String>) {
        self.toast = Some(ToastState {
            message: message.into(),
            variant: ToastVariant::Mode,
            remaining_frames: 69,
        });
    }

    pub fn mode_banner_alpha_for_evidence(&self) -> Option<f32> {
        self.toast
            .as_ref()
            .filter(|toast| toast.variant == ToastVariant::Mode)
            .map(ToastState::fade_alpha)
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

        let max_scroll = self.transcript_view.last_transcript_max_scroll.get();
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
            let max_scroll = self.transcript_view.last_transcript_max_scroll.get();
            let next = MeasuredTranscriptViewport::detached(scroll_top, max_scroll)
                .scroll_up(usize::from(amount.max(1)));
            self.transcript_view.set_measured_viewport(next);
            self.transcript_view
                .page_flip
                .set(self.transcript_view.page_flip.get().detach_at(next.top()));
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
                .get()
                .saturating_sub(2)
                .max(1),
        )
        .unwrap_or(u16::MAX)
    }

    pub(in crate::app) fn scroll_transcript_down(&mut self, amount: u16) {
        if let Some(scroll_top) = self.transcript_page_flip_scroll_top() {
            let max_scroll = self.transcript_view.last_transcript_max_scroll.get();
            let next = MeasuredTranscriptViewport::detached(scroll_top, max_scroll)
                .scroll_down(usize::from(amount.max(1)));
            self.transcript_view.set_measured_viewport(next);
            if next.is_following() {
                self.cancel_transcript_page_flip();
            } else {
                self.transcript_view
                    .page_flip
                    .set(self.transcript_view.page_flip.get().detach_at(next.top()));
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
        let max_scroll = self.transcript_view.last_transcript_max_scroll.get();
        self.set_transcript_scroll_from_top_with_max(scroll_top, max_scroll);
    }

    pub(in crate::app) fn set_transcript_scroll_from_top_with_max(
        &mut self,
        scroll_top: usize,
        max_scroll: usize,
    ) {
        let clamped = scroll_top.min(max_scroll);
        let page_flip = self.transcript_view.page_flip.get();
        self.transcript_view.record_measured_max_scroll(max_scroll);
        let next = self.transcript_view.measured_viewport().detach_at(clamped);
        self.transcript_view.set_measured_viewport(next);
        if next.is_following() {
            self.cancel_transcript_page_flip();
            return;
        }

        self.transcript_view
            .page_flip
            .set(page_flip.detach_at(clamped));
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
        self.transcript_view.transcript_scroll = scroll;
        self.transcript_view.follow_mode = scroll == 0;
    }

    pub fn set_selected_activity_index_for_test(&mut self, index: usize) {
        self.transcript_view.selected_activity_index = index;
        if self.transcript_view.last_transcript_max_scroll.get() == 0 {
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
    fn ambient_toast_lasts_ninety_ticks_and_pauses_behind_overlays() {
        // arrange
        // act
        // assert
        let mut app = AppState::new_live(None, false, None);
        app.show_toast("Saved", ToastVariant::Info);
        assert_eq!(app.toast().map(|toast| toast.remaining_frames), Some(90));

        for _ in 0..45 {
            app.advance_animation_tick_for_evidence();
        }
        app.palette_visible = true;
        for _ in 0..30 {
            app.advance_animation_tick_for_evidence();
        }
        assert_eq!(app.toast().map(|toast| toast.remaining_frames), Some(45));

        app.palette_visible = false;
        for _ in 0..45 {
            app.advance_animation_tick_for_evidence();
        }
        assert!(app.toast().is_none());
    }
}
