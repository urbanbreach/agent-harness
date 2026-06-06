use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::hash::{Hash, Hasher};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastVariant {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToastState {
    pub message: String,
    pub variant: ToastVariant,
    remaining_frames: u16,
}

impl AppState {
    pub(crate) fn transcript_thinking_visible(&self) -> bool {
        self.show_transcript_thinking
    }

    pub(crate) fn transcript_timestamps_visible(&self) -> bool {
        self.show_transcript_timestamps
    }

    pub(crate) fn transcript_animation_phase(&self) -> usize {
        self.transcript_animation_phase
    }

    pub(crate) fn hovered_transcript_target(&self) -> Option<&TranscriptMouseTarget> {
        self.hovered_transcript_target.as_ref()
    }

    pub(crate) fn hovered_subagent_footer_target(&self) -> Option<SubagentFooterTarget> {
        self.hovered_subagent_footer_target
    }

    pub(crate) fn transcript_cache_instance_id(&self) -> u64 {
        self.transcript_cache.instance_id()
    }

    pub(crate) fn transcript_render_cache_key(&self) -> u64 {
        let stamp = self.transcript_render_cache_stamp();
        self.transcript_cache
            .cache_key(stamp, || self.compute_transcript_render_cache_key())
    }

    fn transcript_render_cache_stamp(&self) -> u64 {
        let mut hasher = DefaultHasher::new();

        self.hash_transcript_render_settings(&mut hasher);
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

    fn hash_transcript_render_settings(&self, hasher: &mut impl Hasher) {
        self.replay_mode.hash(hasher);
        self.selected_activity_index.hash(hasher);
        self.show_transcript_thinking.hash(hasher);
        self.show_transcript_timestamps.hash(hasher);
        self.show_tool_details.hash(hasher);
        self.show_generic_tool_output.hash(hasher);
        self.stacked_transcript_diffs.hash(hasher);
        self.transcript_animation_phase.hash(hasher);
        self.hovered_transcript_target.hash(hasher);
        self.transcript_cache.epoch().hash(hasher);
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
        for tool_call_id in &self.expanded_tool_outputs {
            tool_call_id.hash(hasher);
        }
        for file_key in &self.expanded_patch_file_outputs {
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
        self.transcript_animation_phase = self.transcript_animation_phase.wrapping_add(1);
        self.clear_expired_interrupt_confirmation();
        if let Some(toast) = self.toast.as_mut() {
            toast.remaining_frames = toast.remaining_frames.saturating_sub(1);
            if toast.remaining_frames == 0 {
                self.toast = None;
            }
        }
    }

    pub(crate) fn has_active_animations(&self) -> bool {
        self.active_turn_in_progress()
            || self.toast.is_some()
            || self.interrupt_confirmation_pending()
    }

    pub(in crate::app) fn bump_transcript_render_epoch(&mut self) {
        self.transcript_cache.bump_epoch();
    }

    pub(crate) fn tool_details_visible(&self) -> bool {
        self.show_tool_details
    }

    pub(crate) fn generic_tool_output_visible(&self) -> bool {
        self.show_generic_tool_output
    }

    pub(crate) fn stacked_transcript_diffs(&self) -> bool {
        self.stacked_transcript_diffs
    }

    pub(crate) fn tool_output_expanded(&self, tool_call: &ToolCallEntry) -> bool {
        self.expanded_tool_outputs.contains(&tool_call.tool_call_id)
    }

    pub(crate) fn patch_file_output_expanded(&self, tool_call_id: &str, file_path: &str) -> bool {
        self.expanded_patch_file_outputs
            .contains(&Self::patch_file_disclosure_key(tool_call_id, file_path))
    }

    fn patch_file_disclosure_key(tool_call_id: &str, file_path: &str) -> String {
        format!("{tool_call_id}\u{1f}{file_path}")
    }

    fn toggle_tool_output(&mut self, tool_call_id: &str) {
        if !self.expanded_tool_outputs.insert(tool_call_id.to_string()) {
            self.expanded_tool_outputs.remove(tool_call_id);
        }
    }

    fn set_tool_output_expanded(&mut self, tool_call_id: &str, expanded: bool) {
        if expanded {
            self.expanded_tool_outputs.insert(tool_call_id.to_string());
        } else {
            self.expanded_tool_outputs.remove(tool_call_id);
        }
    }

    fn toggle_patch_file_output(&mut self, tool_call_id: &str, file_path: &str) {
        let disclosure_key = Self::patch_file_disclosure_key(tool_call_id, file_path);
        if !self
            .expanded_patch_file_outputs
            .insert(disclosure_key.clone())
        {
            self.expanded_patch_file_outputs.remove(&disclosure_key);
        }
    }

    fn set_tool_group_outputs_expanded(&mut self, tool_call_ids: &[String], expanded: bool) {
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

    fn apply_patch_default_expanded_files(tool_call: &ToolCallEntry) -> Vec<String> {
        if tool_call.effective_tool_id() != "apply_patch" {
            return Vec::new();
        }

        let mut seen = BTreeSet::new();
        let mut files = Vec::new();

        if let Some(edits) = tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("edits"))
            .and_then(serde_json::Value::as_array)
        {
            for edit in edits {
                let Some(path) = edit
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_string)
                else {
                    continue;
                };
                let deleted = edit
                    .get("deleted")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if deleted || !seen.insert(path.clone()) {
                    continue;
                }
                files.push(path);
            }
        }

        if !files.is_empty() {
            return files;
        }

        let Some(rows) = tool_call
            .output_json
            .as_ref()
            .and_then(|value| value.get("files"))
            .and_then(serde_json::Value::as_array)
        else {
            return files;
        };

        for row in rows {
            let Some(row) = row.as_str().map(str::trim).filter(|row| !row.is_empty()) else {
                continue;
            };
            let (status, path) = row
                .split_once(' ')
                .map(|(status, path)| (status.trim(), path.trim()))
                .filter(|(_, path)| !path.is_empty())
                .unwrap_or(("", row));
            if status.eq_ignore_ascii_case("D") {
                continue;
            }
            let path = path.to_string();
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }

        files
    }

    pub(in crate::app) fn seed_apply_patch_file_outputs_for_tool_call(
        &mut self,
        tool_call_id: &str,
    ) {
        let files = self
            .tool_call_entry(tool_call_id)
            .map(Self::apply_patch_default_expanded_files)
            .unwrap_or_default();
        for file_path in files {
            self.expanded_patch_file_outputs
                .insert(Self::patch_file_disclosure_key(tool_call_id, &file_path));
        }
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
            self.expanded_patch_file_outputs.insert(disclosure_key);
        } else {
            self.expanded_patch_file_outputs.remove(&disclosure_key);
        }
    }

    pub(in crate::app) fn activate_transcript_mouse_target(
        &mut self,
        target: TranscriptMouseTarget,
    ) {
        match target {
            TranscriptMouseTarget::FirstSubagentSession => {
                self.navigate_to_first_child_session();
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
                let expand_group = tool_call_ids
                    .iter()
                    .any(|tool_call_id| !self.expanded_tool_outputs.contains(tool_call_id));
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

    pub(in crate::app) fn activate_subagent_footer_target(&mut self, target: SubagentFooterTarget) {
        match target {
            SubagentFooterTarget::Parent => self.navigate_to_parent_session(),
            SubagentFooterTarget::Previous => self.navigate_to_child_sibling(true),
            SubagentFooterTarget::Next => self.navigate_to_child_sibling(false),
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
            .get(self.selected_activity_index)
            .into_iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter(|tool_call| tool_call_has_expandable_output(tool_call))
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect()
    }

    pub(in crate::app) fn set_selected_activity_expandable_outputs(&mut self, expanded: bool) {
        for tool_call_id in self.selected_activity_expandable_tool_ids() {
            if expanded {
                self.expanded_tool_outputs.insert(tool_call_id);
            } else {
                self.expanded_tool_outputs.remove(&tool_call_id);
            }
        }
    }
}

impl AppState {
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.toast = Some(ToastState {
            message: message.into(),
            variant,
            remaining_frames: 30,
        });
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

        let max_scroll = self.last_transcript_max_scroll.get();
        let current_top = if self.follow_mode {
            max_scroll
        } else {
            max_scroll
                .saturating_sub(self.transcript_scroll)
                .min(max_scroll)
        };
        let anchor = self.selected_diff_hunk_row.unwrap_or(current_top);
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
                .unwrap_or_else(|| *hunk_rows.last().expect("non-empty hunk rows"))
        };

        self.selected_diff_hunk_row = Some(target);
        self.follow_mode = false;
        let target_top = target.min(max_scroll);
        self.transcript_scroll = max_scroll.saturating_sub(target_top);
        true
    }

    #[cfg(test)]
    pub(crate) fn selected_diff_hunk_row_for_test(&self) -> Option<usize> {
        self.selected_diff_hunk_row
    }

    pub(in crate::app) fn scroll_transcript_up(&mut self, amount: u16) {
        self.follow_mode = false;
        self.transcript_scroll = self
            .transcript_scroll
            .saturating_add(usize::from(amount.max(1)));
    }

    pub(in crate::app) fn scroll_transcript_down(&mut self, amount: u16) {
        self.transcript_scroll = self
            .transcript_scroll
            .saturating_sub(usize::from(amount.max(1)));
        if self.transcript_scroll == 0 {
            self.follow_mode = true;
        }
    }
}
