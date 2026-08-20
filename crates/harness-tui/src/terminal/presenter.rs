use std::time::Instant;

use crate::presentation::RenderDemand;

use super::{FrameKind, FrameSubmission};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presenter {
    dirty: bool,
    force_full_repaint: bool,
    last_draw_at: Option<Instant>,
    scheduled_at: Option<Instant>,
    render_demand: Option<RenderDemand>,
    immediate: bool,
}

impl Presenter {
    pub const fn new() -> Self {
        Self {
            dirty: true,
            force_full_repaint: true,
            last_draw_at: None,
            scheduled_at: None,
            render_demand: None,
            immediate: false,
        }
    }

    pub fn request_redraw(&mut self, now: Instant) {
        self.dirty = true;
        if self.scheduled_at.is_none() {
            self.scheduled_at = Some(now);
        }
    }

    pub fn request_immediate_redraw(&mut self, now: Instant) {
        self.request_redraw(now);
        self.immediate = true;
    }

    pub fn request_redraw_for(&mut self, demand: RenderDemand, now: Instant) {
        self.request_redraw(now);
        match self.render_demand.as_mut() {
            Some(pending) => pending.merge(demand),
            None => self.render_demand = Some(demand),
        }
    }

    pub fn take_render_demand(&mut self) -> Option<RenderDemand> {
        self.render_demand.take()
    }

    pub const fn should_present(&self, writer_ready: bool) -> bool {
        self.dirty && writer_ready
    }

    pub const fn has_pending_redraw(&self) -> bool {
        self.dirty
    }

    pub fn record_submission(&mut self, submission: FrameSubmission, now: Instant) {
        self.last_draw_at = Some(now);
        match submission {
            FrameSubmission::Accepted(kind) => {
                self.dirty = false;
                self.scheduled_at = None;
                self.immediate = false;
                if kind == FrameKind::FullRepaint {
                    self.force_full_repaint = false;
                }
            }
            FrameSubmission::Unchanged => {
                self.dirty = false;
                self.scheduled_at = None;
                self.immediate = false;
            }
            FrameSubmission::ResyncRequired => {
                self.dirty = true;
                self.force_full_repaint = true;
                self.scheduled_at = Some(now);
            }
        }
    }

    pub const fn force_full_repaint(&self) -> bool {
        self.force_full_repaint
    }

    pub const fn last_draw_at(&self) -> Option<Instant> {
        self.last_draw_at
    }

    pub const fn scheduled_at(&self) -> Option<Instant> {
        self.scheduled_at
    }

    pub const fn immediate_pending(&self) -> bool {
        self.immediate
    }
}

impl Default for Presenter {
    fn default() -> Self {
        Self::new()
    }
}
