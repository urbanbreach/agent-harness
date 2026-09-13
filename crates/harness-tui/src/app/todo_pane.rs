use super::{AppState, Focus, ToolCallDisplayStatus};
use crate::keybindings::Action;
use crate::ui::{TranscriptTodoItem, TranscriptTodoStatus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

mod pointer;
mod query;
pub(crate) use query::{TodoQuery, TodoQueryMode};

#[derive(Debug, Default)]
pub(crate) struct TodoPaneState {
    pub(crate) visible: bool,
    pub(crate) focused: bool,
    pub(crate) fullscreen: bool,
    pub(crate) hide_done: bool,
    pub(crate) items: Vec<TranscriptTodoItem>,
    // Original item index, preserved when completed items are hidden.
    pub(crate) selected: Option<usize>,
    pub(crate) scroll: usize,
    pub(crate) query: TodoQuery,
    pub(crate) hovered: bool,
    pub(crate) close_hovered: bool,
    close_pressed: Option<ratatui::layout::Rect>,
}

impl TodoPaneState {
    pub(crate) fn invalidate_pointer(&mut self) {
        self.close_pressed = None;
    }
    pub(crate) fn visible_items(&self) -> Vec<(usize, &TranscriptTodoItem)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.status_visible(item.status) && self.query.permits(&item.content)
            })
            .collect()
    }

    fn status_visible(&self, status: TranscriptTodoStatus) -> bool {
        !self.hide_done
            || !matches!(
                status,
                TranscriptTodoStatus::Completed | TranscriptTodoStatus::Cancelled
            )
    }

    pub(crate) fn desired_height(&self, view_height: u16) -> u16 {
        if !self.visible {
            return 0;
        }
        let count = self
            .items
            .iter()
            .filter(|item| self.status_visible(item.status))
            .count();
        let cap = view_height.saturating_mul(15) / 100;
        u16::try_from(count)
            .unwrap_or(u16::MAX)
            .min(10)
            .min(cap.max(1))
            .max(1)
    }

    pub(crate) fn viewport_height(&self, height: u16) -> usize {
        usize::from(if self.query.has_bar() && height > 1 {
            height - 1
        } else {
            height
        })
    }

    pub(crate) fn scroll_offset(&self, height: u16) -> usize {
        self.scroll.min(
            self.visible_items()
                .len()
                .saturating_sub(self.viewport_height(height)),
        )
    }

    fn move_selection(&mut self, delta: isize, height: u16) {
        let visible = self.visible_items();
        let current = self
            .selected
            .and_then(|selected| visible.iter().position(|(id, _)| *id == selected));
        let target = match (delta, current) {
            (isize::MIN, _) => 0,
            (isize::MAX, _) => visible.len().saturating_sub(1),
            (delta, None) if delta < 0 => return,
            (_, None) => 0,
            (delta, Some(current)) => current
                .saturating_add_signed(delta)
                .min(visible.len().saturating_sub(1)),
        };
        self.selected = visible.get(target).map(|(id, _)| *id);
        self.reveal_selection(height);
    }

    fn reveal_selection(&mut self, height: u16) {
        let visible = self.visible_items();
        let index = self
            .selected
            .and_then(|id| visible.iter().position(|(candidate, _)| *candidate == id));
        let max_scroll = visible.len().saturating_sub(self.viewport_height(height));
        if let Some(index) = index {
            let viewport = self.viewport_height(height);
            let margin = 2.min(viewport.saturating_sub(1) / 2);
            if index < self.scroll.saturating_add(margin) {
                self.scroll = index.saturating_sub(margin);
            }
            let bottom = index.saturating_add(1).saturating_add(margin);
            if bottom > self.scroll.saturating_add(viewport) {
                self.scroll = bottom.saturating_sub(viewport);
            }
        }
        self.scroll = self.scroll.min(max_scroll);
    }

    fn find_match(&mut self, backwards: bool, advance: bool, height: u16) {
        let matches: Vec<_> = self
            .visible_items()
            .into_iter()
            .filter(|(_, item)| self.query.matches(&item.content))
            .map(|(id, _)| id)
            .collect();
        let current = self.selected.unwrap_or(0);
        self.selected = if backwards {
            matches
                .iter()
                .rev()
                .copied()
                .find(|id| *id < current)
                .or_else(|| matches.last().copied())
        } else {
            matches
                .iter()
                .copied()
                .find(|id| *id > current || (!advance && *id == current))
                .or_else(|| matches.first().copied())
        }
        .or(self.selected);
        self.reveal_selection(height);
    }

    fn hide(&mut self) {
        self.visible = false;
        self.focused = false;
        self.fullscreen = false;
        self.close_pressed = None;
        self.query.close_unaccepted();
    }

    pub(crate) fn placeholder(&self) -> String {
        if self.items.is_empty() {
            return "No todo items.".into();
        }
        let completed = self
            .items
            .iter()
            .filter(|item| item.status == TranscriptTodoStatus::Completed)
            .count();
        let cancelled = self
            .items
            .iter()
            .filter(|item| item.status == TranscriptTodoStatus::Cancelled)
            .count();
        match (completed, cancelled) {
            (_, 0) => "All done.".into(),
            (0, count) => format!("{count} cancelled."),
            (done, cancelled) => format!("{done} done. {cancelled} cancelled."),
        }
    }
}

