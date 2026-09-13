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
        let entries = ui::transcript_navigation_entries(
            self,
            self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
        );
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
        let top = self.transcript_view.measured_viewport().top();
        let target = if forward {
            responses
                .iter()
                .find(|entry| entry.top > top)
                .or_else(|| responses.last())
        } else {
            responses
                .iter()
                .rev()
                .find(|entry| entry.top < top)
                .or_else(|| responses.first())
        };
        let Some(entry) = target else {
            return false;
        };
        self.transcript_view.response_position = responses
            .iter()
            .position(|response| response.id == entry.id)
            .map(|index| crate::transcript_timeline::ResponsePosition {
                index: index + 1,
                total: responses.len(),
            });
        self.select_transcript_entry(entry);
        let viewport = self.transcript_view.measured_viewport();
        self.transcript_view.set_measured_viewport(
            super::transcript_viewport::MeasuredTranscriptViewport::detached(
                entry.top,
                viewport.max_scroll(),
            ),
        );
        true
    }

    pub(crate) fn handle_transcript_search_key(&mut self, key: KeyEvent) -> bool {
        if !self.transcript_view.search_editing {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.transcript_view.search_editing = false,
            KeyCode::Backspace => {
                let _ = self.transcript_view.search_query.pop();
                self.find_transcript_match(None);
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.transcript_view.search_query.push(c);
                self.find_transcript_match(None);
            }
            _ => {}
        }
        true
    }

    pub(crate) fn begin_transcript_search(&mut self) {
        self.transcript_view.search_editing = true;
        self.transcript_view.search_query.clear();
        self.transcript_view.search_match_count = 0;
    }

    pub(crate) fn find_transcript_match(&mut self, forward: Option<bool>) {
        let query = self.transcript_view.search_query.clone();
        if query.is_empty() {
            self.transcript_view.search_match_count = 0;
            return;
        }
        let area = self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let entries = ui::transcript_navigation_entries(self, area);
        let matches = entries
            .iter()
            .filter(|entry| {
                entry.text.contains(&query)
                    || self
                        .selected_entry_content(entry)
                        .content()
                        .contains(&query)
            })
            .collect::<Vec<_>>();
        self.transcript_view.search_match_count = matches.len();
        if matches.is_empty() {
            return;
        }
        let previous = self
            .transcript_view
            .search_match
            .min(matches.len().saturating_sub(1));
        let index = match forward {
            None => 0,
            Some(true) => (previous + 1) % matches.len(),
            Some(false) => (previous + matches.len() - 1) % matches.len(),
        };
        self.transcript_view.search_match = index;
        let entry = matches[index];
        // Reveal recorded content through the existing disclosure owners.
        match &entry.target {
            Some(TranscriptMouseTarget::Reasoning { request_id }) => {
                self.transcript_view
                    .expanded_reasoning_requests
                    .insert(request_id.clone());
            }
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => self.set_tool_output_expanded(tool_call_id, true),
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => {
                self.set_tool_group_outputs_expanded(tool_call_ids, true);
                for id in tool_call_ids {
                    if self
                        .tool_call_entry(id)
                        .is_some_and(|tool| ui::recorded_tool_viewer_text(tool).contains(&query))
                    {
                        self.set_tool_output_expanded(id, true);
                    }
                }
            }
            _ => {}
        }
        let revealed = ui::transcript_navigation_entries(self, area);
        let selected = revealed
            .iter()
            .find(|candidate| match (&entry.target, &candidate.target) {
                (
                    Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }),
                    Some(
                        TranscriptMouseTarget::Tool { tool_call_id }
                        | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
                    ),
                ) => tool_call_ids.contains(tool_call_id) && candidate.text.contains(&query),
                _ => false,
            })
            .or_else(|| revealed.iter().find(|candidate| candidate.id == entry.id))
            .unwrap_or(entry);
        self.select_transcript_entry(selected);
        if let Some(line) = selected.text.lines().position(|line| line.contains(&query)) {
            let viewport = self.transcript_view.measured_viewport();
            self.transcript_view.set_measured_viewport(
                super::transcript_viewport::MeasuredTranscriptViewport::detached(
                    selected.top + line,
                    viewport.max_scroll(),
                ),
            );
        }
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
        let height = self
            .transcript_view
            .last_transcript_viewport_height
            .get()
            .max(1);
        let top = if entry.top < viewport.top() {
            entry.top
        } else if entry.top + entry.height > viewport.top() + height {
            (entry.top + entry.height)
                .saturating_sub(height)
                .min(entry.top)
        } else {
            viewport.top()
        };
        self.transcript_view.set_measured_viewport(
            super::transcript_viewport::MeasuredTranscriptViewport::detached(
                top,
                viewport.max_scroll(),
            ),
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
        let Some(entry) = self.selected_transcript_entry() else {
            return false;
        };
        match entry.target {
            Some(TranscriptMouseTarget::Reasoning { request_id }) => {
                self.toggle_reasoning_expansion(&request_id)
            }
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => {
                let expanded = self
                    .tool_call_entry(&tool_call_id)
                    .is_some_and(|tool| self.tool_output_expanded(tool));
                self.set_tool_output_expanded(&tool_call_id, !expanded);
            }
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => {
                let expanded = tool_call_ids
                    .first()
                    .is_some_and(|id| self.tool_group_expanded(id));
                self.set_tool_group_outputs_expanded(&tool_call_ids, !expanded);
            }
            Some(TranscriptMouseTarget::SubagentSession { session_id }) => {
                self.navigate_to_child_session_id(session_id);
            }
            _ => return false,
        }
        true
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
