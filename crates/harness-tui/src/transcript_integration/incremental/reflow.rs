// allow: SIZE_OK — incremental transcript reflow state machine
use std::collections::HashMap;

use crate::shell_geometry::layout_for_rect;
use crate::transcript_blocks::BlockSnapshot;
use crate::transcript_identity::{BlockId, TranscriptIdentity};
use crate::transcript_scroll::TranscriptLayout;
use crate::transcript_timeline::{TimelineHitMap, TimelineTurn};

use super::super::composite::TranscriptComposite;
use super::super::invalidation::TurnSeed;
use super::super::TranscriptIntegrationError;

impl TranscriptComposite {
    pub(in crate::transcript_integration) fn remeasure_projected_document(
        &mut self,
    ) -> Result<(), TranscriptIntegrationError> {
        let old_anchor = self
            .layout
            .as_ref()
            .and_then(|layout| layout.capture_anchor(self.scroll_top).ok());
        let regions = layout_for_rect(self.viewport, self.shell_state);
        let snapshots = self.blocks.snapshot();
        let extents = snapshots
            .iter()
            .map(|snapshot| {
                let measured = super::super::cached_layout(
                    snapshot,
                    regions.transcript_viewport.width,
                    &mut self.cache,
                );
                (snapshot.id, f64::from(measured.rows.max(1)))
            })
            .collect::<HashMap<_, _>>();
        self.work_metrics.blocks_measured = self
            .work_metrics
            .blocks_measured
            .saturating_add(snapshots.len());
        self.layout = TranscriptLayout::from_heights(
            ordered_placements(&self.turn_seeds, &extents),
            f64::from(regions.transcript_viewport.height.max(1)),
        )
        .ok();
        self.reflow_projected_state_with_anchor(None, old_anchor)
    }

