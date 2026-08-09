use super::*;

impl AppState {
    pub(in crate::app) fn prompt_stash_push(&mut self) {
        if self.composer_disabled() || self.replay_mode {
            return;
        }
        if self.composer.prompt_buffer.is_empty() {
            return;
        }
        let entry = PromptStashEntry {
            text: std::mem::take(&mut self.composer.prompt_buffer),
            cursor: self.composer.prompt_cursor,
            selection_anchor: self.composer.selection_anchor,
            timestamp: prompt_stash_now_unix_millis(),
        };
        self.prompt_stash.entries.push(entry);
        self.composer.prompt_cursor = 0;
        self.composer.selection_anchor = None;
        self.composer.shell_mode = false;
        self.composer.prompt_history_index = None;
        self.composer.prompt_history_draft = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
        self.save_prompt_stash();
    }

    pub(in crate::app) fn prompt_stash_pop(&mut self) {
        if self.composer_disabled() || self.replay_mode {
            return;
        }
        let Some(entry) = self.prompt_stash.entries.pop() else {
            return;
        };
        self.composer.push_undo();
        self.replace_prompt_input(entry.text);
        self.composer.prompt_cursor = entry.cursor.min(self.prompt_char_count());
        self.composer.selection_anchor = entry.selection_anchor;
        self.composer.prompt_history_index = None;
        self.composer.prompt_history_draft = None;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
        self.save_prompt_stash();
    }

    pub(in crate::app) fn open_prompt_stash_list(&mut self) {
        if self.replay_mode {
            return;
        }
        self.prompt_stash.list_visible = true;
        self.prompt_stash.list_selected = self
            .prompt_stash
            .entries
            .len()
            .saturating_sub(1)
            .min(self.prompt_stash.entries.len().saturating_sub(1));
    }

    pub(in crate::app) fn close_prompt_stash_list(&mut self) {
        self.prompt_stash.list_visible = false;
    }

    pub(in crate::app) fn prompt_stash_list_move(&mut self, delta: isize) {
        if self.prompt_stash.entries.is_empty() {
            self.prompt_stash.list_selected = 0;
            return;
        }
        let len = self.prompt_stash.entries.len();
        let current =
            isize::try_from(self.prompt_stash.list_selected.min(len - 1)).unwrap_or(isize::MAX);
        let next = (current + delta).clamp(0, isize::try_from(len - 1).unwrap_or(isize::MAX));
        self.prompt_stash.list_selected = usize::try_from(next).unwrap_or(0);
    }

    pub(in crate::app) fn prompt_stash_list_delete_selected(&mut self) {
        if self.prompt_stash.entries.is_empty() {
            return;
        }
        let index = self
            .prompt_stash
            .list_selected
            .min(self.prompt_stash.entries.len() - 1);
        self.prompt_stash.entries.remove(index);
        let len = self.prompt_stash.entries.len();
        if len == 0 {
            self.prompt_stash.list_selected = 0;
        } else if self.prompt_stash.list_selected >= len {
            self.prompt_stash.list_selected = len - 1;
        }
        self.save_prompt_stash();
    }

    pub(in crate::app) fn prompt_stash_list_restore_selected(&mut self) {
        if self.composer_disabled() || self.replay_mode {
            return;
        }
        if self.prompt_stash.entries.is_empty() {
            return;
        }
        let index = self
            .prompt_stash
            .list_selected
            .min(self.prompt_stash.entries.len() - 1);
        let entry = self.prompt_stash.entries.remove(index);
        self.composer.push_undo();
        self.replace_prompt_input(entry.text);
        self.composer.prompt_cursor = entry.cursor.min(self.prompt_char_count());
        self.composer.selection_anchor = entry.selection_anchor;
        self.composer.prompt_history_index = None;
        self.composer.prompt_history_draft = None;
        self.continued_live_reopen_surface_active = false;
        self.slash_draft_snapshot = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
        let len = self.prompt_stash.entries.len();
        if len == 0 {
            self.prompt_stash.list_selected = 0;
        } else if self.prompt_stash.list_selected >= len {
            self.prompt_stash.list_selected = len - 1;
        }
        self.prompt_stash.list_visible = false;
        self.save_prompt_stash();
    }

    fn save_prompt_stash(&mut self) {
        let Some(path) = self.prompt_stash.path.as_deref() else {
            return;
        };
        if let Err(err) = prompt_stash::save_prompt_stash(path, &self.prompt_stash.entries) {
            self.status_banner = Some(err);
        }
    }
}

fn prompt_stash_now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