impl AppState {
    pub(crate) fn refresh_todo_items(&mut self) {
        self.todo_pane.items = self
            .activities
            .iter()
            .flat_map(|activity| &activity.tool_calls)
            .rev()
            .find(|tool| {
                tool.status == ToolCallDisplayStatus::Succeeded
                    && matches!(
                        tool.effective_tool_id(),
                        "todo.write" | "todowrite" | "todo.read" | "todoread"
                    )
            })
            .map(|tool| crate::ui::todo_items_from_tool_call(tool, self.session_path.as_deref()))
            .unwrap_or_default();
    }

    pub(crate) fn todo_pane_focused(&self) -> bool {
        self.todo_pane.visible
            && self.todo_pane.focused
            && self.focus == Focus::Details
            && self.active_review_surface.is_none()
            && self.overlay_stack().top().is_none()
    }

    pub(crate) fn toggle_todo_pane(&mut self) {
        if self.todo_pane_focused() {
            self.todo_pane.hide();
        } else {
            self.refresh_todo_items();
            self.todo_pane.visible = true;
            self.todo_pane.focused = true;
            if self.todo_pane.selected.is_none() {
                self.todo_pane.selected = self.todo_pane.visible_items().first().map(|(id, _)| *id);
            }
            self.live_details_drawer_open = false;
        }
        self.focus = Focus::Details;
    }

    fn todo_input_height(&self) -> u16 {
        self.last_frame_area()
            .and_then(|area| crate::layout::FrameLayoutPlan::for_app(self, area).todo)
            .map_or(1, |area| area.height)
    }

    pub(super) fn handle_todo_pane_key(&mut self, key: KeyEvent) -> bool {
        self.todo_pane.close_pressed = None;
        if !self.todo_pane_focused() {
            return false;
        }
        let action = self.keymap.get_action(&key);
        if matches!(
            action,
            Some(Action::ToggleTodos | Action::Quit | Action::Help | Action::Palette)
        ) {
            return false;
        }
        let height = self.todo_input_height();
        if key.code == KeyCode::Char('f') && key.modifiers == KeyModifiers::CONTROL {
            self.todo_pane.fullscreen = !self.todo_pane.fullscreen;
        } else if self.todo_pane.query.editing {
            if let Err(error) = self.todo_pane.query.handle_key(key) {
                self.status_banner = Some(error.to_string());
            }
            self.todo_pane.find_match(false, false, height);
        } else {
            self.handle_todo_navigation_key(key, height);
        }
        true
    }

    fn handle_todo_navigation_key(&mut self, key: KeyEvent, height: u16) {
        let page =
            isize::try_from(self.todo_pane.viewport_height(height).max(1)).unwrap_or(isize::MAX);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.todo_pane.fullscreen {
                    self.todo_pane.fullscreen = false;
                } else {
                    self.todo_pane.hide();
                }
            }
            KeyCode::Tab | KeyCode::Char(' ') => {
                self.todo_pane.focused = false;
                self.todo_pane.fullscreen = false;
                self.focus = if key.code == KeyCode::Tab {
                    Focus::Details
                } else {
                    Focus::Prompt
                };
            }
            KeyCode::Char('h') => {
                self.todo_pane.hide_done = !self.todo_pane.hide_done;
                self.todo_pane.reveal_selection(height);
            }
            KeyCode::Char('/') => self.todo_pane.query.open(TodoQueryMode::Search),
            KeyCode::Char('f') => self.todo_pane.query.open(TodoQueryMode::Filter),
            KeyCode::Char('n' | 'N') => {
                self.todo_pane
                    .find_match(key.code == KeyCode::Char('N'), true, height)
            }
            KeyCode::Up | KeyCode::Char('k') => self.todo_pane.move_selection(-1, height),
            KeyCode::Down | KeyCode::Char('j') => self.todo_pane.move_selection(1, height),
            KeyCode::PageUp => self.todo_pane.move_selection(-page, height),
            KeyCode::PageDown => self.todo_pane.move_selection(page, height),
            KeyCode::Home | KeyCode::Char('g') => self.todo_pane.move_selection(isize::MIN, height),
            KeyCode::End | KeyCode::Char('G') => self.todo_pane.move_selection(isize::MAX, height),
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.todo_pane.move_selection(-(page / 2).max(1), height)
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                self.todo_pane.move_selection((page / 2).max(1), height)
            }
            KeyCode::Char('y') => {
                if let Some(item) = self
                    .todo_pane
                    .selected
                    .and_then(|index| self.todo_pane.items.get(index))
                {
                    if let Err(error) = crate::clipboard::copy(&item.content) {
                        self.status_banner = Some(error.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_todo_pane_paste(&mut self, text: &str) -> bool {
        if !self.todo_pane_focused() {
            return false;
        }
        if let Err(error) = self.todo_pane.query.paste(text) {
            self.status_banner = Some(error.to_string());
        }
        self.todo_pane
            .find_match(false, false, self.todo_input_height());
        true
    }

    pub fn todo_pane_has_tool_calls(&self) -> bool {
        self.orchestration_visible_rows()
            .iter()
            .any(|row| row.child_tool_call_count > 0)
    }

    pub fn todo_pane_total_tool_calls(&self) -> usize {
        self.orchestration_visible_rows()
            .iter()
            .map(|row| row.child_tool_call_count)
            .sum()
    }
}
