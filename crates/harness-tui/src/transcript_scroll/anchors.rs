use crate::transcript_identity::BlockId;

use super::{ScrollError, ScrollResult};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockPlacement {
    id: BlockId,
    top: f64,
    extent: f64,
}

impl BlockPlacement {
    pub const fn new(id: BlockId, top: f64, extent: f64) -> Self {
        Self { id, top, extent }
    }

    pub const fn id(self) -> BlockId {
        self.id
    }

    pub const fn top(self) -> f64 {
        self.top
    }

    pub const fn extent(self) -> f64 {
        self.extent
    }

    fn bottom(self) -> f64 {
        self.top + self.extent
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLayout {
    placements: Vec<BlockPlacement>,
    viewport_extent: f64,
    content_extent: f64,
}

impl TranscriptLayout {
    pub fn from_heights<I>(blocks: I, viewport_extent: f64) -> ScrollResult<Self>
    where
        I: IntoIterator<Item = (BlockId, f64)>,
    {
        validate_positive(viewport_extent, "viewport_extent")?;
        let mut placements = Vec::new();
        let mut top = 0.0;
        for (id, extent) in blocks {
            validate_positive(extent, "block_extent")?;
            placements.push(BlockPlacement::new(id, top, extent));
            top += extent;
        }
        if placements.is_empty() {
            return Err(ScrollError::EmptyLayout);
        }
        Ok(Self {
            placements,
            viewport_extent,
            content_extent: top,
        })
    }

    pub fn placements(&self) -> &[BlockPlacement] {
        &self.placements
    }

    pub const fn viewport_extent(&self) -> f64 {
        self.viewport_extent
    }

    pub const fn content_extent(&self) -> f64 {
        self.content_extent
    }

    pub fn max_scroll(&self) -> f64 {
        (self.content_extent - self.viewport_extent).max(0.0)
    }

    pub fn top_of(&self, id: BlockId) -> ScrollResult<f64> {
        self.placements
            .iter()
            .find(|placement| placement.id() == id)
            .map(|placement| placement.top())
            .ok_or(ScrollError::MissingAnchor)
    }

    pub fn capture_anchor(&self, scroll_top: f64) -> ScrollResult<LogicalAnchor> {
        LogicalAnchor::capture(self, scroll_top)
    }

    fn clamp_scroll(&self, scroll_top: f64) -> f64 {
        scroll_top.clamp(0.0, self.max_scroll())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalAnchor {
    block_id: BlockId,
    within_block: f64,
}

impl LogicalAnchor {
    pub fn capture(layout: &TranscriptLayout, scroll_top: f64) -> ScrollResult<Self> {
        if !scroll_top.is_finite() {
            return Err(ScrollError::NonFinite("scroll_top"));
        }
        if scroll_top < 0.0 {
            return Err(ScrollError::Negative("scroll_top"));
        }
        let point = layout.clamp_scroll(scroll_top);
        let placement = layout
            .placements
            .iter()
            .find(|candidate| {
                (point >= candidate.top() && point <= candidate.bottom()) || point < candidate.top()
            })
            .or_else(|| layout.placements.last())
            .ok_or(ScrollError::EmptyLayout)?;
        let within_block = (point - placement.top()).clamp(0.0, placement.extent());
        Ok(Self {
            block_id: placement.id(),
            within_block,
        })
    }

    pub const fn block_id(self) -> BlockId {
        self.block_id
    }

    pub const fn within_block(self) -> f64 {
        self.within_block
    }

    pub fn resolve(self, layout: &TranscriptLayout) -> ScrollResult<f64> {
        let placement = layout
            .placements
            .iter()
            .find(|candidate| candidate.id() == self.block_id)
            .ok_or(ScrollError::MissingAnchor)?;
        Ok(layout.clamp_scroll(placement.top() + self.within_block.min(placement.extent())))
    }
}

fn validate_positive(value: f64, field: &'static str) -> ScrollResult<()> {
    if !value.is_finite() {
        return Err(ScrollError::NonFinite(field));
    }
    if value <= 0.0 {
        return Err(ScrollError::NonPositive(field));
    }
    Ok(())
}
