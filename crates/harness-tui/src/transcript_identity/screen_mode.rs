use std::fmt::{Display, Formatter};

use super::focus_follow::FocusFollowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InPlaceMode {
    Transcript,
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptScreenMode {
    InPlace(InPlaceMode),
    SelectedBlockViewer,
    ExternalPagerSuspended,
}

impl TranscriptScreenMode {
    pub const fn is_in_place(self) -> bool {
        matches!(self, Self::InPlace(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReturnPoint {
    mode: InPlaceMode,
    focus_follow: FocusFollowState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptScreenState {
    mode: TranscriptScreenMode,
    focus_follow: FocusFollowState,
    return_point: Option<ReturnPoint>,
}

impl TranscriptScreenState {
    pub const fn new(mode: TranscriptScreenMode, focus_follow: FocusFollowState) -> Self {
        Self {
            mode,
            focus_follow,
            return_point: None,
        }
    }

    pub fn switch_to(&self, target: TranscriptScreenMode) -> Self {
        let return_point = match target {
            TranscriptScreenMode::InPlace(_) => None,
            TranscriptScreenMode::SelectedBlockViewer
            | TranscriptScreenMode::ExternalPagerSuspended => {
                self.return_point.or(match self.mode {
                    TranscriptScreenMode::InPlace(mode) => Some(ReturnPoint {
                        mode,
                        focus_follow: self.focus_follow,
                    }),
                    TranscriptScreenMode::SelectedBlockViewer
                    | TranscriptScreenMode::ExternalPagerSuspended => None,
                })
            }
        };
        let focus_follow = match (target, self.return_point) {
            (TranscriptScreenMode::InPlace(_), Some(point)) => point.focus_follow,
            (TranscriptScreenMode::InPlace(_), None)
            | (TranscriptScreenMode::SelectedBlockViewer, _)
            | (TranscriptScreenMode::ExternalPagerSuspended, _) => self.focus_follow,
        };
        Self {
            mode: target,
            focus_follow,
            return_point,
        }
    }

    pub fn with_focus_follow(&self, focus_follow: FocusFollowState) -> Self {
        Self {
            mode: self.mode,
            focus_follow,
            return_point: self.return_point,
        }
    }

    pub fn return_to_in_place(&self) -> Result<Self, ScreenModeError> {
        let Some(point) = self.return_point else {
            return Err(ScreenModeError::NoReturnPoint);
        };
        Ok(Self {
            mode: TranscriptScreenMode::InPlace(point.mode),
            focus_follow: point.focus_follow,
            return_point: None,
        })
    }

    pub const fn mode(&self) -> TranscriptScreenMode {
        self.mode
    }

    pub const fn focus_follow(&self) -> FocusFollowState {
        self.focus_follow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenModeError {
    NoReturnPoint,
}

impl Display for ScreenModeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoReturnPoint => formatter.write_str("screen mode has no in-place return point"),
        }
    }
}

impl std::error::Error for ScreenModeError {}
