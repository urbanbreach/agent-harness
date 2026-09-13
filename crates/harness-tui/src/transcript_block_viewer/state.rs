use crate::transcript_identity::BlockId;
use crate::transcript_scroll::{
    EasingKind, LogicalAnchor, MotionPreference, ScrollError, ScrollFrame, ScrollTransition,
    TranscriptLayout, TransitionRequest,
};
use crate::transcript_selection::{CellPoint, SelectionRange, WrappedText};

use super::render::{render_surface, ViewerRenderSurface};
use super::search::{SearchDirection, SearchNavigation, SearchState};
use super::{
    ViewerBlockContent, ViewerClose, ViewerCloseReason, ViewerError, ViewerMode,
    ViewerReturnSnapshot,
};

const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 24;

pub struct ViewerState {
    block_id: BlockId,
    content: ViewerBlockContent,
    pub(super) display_text: String,
    pub(super) styled_lines: Vec<ratatui::text::Line<'static>>,
    pub(super) row_joiners: Vec<String>,
    theme: crate::theme::Theme,
    return_snapshot: ViewerReturnSnapshot,
    mode: ViewerMode,
    width: usize,
    height: usize,
    pub(super) wrapped: WrappedText,
    pub(super) selection: Option<SelectionRange>,
    pub(super) cursor: CellPoint,
    pub(super) body_start: usize,
    pub(super) close_hovered: bool,
    pub(super) filter_query: String,
    pub(super) filter_editing: bool,
    pub(super) visual_mode: bool,
    pub(super) wrap_enabled: bool,
    search: SearchState,
    search_editing: bool,
    layout: TranscriptLayout,
    scroll_top: f64,
    transition: Option<ScrollTransition>,
    open: bool,
}

impl ViewerState {
    pub fn open(
        block_id: BlockId,
        content: ViewerBlockContent,
        return_snapshot: ViewerReturnSnapshot,
    ) -> Result<Self, ViewerError> {
        let mode = ViewerMode::Wrapped;
        let width = DEFAULT_WIDTH;
        let height = DEFAULT_HEIGHT;
        let wrapped =
            WrappedText::new(content.text(mode), width).map_err(ViewerError::Selection)?;
        let layout = viewer_layout(block_id, content.text(mode), width, height)?;
        let mut state = Self {
            block_id,
            display_text: content.text(mode).to_string(),
            content,
            styled_lines: Vec::new(),
            row_joiners: Vec::new(),
            theme: crate::theme::Theme::default(),
            return_snapshot,
            mode,
            width,
            height,
            wrapped,
            selection: None,
            cursor: CellPoint::new(0, 0),
            body_start: 0,
            close_hovered: false,
            filter_query: String::new(),
            filter_editing: false,
            visual_mode: false,
            wrap_enabled: true,
            search: SearchState::new(),
            search_editing: false,
            layout,
            scroll_top: 0.0,
            transition: None,
            open: true,
        };
        state.rebuild_display()?;
        Ok(state)
    }

    pub fn block_id(&self) -> BlockId {
        self.block_id
    }

    pub fn content(&self) -> &ViewerBlockContent {
        &self.content
    }

    pub const fn mode(&self) -> ViewerMode {
        self.mode
    }

    pub fn toggle_mode(&mut self) -> Result<(), ViewerError> {
        let mode = match self.mode {
            ViewerMode::Wrapped => ViewerMode::Raw,
            ViewerMode::Raw => ViewerMode::Wrapped,
        };
        self.mode = mode;
        if let Err(error) = self.rebuild_display() {
            self.mode = match mode {
                ViewerMode::Wrapped => ViewerMode::Raw,
                ViewerMode::Raw => ViewerMode::Wrapped,
            };
            return Err(error);
        }
        Ok(())
    }

    pub fn set_search_query(&mut self, query: &str) -> SearchNavigation {
        let navigation = self.search.set_query(&self.display_text, query);
        self.reveal_search_match();
        navigation
    }

    pub fn search_forward(&mut self) -> SearchNavigation {
        let navigation = self.search.navigate(SearchDirection::Forward);
        self.reveal_search_match();
        navigation
    }

    pub fn search_backward(&mut self) -> SearchNavigation {
        let navigation = self.search.navigate(SearchDirection::Backward);
        self.reveal_search_match();
        navigation
    }

    pub fn search_editing(&self) -> bool {
        self.search_editing
    }

    pub fn set_search_editing(&mut self, editing: bool) {
        self.search_editing = editing;
    }

    pub fn viewport_height(&self) -> usize {
        self.height
    }

    pub(crate) fn input_active(&self) -> bool {
        self.search_editing
            || !self.search.query().is_empty()
            || self.filter_editing
            || !self.filter_query.is_empty()
    }

