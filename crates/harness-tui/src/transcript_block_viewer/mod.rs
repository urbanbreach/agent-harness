mod render;
mod search;
mod state;

use std::fmt::{Display, Formatter};

use crate::transcript_blocks::{BlockSnapshot, FoldState, RawPayload};
use crate::transcript_identity::FocusFollowState;
use crate::transcript_pager::TranscriptSnapshot;
use crate::transcript_scroll::{LogicalAnchor, ScrollError};
use crate::transcript_selection::{
    CellPoint, DragResult, LocalClipboardError, LocalPlatform, NavigationKey, Osc52Error,
    SelectionError, SelectionMode, SelectionRange, TmuxSequence, Viewport, copy_local,
    copy_local_with_runner, route_osc52,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerMode {
    Wrapped,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerBlockContent {
    content: String,
    raw: Option<String>,
}

impl ViewerBlockContent {
    pub fn new(content: &str, raw: Option<&str>) -> Self {
        Self {
            content: redacted(content),
            raw: raw.map(redacted),
        }
    }

    pub fn from_block(snapshot: &BlockSnapshot) -> Result<Self, ViewerError> {
        let raw = snapshot
            .raw
            .as_ref()
            .map(|disclosure| match &disclosure.payload {
                RawPayload::Text(text) => Ok(text.clone()),
                RawPayload::Json(value) => serde_json::to_string_pretty(value)
                    .map_err(|error| ViewerError::RawSerialization(error.to_string())),
            })
            .transpose()?;
        Ok(Self::new(&snapshot.content, raw.as_deref()))
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn raw(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    pub(crate) fn text(&self, mode: ViewerMode) -> &str {
        match mode {
            ViewerMode::Wrapped => &self.content,
            ViewerMode::Raw => match self.raw.as_deref() {
                Some(raw) => raw,
                None => &self.content,
            },
        }
    }
}

fn redacted(text: &str) -> String {
    TranscriptSnapshot::from_text(text).as_str().to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewerReturnSnapshot {
    pub fold_state: FoldState,
    pub focus_follow: FocusFollowState,
    pub anchor: LogicalAnchor,
}

impl ViewerReturnSnapshot {
    pub const fn new(
        fold_state: FoldState,
        focus_follow: FocusFollowState,
        anchor: LogicalAnchor,
    ) -> Self {
        Self {
            fold_state,
            focus_follow,
            anchor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCloseReason {
    Closed,
    BlockDisappeared,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewerClose {
    reason: ViewerCloseReason,
    return_snapshot: ViewerReturnSnapshot,
}

impl ViewerClose {
    pub const fn reason(self) -> ViewerCloseReason {
        self.reason
    }

    pub const fn return_snapshot(self) -> ViewerReturnSnapshot {
        self.return_snapshot
    }
}

#[derive(Debug)]
pub enum ViewerError {
    InvalidViewport,
    RawSerialization(String),
    Selection(SelectionError),
    Scroll(ScrollError),
}

impl Display for ViewerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidViewport => formatter.write_str("viewer viewport must be non-zero"),
            Self::RawSerialization(detail) => {
                write!(formatter, "raw content serialization failed: {detail}")
            }
            Self::Selection(error) => write!(formatter, "viewer text layout failed: {error}"),
            Self::Scroll(error) => write!(formatter, "viewer scroll failed: {error}"),
        }
    }
}

impl std::error::Error for ViewerError {}

#[derive(Debug)]
pub enum ViewerCopyError {
    Selection(SelectionError),
    Local(LocalClipboardError),
    Osc52(Osc52Error),
}

impl Display for ViewerCopyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Selection(error) => write!(formatter, "viewer selection copy failed: {error}"),
            Self::Local(error) => write!(formatter, "viewer local copy failed: {error}"),
            Self::Osc52(error) => write!(formatter, "viewer OSC52 copy failed: {error}"),
        }
    }
}

impl std::error::Error for ViewerCopyError {}

impl state::ViewerState {
    pub fn cursor(&self) -> CellPoint {
        self.cursor
    }

    pub fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    pub fn move_cursor(&mut self, key: NavigationKey, shift: bool) -> CellPoint {
        let next = self.wrapped.move_focus(self.cursor, key);
        if shift {
            let anchor = self
                .selection
                .map_or(self.cursor, |selection| selection.anchor);
            self.selection = Some(SelectionRange::new(anchor, next));
        } else {
            self.selection = None;
        }
        self.cursor = next;
        next
    }

    pub fn select_word(&mut self, point: CellPoint) -> SelectionRange {
        let selection = self.wrapped.select(point, SelectionMode::Word);
        self.selection = Some(selection);
        self.cursor = point;
        selection
    }

    pub fn select_line(&mut self, point: CellPoint) -> SelectionRange {
        let selection = self.wrapped.select(point, SelectionMode::Line);
        self.selection = Some(selection);
        self.cursor = point;
        selection
    }

    pub fn mouse_drag(
        &mut self,
        anchor: CellPoint,
        focus: CellPoint,
        viewport: Viewport,
    ) -> Result<DragResult, ViewerError> {
        let result = self.wrapped.drag_with_autoscroll(anchor, focus, viewport);
        self.selection = Some(self.wrapped.drag(anchor, result.focus));
        self.cursor = result.focus;
        if result.autoscroll.lines != 0 {
            self.scroll_by(f64::from(result.autoscroll.lines))?;
        }
        Ok(result)
    }

    pub fn copy_selection_text(&self) -> Result<String, ViewerCopyError> {
        let selection = self
            .selection
            .ok_or(ViewerCopyError::Selection(SelectionError::EmptySelection))?;
        self.wrapped
            .copy(selection)
            .map_err(ViewerCopyError::Selection)
    }

    pub fn copy_local(&self, platform: LocalPlatform) -> Result<(), ViewerCopyError> {
        let text = self.copy_selection_text()?;
        copy_local(&text, platform).map_err(ViewerCopyError::Local)
    }

    pub fn copy_local_with_runner<F>(
        &self,
        platform: LocalPlatform,
        runner: F,
    ) -> Result<(), ViewerCopyError>
    where
        F: FnMut(
            &crate::transcript_selection::ClipboardCommand,
            &str,
        ) -> Result<bool, std::io::Error>,
    {
        let text = self.copy_selection_text()?;
        copy_local_with_runner(&text, platform, runner).map_err(ViewerCopyError::Local)
    }

    pub fn copy_osc52(
        &self,
        terminal_available: bool,
        route: TmuxSequence,
    ) -> Result<String, ViewerCopyError> {
        let text = self.copy_selection_text()?;
        route_osc52(&text, terminal_available, route).map_err(ViewerCopyError::Osc52)
    }
}

pub use render::{RenderedLine, ViewerRenderSurface, render_surface, render_to_buffer};
pub use search::{SearchDirection, SearchMatch, SearchNavigation, SearchState};
pub use state::ViewerState;
