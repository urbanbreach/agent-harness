use super::*;

impl ViewerState {
    pub(in crate::transcript_block_viewer) fn rebuild_display(
        &mut self,
    ) -> Result<(), ViewerError> {
        let previous_body = self.body_start;
        let selected_id = self.row_line_ids.get(self.cursor.row).copied();
        let (lines, body_start) = self.source_lines();
        self.unfiltered_line_count = lines.len();
        let pattern = if self.child {
            self.filter_query.clone()
        } else {
            regex::escape(&self.filter_query)
        };
        let matcher = (!self.filter_query.is_empty())
            .then(|| {
                regex::RegexBuilder::new(&pattern)
                    .case_insensitive(!self.filter_query.chars().any(char::is_uppercase))
                    .build()
                    .ok()
            })
            .flatten();
        let lines = lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| {
                self.filter_query.is_empty()
                    || matcher
                        .as_ref()
                        .is_some_and(|regex| regex.is_match(&line.to_string()))
            })
            .collect::<Vec<_>>();
        self.body_start = lines.iter().take_while(|(id, _)| *id < body_start).count();
        let wrap_width = self.wrap_width(&lines);
        (self.styled_lines, self.row_joiners, self.row_line_ids) = if self.wrap_enabled {
            crate::ui::viewer_wrap_lines(lines, wrap_width)
        } else {
            let joiners = vec!["\n".to_owned(); lines.len()];
            let (ids, lines) = lines.into_iter().unzip();
            (lines, joiners, ids)
        };
        let display_text = self
            .styled_lines
            .iter()
            .map(ratatui::text::Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let width = if self.wrap_enabled {
            wrap_width
        } else {
            display_text
                .lines()
                .map(unicode_width::UnicodeWidthStr::width)
                .max()
                .unwrap_or(1)
                .max(self.width)
        };
        self.wrapped = TextLayout::new(display_text, width).map_err(ViewerError::Selection)?;
        self.layout = viewer_layout(self.block_id, self.wrapped.row_count(), self.height)?;
        if self.child {
            self.body_start = self
                .row_line_ids
                .iter()
                .take_while(|id| **id < body_start)
                .count();
            self.cursor.row = selected_id
                .and_then(|id| self.row_line_ids.iter().position(|row_id| *row_id == id))
                .unwrap_or(0);
        } else if self.cursor.row >= previous_body {
            self.cursor.row = self
                .cursor
                .row
                .saturating_sub(previous_body)
                .saturating_add(self.body_start);
        }
        self.cursor.row = self
            .cursor
            .row
            .min(self.wrapped.row_count().saturating_sub(1));
        self.scroll_top = if self.following {
            self.layout.max_scroll()
        } else {
            self.scroll_top.min(self.layout.max_scroll())
        };
        if !self.search.query().is_empty() {
            let query = self.search.query().to_owned();
            let _ = self.update_search(&query);
        }
        Ok(())
    }

    fn wrap_width(&self, lines: &[(usize, ratatui::text::Line<'static>)]) -> usize {
        // The native pane allocates its scrollbar gutter from the unfiltered
        // item count. Filtering can make that gutter available to the text.
        let full_height = self.height + usize::from(self.input_active());
        if self.child && self.unfiltered_line_count > full_height && lines.len() <= self.height {
            let wider = self.width.saturating_add(2);
            if crate::ui::viewer_wrap_lines(lines.to_vec(), wider).0.len() <= self.height {
                return wider;
            }
        }
        self.width
    }

    fn source_lines(&self) -> (Vec<ratatui::text::Line<'static>>, usize) {
        let mut body = if let Some(crate::transcript_block_viewer::ViewerPreamble::Read {
            path,
            start_line: Some(start),
            ..
        }) = &self.content.preamble
        {
            crate::ui::viewer_read_lines(self.content.text(self.mode), path, *start, &self.theme)
        } else if self.mode == ViewerMode::Wrapped && self.content.markdown {
            crate::ui::viewer_markdown_lines(
                self.content.content(),
                if self.child {
                    u16::MAX
                } else {
                    u16::try_from(self.width).unwrap_or(u16::MAX)
                },
                &self.theme,
            )
        } else {
            self.content
                .text(self.mode)
                .split('\n')
                .map(|line| ratatui::text::Line::from(line.to_owned()))
                .collect()
        };
        if self.child && self.content.markdown {
            while body
                .last()
                .is_some_and(|line| line.width() == 0 && line.style.bg.is_none())
            {
                body.pop();
            }
        }
        let mut lines = self
            .content
            .preamble
            .as_ref()
            .map(|preamble| crate::ui::viewer_preamble_lines(preamble, self.width, &self.theme))
            .unwrap_or_default();
        let body_start = lines.len();
        lines.extend(body);
        (lines, body_start)
    }
}
