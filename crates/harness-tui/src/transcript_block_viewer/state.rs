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
    display_text: String,
    pub(super) styled_lines: Vec<ratatui::text::Line<'static>>,
    theme: crate::theme::Theme,
    return_snapshot: ViewerReturnSnapshot,
    mode: ViewerMode,
    width: usize,
    height: usize,
    pub(super) wrapped: WrappedText,
    pub(super) selection: Option<SelectionRange>,
    pub(super) cursor: CellPoint,
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
            theme: crate::theme::Theme::default(),
            return_snapshot,
            mode,
            width,
            height,
            wrapped,
            selection: None,
            cursor: CellPoint::new(0, 0),
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

    pub fn scroll_top(&self) -> usize {
        // The layout only accepts finite, bounded row counts.
        super::render::scroll_offset(self.scroll_top)
    }

    pub fn reveal_cursor(&mut self) {
        let row = f64::from(u32::try_from(self.cursor.row).unwrap_or(u32::MAX));
        let height = f64::from(u32::try_from(self.height).unwrap_or(u32::MAX));
        if row < self.scroll_top {
            self.scroll_top = row;
        } else if row >= self.scroll_top + height {
            self.scroll_top = (row + 1.0 - height).min(self.layout.max_scroll());
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
        self.width = width;
        self.height = height;
        self.rebuild_display()?;
        self.scroll_top = anchor.resolve(&self.layout).map_err(ViewerError::Scroll)?;
        self.transition = None;
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

    fn rebuild_display(&mut self) -> Result<(), ViewerError> {
        self.styled_lines = if self.mode == ViewerMode::Wrapped && self.content.markdown {
            crate::ui::viewer_markdown_lines(
                self.content.content(),
                u16::try_from(self.width).unwrap_or(u16::MAX),
                &self.theme,
            )
        } else {
            Vec::new()
        };
        self.display_text = if self.styled_lines.is_empty() {
            self.content.text(self.mode).to_string()
        } else {
            self.styled_lines
                .iter()
                .map(ratatui::text::Line::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.wrapped =
            WrappedText::new(&self.display_text, self.width).map_err(ViewerError::Selection)?;
        self.layout = viewer_layout(self.block_id, &self.display_text, self.width, self.height)?;
        self.selection = None;
        self.cursor = CellPoint::new(0, 0);
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
