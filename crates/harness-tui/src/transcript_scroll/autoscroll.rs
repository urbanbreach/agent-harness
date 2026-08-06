use crate::scheduling::FrameInputs;

use super::{ScrollError, ScrollResult};

const AUTOSCROLL_INTERVAL_MS: u64 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragViewport {
    top: f64,
    bottom: f64,
    edge_threshold: f64,
}

impl DragViewport {
    pub fn new(top: f64, height: f64, edge_threshold: f64) -> ScrollResult<Self> {
        for (value, field) in [
            (top, "viewport_top"),
            (height, "viewport_height"),
            (edge_threshold, "edge_threshold"),
        ] {
            if !value.is_finite() {
                return Err(ScrollError::NonFinite(field));
            }
        }
        if height <= 0.0 || edge_threshold <= 0.0 || edge_threshold > height {
            return Err(ScrollError::InvalidGeometry);
        }
        Ok(Self {
            top,
            bottom: top + height,
            edge_threshold,
        })
    }

    fn edge_at(self, pointer_y: f64) -> Option<Edge> {
        if !pointer_y.is_finite() {
            return None;
        }
        if pointer_y <= self.top + self.edge_threshold {
            Some(Edge::Start)
        } else if pointer_y >= self.bottom - self.edge_threshold {
            Some(Edge::End)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoscrollStep {
    pub edge: Edge,
    pub scroll_delta: f64,
    pub selection_delta: i32,
    pub next_deadline_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragAutoscroll {
    edge: Option<Edge>,
    next_deadline_ms: Option<u64>,
    selection_end: i64,
}

impl DragAutoscroll {
    pub const fn new() -> Self {
        Self {
            edge: None,
            next_deadline_ms: None,
            selection_end: 0,
        }
    }

    pub const fn start(&mut self, selection_end: i64, now_ms: u64) {
        self.selection_end = selection_end;
        self.edge = None;
        self.next_deadline_ms = None;
        let _ = now_ms;
    }

    pub fn update_pointer(
        &mut self,
        pointer_y: f64,
        viewport: &DragViewport,
        now_ms: u64,
    ) -> ScrollResult<Option<Edge>> {
        if !pointer_y.is_finite() {
            return Err(ScrollError::NonFinite("pointer_y"));
        }
        let next_edge = viewport.edge_at(pointer_y);
        if next_edge != self.edge {
            self.edge = next_edge;
            self.next_deadline_ms =
                next_edge.map(|_| now_ms.saturating_add(AUTOSCROLL_INTERVAL_MS));
        } else if next_edge.is_none() {
            self.next_deadline_ms = None;
        }
        Ok(next_edge)
    }

    pub fn tick(&mut self, now_ms: u64) -> Option<AutoscrollStep> {
        let deadline = self.next_deadline_ms?;
        if now_ms < deadline {
            return None;
        }
        let edge = self.edge?;
        let (scroll_delta, selection_delta) = match edge {
            Edge::Start => (-1.0, -1),
            Edge::End => (1.0, 1),
        };
        self.selection_end = self
            .selection_end
            .saturating_add(i64::from(selection_delta));
        let next_deadline_ms = now_ms.saturating_add(AUTOSCROLL_INTERVAL_MS);
        self.next_deadline_ms = Some(next_deadline_ms);
        Some(AutoscrollStep {
            edge,
            scroll_delta,
            selection_delta,
            next_deadline_ms,
        })
    }

    pub const fn stop(&mut self) {
        self.edge = None;
        self.next_deadline_ms = None;
    }

    pub const fn next_deadline_ms(self) -> Option<u64> {
        self.next_deadline_ms
    }

    pub const fn selection_end(self) -> i64 {
        self.selection_end
    }

    pub const fn frame_inputs(self) -> FrameInputs {
        if self.next_deadline_ms.is_some() {
            FrameInputs::flush()
        } else {
            FrameInputs::idle()
        }
    }
}

impl Default for DragAutoscroll {
    fn default() -> Self {
        Self::new()
    }
}
