use super::{ScrollError, ScrollResult};

const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowMode {
    Following,
    Detached,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PageFlipState {
    #[default]
    Idle,
    Preserving {
        activity_first_seq: u64,
        scroll_top: Option<usize>,
    },
    Detached {
        activity_first_seq: u64,
        scroll_top: usize,
    },
    Consumed {
        activity_first_seq: u64,
    },
}

impl PageFlipState {
    pub const fn begin(self, activity_first_seq: u64) -> Self {
        match self.activity_first_seq() {
            Some(current) if current == activity_first_seq => self,
            Some(_) | None => Self::Preserving {
                activity_first_seq,
                scroll_top: None,
            },
        }
    }

    pub const fn is_preserving(self) -> bool {
        matches!(self, Self::Preserving { .. })
    }

    pub const fn scroll_top(self) -> Option<usize> {
        match self {
            Self::Preserving { scroll_top, .. } => scroll_top,
            Self::Detached { scroll_top, .. } => Some(scroll_top),
            Self::Idle | Self::Consumed { .. } => None,
        }
    }

    pub const fn activity_first_seq(self) -> Option<u64> {
        match self {
            Self::Preserving {
                activity_first_seq, ..
            }
            | Self::Detached {
                activity_first_seq, ..
            }
            | Self::Consumed { activity_first_seq } => Some(activity_first_seq),
            Self::Idle => None,
        }
    }

    pub const fn retarget(self, activity_first_seq: u64) -> Self {
        match self {
            Self::Preserving { scroll_top, .. } => Self::Preserving {
                activity_first_seq,
                scroll_top,
            },
            Self::Detached { scroll_top, .. } => Self::Detached {
                activity_first_seq,
                scroll_top,
            },
            Self::Consumed { .. } => Self::Consumed { activity_first_seq },
            Self::Idle => Self::Idle,
        }
    }

    pub const fn preserve_at(self, scroll_top: usize) -> Self {
        match self {
            Self::Preserving {
                activity_first_seq, ..
            } => Self::Preserving {
                activity_first_seq,
                scroll_top: Some(scroll_top),
            },
            Self::Idle | Self::Detached { .. } | Self::Consumed { .. } => self,
        }
    }

    pub const fn detach_at(self, scroll_top: usize) -> Self {
        match self {
            Self::Preserving {
                activity_first_seq, ..
            }
            | Self::Detached {
                activity_first_seq, ..
            } => Self::Detached {
                activity_first_seq,
                scroll_top,
            },
            Self::Idle | Self::Consumed { .. } => self,
        }
    }

    pub const fn consume(self) -> Self {
        match self {
            Self::Preserving {
                activity_first_seq, ..
            }
            | Self::Detached {
                activity_first_seq, ..
            } => Self::Consumed { activity_first_seq },
            Self::Idle | Self::Consumed { .. } => self,
        }
    }

    pub const fn cancel(self) -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowState {
    mode: FollowMode,
    offset: f64,
}

impl FollowState {
    pub const fn new() -> Self {
        Self {
            mode: FollowMode::Following,
            offset: 0.0,
        }
    }

    pub const fn mode(self) -> FollowMode {
        self.mode
    }

    pub const fn is_following(self) -> bool {
        matches!(self.mode, FollowMode::Following)
    }

    pub const fn offset(self) -> f64 {
        self.offset
    }

    pub fn scroll_by(&mut self, delta: f64, max_offset: f64) -> ScrollResult<()> {
        validate_max(max_offset)?;
        if !delta.is_finite() {
            return Err(ScrollError::NonFinite("scroll_delta"));
        }
        let was_detached_at_bottom = !self.is_following() && self.offset <= EPSILON;
        let next = (self.offset + delta).clamp(0.0, max_offset);
        self.offset = next;
        if delta < -EPSILON && was_detached_at_bottom {
            self.mode = FollowMode::Following;
            self.offset = 0.0;
        } else if delta > EPSILON {
            self.mode = FollowMode::Detached;
        }
        Ok(())
    }

    pub fn content_changed(&mut self, max_offset: f64) -> ScrollResult<()> {
        validate_max(max_offset)?;
        if self.is_following() {
            self.offset = 0.0;
        } else {
            self.offset = self.offset.min(max_offset);
            if self.offset <= EPSILON {
                self.mode = FollowMode::Following;
                self.offset = 0.0;
            }
        }
        Ok(())
    }

    pub const fn jump_to_bottom(&mut self) {
        self.mode = FollowMode::Following;
        self.offset = 0.0;
    }
}

impl Default for FollowState {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_max(max_offset: f64) -> ScrollResult<()> {
    if !max_offset.is_finite() {
        return Err(ScrollError::NonFinite("max_offset"));
    }
    if max_offset < 0.0 {
        return Err(ScrollError::Negative("max_offset"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PageFlipState;

    #[test]
    fn page_flip_begin_is_idempotent_for_the_same_activity() {
        // arrange
        // act
        let preserving = PageFlipState::Idle.begin(7).preserve_at(42);
        let detached = preserving.detach_at(41);
        let consumed = detached.consume();

        // assert
        assert_eq!(preserving.begin(7), preserving);
        assert_eq!(detached.begin(7), detached);
        assert_eq!(consumed.begin(7), consumed);
        assert_eq!(
            consumed.begin(8),
            PageFlipState::Preserving {
                activity_first_seq: 8,
                scroll_top: None,
            }
        );
    }

    #[test]
    fn page_flip_retarget_preserves_the_visible_scroll_position() {
        // arrange
        // act
        let preserving = PageFlipState::Preserving {
            activity_first_seq: 0,
            scroll_top: Some(17),
        };
        // assert
        assert_eq!(
            preserving.retarget(42),
            PageFlipState::Preserving {
                activity_first_seq: 42,
                scroll_top: Some(17),
            }
        );
    }
}
