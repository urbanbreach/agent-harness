use super::*;
use crate::keybindings::Action;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

impl AppState {
    fn reconcile_task_index(&mut self) {
        self.tasks_pane.selected = self
            .tasks_pane
            .selected
            .min(self.task_pane_rows().len().saturating_sub(1));
        self.remember_task_selection();
    }

    fn copy_task_row(&mut self) {
        if let Some(row) = self.task_pane_rows().get(self.tasks_pane.selected)
            && let Err(error) = super::super::clipboard::copy(&row.search_label())
        {
            self.show_toast(
                format!("clipboard copy failed: {error}"),
                super::super::ToastVariant::Error,
            );
        }
    }

    fn move_task_page(&mut self, down: bool) {
        let height =
            usize::from(self.task_pane_height(self.last_frame_area.map_or(40, |area| area.height)));
        self.tasks_pane.selected = if down {
            self.tasks_pane.selected.saturating_add(height)
        } else {
            self.tasks_pane.selected.saturating_sub(height)
        };
    }

    fn find_task_match(&mut self, backwards: bool) {
        let rows = self.task_pane_rows();
        let count = rows.len();
        if count == 0 || !self.tasks_pane.query.active {
            return;
        }
        for step in 1..=count {
            let index = if backwards {
                (self.tasks_pane.selected + count - step) % count
            } else {
                (self.tasks_pane.selected + step) % count
            };
            if !rows[index].header && self.tasks_pane.query.matches(&rows[index].search_label()) {
                self.tasks_pane.selected = index;
                self.remember_task_selection();
                break;
            }
        }
    }

    fn handle_task_query_key(&mut self, key: KeyEvent) -> bool {
        if !self.tasks_pane.query.editing {
            return false;
        }
        if let Err(error) = self.tasks_pane.query.handle_key(key) {
            self.show_toast(error.to_string(), super::super::ToastVariant::Error);
        }
        self.reconcile_task_selection();
        if self.tasks_pane.query.mode == PaneQueryMode::Search
            && let Some(index) = self
                .task_pane_rows()
                .iter()
                .position(|row| !row.header && self.tasks_pane.query.matches(&row.search_label()))
        {
            self.tasks_pane.selected = index;
        }
        self.remember_task_selection();
        true
    }

    pub(in crate::app) fn handle_tasks_pane_paste(&mut self, text: &str) -> bool {
        if !self.tasks_pane.visible || !self.tasks_pane.focused {
            return false;
        }
        if let Err(error) = self.tasks_pane.query.paste(text) {
            self.show_toast(error.to_string(), super::super::ToastVariant::Error);
        }
        self.reconcile_task_selection();
        if self.tasks_pane.query.mode == PaneQueryMode::Search {
            self.find_task_match(false);
        }
        self.reveal_task_selection();
        true
    }

