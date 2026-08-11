use crate::design_contract::{MotionKind, DESIGN_TOKENS};
use crate::scheduling::{FrameDecision, FrameInputs, FrameNow, FrameScheduler};
use crate::shell_geometry::layout_for_rect;
use crate::transcript_blocks::{BlockKind, BlockLifecycle};
use crate::transcript_identity::{BlockId, TranscriptIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    Pending,
    Streaming,
    Completed,
    Failed,
}

impl LifecyclePhase {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Streaming)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleSnapshot {
    pub block_id: BlockId,
    pub kind: BlockKind,
    pub lifecycle: BlockLifecycle,
    pub phase: LifecyclePhase,
    pub frame: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleFrame {
    pub decision: Option<FrameDecision>,
    pub animation_active: bool,
    pub states: Vec<LifecycleSnapshot>,
}

#[derive(Debug, Clone, Copy)]
struct LifecycleEntry {
    snapshot: LifecycleSnapshot,
}

#[derive(Debug)]
pub struct LifecycleCoordinator {
    scheduler: FrameScheduler,
    entries: Vec<LifecycleEntry>,
}

impl LifecycleCoordinator {
    pub const fn new(reduced_motion: bool) -> Self {
        Self {
            scheduler: FrameScheduler::with_reduced_motion(reduced_motion),
            entries: Vec::new(),
        }
    }

    pub fn set_block(&mut self, block_id: BlockId, kind: BlockKind, lifecycle: BlockLifecycle) {
        let next = LifecycleSnapshot {
            block_id,
            kind,
            lifecycle,
            phase: phase_for(lifecycle),
            frame: 0,
        };
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.snapshot.block_id == block_id)
        {
            if entry.snapshot.lifecycle != lifecycle || entry.snapshot.kind != kind {
                entry.snapshot = next;
            }
            return;
        }
        self.entries.push(LifecycleEntry { snapshot: next });
    }

    pub fn remove_missing(&mut self, present: &[BlockId]) {
        self.entries
            .retain(|entry| present.contains(&entry.snapshot.block_id));
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.scheduler.set_reduced_motion(reduced_motion);
        if reduced_motion {
            for entry in &mut self.entries {
                entry.snapshot.frame = 0;
            }
        }
    }

    pub const fn reduced_motion(&self) -> bool {
        self.scheduler.reduced_motion()
    }

    pub fn tick(&mut self, now: FrameNow) -> LifecycleFrame {
        let animation_active = self.animation_active();
        let decision = self.scheduler.schedule(
            now,
            FrameInputs {
                animation_active,
                flush_requested: false,
            },
        );
        if decision.is_some_and(|frame| frame.render) && animation_active && !self.reduced_motion()
        {
            for entry in &mut self.entries {
                if entry.snapshot.phase.is_active() && is_animated(entry.snapshot.kind) {
                    let frames = frames_for(entry.snapshot.kind);
                    entry.snapshot.frame = entry.snapshot.frame.wrapping_add(1) % frames.max(1);
                }
            }
        }
        LifecycleFrame {
            decision,
            animation_active,
            states: self.entries.iter().map(|entry| entry.snapshot).collect(),
        }
    }

    pub fn animation_active(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.snapshot.phase.is_active() && is_animated(entry.snapshot.kind))
    }

    pub fn snapshots(&self) -> Vec<LifecycleSnapshot> {
        self.entries.iter().map(|entry| entry.snapshot).collect()
    }

    pub(crate) fn snapshot(&self, block_id: BlockId) -> Option<LifecycleSnapshot> {
        self.entries
            .iter()
            .find(|entry| entry.snapshot.block_id == block_id)
            .map(|entry| entry.snapshot)
    }
}

fn phase_for(lifecycle: BlockLifecycle) -> LifecyclePhase {
    match lifecycle {
        BlockLifecycle::Waiting => LifecyclePhase::Pending,
        BlockLifecycle::Streaming | BlockLifecycle::Tool => LifecyclePhase::Streaming,
        BlockLifecycle::Completed => LifecyclePhase::Completed,
        BlockLifecycle::Failed => LifecyclePhase::Failed,
    }
}

fn is_animated(kind: BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::Thinking | BlockKind::Tool | BlockKind::Diff
    )
}

