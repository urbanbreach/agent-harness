// allow: SIZE_OK — transcript composite command surface
#![allow(
    clippy::module_name_repetitions,
    reason = "task 29 facade names the integrated transcript state explicitly"
)]

use std::fmt::Write as _;

mod cache;
mod composite;
mod incremental;
mod invalidation;
mod lifecycle;

pub use crate::transcript_timeline::TimelineStatus;
pub use cache::{
    BoundedLayoutCache, CachedLayout, LayoutCacheKey, DEFAULT_CACHE_CAPACITY,
    MAX_LAYOUT_PAYLOAD_BYTES,
};
pub use composite::{TranscriptComposite, TranscriptIntegrationError, TranscriptViewModel};
pub use incremental::TranscriptWorkMetrics;
pub use invalidation::{
    BlockSeed, CacheInvalidation, InvalidationReport, TranscriptEvent, TurnSeed,
};
pub use lifecycle::LifecycleFrame;
pub use lifecycle::{LifecycleCoordinator, LifecyclePhase, LifecycleSnapshot};

use crate::scheduling::FrameNow;
use crate::shell_geometry::layout_for_rect;
use crate::terminal::char_display_width;
use crate::transcript_block_viewer::{
    ViewerBlockContent, ViewerCloseReason, ViewerReturnSnapshot, ViewerState,
};
use crate::transcript_blocks::{BlockLifecycle, BlockSnapshot, FoldState};
use crate::transcript_identity::{
    BlockId, FocusFollowState, InPlaceMode, TranscriptFocus, TranscriptScreenMode,
};
use crate::transcript_pager::TranscriptSnapshot;
use crate::transcript_scroll::{
    EasingKind, MotionPreference, ScrollFrame, ScrollTransition, TranscriptLayout,
    TransitionRequest,
};
use crate::transcript_timeline::{TimelineJump, TimelineNavigationSnapshot};

impl TranscriptComposite {
    pub fn replace_events(
        &mut self,
        events: Vec<TranscriptEvent>,
    ) -> Result<(), TranscriptIntegrationError> {
        let previous = std::mem::replace(&mut self.events, events);
        self.cache.apply(CacheInvalidation::ReplayReset);
        if let Err(error) = self.rebuild() {
            self.events = previous;
            self.cache.apply(CacheInvalidation::ReplayReset);
            let _ = self.rebuild();
            return Err(error);
        }
        Ok(())
    }

    pub fn select_turn(
        &mut self,
        turn_id: crate::transcript_identity::TurnId,
    ) -> Result<TimelineNavigationSnapshot, TranscriptIntegrationError> {
        let snapshot = self.timeline.select_turn(turn_id)?;
        self.screen = self
            .screen
            .with_focus_follow(FocusFollowState::new(TranscriptFocus::Timeline, false));
        self.scroll_top = f64::from(u32::try_from(snapshot.scroll_top).unwrap_or(u32::MAX));
        self.follow.scroll_by(
            self.scroll_top - self.follow.offset(),
            self.layout
                .as_ref()
                .map_or(0.0, TranscriptLayout::max_scroll),
        )?;
        self.refresh_view();
        Ok(snapshot)
    }

    pub fn is_following(&self) -> bool {
        self.follow.is_following()
    }

