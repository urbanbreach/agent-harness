use std::fmt::{Display, Formatter};

use ratatui::layout::Rect;

use crate::design_contract::LifecycleState;
use crate::shell_geometry::{layout_for_rect, ShellState};
use crate::transcript_block_viewer::ViewerError;
use crate::transcript_blocks::{BlockLifecycle, BlockSnapshot, BlockStoreError, TranscriptBlocks};
use crate::transcript_identity::{
    BlockId, FocusFollowState, IdentityError, InPlaceMode, TranscriptFocus, TranscriptIdentity,
    TranscriptScreenMode, TranscriptScreenState, TurnId,
};
use crate::transcript_pager::PagerError;
use crate::transcript_scroll::{FollowState, ScrollError, ScrollTransition, TranscriptLayout};
use crate::transcript_timeline::{
    TimelineGeometry, TimelineHitMap, TimelineNavigation, TimelineNavigationError, TimelineTurn,
};

use super::cache::{empty_view, BoundedLayoutCache};
use super::invalidation::{CacheInvalidation, TranscriptEvent, TurnSeed};
use super::lifecycle::{LifecycleCoordinator, LifecycleSnapshot};

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptViewModel {
    pub viewport: Rect,
    pub identity: TranscriptIdentity,
    pub turns: Vec<TimelineTurn>,
    pub blocks: Vec<BlockSnapshot>,
    pub timeline: TimelineGeometry,
    pub hit_map: TimelineHitMap,
    pub layout: Option<TranscriptLayout>,
    pub scroll_top: f64,
    pub follow: FollowState,
    pub screen: TranscriptScreenState,
    pub lifecycle: Vec<LifecycleSnapshot>,
    pub cache_entries: usize,
}

#[derive(Debug)]
pub enum TranscriptIntegrationError {
    InvalidViewport,
    Identity(IdentityError),
    Blocks(BlockStoreError),
    Timeline(TimelineNavigationError),
    Scroll(ScrollError),
    Viewer(ViewerError),
    ScreenMode(crate::transcript_identity::ScreenModeError),
    Pager(PagerError),
    MissingTurn(TurnId),
    MissingBlock(BlockId),
    NoViewer,
    NoPager,
}

impl Display for TranscriptIntegrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidViewport => formatter.write_str("transcript viewport must be non-zero"),
            Self::Identity(error) => write!(formatter, "transcript identity failed: {error}"),
            Self::Blocks(error) => write!(formatter, "transcript blocks failed: {error}"),
            Self::Timeline(error) => write!(formatter, "transcript timeline failed: {error}"),
            Self::Scroll(error) => write!(formatter, "transcript scroll failed: {error}"),
            Self::Viewer(error) => write!(formatter, "transcript viewer failed: {error}"),
            Self::ScreenMode(error) => write!(formatter, "transcript screen mode failed: {error}"),
            Self::Pager(error) => write!(formatter, "transcript pager failed: {error}"),
            Self::MissingTurn(id) => write!(formatter, "missing transcript turn {id}"),
            Self::MissingBlock(id) => write!(formatter, "missing transcript block {id}"),
            Self::NoViewer => formatter.write_str("no transcript viewer is open"),
            Self::NoPager => formatter.write_str("no transcript pager is suspended"),
        }
    }
}

impl std::error::Error for TranscriptIntegrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Blocks(error) => Some(error),
            Self::Timeline(error) => Some(error),
            Self::Scroll(error) => Some(error),
            Self::Viewer(error) => Some(error),
            Self::ScreenMode(error) => Some(error),
            Self::Pager(error) => Some(error),
            Self::InvalidViewport
            | Self::MissingTurn(_)
            | Self::MissingBlock(_)
            | Self::NoViewer
            | Self::NoPager => None,
        }
    }
}

impl From<IdentityError> for TranscriptIntegrationError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}
impl From<BlockStoreError> for TranscriptIntegrationError {
    fn from(error: BlockStoreError) -> Self {
        Self::Blocks(error)
    }
}
impl From<TimelineNavigationError> for TranscriptIntegrationError {
    fn from(error: TimelineNavigationError) -> Self {
        Self::Timeline(error)
    }
}
impl From<ScrollError> for TranscriptIntegrationError {
    fn from(error: ScrollError) -> Self {
        Self::Scroll(error)
    }
}
impl From<ViewerError> for TranscriptIntegrationError {
    fn from(error: ViewerError) -> Self {
        Self::Viewer(error)
    }
}
impl From<crate::transcript_identity::ScreenModeError> for TranscriptIntegrationError {
    fn from(error: crate::transcript_identity::ScreenModeError) -> Self {
        Self::ScreenMode(error)
    }
}
impl From<PagerError> for TranscriptIntegrationError {
    fn from(error: PagerError) -> Self {
        Self::Pager(error)
    }
}

