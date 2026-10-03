use super::pane_query::{PaneQuery, PaneQueryMode};
use super::*;

impl AppState {
    pub(crate) fn handle_transcript_search_key(&mut self, key: KeyEvent) -> bool {
        let child = self.current_subagent_session_present();
        if !self.transcript_view.search.has_bar()
            || (!child && !self.transcript_view.search.editing)
            || (child && key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL)
        {
            return false;
        }
        if key.kind == crossterm::event::KeyEventKind::Release {
            return true;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) if !child => {
                self.transcript_view.search.editing = false;
            }
            (KeyCode::Esc, _) => self.transcript_view.search = PaneQuery::default(),
            (KeyCode::Down, KeyModifiers::NONE) if child => self.find_transcript_match(Some(true)),
            (KeyCode::Up, KeyModifiers::NONE) if child => self.find_transcript_match(Some(false)),
            (KeyCode::Enter, _) if self.transcript_view.search.editing => {
                self.transcript_view.search.editing = false;
            }
            _ if self.transcript_view.search.editing => {
                let before = self.transcript_view.search.editor.text();
                if let Err(error) = self.transcript_view.search.edit_key(key) {
                    self.status_banner = Some(error.to_string());
                }
                if before != self.transcript_view.search.editor.text() {
                    self.find_transcript_match(None);
                }
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) => self.find_transcript_match(Some(true)),
            (KeyCode::Char('N'), modifiers)
                if !modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.find_transcript_match(Some(false));
            }
            _ => return false,
        }
        true
    }

    pub(super) fn handle_transcript_search_paste(&mut self, text: &str) -> bool {
        if !self.transcript_view.search.has_bar() {
            return false;
        }
        if self.transcript_view.search.editing {
            if let Err(error) = self.transcript_view.search.paste(text) {
                self.status_banner = Some(error.to_string());
            }
            self.find_transcript_match(None);
        }
        true
    }

    pub(crate) fn begin_transcript_search(&mut self) {
        self.transcript_view.search = PaneQuery::default();
        self.transcript_view.search.open(PaneQueryMode::Search);
        self.transcript_view.search_match = 0;
        self.transcript_view.search_match_count = 0;
    }

    pub(crate) fn find_transcript_match(&mut self, forward: Option<bool>) {
        let native = self.current_subagent_session_present();
        let query = self.transcript_view.search.editor.text();
        let regex = if native {
            self.transcript_view.search.regex.clone()
        } else if query.is_empty() {
            None
        } else {
            regex::Regex::new(&regex::escape(&query)).ok()
        };
        let Some(regex) = regex else {
            self.transcript_view.search_match_count = 0;
            return;
        };
        let area = self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let entries = ui::transcript_navigation_entries(self, area);
        let mut matches = Vec::new();
        for entry in &entries {
            let content = self.selected_entry_content(entry);
            let text = if regex.is_match(&entry.text) {
                entry.text.as_ref()
            } else {
                content.content()
            };
            for found in regex
                .find_iter(text)
                .filter(|found| !found.is_empty())
                .take(if native { usize::MAX } else { 1 })
            {
                let line = text[..found.start()]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count();
                matches.push((entry, line));
            }
        }
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
        let (entry, line) = matches[index];
        self.reveal_transcript_search_entry(entry, &regex);
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
                ) => tool_call_ids.contains(tool_call_id) && regex.is_match(&candidate.text),
                _ => false,
            })
            .or_else(|| revealed.iter().find(|candidate| candidate.id == entry.id))
            .unwrap_or(entry);
        self.select_transcript_entry(selected);
        let line = selected
            .text
            .lines()
            .enumerate()
            .filter(|(_, text)| regex.is_match(text))
            .min_by_key(|(index, _)| index.abs_diff(line))
            .map_or(0, |(index, _)| index);
        if let Some(top) = ui::transcript_entry_scroll_top(self, area, selected.top + line) {
            self.set_transcript_scroll_from_top_with_max(top, selected.max_scroll);
        }
    }

    fn reveal_transcript_search_entry(
        &mut self,
        entry: &ui::TranscriptNavigationEntry,
        regex: &regex::Regex,
    ) {
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
                        .is_some_and(|tool| regex.is_match(&ui::recorded_tool_viewer_text(tool)))
                    {
                        self.set_tool_output_expanded(id, true);
                    }
                }
            }
            _ => {}
        }
    }
}