    pub fn set_following(&mut self, following: bool) -> Result<(), TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        if following {
            self.follow.jump_to_bottom();
            self.scroll_top = max;
        } else if self.follow.is_following() && max > 0.0 {
            self.follow.scroll_by(1.0, max)?;
            self.scroll_top = (max - self.follow.offset()).max(0.0);
        }
        self.refresh_view();
        Ok(())
    }

    pub fn jump_to_top(&mut self) -> Result<ScrollFrame, TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        if max > 0.0 {
            self.follow.scroll_by(max, max)?;
        }
        self.scroll_to(0.0, 0, MotionPreference::Full)
    }

    pub fn jump_to_bottom(&mut self) -> Result<ScrollFrame, TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        self.follow.jump_to_bottom();
        self.scroll_to(max, 0, MotionPreference::Full)
    }

    pub fn scroll_top(&self) -> f64 {
        self.scroll_top
    }

    pub fn scroll_by(&mut self, delta: f64) -> Result<ScrollFrame, TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        self.follow.scroll_by(delta, max)?;
        self.scroll_to(
            (max - self.follow.offset()).max(0.0),
            0,
            MotionPreference::Full,
        )
    }

    pub fn scroll_to(
        &mut self,
        target: f64,
        started_at_ms: u64,
        motion: MotionPreference,
    ) -> Result<ScrollFrame, TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        let target = target.clamp(0.0, max);
        let transition = ScrollTransition::start(TransitionRequest::new(
            self.scroll_top,
            target,
            started_at_ms,
            EasingKind::Jump,
            motion,
        ))?;
        let frame = transition.sample(started_at_ms);
        self.scroll_top = frame.value;
        self.transition = (!frame.settled).then_some(transition);
        self.refresh_view();
        Ok(frame)
    }

    pub fn detach_at(&mut self, target: f64) -> Result<ScrollFrame, TranscriptIntegrationError> {
        let max = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
        let target = target.clamp(0.0, max);
        self.follow.jump_to_bottom();
        self.follow.scroll_by(max - target, max)?;
        self.scroll_to(target, 0, MotionPreference::Full)
    }

    pub fn tick_at(&mut self, now: FrameNow) -> LifecycleFrame {
        let frame = self.lifecycle.tick(now);
        if let Some(transition) = self.transition {
            let scroll = transition.sample(now.animation_ms);
            self.scroll_top = scroll.value;
            if scroll.settled {
                self.transition = None;
            }
        }
        self.refresh_view();
        frame
    }

    pub fn jump(
        &mut self,
        jump: TimelineJump,
    ) -> Result<TimelineNavigationSnapshot, TranscriptIntegrationError> {
        let snapshot = self.timeline.jump(jump)?;
        if matches!(
            jump,
            TimelineJump::NextResponse | TimelineJump::PreviousResponse
        ) && snapshot.response_position.is_none()
        {
            return Ok(snapshot);
        }
        self.screen = self
            .screen
            .with_focus_follow(FocusFollowState::new(TranscriptFocus::Timeline, false));
        self.scroll_top = f64::from(u32::try_from(snapshot.scroll_top).unwrap_or(u32::MAX));
        self.follow.scroll_by(
            self.scroll_top - self.follow.offset(),
            self.layout
                .as_ref()
                .map_or(0.0, TranscriptLayout::max_scroll),
        )?;
        self.refresh_view();
        Ok(snapshot)
    }

    pub fn open_viewer(&mut self, id: BlockId) -> Result<(), TranscriptIntegrationError> {
        let block = self
            .blocks
            .get(id)
            .ok_or(TranscriptIntegrationError::MissingBlock(id))?;
        let snapshot = BlockSnapshot {
            id: block.id(),
            kind: block.kind(),
            lifecycle: block.lifecycle(),
            content: block.content().to_owned(),
            fold_state: block.fold_state(),
            raw: block.raw().cloned(),
        };
        let content = ViewerBlockContent::from_block(&snapshot)?;
        let layout = self
            .layout
            .as_ref()
            .ok_or(crate::transcript_scroll::ScrollError::EmptyLayout)?;
        let anchor = layout.capture_anchor(self.scroll_top)?;
        let return_snapshot =
            ViewerReturnSnapshot::new(snapshot.fold_state, self.screen.focus_follow(), anchor);
        self.viewer = Some(ViewerState::open(id, content, return_snapshot)?);
        self.screen = self
            .screen
            .switch_to(TranscriptScreenMode::SelectedBlockViewer)
            .with_focus_follow(FocusFollowState::new(TranscriptFocus::Viewer, false));
        self.refresh_view();
        Ok(())
    }

    pub fn close_viewer(&mut self) -> Result<ViewerCloseReason, TranscriptIntegrationError> {
        let viewer = self
            .viewer
            .take()
            .ok_or(TranscriptIntegrationError::NoViewer)?;
        let close = viewer.close();
        let snapshot = close.return_snapshot();
        if self.screen.mode() == TranscriptScreenMode::SelectedBlockViewer {
            self.screen = self
                .screen
                .return_to_in_place()?
                .with_focus_follow(snapshot.focus_follow);
        }
        let layout = self
            .layout
            .as_ref()
            .ok_or(crate::transcript_scroll::ScrollError::EmptyLayout)?;
        self.scroll_top = snapshot.anchor.resolve(layout)?;
        self.refresh_view();
        Ok(close.reason())
    }

    pub fn suspend_pager(&mut self) -> Result<&TranscriptSnapshot, TranscriptIntegrationError> {
        let mut text = String::new();
        for block in &self.view.blocks {
            let _ = writeln!(text, "{}", block.kind.as_str());
            let _ = writeln!(text, "{}", block.content);
        }
        self.pager = Some(TranscriptSnapshot::from_text(&text));
        self.screen = self
            .screen
            .switch_to(TranscriptScreenMode::ExternalPagerSuspended);
        self.refresh_view();
        self.pager
            .as_ref()
            .ok_or(TranscriptIntegrationError::NoPager)
    }

    pub fn restore_pager(&mut self) -> Result<(), TranscriptIntegrationError> {
        if self.pager.take().is_none() {
            return Err(TranscriptIntegrationError::NoPager);
        }
        self.screen = self.screen.return_to_in_place()?;
        self.refresh_view();
        Ok(())
    }

    pub fn viewer(&self) -> Option<&ViewerState> {
        self.viewer.as_ref()
    }

    pub fn pager_snapshot(&self) -> Option<&TranscriptSnapshot> {
        self.pager.as_ref()
    }
}

