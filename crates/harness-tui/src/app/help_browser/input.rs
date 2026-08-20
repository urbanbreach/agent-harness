use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{HelpBrowserState, HelpMode, HelpRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HelpOutcome {
    Close,
    Changed,
    Unchanged,
}

impl HelpBrowserState {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn select(&mut self, index: usize, rows: &[HelpRow]) -> bool {
        let next = index.min(rows.len().saturating_sub(1));
        let changed = self.selected != next;
        self.selected = next;
        changed
    }

    pub(crate) fn activate_search(&mut self) -> bool {
        let changed = !self.search_active;
        self.search_active = true;
        changed
    }

    pub(crate) fn scroll_by(&mut self, down: bool, current: usize, max: usize) -> bool {
        let step = if matches!(self.mode, HelpMode::Browse) {
            self.visual_offset = current;
            self.follow_selection = false;
            3
        } else {
            1
        };
        let offset = match &mut self.mode {
            HelpMode::Browse => &mut self.visual_offset,
            HelpMode::Detail { scroll, .. } => scroll,
        };
        let previous = *offset;
        *offset = if down {
            previous.saturating_add(step).min(max)
        } else {
            previous.saturating_sub(step)
        };
        previous != *offset
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        rows: &[HelpRow],
        detail_max_scroll: usize,
    ) -> HelpOutcome {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('.') | KeyCode::Char('x'))
        {
            return HelpOutcome::Close;
        }
        if matches!(self.mode, HelpMode::Detail { .. }) {
            return self.handle_detail_key(key, detail_max_scroll);
        }
        if self.search_active || !self.query.is_empty() {
            return self.handle_search_key(key, rows);
        }
        match key.code {
            KeyCode::Esc => HelpOutcome::Close,
            KeyCode::Char('/') | KeyCode::Char('i') if key.modifiers.is_empty() => {
                self.search_active = true;
                HelpOutcome::Changed
            }
            KeyCode::Char('f') if key.modifiers.is_empty() => {
                self.hide_dimmed = !self.hide_dimmed;
                self.selected = 0;
                self.visual_offset = 0;
                self.follow_selection = true;
                HelpOutcome::Changed
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(rows, 1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(rows, -1),
            KeyCode::PageDown => self.move_selection(rows, 10),
            KeyCode::PageUp => self.move_selection(rows, -10),
            KeyCode::Home | KeyCode::Char('g') => self.set_selection(0, rows),
            KeyCode::End | KeyCode::Char('G') => {
                self.set_selection(rows.len().saturating_sub(1), rows)
            }
            KeyCode::Enter => self.activate_selected(rows),
            KeyCode::Char('e') | KeyCode::Char(' ') | KeyCode::Right => {
                self.expand_selected(rows, true)
            }
            KeyCode::Char('E') | KeyCode::Left => self.expand_selected(rows, false),
            KeyCode::Char(character) if key.modifiers.is_empty() => {
                self.search_active = true;
                self.query.push(character);
                self.selected = 0;
                self.visual_offset = 0;
                self.follow_selection = true;
                HelpOutcome::Changed
            }
            KeyCode::BackTab
            | KeyCode::Tab
            | KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => HelpOutcome::Unchanged,
        }
    }

    pub(super) fn normalize_selection(&mut self, rows: &[HelpRow]) {
        if let Some(index) = self.return_action.take().and_then(|action| {
            rows.iter().position(
                |row| matches!(row, HelpRow::Shortcut { action: candidate, .. } if *candidate == action),
            )
        }) {
            self.selected = index;
            self.visual_offset = 0;
            self.follow_selection = true;
            return;
        }
        self.selected = self.selected.min(rows.len().saturating_sub(1));
    }

    fn handle_detail_key(&mut self, key: KeyEvent, max_scroll: usize) -> HelpOutcome {
        if matches!(key.code, KeyCode::Esc | KeyCode::Left | KeyCode::Backspace) {
            let HelpMode::Detail { action, .. } = self.mode else {
                return HelpOutcome::Unchanged;
            };
            if let Some(section) = action.help_category() {
                self.collapsed_sections.remove(&section);
            }
            self.return_action = Some(action);
            self.mode = HelpMode::Browse;
            return HelpOutcome::Changed;
        }
        let HelpMode::Detail { scroll, .. } = &mut self.mode else {
            return HelpOutcome::Unchanged;
        };
        match key.code {
            KeyCode::Down | KeyCode::PageDown => *scroll = scroll.saturating_add(1).min(max_scroll),
            KeyCode::Up | KeyCode::PageUp => *scroll = scroll.saturating_sub(1),
            KeyCode::Home => *scroll = 0,
            _ => return HelpOutcome::Unchanged,
        }
        HelpOutcome::Changed
    }

    fn handle_search_key(&mut self, key: KeyEvent, rows: &[HelpRow]) -> HelpOutcome {
        match key.code {
            KeyCode::Esc => {
                self.query.clear();
                self.search_active = false;
                self.selected = 0;
                self.visual_offset = 0;
                self.follow_selection = true;
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                self.visual_offset = 0;
                self.follow_selection = true;
            }
            KeyCode::Enter => return self.activate_selected(rows),
            KeyCode::Down => return self.move_selection(rows, 1),
            KeyCode::Up => return self.move_selection(rows, -1),
            KeyCode::Char(character) if key.modifiers.is_empty() => {
                self.query.push(character);
                self.selected = 0;
                self.visual_offset = 0;
                self.follow_selection = true;
            }
            _ => return HelpOutcome::Unchanged,
        }
        HelpOutcome::Changed
    }

    fn activate_selected(&mut self, rows: &[HelpRow]) -> HelpOutcome {
        match rows.get(self.selected).cloned() {
            Some(HelpRow::Section { section, .. }) => {
                if !self.collapsed_sections.remove(&section) {
                    self.collapsed_sections.insert(section);
                }
                HelpOutcome::Changed
            }
            Some(HelpRow::Shortcut { action, .. }) => {
                self.query.clear();
                self.search_active = false;
                self.mode = HelpMode::Detail { action, scroll: 0 };
                HelpOutcome::Changed
            }
            None => HelpOutcome::Unchanged,
        }
    }

    fn expand_selected(&mut self, rows: &[HelpRow], expand: bool) -> HelpOutcome {
        match rows.get(self.selected) {
            Some(HelpRow::Section {
                section, collapsed, ..
            }) if expand || !collapsed => {
                if expand {
                    self.collapsed_sections.remove(section);
                } else {
                    self.collapsed_sections.insert(*section);
                }
                HelpOutcome::Changed
            }
            Some(HelpRow::Shortcut {
                action, expanded, ..
            }) if expand || *expanded => {
                if expand {
                    self.expanded_actions.insert(*action);
                } else {
                    self.expanded_actions.remove(action);
                }
                HelpOutcome::Changed
            }
            Some(HelpRow::Section { .. }) | Some(HelpRow::Shortcut { .. }) | None => {
                HelpOutcome::Unchanged
            }
        }
    }

    fn move_selection(&mut self, rows: &[HelpRow], delta: isize) -> HelpOutcome {
        if rows.is_empty() {
            return HelpOutcome::Unchanged;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(rows.len().saturating_sub(1));
        self.visual_offset = 0;
        self.follow_selection = true;
        HelpOutcome::Changed
    }

    fn set_selection(&mut self, index: usize, rows: &[HelpRow]) -> HelpOutcome {
        let index = index.min(rows.len().saturating_sub(1));
        self.selected = index;
        self.visual_offset = 0;
        self.follow_selection = true;
        HelpOutcome::Changed
    }
}
