use std::time::Instant;

use super::{FrameKind, FrameSubmission};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presenter {
    dirty: bool,
    force_full_repaint: bool,
    last_draw_at: Option<Instant>,
    scheduled_at: Option<Instant>,
}

impl Presenter {
    pub const fn new() -> Self {
        Self {
            dirty: true,
            force_full_repaint: true,
            last_draw_at: None,
            scheduled_at: None,
        }
    }

    pub fn request_redraw(&mut self, now: Instant) {
        self.dirty = true;
        if self.scheduled_at.is_none() {
            self.scheduled_at = Some(now);
        }
    }

    pub const fn should_present(&self, writer_ready: bool) -> bool {
        self.dirty && writer_ready
    }

    pub fn record_submission(&mut self, submission: FrameSubmission, now: Instant) {
        self.last_draw_at = Some(now);
        match submission {
            FrameSubmission::Accepted(kind) => {
                self.dirty = false;
                self.scheduled_at = None;
                if kind == FrameKind::FullRepaint {
                    self.force_full_repaint = false;
                }
            }
            FrameSubmission::Unchanged => {
                self.dirty = false;
                self.scheduled_at = None;
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
}

impl Default for Presenter {
    fn default() -> Self {
        Self::new()
    }
}