fn cached_layout(
    block: &BlockSnapshot,
    width: u16,
    cache: &mut BoundedLayoutCache,
) -> CachedLayout {
    let key = LayoutCacheKey::new(block.id, width, block.fold_state, block.lifecycle);
    if let Some(layout) = cache.get(&key) {
        return layout;
    }
    let layout = measure_layout(block, width);
    let _ = cache.insert(key, layout);
    layout
}

fn measure_layout(block: &BlockSnapshot, width: u16) -> CachedLayout {
    if block.fold_state == FoldState::Collapsed {
        return CachedLayout::new(1, 1, 1);
    }
    let width = usize::from(width.max(1));
    let (rows, cells) =
        block
            .content
            .split('\n')
            .fold((0_usize, 0_usize), |(rows, cells), line| {
                let line_cells = line
                    .chars()
                    .map(|character| usize::from(char_display_width(character)))
                    .sum::<usize>();
                (
                    rows.saturating_add(line_cells.max(1).div_ceil(width)),
                    cells.saturating_add(line_cells),
                )
            });
    CachedLayout::new(rows.max(1), cells, block.content.len())
}

impl TranscriptComposite {
    pub(super) fn refresh_view(&mut self) {
        let regions = layout_for_rect(self.viewport, self.shell_state);
        let rows = if self.scroll_top.is_sign_negative() {
            0
        } else {
            scroll_rows(self.scroll_top)
        };
        let timeline = crate::transcript_timeline::geometry_for_rect(
            regions.transcript_viewport,
            &self.turns,
            rows,
        );
        let hit_map = crate::transcript_timeline::TimelineHitMap::from_geometry(&timeline);
        self.view = TranscriptViewModel {
            viewport: self.viewport,
            identity: self.identity.clone(),
            turns: self.turns.clone(),
            blocks: self.blocks.snapshot(),
            timeline,
            hit_map,
            response_position: self.timeline.response_position(),
            layout: self.layout.clone(),
            scroll_top: self.scroll_top,
            follow: self.follow,
            screen: self.screen.clone(),
            lifecycle: self.lifecycle.snapshots(),
            cache_entries: self.cache.len(),
        };
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "scroll rows are clamped to the non-negative terminal geometry contract"
)]
fn scroll_rows(value: f64) -> usize {
    value.floor() as usize
}
