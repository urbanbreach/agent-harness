use std::fmt::{Display, Formatter};

use crate::dashboard::{DashboardReadModel, SelectionKey};
use crate::dashboard_controls::DashboardControlState;
use crate::dashboard_details::{DashboardDetails, NavigationError};
use crate::dashboard_peek::{DashboardPeek, DashboardPeekError};
use crate::dashboard_roster::RosterState;
use crate::transcript_identity::TranscriptFocus;
use crate::transcript_scroll::LogicalAnchor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardIntegrationError {
    InvalidViewport,
    UnknownSelection(SelectionKey),
    Peek(DashboardPeekError),
    Navigation(NavigationError),
    DetailsUnavailable,
}

impl Display for DashboardIntegrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidViewport => formatter.write_str("dashboard viewport must be non-zero"),
            Self::UnknownSelection(key) => {
                write!(
                    formatter,
                    "dashboard selection is unavailable: {}",
                    key.as_str()
                )
            }
            Self::Peek(error) => write!(formatter, "dashboard peek failed: {error}"),
            Self::Navigation(error) => write!(formatter, "dashboard details failed: {error}"),
            Self::DetailsUnavailable => formatter.write_str("dashboard details are unavailable"),
        }
    }
}

impl std::error::Error for DashboardIntegrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Peek(error) => Some(error),
            Self::Navigation(error) => Some(error),
            Self::InvalidViewport | Self::UnknownSelection(_) | Self::DetailsUnavailable => None,
        }
    }
}

impl From<DashboardPeekError> for DashboardIntegrationError {
    fn from(error: DashboardPeekError) -> Self {
        Self::Peek(error)
    }
}

impl From<NavigationError> for DashboardIntegrationError {
    fn from(error: NavigationError) -> Self {
        Self::Navigation(error)
    }
}

#[derive(Debug, Clone)]
pub struct DashboardIntegrationParts {
    pub dashboard: DashboardReadModel,
    pub roster: RosterState,
    pub peek: DashboardPeek,
    pub details: Option<DashboardDetails>,
    pub controls: DashboardControlState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardReturnState {
    pub transcript_focus: TranscriptFocus,
    pub transcript_follow: bool,
    pub transcript_anchor: Option<LogicalAnchor>,
}

impl DashboardReturnState {
    pub const fn new(
        transcript_focus: TranscriptFocus,
        transcript_follow: bool,
        transcript_anchor: Option<LogicalAnchor>,
    ) -> Self {
        Self {
            transcript_focus,
            transcript_follow,
            transcript_anchor,
        }
    }
}