    pub(in crate::app) fn handle_tasks_pane_key(&mut self, key: KeyEvent) -> bool {
        if self.current_subagent_session_present()
            || !self.tasks_pane.visible
            || !self.tasks_pane.focused
        {
            return false;
        }
        if self.keymap.get_action(&key) == Some(Action::ToggleTasks) {
            self.toggle_tasks_pane();
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return false;
        }
        if self.handle_task_query_key(key) {
            self.reveal_task_selection();
            return true;
        }
        match key.code {
            KeyCode::Char('f') if key.modifiers == KeyModifiers::CONTROL => {
                if self.task_pane_rows().is_empty() {
                    self.tasks_pane.fullscreen = !self.tasks_pane.fullscreen;
                } else {
                    self.activate_task_pane_row(false);
                }
            }
            KeyCode::Esc if self.tasks_pane.fullscreen => self.tasks_pane.fullscreen = false,
            KeyCode::Esc if self.tasks_pane.query.active => {
                self.tasks_pane.query = PaneQuery::default();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.tasks_pane.visible = false;
                self.tasks_pane.focused = false;
                self.tasks_pane.auto_opened = false;
                self.focus = Focus::Details;
            }
            KeyCode::Tab | KeyCode::Char(' ') => {
                self.tasks_pane.fullscreen = false;
                self.tasks_pane.focused = false;
                self.focus = if key.code == KeyCode::Tab || self.replay_mode {
                    Focus::Details
                } else {
                    Focus::Prompt
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.tasks_pane.selected = self
                    .tasks_pane
                    .selected
                    .saturating_add(1)
                    .min(self.task_pane_rows().len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.tasks_pane.selected = self.tasks_pane.selected.saturating_sub(1)
            }
            KeyCode::Home | KeyCode::Char('g') => self.tasks_pane.selected = 0,
            KeyCode::End | KeyCode::Char('G') => {
                self.tasks_pane.selected = self.task_pane_rows().len().saturating_sub(1)
            }
            KeyCode::PageDown => self.move_task_page(true),
            KeyCode::PageUp => self.move_task_page(false),
            KeyCode::Char('h') => self.tasks_pane.show_done = !self.tasks_pane.show_done,
            KeyCode::Char('/') | KeyCode::Char('f') => {
                self.tasks_pane
                    .query
                    .open(if key.code == KeyCode::Char('f') {
                        PaneQueryMode::Filter
                    } else {
                        PaneQueryMode::Search
                    });
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.find_task_match(key.code == KeyCode::Char('N'))
            }
            KeyCode::Char('y') => self.copy_task_row(),
            KeyCode::Enter => self.activate_task_pane_row(false),
            KeyCode::Char('x') => self.activate_task_pane_row(true),
            KeyCode::Left | KeyCode::Right => {
                if let Some(row) = self
                    .task_pane_all_rows()
                    .get(self.tasks_pane.selected)
                    .filter(|row| row.header)
                {
                    if key.code == KeyCode::Left {
                        self.tasks_pane.collapsed.insert(row.group);
                    } else {
                        self.tasks_pane.collapsed.remove(row.group);
                    }
                }
            }
            _ => return false,
        }
        self.reconcile_task_index();
        self.reveal_task_selection();
        true
    }

    fn reveal_task_selection(&mut self) {
        let screen = self.last_frame_area.unwrap_or(Rect::new(0, 0, 120, 40));
        let Some(area) = crate::layout::FrameLayoutPlan::for_app(self, screen).tasks else {
            return;
        };
        let height = usize::from(self.task_pane_layout(area).rows.height).max(1);
        self.tasks_pane.scroll = self.tasks_pane.scroll.min(self.tasks_pane.selected).max(
            self.tasks_pane
                .selected
                .saturating_add(1)
                .saturating_sub(height),
        );
    }

    pub(in crate::app) fn handle_tasks_pane_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> bool {
        let Some(pane) = crate::layout::FrameLayoutPlan::for_app(self, area).tasks else {
            return false;
        };
        let border = Rect::new(
            pane.x.saturating_sub(4),
            pane.y.saturating_sub(1),
            pane.width.saturating_add(7),
            pane.height.saturating_add(2),
        );
        let was_hovered = self.tasks_pane.hovered;
        self.tasks_pane.hovered = border.contains(Position::new(mouse.column, mouse.row));
        let close = Rect::new(border.right().saturating_sub(1), border.y, 1, 1);
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && close.contains(Position::new(mouse.column, mouse.row))
        {
            self.tasks_pane.focused = true;
            self.toggle_tasks_pane();
            return true;
        }
        if !pane.contains(Position::new(mouse.column, mouse.row)) {
            return was_hovered != self.tasks_pane.hovered;
        }
        let layout = self.task_pane_layout(pane);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.tasks_pane.focused = true;
                self.focus = Focus::List;
                if layout.rows.contains(Position::new(mouse.column, mouse.row)) {
                    self.tasks_pane.selected =
                        layout.offset + usize::from(mouse.row - layout.rows.y);
                    self.remember_task_selection();
                    let running = self
                        .task_pane_rows()
                        .get(self.tasks_pane.selected)
                        .is_some_and(|row| !row.state.is_terminal());
                    self.activate_task_pane_row(
                        running && mouse.column >= pane.right().saturating_sub(3),
                    );
                }
            }
            MouseEventKind::ScrollDown => {
                self.tasks_pane.scroll = self.tasks_pane.scroll.saturating_add(1).min(
                    self.task_pane_rows()
                        .len()
                        .saturating_sub(usize::from(pane.height)),
                )
            }
            MouseEventKind::ScrollUp => {
                self.tasks_pane.scroll = self.tasks_pane.scroll.saturating_sub(1)
            }
            _ => {}
        }
        true
    }
}
