use super::{ViewerError, ViewerState};
use crate::app::pane_query::PaneQueryMode;
use crate::transcript_selection::{CellPoint, NavigationKey};

impl ViewerState {
    pub(crate) fn scroll_keeping_cursor(&mut self, delta: f64) -> Result<(), ViewerError> {
        if self.child {
            return self.scroll_child_cursor(delta, true);
        }
        let previous = self.scroll_top();
        self.scroll_by(delta)?;
        self.cursor.row = self
            .cursor
            .row
            .saturating_add(self.scroll_top())
            .saturating_sub(previous)
            .min(self.wrapped.row_count().saturating_sub(1));
        Ok(())
    }

    pub(crate) fn scroll_wheel(&mut self, delta: f64) -> Result<(), ViewerError> {
        if !self.child {
            return self.scroll_by(delta);
        }
        if !self.following && self.wrapped.row_count() <= self.viewport_height() {
            return Ok(());
        }
        self.visual_mode = false;
        self.selection = None;
        self.scroll_child_cursor(delta, false)
    }

    fn scroll_child_cursor(&mut self, delta: f64, move_past_edge: bool) -> Result<(), ViewerError> {
        if self.following && delta > 0.0 {
            return Ok(());
        }
        self.exit_follow();
        if delta < 0.0 {
            self.at_end = false;
        }
        let previous = self.scroll_top();
        let screen_y = *self
            .scroll_screen_y
            .get_or_insert(self.cursor.row.saturating_sub(previous));
        self.scroll_by(delta)?;
        let offset = if move_past_edge {
            f64::from(u32::try_from(previous).unwrap_or(u32::MAX)) + delta
        } else {
            f64::from(u32::try_from(self.scroll_top()).unwrap_or(u32::MAX))
        };
        let target = offset + f64::from(u32::try_from(screen_y).unwrap_or(u32::MAX));
        let row = super::render::scroll_offset(target.max(0.0))
            .min(self.wrapped.row_count().saturating_sub(1));
        if move_past_edge || previous != self.scroll_top() {
            self.cursor = CellPoint::new(self.logical_rows(row).start, 0);
        }
        if self.running
            && !self.visual_mode
            && delta > 0.0
            && self.scroll_top()
                >= self
                    .wrapped
                    .row_count()
                    .saturating_sub(self.viewport_height())
        {
            if self.at_end {
                self.toggle_follow();
            } else {
                self.at_end = true;
            }
        }
        Ok(())
    }

    pub(crate) fn select_edge(&mut self, last: bool) {
        self.scroll_screen_y = None;
        self.at_end = false;
        self.cursor = CellPoint::new(
            if last {
                self.wrapped.row_count().saturating_sub(1)
            } else {
                0
            },
            0,
        );
        self.selection = None;
        self.following = false;
        self.reveal_cursor();
    }

    pub fn search_editing(&self) -> bool {
        self.input.editing && self.input.mode == PaneQueryMode::Search
    }

    pub fn set_search_editing(&mut self, editing: bool) {
        if editing {
            self.exit_follow();
            self.input.open(PaneQueryMode::Search);
        } else {
            self.input.editing = false;
        }
    }

    pub(crate) fn input_active(&self) -> bool {
        self.search_editing()
            || !self.search().query().is_empty()
            || self.filter_editing()
            || !self.filter_query.is_empty()
    }

    pub(crate) fn filter_editing(&self) -> bool {
        self.input.editing && self.input.mode == PaneQueryMode::Filter
    }
    pub(crate) fn filter_query(&self) -> &str {
        &self.filter_query
    }
    pub(crate) fn set_filter_editing(&mut self, editing: bool) {
        if editing {
            self.exit_follow();
            self.input.open(PaneQueryMode::Filter);
        } else {
            self.input.editing = false;
        }
    }
    pub(crate) fn apply_input(&mut self) {
        let text = self.input.editor.text();
        if self.input.mode == PaneQueryMode::Filter {
            if self.filter_query != text {
                let _ = self.set_filter_query(text);
            }
        } else if self.search().query() != text {
            let row = self.logical_rows(self.cursor.row).start;
            if self.child {
                let _ = self.update_search(&text);
                self.find_matching_line(row, true, true);
            } else {
                let _ = self.set_search_query(&text);
            }
        }
    }

    pub(crate) fn set_child_running(&mut self, running: bool) {
        if !self.child {
            self.child = true;
            self.following = running;
            let _ = self.rebuild_display();
        } else if self.running && !running {
            self.pin_to_tail();
        }
        self.running = running;
    }

    pub(super) fn exit_follow(&mut self) {
        if self.following {
            self.following = false;
            self.at_end = false;
            self.cursor = CellPoint::new(
                self.logical_rows(self.wrapped.row_count().saturating_sub(1))
                    .start,
                0,
            );
        }
    }

    pub(crate) fn toggle_follow(&mut self) {
        if !self.running {
            return;
        }
        if self.following {
            self.exit_follow();
        } else {
            self.select_edge(true);
            self.following = true;
        }
    }

    pub(crate) fn navigate_line(&mut self, forward: bool) {
        self.scroll_screen_y = None;
        if self.following && forward {
            return;
        }
        self.exit_follow();
        let before = self.cursor.row;
        self.move_cursor(
            if forward {
                NavigationKey::Down
            } else {
                NavigationKey::Up
            },
            self.visual_mode,
        );
        if forward && before == self.cursor.row && self.running && !self.visual_mode {
            if self.at_end {
                self.toggle_follow();
            } else {
                self.at_end = true;
            }
        } else {
            self.at_end = false;
        }
        self.reveal_cursor();
    }

    pub(crate) fn find_matching_line(&mut self, row: usize, forward: bool, include_current: bool) {
        let rows = self
            .search()
            .matches()
            .iter()
            .map(|found| {
                self.logical_rows(self.wrapped.point_for_byte(found.byte_range.start).row)
                    .start
            })
            .collect::<std::collections::BTreeSet<_>>();
        let target = if forward {
            rows.iter()
                .find(|target| **target > row || (include_current && **target == row))
                .or_else(|| rows.first())
        } else {
            rows.iter()
                .rev()
                .find(|target| **target < row)
                .or_else(|| rows.last())
        };
        if let Some(target) = target {
            if let Some(index) = self.search.matches().iter().position(|found| {
                self.logical_rows(self.wrapped.point_for_byte(found.byte_range.start).row)
                    .start
                    == *target
            }) {
                self.search.select(
                    index,
                    !include_current
                        && if forward {
                            *target <= row
                        } else {
                            *target >= row
                        },
                );
            }
            self.exit_follow();
            self.cursor = CellPoint::new(*target, 0);
            if !include_current || *target != row {
                self.reveal_cursor();
            }
        }
    }
}