    pub(crate) fn filter_editing(&self) -> bool {
        self.filter_editing
    }
    pub(crate) fn filter_query(&self) -> &str {
        &self.filter_query
    }
    pub(crate) fn set_filter_editing(&mut self, editing: bool) {
        self.filter_editing = editing;
    }
    pub(crate) fn set_filter_query(&mut self, query: String) -> Result<(), ViewerError> {
        self.filter_query = query;
        self.scroll_top = 0.0;
        self.rebuild_display()?;
        self.cursor = CellPoint::new(0, 0);
        Ok(())
    }
    pub(crate) fn toggle_wrap(&mut self) -> Result<(), ViewerError> {
        self.wrap_enabled = !self.wrap_enabled;
        self.rebuild_display()
    }
    pub(crate) fn toggle_visual(&mut self) {
        self.visual_mode = !self.visual_mode;
        if self.visual_mode {
            let rows = self.logical_rows(self.cursor.row);
            let end = self
                .wrapped
                .select(
                    CellPoint::new(rows.end - 1, 0),
                    crate::transcript_selection::SelectionMode::Line,
                )
                .focus;
            self.selection = Some(SelectionRange::new(CellPoint::new(rows.start, 0), end));
        } else {
            self.selection = None;
        }
    }
    pub(crate) fn visual_mode(&self) -> bool {
        self.visual_mode
    }

    pub(super) fn logical_rows(&self, row: usize) -> std::ops::Range<usize> {
        let mut start = row;
        while start > 0
            && self
                .row_joiners
                .get(start - 1)
                .is_some_and(|joiner| joiner != "\n")
        {
            start -= 1;
        }
        let mut end = row + 1;
        while end < self.wrapped.row_count()
            && self
                .row_joiners
                .get(end - 1)
                .is_some_and(|joiner| joiner != "\n")
        {
            end += 1;
        }
        start..end
    }
    pub(crate) fn quote_text(&self) -> String {
        self.copy_selection_text()
            .unwrap_or_else(|_| self.wrapped.row_text(self.cursor.row))
    }
    pub(crate) fn command_text(&self) -> Option<String> {
        match &self.content.preamble {
            Some(super::ViewerPreamble::Command { command, .. }) => Some(command.clone()),
            Some(super::ViewerPreamble::Read { path, .. }) => Some(path.clone()),
            _ => None,
        }
    }
    pub(crate) fn set_close_hovered(&mut self, hovered: bool) {
        self.close_hovered = hovered;
    }

    pub fn scroll_top(&self) -> usize {
        // The layout only accepts finite, bounded row counts.
        super::render::scroll_offset(self.scroll_top)
    }

    pub fn reveal_cursor(&mut self) {
        let row = f64::from(u32::try_from(self.cursor.row).unwrap_or(u32::MAX));
        let height = f64::from(u32::try_from(self.height).unwrap_or(u32::MAX));
        let margin = ((height - 1.0) / 2.0).floor().min(2.0);
        if row < self.scroll_top + margin {
            self.scroll_top = (row - margin).max(0.0);
        } else if row + 1.0 + margin > self.scroll_top + height {
            self.scroll_top = (row + 1.0 + margin - height).min(self.layout.max_scroll());
        }
        self.transition = None;
    }

    fn reveal_search_match(&mut self) {
        if let Some(found) = self.search.current_match() {
            self.cursor = self.wrapped.point_for_byte(found.byte_range.start);
            self.reveal_cursor();
        }
    }

    pub fn search(&self) -> &SearchState {
        &self.search
    }

    pub(crate) fn set_theme(&mut self, theme: crate::theme::Theme) -> Result<(), ViewerError> {
        if theme == self.theme {
            return Ok(());
        }
        self.theme = theme;
        self.rebuild_display()
    }

    pub fn resize(&mut self, width: usize, height: usize) -> Result<(), ViewerError> {
        if width == 0 || height == 0 {
            return Err(ViewerError::InvalidViewport);
        }
        if (width, height) == (self.width, self.height) {
            return Ok(());
        }
        let anchor = self.scroll_anchor().map_err(ViewerError::Scroll)?;
        let width_changed = self.width != width;
        self.width = width;
        self.height = height;
        if width_changed {
            self.rebuild_display()?;
        } else {
            self.layout = viewer_layout(
                self.block_id,
                &self.display_text,
                self.wrapped_width(),
                height,
            )?;
        }
        self.scroll_top = anchor.resolve(&self.layout).map_err(ViewerError::Scroll)?;
        self.transition = None;
        Ok(())
    }

