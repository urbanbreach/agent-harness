use super::pane_query::{PaneQuery, PaneQueryMode};
use super::*;

#[derive(Clone, Debug)]
pub(super) struct TranscriptSearchHit {
    id: ui::TranscriptVisualEntryId,
    target: Option<TranscriptMouseTarget>,
    line: usize,
}

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
        self.transcript_view.search_hits.clear();
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
        if !native || forward.is_none() {
            self.transcript_view.search_hits = self.collect_transcript_search_hits(area, &regex);
        }
        let count = self.transcript_view.search_hits.len();
        self.transcript_view.search_match_count = count;
        if count == 0 {
            return;
        }
        let previous = self
            .transcript_view
            .search_match
            .min(count.saturating_sub(1));
        let index = match forward {
            None => 0,
            Some(true) => (previous + 1) % count,
            Some(false) => (previous + count - 1) % count,
        };
        self.transcript_view.search_match = index;
        let hit = self.transcript_view.search_hits[index].clone();
        self.reveal_transcript_search_hit(area, &regex, hit);
    }

    fn reveal_transcript_search_hit(
        &mut self,
        area: Rect,
        regex: &regex::Regex,
        hit: TranscriptSearchHit,
    ) {
        let native = self.current_subagent_session_present();
        let entries = ui::transcript_navigation_entries(self, area);
        let Some(entry) = entries.iter().find(|entry| hit.matches(entry)) else {
            return;
        };
        if let Some(TranscriptMouseTarget::Tool { tool_call_id }) =
            hit.target.as_ref().filter(|_| native)
        {
            if self
                .tool_call_entry(tool_call_id)
                .is_some_and(ui::tool_call_has_transcript_disclosure)
            {
                if let Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) = &entry.target {
                    self.set_tool_group_outputs_expanded(tool_call_ids, true);
                }
                self.set_tool_output_expanded(tool_call_id, true);
            }
        } else {
            self.reveal_transcript_search_entry(entry, regex);
        }
        let revealed = ui::transcript_navigation_entries(self, area);
        let selected = revealed
            .iter()
            .find(|candidate| {
                native
                    && hit.matches(candidate)
                    && !matches!(
                        candidate.target,
                        Some(TranscriptMouseTarget::ToolGroup { .. })
                    )
            })
            .or_else(|| {
                revealed
                    .iter()
                    .find(|candidate| match (&entry.target, &candidate.target) {
                        (
                            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }),
                            Some(
                                TranscriptMouseTarget::Tool { tool_call_id }
                                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
                            ),
                        ) => {
                            tool_call_ids.contains(tool_call_id) && regex.is_match(&candidate.text)
                        }
                        _ => false,
                    })
            })
            .or_else(|| revealed.iter().find(|candidate| candidate.id == entry.id))
            .unwrap_or(entry);
        self.select_transcript_entry(selected);
        let line = if native {
            selected
                .logical_line_rows
                .get(hit.line)
                .copied()
                .unwrap_or(hit.line)
        } else {
            selected
                .text
                .lines()
                .position(|text| regex.is_match(text))
                .unwrap_or(0)
        };
        let top = if native {
            ui::transcript_search_scroll_top(self, area, selected.top).map(|top| top + line)
        } else {
            ui::transcript_entry_scroll_top(self, area, selected.top + line)
        };
        if let Some(top) = top {
            if native {
                self.cancel_transcript_page_flip();
                let viewport = self
                    .transcript_view
                    .viewport
                    .preserve_detachment(selected.max_scroll)
                    .with_detached_top(top);
                self.transcript_view.set_measured_viewport(viewport);
            } else {
                self.set_transcript_scroll_from_top_with_max(top, selected.max_scroll);
            }
        }
    }

    fn collect_transcript_search_hits(
        &self,
        area: Rect,
        regex: &regex::Regex,
    ) -> Vec<TranscriptSearchHit> {
        let native = self.current_subagent_session_present();
        let mut hits = Vec::new();
        let mut tools = BTreeSet::new();
        for entry in ui::transcript_navigation_entries(self, area) {
            if native && self.collect_tool_search_hits(&entry, regex, &mut tools, &mut hits) {
                continue;
            }
            let content = self.selected_entry_content(&entry);
            let text = if native {
                self.child_search_text(&entry, content.content())
            } else if regex.is_match(&entry.text) {
                std::borrow::Cow::Borrowed(entry.text.as_ref())
            } else {
                std::borrow::Cow::Borrowed(content.content())
            };
            append_search_hits(
                &mut hits,
                &entry,
                entry.target.clone(),
                &text,
                regex,
                if native { usize::MAX } else { 1 },
            );
        }
        hits
    }

    fn collect_tool_search_hits(
        &self,
        entry: &ui::TranscriptNavigationEntry,
        regex: &regex::Regex,
        seen: &mut BTreeSet<String>,
        hits: &mut Vec<TranscriptSearchHit>,
    ) -> bool {
        let ids = match &entry.target {
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => tool_call_ids.as_slice(),
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => std::slice::from_ref(tool_call_id),
            _ => return false,
        };
        for id in ids {
            let Some(tool) = self.tool_call_entry(id).filter(|_| seen.insert(id.clone())) else {
                continue;
            };
            append_search_hits(
                hits,
                entry,
                Some(TranscriptMouseTarget::Tool {
                    tool_call_id: id.clone(),
                }),
                &ui::recorded_tool_search_text(tool),
                regex,
                usize::MAX,
            );
        }
        true
    }

    fn child_search_text<'a>(
        &self,
        entry: &'a ui::TranscriptNavigationEntry,
        content: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        use crate::ui::TranscriptRenderSurfaceKind as Kind;
        match entry.kind {
            Kind::AssistantBody | Kind::AssistantReasoning => {
                std::borrow::Cow::Owned(ui::transcript_search_markdown_text(content, self.theme()))
            }
            Kind::AssistantFooter => std::borrow::Cow::Borrowed(""),
            _ => std::borrow::Cow::Borrowed(entry.source_text.as_deref().unwrap_or(content)),
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

impl TranscriptSearchHit {
    fn matches(&self, entry: &ui::TranscriptNavigationEntry) -> bool {
        let Some(TranscriptMouseTarget::Tool { tool_call_id: id }) = &self.target else {
            return entry.id == self.id;
        };
        match &entry.target {
            Some(
                TranscriptMouseTarget::Tool { tool_call_id }
                | TranscriptMouseTarget::PatchFile { tool_call_id, .. },
            ) => tool_call_id == id,
            Some(TranscriptMouseTarget::ToolGroup { tool_call_ids }) => tool_call_ids.contains(id),
            _ => false,
        }
    }
}

fn append_search_hits(
    hits: &mut Vec<TranscriptSearchHit>,
    entry: &ui::TranscriptNavigationEntry,
    target: Option<TranscriptMouseTarget>,
    text: &str,
    regex: &regex::Regex,
    limit: usize,
) {
    let mut line = 0;
    let mut counted = 0;
    for found in regex
        .find_iter(text)
        .filter(|found| !found.is_empty())
        .take(limit)
    {
        line += text[counted..found.start()]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        counted = found.start();
        hits.push(TranscriptSearchHit {
            id: entry.id,
            target: target.clone(),
            line,
        });
    }
}
