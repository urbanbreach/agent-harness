use super::anchors::LogicalAnchor;
use super::{ScrollError, ScrollResult};

const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarGeometry {
    track_start: f64,
    track_extent: f64,
    content_extent: f64,
    viewport_extent: f64,
    thumb_extent: f64,
}

impl ScrollbarGeometry {
    pub fn new(
        track_start: f64,
        track_extent: f64,
        content_extent: f64,
        viewport_extent: f64,
    ) -> ScrollResult<Self> {
        for (value, field) in [
            (track_start, "track_start"),
            (track_extent, "track_extent"),
            (content_extent, "content_extent"),
            (viewport_extent, "viewport_extent"),
        ] {
            if !value.is_finite() {
                return Err(ScrollError::NonFinite(field));
            }
        }
        if track_extent <= 0.0 || viewport_extent <= 0.0 {
            return Err(ScrollError::NonPositive("scrollbar_extent"));
        }
        if content_extent < 0.0 {
            return Err(ScrollError::Negative("content_extent"));
        }
        let max_scroll = (content_extent - viewport_extent).max(0.0);
        let raw_thumb = if max_scroll <= EPSILON {
            track_extent
        } else {
            track_extent * viewport_extent / content_extent
        };
        let minimum_thumb = track_extent.min(3.0);
        Ok(Self {
            track_start,
            track_extent,
            content_extent,
            viewport_extent,
            thumb_extent: raw_thumb.clamp(minimum_thumb, track_extent),
        })
    }

    pub const fn track_start(self) -> f64 {
        self.track_start
    }

    pub const fn thumb_extent(self) -> f64 {
        self.thumb_extent
    }

    pub fn max_scroll(self) -> f64 {
        (self.content_extent - self.viewport_extent).max(0.0)
    }

    pub fn thumb_start(self, offset: f64) -> ScrollResult<f64> {
        validate_offset(offset)?;
        let travel = self.track_extent - self.thumb_extent;
        if travel <= EPSILON || self.max_scroll() <= EPSILON {
            return Ok(self.track_start);
        }
        Ok(self.track_start + travel * (offset.clamp(0.0, self.max_scroll()) / self.max_scroll()))
    }

    fn offset_from_thumb_start(self, thumb_start: f64) -> ScrollResult<f64> {
        if !thumb_start.is_finite() {
            return Err(ScrollError::NonFinite("thumb_start"));
        }
        let travel = self.track_extent - self.thumb_extent;
        if travel <= EPSILON || self.max_scroll() <= EPSILON {
            return Ok(0.0);
        }
        let position = (thumb_start - self.track_start).clamp(0.0, travel);
        Ok(self.max_scroll() * position / travel)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarDrag {
    grab_offset: f64,
    logical_anchor: Option<LogicalAnchor>,
}

impl ScrollbarDrag {
    pub fn begin(pointer: f64, offset: f64, geometry: &ScrollbarGeometry) -> ScrollResult<Self> {
        if !pointer.is_finite() {
            return Err(ScrollError::NonFinite("pointer"));
        }
        validate_offset(offset)?;
        let grab_offset = pointer - geometry.thumb_start(offset)?;
        Ok(Self {
            grab_offset,
            logical_anchor: None,
        })
    }

    pub const fn with_logical_anchor(mut self, anchor: LogicalAnchor) -> Self {
        self.logical_anchor = Some(anchor);
        self
    }

    pub const fn logical_anchor(self) -> Option<LogicalAnchor> {
        self.logical_anchor
    }

    pub const fn grab_offset(self) -> f64 {
        self.grab_offset
    }

    pub fn move_to(&mut self, pointer: f64, geometry: &ScrollbarGeometry) -> ScrollResult<f64> {
        if !pointer.is_finite() {
            return Err(ScrollError::NonFinite("pointer"));
        }
        geometry.offset_from_thumb_start(pointer - self.grab_offset)
    }
}

fn validate_offset(offset: f64) -> ScrollResult<()> {
    if !offset.is_finite() {
        return Err(ScrollError::NonFinite("scroll_offset"));
    }
    if offset < 0.0 {
        return Err(ScrollError::Negative("scroll_offset"));
    }
    Ok(())
}
