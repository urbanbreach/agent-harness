use super::*;
use crate::composer_editing::{ComposerEditor, DeleteKind};
use crossterm::event::KeyModifiers;

impl DashboardIntegration {
    pub(super) fn reflow(&mut self) -> Result<(), DashboardIntegrationError> {
        let details = self.layout.details.is_some();
        let width = self.layout.viewport.width.saturating_sub(2).max(1);
        let rows = self
            .reply_editor()
            .and_then(|editor| {
                crate::transcript_selection::WrappedText::new(&editor.text(), usize::from(width))
                    .ok()
            })
            .map_or(1, |wrapped| {
                u16::try_from(wrapped.row_count()).unwrap_or(u16::MAX)
            });
        self.layout = super::responsive::layout_with_reply_rows(
            self.layout.viewport,
            ShellState::Streaming,
            rows,
        );
        if details {
            self.open_details();
        }
        if let Ok(view) = self.peek.view() {
            // Measurement uses the same Markdown projection as paint. Theme color
            // choices do not change the measured cell widths.
            let theme = crate::theme::Theme::default();
            let heights = view.blocks.iter().map(|block| {
                let height =
                    crate::ui::dashboard_preview_lines(std::slice::from_ref(block), width, &theme)
                        .len()
                        .max(1);
                (
                    block.id,
                    f64::from(u32::try_from(height).unwrap_or(u32::MAX)),
                )
            });
            if !view.blocks.is_empty() {
                let layout = crate::transcript_scroll::TranscriptLayout::from_heights(
                    heights,
                    f64::from(self.layout.peek.height.saturating_sub(1).max(1)),
                )
                .map_err(crate::dashboard_peek::DashboardPeekError::from)?;
                self.peek.set_layout(&view.session_id, layout)?;
            }
        }
        self.reconcile_focus();
        Ok(())
    }

    pub(super) fn open_details(&mut self) {
        let viewport = self.layout.viewport;
        let width = viewport.width.saturating_sub(4).min(88);
        let height = viewport.height.saturating_sub(2).min(36);
        self.layout.details = Some(Rect::new(
            viewport.x + (viewport.width - width) / 2,
            viewport.y + (viewport.height - height) / 2,
            width,
            height,
        ));
        self.layout.visibility.details = true;
        self.focus.set(DashboardPane::Details);
    }

    pub(super) fn close_details(&mut self) {
        self.layout.details = None;
        self.layout.visibility.details = false;
        self.focus.set(DashboardPane::Roster);
    }

    pub fn reply_editor(&self) -> Option<&ComposerEditor> {
        self.replies.get(self.roster.selected_key()?)
    }

    pub fn clear_reply(&mut self) {
        if let Some(key) = self.roster.selected_key() {
            self.replies.remove(key);
        }
        let _ = self.peek.set_draft("");
        let _ = self.reflow();
    }

    pub fn search_state(&self) -> &SearchState {
        &self.search
    }

    pub fn preserve_interaction_from(&mut self, previous: &Self) {
        self.roster = previous.roster.clone();
        self.replies = previous.replies.clone();
        self.search = previous.search.clone();
        self.help_visible = previous.help_visible;
        let next_views = self
            .dashboard
            .rows
            .iter()
            .filter_map(|row| self.peek.view_for(&row.selection_key).ok())
            .collect::<Vec<_>>();
        self.peek = previous.peek.clone();
        let _ = self.peek.sync_dashboard(&self.dashboard);
        for view in next_views {
            let _ = self.peek.replace_blocks(&view.session_id, &view.blocks);
        }
        if let Some(key) = self
            .dashboard
            .fallback_selection(self.roster.selected_key())
        {
            let _ = self.select(key);
            if let Some(editor) = self.reply_editor() {
                let _ = self.peek.set_draft(editor.text());
            }
        }
        if previous.layout.details.is_some() {
            self.open_details();
        }
    }

    pub fn paste_reply(&mut self, text: &str) -> Result<(), DashboardIntegrationError> {
        let key = self
            .roster
            .selected_key()
            .cloned()
            .ok_or(DashboardIntegrationError::Peek(
                crate::dashboard_peek::DashboardPeekError::NoSelectedSession,
            ))?;
        let editor = self.replies.entry(key).or_default();
        editor
            .paste(text)
            .map_err(DashboardIntegrationError::Editing)?;
        self.peek.set_draft(editor.text())?;
        self.reflow()
    }

    pub(super) fn edit_reply(&mut self, event: KeyEvent) -> Result<(), DashboardIntegrationError> {
        let Some(key) = self.roster.selected_key().cloned() else {
            return Ok(());
        };
        let editor = self.replies.entry(key).or_default();
        let control = event.modifiers.contains(KeyModifiers::CONTROL);
        let result = match event.code {
            KeyCode::Char('z') if control => {
                let _ = editor.undo();
                Ok(())
            }
            KeyCode::Char('y') if control => {
                let _ = editor.redo();
                Ok(())
            }
            KeyCode::Char(c)
                if !event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                editor.insert_text(&c.to_string())
            }
            KeyCode::Enter if !event.modifiers.is_empty() => editor.insert_text("\n"),
            KeyCode::Backspace => editor.backspace(),
            KeyCode::Delete => editor.delete(DeleteKind::CharacterForward),
            KeyCode::Left => {
                if control {
                    editor.move_word_left();
                } else {
                    editor.move_left();
                }
                Ok(())
            }
            KeyCode::Right => {
                if control {
                    editor.move_word_right();
                } else {
                    editor.move_right();
                }
                Ok(())
            }
            KeyCode::Home => {
                editor.move_line_start();
                Ok(())
            }
            KeyCode::End => {
                editor.move_line_end();
                Ok(())
            }
            _ => Ok(()),
        };
        result.map_err(DashboardIntegrationError::Editing)?;
        self.peek.set_draft(editor.text())?;
        self.reflow()
    }

    pub(super) fn move_selection(
        &mut self,
        direction: i8,
    ) -> Result<(), DashboardIntegrationError> {
        let layout = self.roster_layout();
        let keys = layout
            .groups
            .iter()
            .flat_map(|group| group.visible_row_keys.iter())
            .collect::<Vec<_>>();
        let current = keys
            .iter()
            .position(|key| Some(*key) == self.roster.selected_key())
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(isize::from(direction))
            .min(keys.len().saturating_sub(1));
        let Some(key) = keys.get(next).cloned().cloned() else {
            return Ok(());
        };
        self.select(key.clone())?;
        // Scroll by logical items, measured by the shared roster layout.
        for _ in 0..self.dashboard.rows.len().saturating_mul(2) {
            let layout = self.roster_layout();
            if layout.row(&key).is_some() {
                break;
            }
            let previous = self.roster.scroll_top;
            self.roster.scroll_top = previous
                .saturating_add_signed(if direction > 0 { 1 } else { -1 })
                .min(layout.max_scroll);
            if previous == self.roster.scroll_top {
                break;
            }
        }
        Ok(())
    }
}
