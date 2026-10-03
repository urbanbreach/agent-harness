use super::*;
use crate::ui::{TranscriptMouseTarget, TranscriptNavigationEntry};

impl AppState {
    /// Select a visible tool entry, or its collapsed group when the entry is hidden.
    pub fn select_transcript_tool(&mut self, tool_call_id: &str) -> bool {
        let entries = ui::transcript_navigation_entries(
            self,
            self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
        );
        let entry = entries.iter().find(|entry| matches!(&entry.target,
            Some(TranscriptMouseTarget::Tool { tool_call_id: id }) if id == tool_call_id))
            .or_else(|| entries.iter().find(|entry| matches!(&entry.target,
                Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) if tool_call_ids.iter().any(|id| id == tool_call_id))));
        let Some(entry) = entry else {
            return false;
        };
        self.select_transcript_entry(entry);
        true
    }

    pub(crate) fn jump_transcript_response(&mut self, forward: bool) -> bool {
        let area = self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let entries = ui::transcript_navigation_entries(self, area);
        let responses = entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| {
                entry.kind == crate::ui::TranscriptRenderSurfaceKind::AssistantBody
                    && entries
                        .get(index + 1)
                        .is_none_or(|next| next.activity_first_seq != entry.activity_first_seq)
            })
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        if responses.is_empty() {
            return false;
        }
        let viewport = self.transcript_view.measured_viewport();
        // Several responses can share a clamped viewport. Repeated jumps keep
        // their selected identity; scrolling elsewhere starts from that position.
        let current = self
            .transcript_view
            .response_position
            .and_then(|_| {
                responses
                    .iter()
                    .position(|entry| Some(entry.id) == self.transcript_view.selected_entry)
            })
            .filter(|index| {
                ui::transcript_entry_scroll_top(self, area, responses[*index].top)
                    .is_some_and(|top| top.min(viewport.max_scroll()) == viewport.top())
            });
        let index = match (current, forward) {
            (Some(index), true) => (index + 1).min(responses.len() - 1),
            (Some(index), false) => index.saturating_sub(1),
            (None, true) => responses
                .iter()
                .position(|entry| entry.top > viewport.top())
                .unwrap_or(responses.len() - 1),
            (None, false) => responses
                .iter()
                .rposition(|entry| entry.top < viewport.top())
                .unwrap_or(0),
        };
        let entry = responses[index];
        let top = ui::transcript_entry_scroll_top(self, area, entry.top).unwrap_or(entry.top);
        self.select_transcript_entry(entry);
        self.transcript_view.set_measured_viewport(
            super::transcript_viewport::TranscriptViewport::detached(top, entry.max_scroll),
        );
        self.transcript_view.response_position =
            Some(crate::transcript_timeline::ResponsePosition {
                index: index + 1,
                total: responses.len(),
            });
        true
    }

    pub(crate) fn selected_transcript_entry(&self) -> Option<TranscriptNavigationEntry> {
        let entries = ui::transcript_navigation_entries(
            self,
            self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
        );
        self.transcript_view
            .selected_entry
            .and_then(|id| entries.iter().find(|entry| entry.id == id).cloned())
            .or_else(|| {
                let seq = self
                    .activities
                    .get(self.transcript_view.selected_activity_index)?
                    .first_seq;
                entries
                    .into_iter()
                    .find(|entry| entry.activity_first_seq == seq)
            })
    }

    pub(crate) fn move_transcript_entry(&mut self, forward: bool) -> bool {
        let entries = ui::transcript_navigation_entries(
            self,
            self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
        );
        let selected = self.selected_transcript_entry();
        let current = selected
            .and_then(|selected| entries.iter().position(|entry| entry.id == selected.id))
            .unwrap_or(0);
        let next = if forward {
            current
                .saturating_add(1)
                .min(entries.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        let Some(entry) = entries.get(next) else {
            return false;
        };
        self.select_transcript_entry(entry);
        true
    }

    pub(crate) fn select_transcript_entry(&mut self, entry: &TranscriptNavigationEntry) {
        self.transcript_view.highlighted_link = None;
        self.transcript_view.selected_entry = Some(entry.id);
        if let Some(index) = self
            .activities
            .iter()
            .position(|activity| activity.first_seq == entry.activity_first_seq)
        {
            self.transcript_view.selected_activity_index = index;
        }
        self.cancel_transcript_page_flip();
        let viewport = self.transcript_view.measured_viewport();
        let height = self.transcript_view.last_transcript_viewport_height.max(1);
        let entry_top = if self.current_subagent_session_present() {
            ui::transcript_entry_scroll_top(
                self,
                self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
                entry.top,
            )
            .unwrap_or(entry.top)
        } else {
            entry.top
        };
        let top = if entry_top < viewport.top() {
            entry_top
        } else if entry.top + entry.height > viewport.top() + height {
            (entry.top + entry.height)
                .saturating_sub(height)
                .min(entry_top)
        } else {
            viewport.top()
        };
        self.transcript_view.set_measured_viewport(
            super::transcript_viewport::TranscriptViewport::detached(top, viewport.max_scroll()),
        );
    }

    pub(crate) fn activate_selected_transcript_entry(&mut self) -> bool {
        let Some(entry) = self.selected_transcript_entry() else {
            return false;
        };
        if self.transcript_view.selected_entry.is_none() && entry.target.is_none() {
            return false;
        }
        let collapsed_group = matches!(entry.target, Some(TranscriptMouseTarget::ToolGroup { ref tool_call_ids })
            if !tool_call_ids.first().is_some_and(|id| self.tool_group_expanded(id)));
        if collapsed_group {
            self.fold_selected_entry()
        } else {
            self.open_selected_transcript_viewer()
        }
    }

    pub(crate) fn fold_selected_entry(&mut self) -> bool {
        self.fold_selected_entry_to(None)
    }

    pub(crate) fn set_selected_entry_expanded(&mut self, expanded: bool) -> bool {
        self.fold_selected_entry_to(Some(expanded))
    }

    fn fold_selected_entry_to(&mut self, expand: Option<bool>) -> bool {
        let Some(entry) = self.selected_transcript_entry() else {
            return false;
        };
        match entry.target {
            Some(TranscriptMouseTarget::Reasoning { request_id }) => {
                if expand.is_none_or(|expanded| expanded != self.reasoning_expanded(&request_id)) {
                    self.toggle_reasoning_expansion(&request_id);
                }
            }
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => {
                let expanded = self
                    .tool_call_entry(&tool_call_id)
                    .is_some_and(|tool| self.tool_output_expanded(tool));
                self.set_tool_output_expanded(&tool_call_id, expand.unwrap_or(!expanded));
            }
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => {
                let expanded = tool_call_ids
                    .first()
                    .is_some_and(|id| self.tool_group_expanded(id));
                self.set_tool_group_outputs_expanded(&tool_call_ids, expand.unwrap_or(!expanded));
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn child_viewer_entry_running(&self, entry: &TranscriptNavigationEntry) -> bool {
        match &entry.target {
            Some(TranscriptMouseTarget::Tool { tool_call_id }) => self
                .tool_call_entry(tool_call_id)
                .is_some_and(|tool| tool.status == ToolCallDisplayStatus::Running),
            _ if entry.kind == crate::ui::TranscriptRenderSurfaceKind::AssistantBody => self
                .current_session_id()
                .and_then(|id| self.projection.subagents.history.records.get(id))
                .is_some_and(|child| {
                    child.outcome.is_none()
                        && self.activities.iter().any(|activity| {
                            activity.first_seq == entry.activity_first_seq
                                && child.lifecycle.current_attempt_id()
                                    == Some(activity.request_id.as_str())
                        })
                }),
            _ => false,
        }
    }

    pub(crate) fn selected_entry_content(
        &self,
        entry: &TranscriptNavigationEntry,
    ) -> crate::transcript_block_viewer::ViewerBlockContent {
        let tool_ids = match &entry.target {
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => vec![tool_call_id.as_str()],
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => {
                tool_call_ids.iter().map(String::as_str).collect()
            }
            _ => Vec::new(),
        };
        if let [id] = tool_ids.as_slice() {
            if let Some(tool) = self.tool_call_entry(id) {
                return ui::recorded_tool_viewer_content(tool);
            }
        }
        if !tool_ids.is_empty() {
            let text = tool_ids
                .iter()
                .filter_map(|id| self.tool_call_entry(id))
                .map(ui::recorded_tool_viewer_text)
                .collect::<Vec<_>>()
                .join("\n\n");
            return crate::transcript_block_viewer::ViewerBlockContent::new(&text, Some(&text));
        }
        let text = entry.source_text.as_deref().unwrap_or(&entry.text);
        if matches!(
            entry.kind,
            crate::ui::TranscriptRenderSurfaceKind::AssistantBody
                | crate::ui::TranscriptRenderSurfaceKind::AssistantReasoning
        ) {
            crate::transcript_block_viewer::ViewerBlockContent::markdown(text)
        } else {
            crate::transcript_block_viewer::ViewerBlockContent::new(text, None)
        }
    }
}