pub struct TranscriptComposite {
    pub(super) events: Vec<TranscriptEvent>,
    pub(super) viewport: Rect,
    pub(super) shell_state: ShellState,
    pub(super) identity: TranscriptIdentity,
    pub(super) turns: Vec<TimelineTurn>,
    pub(super) blocks: TranscriptBlocks,
    pub(super) timeline: TimelineNavigation,
    pub(super) layout: Option<TranscriptLayout>,
    pub(super) scroll_top: f64,
    pub(super) transition: Option<ScrollTransition>,
    pub(super) follow: FollowState,
    pub(super) screen: TranscriptScreenState,
    pub(super) viewer: Option<crate::transcript_block_viewer::ViewerState>,
    pub(super) pager: Option<crate::transcript_pager::TranscriptSnapshot>,
    pub(super) lifecycle: LifecycleCoordinator,
    pub(super) cache: BoundedLayoutCache,
    pub(super) view: TranscriptViewModel,
}

impl TranscriptComposite {
    pub fn new(viewport: Rect) -> Result<Self, TranscriptIntegrationError> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(TranscriptIntegrationError::InvalidViewport);
        }
        let regions = layout_for_rect(viewport, ShellState::Streaming);
        let identity = TranscriptIdentity::from_replay([])?;
        let timeline =
            TimelineNavigation::from_turns(Vec::new(), regions.transcript_viewport.height)?;
        let follow = FollowState::new();
        let screen = TranscriptScreenState::new(
            TranscriptScreenMode::InPlace(InPlaceMode::Transcript),
            FocusFollowState::new(TranscriptFocus::Transcript, true),
        );
        let cache = BoundedLayoutCache::new();
        let view = empty_view(viewport, &identity, &screen, follow, &cache);
        Ok(Self {
            events: Vec::new(),
            viewport,
            shell_state: ShellState::Streaming,
            identity,
            turns: Vec::new(),
            blocks: TranscriptBlocks::new(),
            timeline,
            layout: None,
            scroll_top: 0.0,
            transition: None,
            follow,
            screen,
            viewer: None,
            pager: None,
            lifecycle: LifecycleCoordinator::new(false),
            cache,
            view,
        })
    }

    pub fn apply(&mut self, event: TranscriptEvent) -> Result<(), TranscriptIntegrationError> {
        self.cache.apply(self.invalidation_for(&event));
        self.events.push(event);
        if let Err(error) = self.rebuild() {
            let _ = self.events.pop();
            let _ = self.rebuild();
            return Err(error);
        }
        Ok(())
    }

    pub fn resize(&mut self, viewport: Rect) -> Result<(), TranscriptIntegrationError> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(TranscriptIntegrationError::InvalidViewport);
        }
        let previous = self.viewport.width;
        self.viewport = viewport;
        self.cache.apply(CacheInvalidation::WidthChanged {
            from: previous,
            to: viewport.width,
        });
        self.rebuild()
    }

    pub fn set_block_lifecycle(
        &mut self,
        id: BlockId,
        lifecycle: BlockLifecycle,
    ) -> Result<(), TranscriptIntegrationError> {
        self.apply(TranscriptEvent::BlockLifecycle { id, lifecycle })
    }

    pub fn append_block(
        &mut self,
        id: BlockId,
        content: impl Into<String>,
    ) -> Result<(), TranscriptIntegrationError> {
        self.apply(TranscriptEvent::BlockAppended {
            id,
            content: content.into(),
        })
    }

    pub fn toggle_fold(&mut self, id: BlockId) -> Result<(), TranscriptIntegrationError> {
        self.apply(TranscriptEvent::FoldToggled { id })
    }

    pub fn compact_turn(
        &mut self,
        previous_turn_id: TurnId,
        replacement: TurnSeed,
    ) -> Result<(), TranscriptIntegrationError> {
        self.apply(TranscriptEvent::Compacted {
            previous_turn_id,
            replacement,
        })
    }

    pub fn view(&self) -> &TranscriptViewModel {
        &self.view
    }
    pub fn events(&self) -> &[TranscriptEvent] {
        &self.events
    }
    pub fn cache(&self) -> &BoundedLayoutCache {
        &self.cache
    }
    pub fn screen(&self) -> TranscriptScreenState {
        self.screen.clone()
    }

    pub fn viewer_mut(&mut self) -> Option<&mut crate::transcript_block_viewer::ViewerState> {
        self.viewer.as_mut()
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.lifecycle.set_reduced_motion(reduced_motion);
        self.refresh_view();
    }
}
