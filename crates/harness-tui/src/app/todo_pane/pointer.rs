use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

impl AppState {
    pub(crate) fn handle_todo_pane_mouse(&mut self, mouse: MouseEvent, frame: Rect) -> bool {
        let Some(area) = crate::layout::FrameLayoutPlan::for_app(self, frame).todo else {
            return false;
        };
        let close = crate::layout::todo_close_rect(area);
        let bounds = Rect::new(
            area.x.saturating_sub(1),
            area.y.saturating_sub(1),
            area.width.saturating_add(2),
            area.height.saturating_add(2),
        );
        let point = Position::new(mouse.column, mouse.row);
        let inside = bounds.contains(point);
        match mouse.kind {
            MouseEventKind::Moved => {
                let changed = self.todo_pane.hovered != inside
                    || self.todo_pane.close_hovered != close.contains(point);
                self.todo_pane.hovered = inside;
                self.todo_pane.close_hovered = close.contains(point);
                return inside || changed;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.todo_pane.close_pressed = None;
                if !inside {
                    self.todo_pane.focused = false;
                    return false;
                }
                if self.todo_pane_focused() && close.contains(point) {
                    self.todo_pane.close_pressed = Some(close);
                } else {
                    self.todo_pane.focused = true;
                    self.focus = Focus::Details;
                    if area.contains(point) {
                        let index = usize::from(mouse.row.saturating_sub(area.y))
                            + self.todo_pane.scroll_offset(area.height);
                        self.todo_pane.selected =
                            self.todo_pane.visible_items().get(index).map(|(id, _)| *id);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.todo_pane.close_pressed.take() == Some(close) && close.contains(point) {
                    self.todo_pane.hide();
                    return true;
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if inside => {
                let delta = if area.height > 5 { 2 } else { 1 };
                self.todo_pane.scroll = if mouse.kind == MouseEventKind::ScrollUp {
                    self.todo_pane.scroll.saturating_sub(delta)
                } else {
                    self.todo_pane.scroll.saturating_add(delta)
                };
                self.todo_pane.scroll = self.todo_pane.scroll_offset(area.height);
            }
            _ => {}
        }
        inside
    }
}