fn frames_for(kind: BlockKind) -> u8 {
    let motion = match kind {
        BlockKind::Thinking => MotionKind::StreamingSpinner,
        BlockKind::Tool | BlockKind::Diff => MotionKind::ToolPulse,
        BlockKind::User | BlockKind::Assistant | BlockKind::System => MotionKind::ActiveTick,
    };
    DESIGN_TOKENS
        .motion_tokens
        .all
        .iter()
        .find(|token| token.kind == motion)
        .map_or(1, |token| token.frames.max(1))
}

use super::cache::{build_layout, build_turns};
use super::composite::TranscriptComposite;
use super::invalidation::{project, CacheInvalidation, Projection, TranscriptEvent};
use super::TranscriptIntegrationError;

impl TranscriptComposite {
    pub(crate) fn invalidation_for(&self, event: &TranscriptEvent) -> CacheInvalidation {
        match event {
            TranscriptEvent::TurnStarted(_)
            | TranscriptEvent::TurnUpdated(_)
            | TranscriptEvent::TurnStatus { .. } => CacheInvalidation::ReplayReset,
            TranscriptEvent::BlockCreated(block) => CacheInvalidation::BlockRemoved(block.id),
            TranscriptEvent::BlockAppended { id, .. } => CacheInvalidation::BlockAppended(*id),
            TranscriptEvent::BlockLifecycle { id, lifecycle } => {
                CacheInvalidation::LifecycleTransition {
                    block_id: *id,
                    from: self
                        .blocks
                        .get(*id)
                        .map_or(*lifecycle, |block| block.lifecycle()),
                    to: *lifecycle,
                }
            }
            TranscriptEvent::FoldToggled { id } => CacheInvalidation::FoldToggled(*id),
            TranscriptEvent::BlockRemoved { id } => CacheInvalidation::BlockRemoved(*id),
            TranscriptEvent::TurnRemoved { .. } | TranscriptEvent::Compacted { .. } => {
                CacheInvalidation::Compaction
            }
        }
    }

    pub(crate) fn rebuild(&mut self) -> Result<(), TranscriptIntegrationError> {
        self.work_metrics.full_rebuilds = self.work_metrics.full_rebuilds.saturating_add(1);
        self.work_metrics.events_reprojected = self
            .work_metrics
            .events_reprojected
            .saturating_add(self.events.len());
        let Projection {
            turns: seeds,
            blocks,
        } = project(&self.events)?;
        let old_anchor = self
            .layout
            .as_ref()
            .and_then(|layout| layout.capture_anchor(self.scroll_top).ok());
        let identity = TranscriptIdentity::from_replay(seeds.iter().map(|turn| turn.replay))?;
        let snapshots = blocks.snapshot();
        let regions = layout_for_rect(self.viewport, self.shell_state);
        self.cache.set_width(regions.transcript_viewport.width);
        self.blocks = blocks;
        let ids = snapshots.iter().map(|block| block.id).collect::<Vec<_>>();
        self.lifecycle.remove_missing(&ids);
        for block in &snapshots {
            self.lifecycle
                .set_block(block.id, block.kind, block.lifecycle);
        }
        let turns = build_turns(
            &seeds,
            &snapshots,
            &mut self.cache,
            regions.transcript_viewport.width,
        );
        self.timeline
            .update_document(identity.clone(), turns.clone())?;
        self.identity = identity;
        self.turns = turns;
        self.turn_seeds = seeds;
        self.layout = build_layout(
            &self.turns,
            &self.blocks,
            regions.transcript_viewport.height,
            &mut self.cache,
            regions.transcript_viewport.width,
        );
        let max_scroll = self
            .layout
            .as_ref()
            .map_or(0.0, crate::transcript_scroll::TranscriptLayout::max_scroll);
        self.follow.content_changed(max_scroll)?;
        self.scroll_top = if self.follow.is_following() {
            max_scroll
        } else {
            old_anchor
                .and_then(|anchor| {
                    self.layout
                        .as_ref()
                        .and_then(|layout| anchor.resolve(layout).ok())
                })
                .unwrap_or(self.scroll_top.min(max_scroll))
        };
        self.transition = None;
        self.refresh_view();
        Ok(())
    }
}
