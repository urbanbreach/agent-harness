use super::transcript_view::TranscriptViewState;

/// A missing top follows the tail; a top at the tail remains detached until
/// another downward gesture. The measured extent belongs to this same state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptViewport {
    top: Option<usize>,
    max_scroll: usize,
}

impl TranscriptViewport {
    pub(crate) const fn following(max_scroll: usize) -> Self {
        Self {
            top: None,
            max_scroll,
        }
    }

    pub(crate) fn detached(top: usize, max_scroll: usize) -> Self {
        Self {
            top: (max_scroll > 0).then_some(top.min(max_scroll)),
            max_scroll,
        }
    }

    pub(crate) const fn is_following(self) -> bool {
        self.top.is_none()
    }
    pub(crate) const fn max_scroll(self) -> usize {
        self.max_scroll
    }
    pub(crate) fn top(self) -> usize {
        self.top.unwrap_or(self.max_scroll)
    }
    pub(crate) fn offset_from_bottom(self) -> usize {
        self.max_scroll.saturating_sub(self.top())
    }

    pub(crate) fn record_max_scroll(self, max_scroll: usize) -> Self {
        match self.top {
            Some(top) if max_scroll > 0 && !(max_scroll < self.max_scroll && top >= max_scroll) => {
                Self::detached(top, max_scroll)
            }
            _ => Self::following(max_scroll),
        }
    }

    pub(crate) fn preserve_detachment(self, max_scroll: usize) -> Self {
        let measured = Self::following(max_scroll);
        self.top
            .map_or(measured, |top| measured.with_detached_top(top))
    }

    pub(crate) fn with_detached_top(self, top: usize) -> Self {
        Self {
            top: Some(top.min(self.max_scroll)),
            ..self
        }
    }

    pub(crate) fn scroll_up(self, amount: usize) -> Self {
        if amount == 0 {
            self
        } else {
            Self::detached(self.top().saturating_sub(amount), self.max_scroll)
        }
    }

    pub(crate) fn scroll_down(self, amount: usize) -> Self {
        if amount == 0 {
            self
        } else if self.top() == self.max_scroll {
            Self::following(self.max_scroll)
        } else {
            Self::detached(self.top().saturating_add(amount), self.max_scroll)
        }
    }

    pub(crate) fn detach_at(self, top: usize) -> Self {
        if top >= self.max_scroll {
            Self::following(self.max_scroll)
        } else {
            Self::detached(top, self.max_scroll)
        }
    }

    pub(crate) fn jump_to_top(self) -> Self {
        Self::detached(0, self.max_scroll)
    }
    pub(crate) fn jump_to_bottom(self) -> Self {
        Self::following(self.max_scroll)
    }
}

impl TranscriptViewState {
    pub(crate) fn measured_viewport(&self) -> TranscriptViewport {
        self.viewport
    }

    pub(crate) fn set_measured_viewport(&mut self, viewport: TranscriptViewport) {
        self.viewport = viewport;
        self.measured_anchor = None;
    }

    pub(crate) fn record_measured_max_scroll(&mut self, max_scroll: usize) {
        self.viewport = self.viewport.record_max_scroll(max_scroll);
    }

    pub(crate) fn set_following(&mut self, following: bool) {
        self.set_measured_viewport(if following {
            self.viewport.jump_to_bottom()
        } else {
            TranscriptViewport::detached(self.viewport.top(), self.viewport.max_scroll())
        });
    }

    pub(crate) fn set_offset(&mut self, offset: usize) {
        if offset == 0 && self.viewport.is_following() {
            return;
        }
        self.set_measured_viewport(TranscriptViewport::detached(
            self.viewport.max_scroll().saturating_sub(offset),
            self.viewport.max_scroll(),
        ));
    }
}
