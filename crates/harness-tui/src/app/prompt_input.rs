// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::prompt_history::PromptHistoryDraft;
use super::*;

impl AppState {
    pub(crate) fn queued_prompt_send_now_available(&self) -> bool {
        let visible_activity = self
            .runtime_state_activity()
            .filter(|activity| activity.status == ActivityStatus::Streaming);
        !self.replay_mode
            && self.queued_prompt_count > 0
            && self.composer.prompt_buffer.trim().is_empty()
            && visible_activity.is_some_and(ActivityEntry::is_sendable_wait)
            && visible_activity.is_some_and(|activity| {
                !self
                    .active_interrupt_task_ids_for_request(&activity.request_id)
                    .is_empty()
            })
    }

    pub(in crate::app) fn send_queued_prompt_now(&mut self) -> bool {
        if !self.queued_prompt_send_now_available() {
            return false;
        }

        let Some(request_id) = self
            .runtime_state_activity()
            .filter(|activity| activity.status == ActivityStatus::Streaming)
            .map(|activity| activity.request_id.clone())
        else {
            return false;
        };
        let task_ids = self.active_interrupt_task_ids_for_request(&request_id);
        self.emit_ui_intent(UiIntent::InterruptSession {
            task_ids: task_ids.iter().cloned().collect(),
            reason: InterruptReason::SendNow,
        });
        self.interrupt_requested_task_ids = task_ids;
        true
    }

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
        self.reset_clear_prompt_confirmation();
        let _ = self.composer.replace_editor_text("");
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

    pub(in crate::app) fn clear_prompt_confirmation_pending(&self) -> bool {
        self.clear_prompt_confirm_deadline
            .is_some_and(|deadline| self.now() < deadline)
    }

    pub(in crate::app) fn reset_clear_prompt_confirmation(&mut self) {
        self.clear_prompt_confirm_deadline = None;
    }

    pub(in crate::app) fn handle_clear_prompt_escape(&mut self) -> bool {
        if self.active_turn_in_progress() || !self.active_interrupt_task_ids().is_empty() {
            self.reset_clear_prompt_confirmation();
            return false;
        }
        if self.focus != Focus::Prompt
            || self.composer_disabled()
            || self.active_turn_in_progress()
            || !self.active_interrupt_task_ids().is_empty()
            || self.composer.shell_mode
            || self.composer.prompt_buffer.is_empty()
        {
            self.reset_clear_prompt_confirmation();
            return false;
        }

        if self.clear_prompt_confirmation_pending() {
            let text = self.composer.prompt_buffer.clone();
            self.composer.push_undo();
            if self.composer.prompt_history.last() != Some(&text) {
                self.composer.prompt_history.push(text);
                self.save_prompt_history();
            }
            self.reset_clear_prompt_confirmation();
            self.toast = None;
            self.clear_prompt_input();
            return true;
        }

        self.clear_prompt_confirm_deadline = Some(self.now() + CLEAR_PROMPT_CONFIRM_TIMEOUT);
        self.show_toast(CLEAR_PROMPT_HINT, ToastVariant::Info);
        true
    }

