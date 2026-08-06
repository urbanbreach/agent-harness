use crate::transcript_identity::BlockId;
use crate::transcript_scroll::{
    FollowMode, FollowState, LogicalAnchor, ScrollError, ScrollResult, TranscriptLayout,
};

const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq)]
pub struct PeekScroll {
    viewport_extent: f64,
    layout: Option<TranscriptLayout>,
    anchor: Option<LogicalAnchor>,
    follow: FollowState,
    scroll_top: f64,
}

impl PeekScroll {
    pub fn new(viewport_extent: f64) -> ScrollResult<Self> {
        if !viewport_extent.is_finite() {
            return Err(ScrollError::NonFinite("viewport_extent"));
        }
        if viewport_extent <= 0.0 {
            return Err(ScrollError::NonPositive("viewport_extent"));
        }
        Ok(Self {
            viewport_extent,
            layout: None,
            anchor: None,
            follow: FollowState::new(),
            scroll_top: 0.0,
        })
    }

    pub fn set_heights<I>(&mut self, blocks: I) -> ScrollResult<()>
    where
        I: IntoIterator<Item = (BlockId, f64)>,
    {
        self.set_layout(TranscriptLayout::from_heights(
            blocks,
            self.viewport_extent,
        )?)
    }

    pub fn set_layout(&mut self, layout: TranscriptLayout) -> ScrollResult<()> {
        let prior_anchor = match self.layout.as_ref() {
            Some(previous) => Some(previous.capture_anchor(self.scroll_top)?),
            None => None,
        };
        let previous_scroll = self.scroll_top;
        let max_scroll = layout.max_scroll();
        let next_scroll = if self.follow.is_following() {
            max_scroll
        } else {
            match prior_anchor {
                Some(anchor) => match anchor.resolve(&layout) {
                    Ok(scroll) => scroll,
                    Err(ScrollError::MissingAnchor) => previous_scroll.min(max_scroll),
                    Err(error) => return Err(error),
                },
                None => previous_scroll.min(max_scroll),
            }
        };
        let next_anchor = layout.capture_anchor(next_scroll)?;
        if self.follow.is_following() {
            self.follow.content_changed(max_scroll)?;
        } else {
            self.follow.jump_to_bottom();
            let distance_from_bottom = max_scroll - next_scroll;
            if distance_from_bottom > EPSILON {
                self.follow.scroll_by(distance_from_bottom, max_scroll)?;
            }
        }
        self.viewport_extent = layout.viewport_extent();
        self.scroll_top = next_scroll;
        self.anchor = Some(next_anchor);
        self.layout = Some(layout);
        Ok(())
    }

    pub fn scroll_by(&mut self, delta: f64) -> ScrollResult<()> {
        let Some(layout) = self.layout.as_ref() else {
            return Err(ScrollError::EmptyLayout);
        };
        self.follow.scroll_by(delta, layout.max_scroll())?;
        self.scroll_top = layout.max_scroll() - self.follow.offset();
        self.anchor = Some(layout.capture_anchor(self.scroll_top)?);
        Ok(())
    }

    pub fn jump_to_bottom(&mut self) {
        self.follow.jump_to_bottom();
        if let Some(layout) = self.layout.as_ref() {
            self.scroll_top = layout.max_scroll();
            self.anchor = layout.capture_anchor(self.scroll_top).ok();
        } else {
            self.scroll_top = 0.0;
            self.anchor = None;
        }
    }

    pub const fn follow(&self) -> FollowMode {
        self.follow.mode()
    }

    pub const fn scroll_top(&self) -> f64 {
        self.scroll_top
    }

    pub const fn anchor(&self) -> Option<LogicalAnchor> {
        self.anchor
    }

    pub fn max_scroll(&self) -> Option<f64> {
        self.layout.as_ref().map(TranscriptLayout::max_scroll)
    }
}
