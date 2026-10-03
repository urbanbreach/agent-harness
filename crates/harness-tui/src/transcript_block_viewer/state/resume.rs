use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewerResume {
    selected_line: Option<usize>,
    scroll_top: usize,
    following: bool,
}

impl ViewerState {
    pub(in crate::transcript_block_viewer) fn pin_to_tail(&mut self) {
        self.select_edge(true);
        if let Some(row) = (self.body_start..self.wrapped.row_count())
            .rev()
            .find(|row| !self.wrapped.row_text(*row).is_empty())
        {
            self.cursor = CellPoint::new(self.logical_rows(row).start, 0);
            self.reveal_cursor();
        }
    }

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
        if resume.following {
            self.pin_to_tail();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_blocks::FoldState;
    use crate::transcript_identity::{FocusFollowState, ReplayTurn, TranscriptFocus};

    #[test]
    fn finishing_followed_content_selects_its_current_nonempty_tail(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = ReplayTurn::event(1, 0, 0).block_id(0);
        let layout = TranscriptLayout::from_heights([(id, 12.0)], 5.0)?;
        let snapshot = ViewerReturnSnapshot::new(
            FoldState::Expanded,
            FocusFollowState::new(TranscriptFocus::Transcript, true),
            LogicalAnchor::capture(&layout, 0.0)?,
        );
        for reopen in [false, true] {
            let mut viewer =
                ViewerState::open(id, ViewerBlockContent::new("Old tail", None), snapshot)?;
            viewer.set_child_running(true);
            let resume = viewer.resume_snapshot();
            let content = ViewerBlockContent::new("Old tail\nNew tail\n\n", None);
            if reopen {
                viewer = ViewerState::open(id, content, snapshot)?;
                viewer.set_child_running(false);
                viewer.restore_resume(resume);
            } else {
                viewer.update_content(content)?;
                viewer.set_child_running(false);
            }
            assert_eq!(viewer.quote_text(), "New tail");
            assert!(!viewer.following);
        }
        Ok(())
    }
}
