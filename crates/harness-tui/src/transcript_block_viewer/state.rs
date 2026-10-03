use crate::app::pane_query::PaneQuery;
use crate::transcript_identity::BlockId;
use crate::transcript_scroll::{
    EasingKind, LogicalAnchor, MotionPreference, ScrollError, ScrollFrame, ScrollTransition,
    TranscriptLayout, TransitionRequest,
};
use crate::transcript_selection::{CellPoint, SelectionRange, TextLayout};

use super::search::{SearchDirection, SearchNavigation, SearchState};
use super::{render_surface, ViewerRenderSurface};
use super::{
    ViewerBlockContent, ViewerClose, ViewerCloseReason, ViewerError, ViewerMode,
    ViewerReturnSnapshot,
};

mod display;
mod markdown_mode;
mod resume;
pub(crate) use resume::ViewerResume;

const DEFAULT_HEIGHT: usize = 24;

pub struct ViewerState {
    block_id: BlockId,
    content: ViewerBlockContent,
    pub(super) styled_lines: Vec<ratatui::text::Line<'static>>,
    pub(super) row_joiners: Vec<String>,
    row_line_ids: Vec<usize>,
    pub(super) unfiltered_line_count: usize,
    theme: crate::theme::Theme,
    return_snapshot: ViewerReturnSnapshot,
    mode: ViewerMode,
    markdown_tail_rebased: bool,
    width: usize,
    height: usize,
    pub(super) wrapped: TextLayout,
    pub(super) selection: Option<SelectionRange>,
    pub(super) cursor: CellPoint,
    pub(super) body_start: usize,
    pub(super) close_hovered: bool,
    pub(super) filter_query: String,
    pub(crate) input: PaneQuery,
    pub(super) visual_mode: bool,
    pub(super) wrap_enabled: bool,
    pub(super) search: SearchState,
    pub(super) child: bool,
    pub(super) following: bool,
    pub(super) running: bool,
    pub(super) at_end: bool,
    pub(super) scroll_screen_y: Option<usize>,
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
        let mut state = Self {
            block_id,
            content,
            styled_lines: Vec::new(),
            row_joiners: Vec::new(),
            row_line_ids: Vec::new(),
            unfiltered_line_count: 0,
            theme: crate::theme::Theme::default(),
            return_snapshot,
            mode: ViewerMode::Wrapped,
            markdown_tail_rebased: false,
            width: 80,
            height: DEFAULT_HEIGHT,
            wrapped: TextLayout::new(String::new(), 1).map_err(ViewerError::Selection)?,
            selection: None,
            cursor: CellPoint::new(0, 0),
            body_start: 0,
            close_hovered: false,
            filter_query: String::new(),
            input: PaneQuery::default(),
            visual_mode: false,
            wrap_enabled: true,
            search: SearchState::new(),
            child: false,
            following: false,
            running: false,
            at_end: false,
            scroll_screen_y: None,
            layout: viewer_layout(block_id, 1, DEFAULT_HEIGHT)?,
            scroll_top: 0.0,
            transition: None,
            open: true,
        };
        state.rebuild_display()?;
        Ok(state)
    }

    pub(super) fn theme(&self) -> &crate::theme::Theme {
        &self.theme
    }

    pub fn block_id(&self) -> BlockId {
        self.block_id
    }

    pub fn content(&self) -> &ViewerBlockContent {
        &self.content
    }

    pub(crate) fn update_content(
        &mut self,
        content: ViewerBlockContent,
    ) -> Result<(), ViewerError> {
        if self.content == content {
            return Ok(());
        }
        self.content = content;
        self.selection = None;
        self.rebuild_display()
    }

    pub const fn mode(&self) -> ViewerMode {
        self.mode
    }

    pub fn toggle_mode(&mut self) -> Result<(), ViewerError> {
        if self.child && self.content.markdown {
            return self.toggle_child_markdown_mode();
        }
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
        let navigation = self.update_search(query);
        self.reveal_search_match();
        navigation
    }

    pub(super) fn update_search(&mut self, query: &str) -> SearchNavigation {
        if self.child {
            self.search
                .set_wrapped_query(self.wrapped.text(), &self.row_joiners, query)
        } else {
            self.search.set_query(self.wrapped.text(), query)
        }
    }

    pub fn search_forward(&mut self) -> SearchNavigation {
        if self.child {
            self.find_matching_line(self.logical_rows(self.cursor.row).start, true, false);
            return self.search.navigation();
        }
        let navigation = self.search.navigate(SearchDirection::Forward);
        self.reveal_search_match();
        navigation
    }

    pub fn search_backward(&mut self) -> SearchNavigation {
        if self.child {
            self.find_matching_line(self.logical_rows(self.cursor.row).start, false, false);
            return self.search.navigation();
        }
        let navigation = self.search.navigate(SearchDirection::Backward);
        self.reveal_search_match();
        navigation
    }

    pub fn viewport_height(&self) -> usize {
        self.height
    }

    pub(crate) fn set_filter_query(&mut self, query: String) -> Result<(), ViewerError> {
        let old_rows = self.row_line_ids.chunk_by(|a, b| a == b).count();
        self.filter_query = query;
        if !self.child {
            self.scroll_top = 0.0;
        }
        self.rebuild_display()?;
        if !self.child {
            self.cursor = CellPoint::new(0, 0);
        } else if self.row_line_ids.chunk_by(|a, b| a == b).count() < old_rows {
            self.reveal_cursor();
        }
        Ok(())
    }
    pub(crate) fn toggle_wrap(&mut self) -> Result<(), ViewerError> {
        self.wrap_enabled = !self.wrap_enabled;
        self.rebuild_display()
    }
    pub(crate) fn toggle_visual(&mut self) {
        self.exit_follow();
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
        if let Ok(text) = self.copy_selection_text() {
            return text;
        }
        if !self.child {
            return self.wrapped.row_text(self.cursor.row).to_owned();
        }
        let row = if self.following {
            (self.body_start..self.wrapped.row_count())
                .rev()
                .find(|row| !self.wrapped.row_text(*row).is_empty())
                .unwrap_or(self.cursor.row)
        } else {
            self.cursor.row
        };
        let rows = self.logical_rows(row);
        let mut text = String::new();
        for row in rows.clone() {
            if row > rows.start {
                text.push_str(&self.row_joiners[row - 1]);
            }
            text.push_str(self.wrapped.row_text(row));
        }
        text
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
        let rows = if self.child {
            self.logical_rows(self.cursor.row)
        } else {
            self.cursor.row..self.cursor.row + 1
        };
        let row = f64::from(u32::try_from(rows.start).unwrap_or(u32::MAX));
        let bottom = f64::from(u32::try_from(rows.end).unwrap_or(u32::MAX));
        let height = f64::from(u32::try_from(self.height).unwrap_or(u32::MAX));
        let margin = ((height - 1.0) / 2.0).floor().min(2.0);
        if row < self.scroll_top + margin {
            self.scroll_top = (row - margin).max(0.0);
        }
        if bottom + margin > self.scroll_top + height {
            self.scroll_top = (bottom + margin - height).min(self.layout.max_scroll());
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
        let keep_cursor_visible = height < self.height
            && (self.scroll_top()..self.scroll_top().saturating_add(self.height))
                .contains(&self.cursor.row);
        let anchor = self.scroll_anchor().map_err(ViewerError::Scroll)?;
        let width_changed = self.width != width;
        self.width = width;
        self.height = height;
        if width_changed {
            self.rebuild_display()?;
        } else {
            self.layout = viewer_layout(self.block_id, self.wrapped.row_count(), height)?;
        }
        self.scroll_top = if self.following {
            self.layout.max_scroll()
        } else {
            anchor.resolve(&self.layout).map_err(ViewerError::Scroll)?
        };
        self.transition = None;
        if keep_cursor_visible && !self.following && !self.child {
            self.reveal_cursor();
        }
        Ok(())
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
}

fn viewer_layout(
    block_id: BlockId,
    rows: usize,
    height: usize,
) -> Result<TranscriptLayout, ViewerError> {
    let rows = u32::try_from(rows.max(1)).map_err(|_| ViewerError::InvalidViewport)?;
    let height = u32::try_from(height.max(1)).map_err(|_| ViewerError::InvalidViewport)?;
    TranscriptLayout::from_heights([(block_id, f64::from(rows))], f64::from(height))
        .map_err(ViewerError::Scroll)
}