    pub(in crate::app) fn replace_prompt_input(&mut self, prompt: String) {
        if !prompt.is_empty() {
            self.dismiss_welcome_for_input();
        }
        if self.composer.replace_editor_text(&prompt).is_err() {
            self.composer.prompt_cursor = prompt.chars().count();
            self.composer.prompt_buffer = prompt;
        }
        self.composer.selection_anchor = None;
        self.clear_file_mention_tags();
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn apply_pending_live_prompt(&mut self, pending_prompt: PendingLivePrompt) {
        if pending_prompt.auto_submit {
            self.replace_prompt_input(pending_prompt.text);
            self.dispatch_submitted_prompt();
        } else {
            self.replace_prompt_input(pending_prompt.text);
        }
    }

    pub(in crate::app) fn insert_prompt_char(&mut self, c: char) {
        if c != '\n' {
            self.dismiss_welcome_for_input();
        }
        self.reset_clear_prompt_confirmation();
        self.continued_live_reopen_surface_active = false;
        if c == '/'
            && self.composer.prompt_cursor == 0
            && !self.composer.prompt_buffer.starts_with('/')
        {
            self.slash_draft_snapshot = Some(self.composer.prompt_buffer.clone());
        }
        if self.composer.editor_matches_prompt_fields()
            && self.composer.editor_insert_text(&c.to_string()).is_ok()
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
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
        if self.composer.editor_matches_prompt_fields() && self.composer.editor_paste(text).is_ok()
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
        }
        for c in text.chars() {
            self.insert_prompt_char(c);
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        if self.handle_new_worktree_dialog_paste(text) {
            return;
        }
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
        if !normalized.is_empty() {
            self.dismiss_welcome_for_input();
        }
        self.insert_prompt_text(&normalized);
    }

    pub(in crate::app) fn backspace_prompt_char(&mut self) {
        if self.composer.editor_matches_prompt_fields() && self.composer.editor_backspace().is_ok()
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
        }
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
        if self.composer.editor_matches_prompt_fields()
            && self
                .composer
                .editor_delete(crate::composer_editing::DeleteKind::CharacterForward)
                .is_ok()
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
        }
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
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: String::new(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 0,
            last_seq: 0,
            first_mono_ms: 0,
            last_mono_ms: 0,
            request_started_mono_ms: None,
            revision: 0,
        });
        self.transcript_view.selected_activity_index = self.activities.len().saturating_sub(1);
        self.details_scroll = 0;
        self.transcript_view.transcript_scroll = 0;
        if status == ActivityStatus::Streaming {
            self.begin_transcript_page_flip(
                self.activities
                    .back()
                    .map_or(0, |activity| activity.first_seq),
            );
        }
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
        if status == ActivityStatus::Streaming {
            self.begin_live_turn_timing(None);
        }
    }

    fn save_prompt_history(&mut self) {
        let Some(path) = self.composer.prompt_history_path.as_deref() else {
            return;
        };
        if let Err(err) = prompt_history::save_prompt_history(path, &self.composer.prompt_history) {
            self.status_banner = Some(err);
        }
    }

    fn dispatch_submitted_prompt(&mut self) {
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
        let submission = match self.composer_submission() {
            Ok(submission) => submission,
            Err(error) => {
                self.status_banner = Some(error.to_string());
                return;
            }
        };
        let text = submission.text;
        let selected_file_tags = self.selected_file_tags();
        let selected_agent_tags = self.selected_agent_tags();
        let selected_resource_tags = self.selected_resource_tags();
        self.record_submitted_prompt_locally(text.clone());
        self.emit_ui_intent(UiIntent::SubmitPrompt {
            text,
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            attachments: submission.attachments,
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
        if !self.replay_mode && !self.composer_disabled() && self.apply_backslash_continuation() {
            return;
        }

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

        self.dispatch_submitted_prompt();
    }

    fn apply_backslash_continuation(&mut self) -> bool {
        let cursor = self.composer.prompt_cursor;
        if cursor == 0
            || self
                .composer
                .prompt_buffer
                .chars()
                .nth(cursor - 1)
                .is_none_or(|character| character != '\\')
        {
            return false;
        }

        self.composer.push_undo();
        let start = self.prompt_char_byte_index(cursor - 1);
        let end = self.prompt_char_byte_index(cursor);
        self.composer.prompt_buffer.replace_range(start..end, "\n");
        self.composer.selection_anchor = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
        true
    }
}

#[cfg(test)]
mod paste_tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        reason = "consistency contract tests use fail-fast asserts for missing state"
    )]

    use super::*;
    use std::sync::{Arc, Mutex};

    fn live_app() -> AppState {
        AppState::new_live(None, false, None)
    }

    fn live_app_capture() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| intents.lock().unwrap().push(intent))
        };
        (AppState::new_live(None, false, Some(sink)), intents)
    }

    fn set_buffer(app: &mut AppState, text: &str) {
        app.composer.prompt_buffer = text.to_string();
        app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
        app.composer.selection_anchor = None;
    }

    fn set_buffer_at(app: &mut AppState, text: &str, cursor: usize) {
        app.composer.prompt_buffer = text.to_string();
        app.composer.prompt_cursor = cursor;
        app.composer.selection_anchor = None;
    }

    /// D1: a multi-line paste normalizes CRLF and CR to LF, places the cursor
    /// at the end, and does NOT submit or record history.
    #[test]
    fn paste_normalizes_line_endings_and_does_not_submit() {
        // arrange
        let (mut app, intents) = live_app_capture();

        // act
        app.handle_paste("alpha\r\n\r\nbeta\rgamma");

        // assert
        assert_eq!(app.composer.prompt_buffer, "alpha\n\nbeta\ngamma");
        assert_eq!(
            app.composer.prompt_cursor,
            app.composer.prompt_buffer.chars().count()
        );
        assert!(app.composer.prompt_history.is_empty());
        assert!(intents.lock().unwrap().is_empty());
    }

    /// D2: pasting with the cursor mid-buffer inserts at the cursor.
    #[test]
    fn paste_inserts_at_cursor_position() {
        // arrange
        let mut app = live_app();
        set_buffer_at(&mut app, "ac", 1);

        // act
        app.handle_paste("b");

        // assert
        assert_eq!(app.composer.prompt_buffer, "abc");
        assert_eq!(app.composer.prompt_cursor, 2);
    }

    /// D3: pasting collapses an active selection and replaces it.
    #[test]
    fn paste_replaces_active_selection() {
        // arrange
        let mut app = live_app();
        set_buffer_at(&mut app, "hello", 5);
        app.composer.selection_anchor = Some(0);

        // act
        app.handle_paste("bye");

        // assert
        assert_eq!(app.composer.prompt_buffer, "bye");
        assert_eq!(app.composer.prompt_cursor, 3);
        assert_eq!(app.composer.selection_anchor, None);
    }

    /// D-Reject: a paste is ignored when the prompt is not focused (P6
    /// rejection).
    #[test]
    fn paste_is_ignored_when_prompt_not_focused() {
        // arrange
        let mut app = live_app();
        app.focus = Focus::List;

        // act
        app.handle_paste("should not appear");

        // assert
        assert!(app.composer.prompt_buffer.is_empty());
    }

    /// D-Reject: an empty paste is a no-op.
    #[test]
    fn paste_empty_string_is_noop() {
        // arrange
        let mut app = live_app();

        // act
        app.handle_paste("");

        // assert
        assert!(app.composer.prompt_buffer.is_empty());
        assert_eq!(app.composer.prompt_cursor, 0);
    }

    #[test]
    fn paste_targets_open_worktree_dialog_and_respects_byte_cap() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.open_new_worktree_dialog();

        // act
        app.handle_paste(&format!("{}\nignored", "a".repeat(100)));

        // assert
        assert_eq!(app.new_worktree_dialog.input, "a".repeat(100));
        assert_eq!(app.new_worktree_dialog.cursor, 100);
        assert!(app.composer.prompt_buffer.is_empty());
    }
}
