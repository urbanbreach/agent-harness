// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::prompt_history::PromptHistoryDraft;
use super::*;

impl AppState {
    pub(in crate::app) fn prompt_char_count(&self) -> usize {
        self.composer.prompt_buffer.chars().count()
    }

    pub(in crate::app) fn prompt_cursor_byte_index(&self) -> usize {
        self.composer
            .prompt_buffer
            .char_indices()
            .nth(self.composer.prompt_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.composer.prompt_buffer.len())
    }

    pub(in crate::app) fn prompt_char_byte_index(&self, char_index: usize) -> usize {
        self.composer
            .prompt_buffer
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(self.composer.prompt_buffer.len())
    }

    pub(in crate::app) fn prompt_cursor_at_start(&self) -> bool {
        self.composer.prompt_cursor == 0
    }

    pub(in crate::app) fn prompt_cursor_at_end(&self) -> bool {
        self.composer.prompt_cursor >= self.prompt_char_count()
    }

    fn prompt_line_starts_and_lengths(&self) -> (Vec<usize>, Vec<usize>) {
        let mut starts = Vec::new();
        let mut lengths = Vec::new();
        let mut start = 0;

        for line in self.composer.prompt_buffer.split('\n') {
            starts.push(start);
            let line_len = line.chars().count();
            lengths.push(line_len);
            start += line_len + 1;
        }

        if starts.is_empty() {
            starts.push(0);
            lengths.push(0);
        }

        (starts, lengths)
    }

    fn prompt_cursor_line_column(&self, starts: &[usize], lengths: &[usize]) -> (usize, usize) {
        let line = starts
            .iter()
            .enumerate()
            .rfind(|(_, start)| self.composer.prompt_cursor >= **start)
            .map(|(line, _)| line)
            .unwrap_or(0)
            .min(lengths.len().saturating_sub(1));
        let column = self
            .composer
            .prompt_cursor
            .saturating_sub(starts[line])
            .min(lengths[line]);
        (line, column)
    }

    pub(in crate::app) fn move_prompt_cursor_up(&mut self) -> bool {
        let (starts, lengths) = self.prompt_line_starts_and_lengths();
        let (line, column) = self.prompt_cursor_line_column(&starts, &lengths);
        if line == 0 {
            return false;
        }

        self.composer.prompt_cursor = starts[line - 1] + column.min(lengths[line - 1]);
        true
    }

    pub(in crate::app) fn move_prompt_cursor_down(&mut self) -> bool {
        let (starts, lengths) = self.prompt_line_starts_and_lengths();
        let (line, column) = self.prompt_cursor_line_column(&starts, &lengths);
        if line + 1 >= lengths.len() {
            return false;
        }

        self.composer.prompt_cursor = starts[line + 1] + column.min(lengths[line + 1]);
        true
    }

    pub(in crate::app) fn select_previous_prompt_history(&mut self) {
        if self.composer.prompt_history.is_empty() {
            return;
        }

        if self.composer.prompt_history_index.is_none() {
            self.composer.prompt_history_draft = Some(PromptHistoryDraft {
                text: self.composer.prompt_buffer.clone(),
                cursor: self.composer.prompt_cursor,
            });
        }

        let next_idx = match self.composer.prompt_history_index {
            Some(idx) => idx.saturating_sub(1),
            None => self.composer.prompt_history.len().saturating_sub(1),
        };
        self.composer.prompt_history_index = Some(next_idx);
        self.composer.push_undo();
        self.replace_prompt_input(self.composer.prompt_history[next_idx].clone());
    }

    pub(in crate::app) fn select_next_prompt_history(&mut self) {
        let Some(idx) = self.composer.prompt_history_index else {
            return;
        };

        if idx + 1 < self.composer.prompt_history.len() {
            let next_idx = idx + 1;
            self.composer.prompt_history_index = Some(next_idx);
            self.composer.push_undo();
            self.replace_prompt_input(self.composer.prompt_history[next_idx].clone());
            return;
        }

        self.composer.push_undo();
        self.restore_prompt_history_draft_or_clear();
    }

