#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeasuredTranscriptViewport {
    Following { max_scroll: usize },
    Detached { top: usize, max_scroll: usize },
}

use super::transcript_view::TranscriptViewState;

impl MeasuredTranscriptViewport {
    pub(crate) const fn following(max_scroll: usize) -> Self {
        Self::Following { max_scroll }
    }

    pub(crate) const fn is_following(self) -> bool {
        matches!(self, Self::Following { .. })
    }

    pub(crate) const fn max_scroll(self) -> usize {
        match self {
            Self::Following { max_scroll } | Self::Detached { max_scroll, .. } => max_scroll,
        }
    }

    pub(crate) const fn top(self) -> usize {
        match self {
            Self::Following { max_scroll } => max_scroll,
            Self::Detached { top, max_scroll } => {
                if top < max_scroll {
                    top
                } else {
                    max_scroll
                }
            }
        }
    }

    pub(crate) const fn offset_from_bottom(self) -> usize {
        self.max_scroll().saturating_sub(self.top())
    }

    pub(crate) const fn record_max_scroll(self, max_scroll: usize) -> Self {
        match self {
            Self::Following { .. } => Self::Following { max_scroll },
            Self::Detached {
                top,
                max_scroll: previous_max,
            } if max_scroll == 0 || (max_scroll < previous_max && top >= max_scroll) => {
                Self::Following { max_scroll }
            }
            Self::Detached { top, .. } => Self::Detached {
                top: if top < max_scroll { top } else { max_scroll },
                max_scroll,
            },
        }
    }

    pub(crate) const fn scroll_up(self, amount: usize) -> Self {
        if amount == 0 || self.max_scroll() == 0 {
            return self;
        }
        Self::Detached {
            top: self.top().saturating_sub(amount),
            max_scroll: self.max_scroll(),
        }
    }

    pub(crate) const fn scroll_down(self, amount: usize) -> Self {
        if amount == 0 || self.is_following() {
            return self;
        }
        let max_scroll = self.max_scroll();
        if self.top() >= max_scroll {
            return Self::Following { max_scroll };
        }
        let next = self.top().saturating_add(amount);
        Self::Detached {
            top: if next < max_scroll { next } else { max_scroll },
            max_scroll,
        }
    }

    pub(crate) const fn detach_at(self, top: usize) -> Self {
        let max_scroll = self.max_scroll();
        if max_scroll == 0 || top >= max_scroll {
            Self::Following { max_scroll }
        } else {
            Self::Detached { top, max_scroll }
        }
    }

    pub(crate) const fn detached(top: usize, max_scroll: usize) -> Self {
        if max_scroll == 0 {
            Self::Following { max_scroll }
        } else {
            Self::Detached {
                top: if top < max_scroll { top } else { max_scroll },
                max_scroll,
            }
        }
    }

    pub(crate) const fn jump_to_top(self) -> Self {
        let max_scroll = self.max_scroll();
        if max_scroll == 0 {
            Self::Following { max_scroll }
        } else {
            Self::Detached { top: 0, max_scroll }
        }
    }

    pub(crate) const fn jump_to_bottom(self) -> Self {
        Self::Following {
            max_scroll: self.max_scroll(),
        }
    }

    const fn from_legacy(following: bool, offset: usize, max_scroll: usize) -> Self {
        if following || max_scroll == 0 {
            Self::Following { max_scroll }
        } else {
            Self::Detached {
                top: max_scroll.saturating_sub(if offset < max_scroll {
                    offset
                } else {
                    max_scroll
                }),
                max_scroll,
            }
        }
    }
}

impl TranscriptViewState {
    pub(crate) fn measured_viewport(&self) -> MeasuredTranscriptViewport {
        let max_scroll = self.last_transcript_max_scroll.get();
        let legacy = (self.follow_mode, self.transcript_scroll);
        let viewport = if legacy == self.legacy_scroll_snapshot.get() {
            self.measured_viewport.get().record_max_scroll(max_scroll)
        } else {
            self.legacy_scroll_snapshot.set(legacy);
            MeasuredTranscriptViewport::from_legacy(
                self.follow_mode,
                self.transcript_scroll,
                max_scroll,
            )
        };
        self.measured_viewport.set(viewport);
        viewport
    }

    pub(crate) fn set_measured_viewport(&mut self, viewport: MeasuredTranscriptViewport) {
        self.measured_viewport.set(viewport);
        self.measured_anchor.set(None);
        self.follow_mode = viewport.is_following();
        self.transcript_scroll = viewport.offset_from_bottom();
        self.legacy_scroll_snapshot
            .set((self.follow_mode, self.transcript_scroll));
    }

    pub(crate) fn record_measured_max_scroll(&self, max_scroll: usize) {
        let legacy = (self.follow_mode, self.transcript_scroll);
        let viewport = if legacy == self.legacy_scroll_snapshot.get() {
            self.measured_viewport.get().record_max_scroll(max_scroll)
        } else {
            self.legacy_scroll_snapshot.set(legacy);
            MeasuredTranscriptViewport::from_legacy(
                self.follow_mode,
                self.transcript_scroll,
                max_scroll,
            )
        };
        self.last_transcript_max_scroll.set(max_scroll);
        self.measured_viewport.set(viewport);
    }

    pub(crate) fn set_resolved_measured_top(&self, top: usize, max_scroll: usize) {
        self.measured_viewport
            .set(MeasuredTranscriptViewport::detached(top, max_scroll));
    }
}

#[cfg(test)]
mod tests {
    use super::MeasuredTranscriptViewport;

    #[test]
    fn landing_at_measured_bottom_remains_detached() {
        // arrange
        // Given: a measured viewport detached ten rows above the tail.
        let viewport = MeasuredTranscriptViewport::following(100).scroll_up(10);

        // When: one downward gesture lands on the measured bottom.
        let landed = viewport.scroll_down(10);

        // act
        // Then: landing does not silently resume following.
        // assert
        assert!(!landed.is_following());
        assert_eq!(landed.top(), 100);
    }

    #[test]
    fn fully_clamped_measured_overscroll_reattaches() {
        // arrange
        // Given: a measured viewport detached at the bottom.
        let viewport = MeasuredTranscriptViewport::following(100)
            .scroll_up(10)
            .scroll_down(10);

        // When: another downward gesture is fully clamped.
        let reattached = viewport.scroll_down(1);

        // act
        // Then: the viewport follows the live tail immediately.
        // assert
        assert!(reattached.is_following());
        assert_eq!(reattached.offset_from_bottom(), 0);
    }

    #[test]
    fn content_growth_preserves_detached_measured_top() {
        // arrange
        // Given: a detached viewport reading row seventy-five.
        let viewport = MeasuredTranscriptViewport::following(100).scroll_up(25);

        // When: streamed content extends the measured maximum.
        let extended = viewport.record_max_scroll(140);

        // act
        // Then: the logical reading position stays fixed while distance grows.
        // assert
        assert_eq!(extended.top(), 75);
        assert_eq!(extended.offset_from_bottom(), 65);
        assert!(!extended.is_following());
    }

    #[test]
    fn shrinking_past_detached_top_reconciles_to_following() {
        // arrange
        // Given: a detached viewport reading row seventy-five.
        let viewport = MeasuredTranscriptViewport::following(100).scroll_up(25);

        // When: reflow removes overflow below that row.
        let reconciled = viewport.record_max_scroll(70);

        // act
        // Then: no unreachable detached state remains.
        // assert
        assert!(reconciled.is_following());
        assert_eq!(reconciled.top(), 70);
    }
}
