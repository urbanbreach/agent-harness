use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewerResume {
    selected_line: Option<usize>,
    scroll_top: usize,
    following: bool,
}

impl ViewerState {
    pub(crate) fn resume_snapshot(&self) -> ViewerResume {
        let row = if self.following {
            (self.body_start..self.wrapped.row_count())
                .rev()
                .find(|row| !self.wrapped.row_text(*row).is_empty())
                .unwrap_or(self.cursor.row)
        } else {
            self.cursor.row
        };
        ViewerResume {
            selected_line: self.row_line_ids.get(row).copied(),
            scroll_top: self.scroll_top(),
            following: self.following,
        }
    }

    pub(crate) fn restore_resume(&mut self, resume: ViewerResume) {
        if resume.following && self.following {
            return;
        }
        self.following = false;
        if let Some(row) = resume
            .selected_line
            .and_then(|id| self.row_line_ids.iter().position(|line| *line == id))
        {
            self.cursor = CellPoint::new(row, 0);
        }
        self.scroll_top = f64::from(u32::try_from(resume.scroll_top).unwrap_or(u32::MAX))
            .min(self.layout.max_scroll());
        self.reveal_cursor();
    }
}