    pub(crate) fn scroll_keeping_cursor(&mut self, delta: f64) -> Result<(), ViewerError> {
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

    pub(crate) fn select_edge(&mut self, last: bool) {
        self.cursor = CellPoint::new(
            if last {
                self.wrapped.row_count().saturating_sub(1)
            } else {
                0
            },
            0,
        );
        self.selection = None;
        self.reveal_cursor();
    }

    fn wrapped_width(&self) -> usize {
        if self.wrap_enabled {
            self.width
        } else {
            self.display_text
                .lines()
                .map(unicode_width::UnicodeWidthStr::width)
                .max()
                .unwrap_or(1)
                .max(self.width)
        }
    }

    pub fn scroll_by(&mut self, delta: f64) -> Result<(), ViewerError> {
        if !delta.is_finite() {
            return Err(ViewerError::Scroll(ScrollError::NonFinite("scroll_delta")));
        }
        self.scroll_to(self.scroll_top + delta, 0, MotionPreference::ReducedMotion)
    }

    pub fn scroll_to(
        &mut self,
        target: f64,
        started_at_ms: u64,
        motion: MotionPreference,
    ) -> Result<(), ViewerError> {
        let target = target.clamp(0.0, self.layout.max_scroll());
        let transition = ScrollTransition::start(TransitionRequest::new(
            self.scroll_top,
            target,
            started_at_ms,
            EasingKind::Jump,
            motion,
        ))
        .map_err(ViewerError::Scroll)?;
        let frame = transition.sample(started_at_ms);
        self.scroll_top = frame.value;
        self.transition = (!frame.settled).then_some(transition);
        Ok(())
    }

    pub fn sample_scroll(&mut self, now_ms: u64) -> ScrollFrame {
        let Some(transition) = self.transition else {
            return ScrollFrame {
                value: self.scroll_top,
                settled: true,
                needs_redraw: false,
            };
        };
        let frame = transition.sample(now_ms);
        self.scroll_top = frame.value;
        if frame.settled {
            self.transition = None;
        }
        frame
    }

    pub fn scroll_anchor(&self) -> Result<LogicalAnchor, ScrollError> {
        self.layout.capture_anchor(self.scroll_top)
    }

    pub fn render_surface(&self, area: ratatui::layout::Rect) -> ViewerRenderSurface {
        render_surface(self, area)
    }

    pub fn return_snapshot(&self) -> ViewerReturnSnapshot {
        self.return_snapshot
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn reconcile_block(&mut self, present: bool) -> Option<ViewerClose> {
        if present || !self.open {
            return None;
        }
        self.open = false;
        Some(ViewerClose {
            reason: ViewerCloseReason::BlockDisappeared,
            return_snapshot: self.return_snapshot,
        })
    }

    pub fn close(mut self) -> ViewerClose {
        self.open = false;
        ViewerClose {
            reason: ViewerCloseReason::Closed,
            return_snapshot: self.return_snapshot,
        }
    }

    fn rebuild_display(&mut self) -> Result<(), ViewerError> {
        let previous_body = self.body_start;
        let mut body = if let Some(super::ViewerPreamble::Read {
            path,
            start_line: Some(start),
            ..
        }) = &self.content.preamble
        {
            crate::ui::viewer_read_lines(self.content.text(self.mode), path, *start, &self.theme)
        } else if self.mode == ViewerMode::Wrapped && self.content.markdown {
            crate::ui::viewer_markdown_lines(
                self.content.content(),
                u16::try_from(self.width).unwrap_or(u16::MAX),
                &self.theme,
            )
        } else {
            self.content
                .text(self.mode)
                .split('\n')
                .map(|line| ratatui::text::Line::from(line.to_owned()))
                .collect()
        };
        let mut lines = self
            .content
            .preamble
            .as_ref()
            .map(|preamble| crate::ui::viewer_preamble_lines(preamble, self.width, &self.theme))
            .unwrap_or_default();
        self.body_start = lines.len();
        if !self.filter_query.is_empty() {
            let matcher = regex::RegexBuilder::new(&regex::escape(&self.filter_query))
                .case_insensitive(!self.filter_query.chars().any(char::is_uppercase))
                .build()
                .ok();
            let matches = |line: &ratatui::text::Line<'_>| {
                matcher
                    .as_ref()
                    .is_some_and(|regex| regex.is_match(&line.to_string()))
            };
            lines.retain(matches);
            self.body_start = lines.len();
            body.retain(matches);
        }
        lines.extend(body);
        (self.styled_lines, self.row_joiners) = if self.wrap_enabled {
            crate::ui::viewer_wrap_lines(lines, self.width)
        } else {
            let joiners = vec!["\n".to_owned(); lines.len()];
            (lines, joiners)
        };
        self.display_text = self
            .styled_lines
            .iter()
            .map(ratatui::text::Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let width = self.wrapped_width();
        self.wrapped =
            WrappedText::new(&self.display_text, width).map_err(ViewerError::Selection)?;
        self.layout = viewer_layout(self.block_id, &self.display_text, width, self.height)?;
        if self.cursor.row >= previous_body {
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
        self.scroll_top = self.scroll_top.min(self.layout.max_scroll());
        if !self.search.query().is_empty() {
            let query = self.search.query().to_owned();
            let _ = self.search.set_query(&self.display_text, &query);
        }
        Ok(())
    }
}

fn viewer_layout(
    block_id: BlockId,
    text: &str,
    width: usize,
    height: usize,
) -> Result<TranscriptLayout, ViewerError> {
    let rows = WrappedText::new(text, width)
        .map_err(ViewerError::Selection)?
        .row_count()
        .max(1);
    let rows = u32::try_from(rows).map_err(|_| ViewerError::InvalidViewport)?;
    let height = u32::try_from(height.max(1)).map_err(|_| ViewerError::InvalidViewport)?;
    TranscriptLayout::from_heights([(block_id, f64::from(rows))], f64::from(height))
        .map_err(ViewerError::Scroll)
}