    pub(super) fn remeasure_block(
        &mut self,
        id: BlockId,
    ) -> Result<(), TranscriptIntegrationError> {
        let old_anchor = self
            .layout
            .as_ref()
            .and_then(|layout| layout.capture_anchor(self.scroll_top).ok());
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
        let regions = layout_for_rect(self.viewport, self.shell_state);
        let previous_extent = self.layout.as_ref().and_then(|layout| {
            layout
                .placements()
                .iter()
                .find(|placement| placement.id() == id)
                .map(|placement| placement.extent())
        });
        let measured = super::super::cached_layout(
            &snapshot,
            regions.transcript_viewport.width,
            &mut self.cache,
        );
        self.work_metrics.blocks_touched = self.work_metrics.blocks_touched.saturating_add(1);
        self.work_metrics.blocks_measured = self.work_metrics.blocks_measured.saturating_add(1);
        let measured_extent = f64::from(measured.rows.max(1));
        if previous_extent == Some(measured_extent) {
            self.work_metrics.placements_reflowed =
                self.work_metrics.placements_reflowed.saturating_add(1);
            self.work_metrics.turns_reflowed = self.work_metrics.turns_reflowed.saturating_add(1);
            self.refresh_damaged_block(snapshot);
            return Ok(());
        }

        if previous_extent.is_none() {
            let mut extents = self
                .layout
                .as_ref()
                .map(|layout| {
                    layout
                        .placements()
                        .iter()
                        .map(|placement| (placement.id(), placement.extent()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            extents.insert(id, measured_extent);
            let placements = ordered_placements(&self.turn_seeds, &extents);
            self.work_metrics.placements_reflowed = self
                .work_metrics
                .placements_reflowed
                .saturating_add(placements.len());
            self.layout = TranscriptLayout::from_heights(
                placements,
                f64::from(regions.transcript_viewport.height.max(1)),
            )
            .ok();
            return self.reflow_projected_state_with_anchor(Some(snapshot), old_anchor);
        }

        let tail_update = self
            .layout
            .as_ref()
            .and_then(|layout| layout.placements().last())
            .is_some_and(|placement| placement.id() == id);
        let placements_reflowed = self
            .layout
            .as_mut()
            .ok_or(TranscriptIntegrationError::MissingBlock(id))?
            .update_extent(id, measured_extent)?;
        self.work_metrics.placements_reflowed = self
            .work_metrics
            .placements_reflowed
            .saturating_add(placements_reflowed);
        if tail_update {
            return self.reflow_tail_block(snapshot, old_anchor);
        }
        self.reflow_projected_state_with_anchor(Some(snapshot), old_anchor)
    }

    pub(super) fn reflow_projected_state(
        &mut self,
        damaged_block: Option<BlockSnapshot>,
    ) -> Result<(), TranscriptIntegrationError> {
        let old_anchor = self
            .layout
            .as_ref()
            .and_then(|layout| layout.capture_anchor(self.scroll_top).ok());
        self.reflow_projected_state_with_anchor(damaged_block, old_anchor)
    }

    fn reflow_projected_state_with_anchor(
        &mut self,
        damaged_block: Option<BlockSnapshot>,
        old_anchor: Option<crate::transcript_scroll::LogicalAnchor>,
    ) -> Result<(), TranscriptIntegrationError> {
        let identity =
            TranscriptIdentity::from_replay(self.turn_seeds.iter().map(|turn| turn.replay))?;
        let extents = self
            .layout
            .as_ref()
            .map(|layout| {
                layout
                    .placements()
                    .iter()
                    .map(|placement| (placement.id(), placement.extent()))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let mut row = 0;
        let turns = self
            .turn_seeds
            .iter()
            .map(|turn| {
                let height = (0..turn.replay.block_count)
                    .filter_map(|index| extents.get(&turn.replay.block_id(index)))
                    .fold(0_usize, |height, extent| {
                        height.saturating_add(extent_to_rows(*extent))
                    })
                    .max(1);
                let projected = TimelineTurn::from_replay(
                    turn.replay,
                    row,
                    height,
                    turn.status,
                    turn.lifecycle,
                )
                .with_label(turn.label.clone());
                row = row.saturating_add(height);
                projected
            })
            .collect::<Vec<_>>();
        self.work_metrics.turns_reflowed =
            self.work_metrics.turns_reflowed.saturating_add(turns.len());
        self.timeline
            .update_document(identity.clone(), turns.clone())?;
        self.identity = identity;
        self.turns = turns;
        let max_scroll = self
            .layout
            .as_ref()
            .map_or(0.0, TranscriptLayout::max_scroll);
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
        self.refresh_reflowed_view(damaged_block);
        Ok(())
    }

    fn reflow_tail_block(
        &mut self,
        snapshot: BlockSnapshot,
        old_anchor: Option<crate::transcript_scroll::LogicalAnchor>,
    ) -> Result<(), TranscriptIntegrationError> {
        let seed = self
            .turn_seeds
            .last()
            .ok_or(TranscriptIntegrationError::MissingBlock(snapshot.id))?;
        let layout = self
            .layout
            .as_ref()
            .ok_or(TranscriptIntegrationError::MissingBlock(snapshot.id))?;
        let height = (0..seed.replay.block_count)
            .filter_map(|index| {
                let id = seed.replay.block_id(index);
                layout
                    .placements()
                    .iter()
                    .find(|placement| placement.id() == id)
                    .map(|placement| extent_to_rows(placement.extent()))
            })
            .fold(0usize, usize::saturating_add)
            .max(1);
        let row = self.turns.last().map_or(0, |turn| turn.row);
        let turn = TimelineTurn::from_replay(seed.replay, row, height, seed.status, seed.lifecycle)
            .with_label(seed.label.clone());
        self.timeline.update_last_turn(turn.clone())?;
        if let Some(last) = self.turns.last_mut() {
            *last = turn.clone();
        }
        self.work_metrics.turns_reflowed = self.work_metrics.turns_reflowed.saturating_add(1);

        let max_scroll = layout.max_scroll();
        self.follow.content_changed(max_scroll)?;
        self.scroll_top = if self.follow.is_following() {
            max_scroll
        } else {
            old_anchor
                .and_then(|anchor| anchor.resolve(layout).ok())
                .unwrap_or(self.scroll_top.min(max_scroll))
        };
        self.transition = None;
        self.refresh_tail_view(snapshot, turn);
        Ok(())
    }

    fn refresh_damaged_block(&mut self, snapshot: BlockSnapshot) {
        let block_id = snapshot.id;
        if self
            .view
            .blocks
            .last()
            .is_some_and(|block| block.id == block_id)
        {
            if let Some(block) = self.view.blocks.last_mut() {
                *block = snapshot;
            }
        } else if let Some(block) = self
            .view
            .blocks
            .iter_mut()
            .find(|block| block.id == block_id)
        {
            *block = snapshot;
        }
        if let Some(snapshot) = self.lifecycle.snapshot(block_id) {
            if let Some(lifecycle) = self
                .view
                .lifecycle
                .iter_mut()
                .find(|lifecycle| lifecycle.block_id == block_id)
            {
                *lifecycle = snapshot;
            }
        }
        self.view.cache_entries = self.cache.len();
    }

    fn refresh_reflowed_view(&mut self, damaged_block: Option<BlockSnapshot>) {
        if let Some(snapshot) = damaged_block {
            if let Some(block) = self
                .view
                .blocks
                .iter_mut()
                .find(|block| block.id == snapshot.id)
            {
                *block = snapshot;
            } else {
                self.view.blocks.push(snapshot);
            }
        }
        let regions = layout_for_rect(self.viewport, self.shell_state);
        let rows = if self.scroll_top.is_sign_negative() {
            0
        } else {
            super::super::scroll_rows(self.scroll_top)
        };
        let timeline = crate::transcript_timeline::geometry_for_rect(
            regions.transcript_viewport,
            &self.turns,
            rows,
        );
        self.view.viewport = self.viewport;
        self.view.identity = self.identity.clone();
        self.view.turns.clone_from(&self.turns);
        self.view.hit_map = TimelineHitMap::from_geometry(&timeline);
        self.view.timeline = timeline;
        self.view.response_position = self.timeline.response_position();
        self.view.layout.clone_from(&self.layout);
        self.view.scroll_top = self.scroll_top;
        self.view.follow = self.follow;
        self.view.screen.clone_from(&self.screen);
        self.view.lifecycle = self.lifecycle.snapshots();
        self.view.cache_entries = self.cache.len();
    }

    fn refresh_tail_view(&mut self, snapshot: BlockSnapshot, turn: TimelineTurn) {
        if self
            .view
            .blocks
            .last()
            .is_some_and(|block| block.id == snapshot.id)
        {
            if let Some(block) = self.view.blocks.last_mut() {
                *block = snapshot.clone();
            }
        } else if let Some(block) = self
            .view
            .blocks
            .iter_mut()
            .find(|block| block.id == snapshot.id)
        {
            *block = snapshot.clone();
        }
        if let Some(last) = self.view.turns.last_mut() {
            *last = turn;
        }
        if let (Some(view_layout), Some(layout)) = (self.view.layout.as_mut(), self.layout.as_ref())
        {
            if let Some(placement) = layout.placements().last() {
                let _ = view_layout.update_extent(placement.id(), placement.extent());
            }
        }
        let regions = layout_for_rect(self.viewport, self.shell_state);
        let rows = super::super::scroll_rows(self.scroll_top.max(0.0));
        let first_visible = self.turns.partition_point(|candidate| candidate.row < rows);
        let visible_turns = &self.turns[first_visible..];
        let timeline = crate::transcript_timeline::geometry_for_rect(
            regions.transcript_viewport,
            visible_turns,
            rows,
        );
        self.view.hit_map = TimelineHitMap::from_geometry(&timeline);
        self.view.timeline = timeline;
        self.view.response_position = self.timeline.response_position();
        self.view.scroll_top = self.scroll_top;
        self.view.follow = self.follow;
        if let Some(lifecycle) = self.lifecycle.snapshot(snapshot.id) {
            if self
                .view
                .lifecycle
                .last()
                .is_some_and(|current| current.block_id == snapshot.id)
            {
                if let Some(last) = self.view.lifecycle.last_mut() {
                    *last = lifecycle;
                }
            }
        }
        self.view.cache_entries = self.cache.len();
    }
}

fn ordered_placements(turns: &[TurnSeed], extents: &HashMap<BlockId, f64>) -> Vec<(BlockId, f64)> {
    turns
        .iter()
        .flat_map(|turn| {
            (0..turn.replay.block_count).filter_map(|index| {
                let id = turn.replay.block_id(index);
                extents.get(&id).copied().map(|extent| (id, extent))
            })
        })
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "layout extents originate from positive terminal row counts"
)]
fn extent_to_rows(extent: f64) -> usize {
    extent.max(1.0).round() as usize
}