    fn restore_prompt_history_draft_or_clear(&mut self) {
        self.composer.prompt_history_index = None;
        let Some(draft) = self.composer.prompt_history_draft.take() else {
            self.clear_prompt_input();
            return;
        };
        self.composer.prompt_buffer = draft.text;
        self.composer.prompt_cursor = draft.cursor.min(self.prompt_char_count());
        self.clear_file_mention_tags();
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn clear_prompt_input(&mut self) {
        self.composer.prompt_buffer.clear();
        self.composer.prompt_cursor = 0;
        self.composer.selection_anchor = None;
        self.clear_file_mention_tags();
        self.composer.prompt_history_index = None;
        self.composer.prompt_history_draft = None;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn replace_prompt_input(&mut self, prompt: String) {
        self.composer.prompt_cursor = prompt.chars().count();
        self.composer.prompt_buffer = prompt;
        self.composer.selection_anchor = None;
        self.clear_file_mention_tags();
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn apply_pending_live_prompt(&mut self, pending_prompt: PendingLivePrompt) {
        if pending_prompt.auto_submit {
            self.dispatch_submitted_prompt(pending_prompt.text);
        } else {
            self.replace_prompt_input(pending_prompt.text);
        }
    }

    pub(in crate::app) fn insert_prompt_char(&mut self, c: char) {
        self.continued_live_reopen_surface_active = false;
        if c == '/'
            && self.composer.prompt_cursor == 0
            && !self.composer.prompt_buffer.starts_with('/')
        {
            self.slash_draft_snapshot = Some(self.composer.prompt_buffer.clone());
        }
        self.composer.push_undo();
        if let Some(anchor) = self.composer.selection_anchor.take() {
            let start = anchor.min(self.composer.prompt_cursor);
            let end = anchor.max(self.composer.prompt_cursor);
            if start < end {
                self.adjust_file_mention_tags_for_delete(start, end);
                let start_byte = self.prompt_char_byte_index(start);
                let end_byte = self.prompt_char_byte_index(end);
                self.composer
                    .prompt_buffer
                    .replace_range(start_byte..end_byte, "");
                self.composer.prompt_cursor = start;
            }
        }
        let byte_idx = self.prompt_cursor_byte_index();
        self.adjust_file_mention_tags_for_insert(self.composer.prompt_cursor, 1);
        self.composer.prompt_buffer.insert(byte_idx, c);
        self.composer.prompt_cursor += 1;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn insert_prompt_text(&mut self, text: &str) {
        for c in text.chars() {
            self.insert_prompt_char(c);
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) {
        if self.composer_disabled() {
            return;
        }

        if self.startup_shell_visible() && self.focus != Focus::Prompt {
            self.focus = Focus::Prompt;
        }

        if self.focus != Focus::Prompt {
            return;
        }

        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_prompt_text(&normalized);
    }

    pub(in crate::app) fn backspace_prompt_char(&mut self) {
        if let Some(anchor) = self.composer.selection_anchor.take() {
            let start = anchor.min(self.composer.prompt_cursor);
            let end = anchor.max(self.composer.prompt_cursor);
            if start < end {
                self.composer.push_undo();
                self.delete_prompt_range(start, end);
            }
            return;
        }
        if self.composer.prompt_cursor == 0 {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        self.composer.push_undo();
        self.composer.prompt_cursor -= 1;
        self.adjust_file_mention_tags_for_delete(
            self.composer.prompt_cursor,
            self.composer.prompt_cursor + 1,
        );
        let byte_idx = self.prompt_cursor_byte_index();
        self.composer.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn delete_prompt_char(&mut self) {
        if let Some(anchor) = self.composer.selection_anchor.take() {
            let start = anchor.min(self.composer.prompt_cursor);
            let end = anchor.max(self.composer.prompt_cursor);
            if start < end {
                self.composer.push_undo();
                self.delete_prompt_range(start, end);
            }
            return;
        }
        if self.composer.prompt_cursor >= self.prompt_char_count() {
            return;
        }

        self.continued_live_reopen_surface_active = false;
        self.composer.push_undo();
        self.adjust_file_mention_tags_for_delete(
            self.composer.prompt_cursor,
            self.composer.prompt_cursor + 1,
        );
        let byte_idx = self.prompt_cursor_byte_index();
        self.composer.prompt_buffer.remove(byte_idx);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn echo_submitted_prompt(&mut self, text: String, status: ActivityStatus) {
        let profile_label = self.active_profile().to_string();
        self.activities.push_back(ActivityEntry {
            request_id: String::new(),
            profile_label,
            model_id: String::new(),
            provider_id: String::new(),
            status,
            user_message: Some(UserMessageSubmittedEvent {
                request_id: String::new().into(),
                text,
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 0,
            last_seq: 0,
            first_mono_ms: 0,
            last_mono_ms: 0,
            revision: 0,
        });
        self.transcript_view.selected_activity_index = self.activities.len().saturating_sub(1);
        self.details_scroll = 0;
        self.transcript_view.transcript_scroll = 0;
    }

    fn record_submitted_prompt_locally(&mut self, text: String) {
        let status = if self.active_turn_in_progress() {
            ActivityStatus::Queued
        } else {
            ActivityStatus::Streaming
        };
        if self.composer.prompt_history.last() != Some(&text) {
            self.composer.prompt_history.push(text.clone());
            self.save_prompt_history();
        }
        self.clear_prompt_input();
        self.echo_submitted_prompt(text.clone(), status);
    }

    fn save_prompt_history(&mut self) {
        let Some(path) = self.composer.prompt_history_path.as_deref() else {
            return;
        };
        if let Err(err) = prompt_history::save_prompt_history(path, &self.composer.prompt_history) {
            self.status_banner = Some(err);
        }
    }

    fn dispatch_submitted_prompt(&mut self, text: String) {
        if self.launch_metadata.model().is_none()
            && self.launch_metadata.provider() == "local"
            && self.launch_metadata.configured_profile().is_some()
        {
            self.status_banner = Some("Connect a provider to send prompts".to_string());
            self.show_toast(
                "Connect a provider to send prompts".to_string(),
                ToastVariant::Error,
            );
            return;
        }
        let selected_file_tags = self.selected_file_tags();
        let selected_agent_tags = self.selected_agent_tags();
        let selected_resource_tags = self.selected_resource_tags();
        self.record_submitted_prompt_locally(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt {
            text,
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            launch_metadata: self.launch_metadata.clone(),
        });
    }

    pub(in crate::app) fn _handle_prompt_key(&mut self, key_code: KeyCode) -> bool {
        match key_code {
            KeyCode::Enter => {
                self.submit_prompt();
                true
            }
            KeyCode::Esc => {
                self.composer.prompt_buffer.clear();
                self.composer.prompt_cursor = 0;
                self.composer.prompt_history_index = None;
                true
            }
            KeyCode::Up => {
                if !self.composer.prompt_history.is_empty() {
                    let next_idx = match self.composer.prompt_history_index {
                        Some(idx) => idx.saturating_sub(1),
                        None => self.composer.prompt_history.len().saturating_sub(1),
                    };
                    self.composer.prompt_history_index = Some(next_idx);
                    self.composer.prompt_buffer = self.composer.prompt_history[next_idx].clone();
                    self.composer.prompt_cursor = self.composer.prompt_buffer.len();
                }
                true
            }
            KeyCode::Down => {
                if let Some(idx) = self.composer.prompt_history_index {
                    if idx + 1 < self.composer.prompt_history.len() {
                        let next_idx = idx + 1;
                        self.composer.prompt_history_index = Some(next_idx);
                        self.composer.prompt_buffer =
                            self.composer.prompt_history[next_idx].clone();
                        self.composer.prompt_cursor = self.composer.prompt_buffer.len();
                    } else {
                        self.composer.prompt_history_index = None;
                        self.composer.prompt_buffer.clear();
                        self.composer.prompt_cursor = 0;
                    }
                }
                true
            }
            KeyCode::Left => {
                if self.composer.prompt_cursor > 0 {
                    self.composer.prompt_cursor -= 1;
                }
                true
            }
            KeyCode::Right => {
                if self.composer.prompt_cursor < self.composer.prompt_buffer.chars().count() {
                    self.composer.prompt_cursor += 1;
                }
                true
            }
            KeyCode::Backspace => {
                if self.composer.prompt_cursor > 0 {
                    self.composer.prompt_cursor -= 1;
                    let byte_idx = self
                        .composer
                        .prompt_buffer
                        .char_indices()
                        .nth(self.composer.prompt_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.composer.prompt_buffer.len());
                    self.composer.prompt_buffer.remove(byte_idx);
                }
                true
            }
            KeyCode::Delete => {
                if self.composer.prompt_cursor < self.composer.prompt_buffer.chars().count() {
                    let byte_idx = self
                        .composer
                        .prompt_buffer
                        .char_indices()
                        .nth(self.composer.prompt_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.composer.prompt_buffer.len());
                    self.composer.prompt_buffer.remove(byte_idx);
                }
                true
            }
            KeyCode::Char(c) => {
                let byte_idx = self
                    .composer
                    .prompt_buffer
                    .char_indices()
                    .nth(self.composer.prompt_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.composer.prompt_buffer.len());
                self.composer.prompt_buffer.insert(byte_idx, c);
                self.composer.prompt_cursor += 1;
                true
            }
            KeyCode::Tab | KeyCode::BackTab => false,
            _ => true,
        }
    }

    pub(in crate::app) fn submit_prompt(&mut self) {
        if !self.replay_mode && !self.composer_disabled() {
            if let Some(command) = self.typed_slash_command() {
                self.execute_slash_command(command, self.slash_draft_snapshot.clone());
                return;
            }
        }

        if self.composer.prompt_buffer.trim().is_empty()
            || self.composer_disabled()
            || self.replay_mode
        {
            return;
        }

        if self.startup_mode {
            let text = self.composer.prompt_buffer.clone();
            set_pending_live_launch_metadata(self.launch_metadata.clone());
            set_pending_live_prompt_auto_submit(Some(text.clone()));
            self.startup_mode = false;
            self.focus = Focus::Prompt;
            self.record_submitted_prompt_locally(text);
            self.emit_ui_intent(UiIntent::NewSession);
            self.should_quit = true;
            return;
        }

        let text = self.composer.prompt_buffer.clone();
        self.dispatch_submitted_prompt(text);
    }
}
