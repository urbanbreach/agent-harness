mod draft;
mod scroll;
mod tail;

pub use draft::{DashboardDrafts, DashboardPeekView};
pub use scroll::PeekScroll;
pub use tail::{PeekTail, TailError, TailUpdate, TailUpdateKind};

use crate::dashboard::SelectionKey;
use crate::transcript_blocks::BlockSnapshot;
use crate::transcript_identity::BlockId;
use crate::transcript_scroll::{FollowMode, ScrollError, TranscriptLayout};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const DEFAULT_MAX_SESSIONS: usize = 16;
pub const DEFAULT_MAX_BLOCKS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeekLimits {
    pub max_sessions: usize,
    pub max_blocks: usize,
}

impl PeekLimits {
    pub const fn new(max_sessions: usize, max_blocks: usize) -> Self {
        Self {
            max_sessions,
            max_blocks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardPeekError {
    InvalidLimit(&'static str),
    NoSelectedSession,
    UnknownSession(String),
    DuplicateSession(String),
    Tail(TailError),
    Scroll(ScrollError),
}

impl Display for DashboardPeekError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimit(name) => write!(formatter, "peek limit {name} must be positive"),
            Self::NoSelectedSession => {
                formatter.write_str("dashboard peek has no selected session")
            }
            Self::UnknownSession(id) => write!(formatter, "unknown dashboard session: {id}"),
            Self::DuplicateSession(id) => write!(formatter, "duplicate dashboard session: {id}"),
            Self::Tail(error) => Display::fmt(error, formatter),
            Self::Scroll(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for DashboardPeekError {}

impl From<TailError> for DashboardPeekError {
    fn from(error: TailError) -> Self {
        Self::Tail(error)
    }
}

impl From<ScrollError> for DashboardPeekError {
    fn from(error: ScrollError) -> Self {
        Self::Scroll(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeekSessionStatus {
    Active,
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
struct SessionPeekState {
    tail: PeekTail,
    scroll: PeekScroll,
    unread_count: usize,
    status: PeekSessionStatus,
}

impl SessionPeekState {
    fn new(limits: PeekLimits, viewport_extent: f64) -> Result<Self, DashboardPeekError> {
        Ok(Self {
            tail: PeekTail::new(limits.max_blocks)?,
            scroll: PeekScroll::new(viewport_extent)?,
            unread_count: 0,
            status: PeekSessionStatus::Active,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardPeek {
    limits: PeekLimits,
    viewport_extent: f64,
    sessions: BTreeMap<SelectionKey, SessionPeekState>,
    order: Vec<SelectionKey>,
    selected: Option<SelectionKey>,
    drafts: DashboardDrafts,
}

impl DashboardPeek {
    pub fn new(viewport_extent: f64) -> Result<Self, DashboardPeekError> {
        Self::with_limits(
            viewport_extent,
            PeekLimits::new(DEFAULT_MAX_SESSIONS, DEFAULT_MAX_BLOCKS),
        )
    }

    pub fn with_limits(
        viewport_extent: f64,
        limits: PeekLimits,
    ) -> Result<Self, DashboardPeekError> {
        if limits.max_sessions == 0 {
            return Err(DashboardPeekError::InvalidLimit("max_sessions"));
        }
        if limits.max_blocks == 0 {
            return Err(DashboardPeekError::InvalidLimit("max_blocks"));
        }
        let _ = PeekScroll::new(viewport_extent)?;
        Ok(Self {
            limits,
            viewport_extent,
            sessions: BTreeMap::new(),
            order: Vec::new(),
            selected: None,
            drafts: DashboardDrafts::new(),
        })
    }

    pub fn sync_sessions(
        &mut self,
        session_ids: &[SelectionKey],
    ) -> Result<(), DashboardPeekError> {
        let mut seen = BTreeSet::new();
        for session_id in session_ids {
            if !seen.insert(session_id.clone()) {
                return Err(DashboardPeekError::DuplicateSession(
                    session_id.as_str().to_string(),
                ));
            }
        }
        for session_id in session_ids {
            if !self.sessions.contains_key(session_id) {
                let state = SessionPeekState::new(self.limits, self.viewport_extent)?;
                self.sessions.insert(session_id.clone(), state);
            }
        }
        self.order = session_ids.to_vec();
        if !self
            .order
            .iter()
            .any(|id| Some(id) == self.selected.as_ref())
        {
            self.selected = self.order.first().cloned();
        }
        self.evict_excess_sessions();
        Ok(())
    }

    pub fn replace_blocks(
        &mut self,
        session_id: &SelectionKey,
        blocks: &[BlockSnapshot],
    ) -> Result<TailUpdate, DashboardPeekError> {
        let state = self.state_mut(session_id)?;
        state.unread_count = 0;
        Ok(state.tail.replace(blocks))
    }

    pub fn append_block(
        &mut self,
        session_id: &SelectionKey,
        block: BlockSnapshot,
    ) -> Result<TailUpdate, DashboardPeekError> {
        let selected = self.selected.as_ref() == Some(session_id);
        let state = self.state_mut(session_id)?;
        let update = state.tail.append_block(block);
        if !selected || state.scroll.follow() == FollowMode::Detached {
            state.unread_count = state
                .unread_count
                .saturating_add(update.appended + update.replaced);
        }
        Ok(update)
    }

    pub fn append_content(
        &mut self,
        session_id: &SelectionKey,
        block_id: BlockId,
        content: &str,
    ) -> Result<TailUpdate, DashboardPeekError> {
        let selected = self.selected.as_ref() == Some(session_id);
        let state = self.state_mut(session_id)?;
        let update = state.tail.append_content(block_id, content)?;
        if !selected || state.scroll.follow() == FollowMode::Detached {
            state.unread_count = state.unread_count.saturating_add(1);
        }
        Ok(update)
    }

    pub fn set_layout(
        &mut self,
        session_id: &SelectionKey,
        layout: TranscriptLayout,
    ) -> Result<(), DashboardPeekError> {
        self.state_mut(session_id)?.scroll.set_layout(layout)?;
        Ok(())
    }

    pub fn scroll_by(&mut self, delta: f64) -> Result<(), DashboardPeekError> {
        let session_id = self.selected_id()?;
        self.state_mut(&session_id)?.scroll.scroll_by(delta)?;
        Ok(())
    }

    pub fn jump_to_bottom(&mut self) -> Result<(), DashboardPeekError> {
        let session_id = self.selected_id()?;
        let state = self.state_mut(&session_id)?;
        state.scroll.jump_to_bottom();
        state.unread_count = 0;
        Ok(())
    }

    fn selected_id(&self) -> Result<SelectionKey, DashboardPeekError> {
        self.selected
            .clone()
            .ok_or(DashboardPeekError::NoSelectedSession)
    }

    fn unknown(&self, session_id: &SelectionKey) -> DashboardPeekError {
        DashboardPeekError::UnknownSession(session_id.as_str().to_string())
    }

    fn state(&self, session_id: &SelectionKey) -> Result<&SessionPeekState, DashboardPeekError> {
        self.sessions
            .get(session_id)
            .ok_or_else(|| self.unknown(session_id))
    }

    fn state_mut(
        &mut self,
        session_id: &SelectionKey,
    ) -> Result<&mut SessionPeekState, DashboardPeekError> {
        if !self.sessions.contains_key(session_id) {
            return Err(self.unknown(session_id));
        }
        self.sessions
            .get_mut(session_id)
            .ok_or_else(|| DashboardPeekError::UnknownSession(session_id.as_str().to_string()))
    }

    fn evict_excess_sessions(&mut self) {
        while self.sessions.len() > self.limits.max_sessions {
            let candidate = self
                .order
                .iter()
                .find(|id| self.selected.as_ref() != Some(*id) && self.sessions.contains_key(*id))
                .cloned()
                .or_else(|| {
                    self.sessions
                        .keys()
                        .find(|id| self.selected.as_ref() != Some(*id))
                        .cloned()
                });
            let Some(candidate) = candidate else {
                break;
            };
            self.sessions.remove(&candidate);
        }
    }
}
