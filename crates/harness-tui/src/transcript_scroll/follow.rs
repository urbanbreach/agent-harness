use super::{ScrollError, ScrollResult};

const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowMode {
    Following,
    Detached,
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
        let next = (self.offset + delta).clamp(0.0, max_offset);
        self.offset = next;
        if next <= EPSILON {
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
