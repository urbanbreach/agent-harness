use super::composite::TranscriptComposite;
use super::invalidation::{BlockSeed, CacheInvalidation, TranscriptEvent, TurnSeed};
use super::TranscriptIntegrationError;

mod reflow;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TranscriptWorkMetrics {
    pub activities_touched: usize,
    pub blocks_touched: usize,
    pub blocks_measured: usize,
    pub events_reprojected: usize,
    pub full_rebuilds: usize,
    pub placements_reflowed: usize,
    pub turns_reflowed: usize,
}

impl TranscriptComposite {
    pub fn sync_turn(
        &mut self,
        events: Vec<TranscriptEvent>,
    ) -> Result<(), TranscriptIntegrationError> {
        let mut events = events.into_iter();
        let Some(TranscriptEvent::TurnStarted(seed)) = events.next() else {
            return Err(TranscriptIntegrationError::IncrementalSync(
                "turn sync must start with TurnStarted",
            ));
        };
        let blocks = events
            .map(|event| match event {
                TranscriptEvent::BlockCreated(block) => Ok(block),
                _ => Err(TranscriptIntegrationError::IncrementalSync(
                    "turn sync accepts only canonical block events",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let updates = self.turn_updates(&seed, &blocks)?;
        for update in updates {
            self.apply(update)?;
        }
        self.work_metrics.activities_touched =
            self.work_metrics.activities_touched.saturating_add(1);
        Ok(())
    }

    pub const fn work_metrics(&self) -> TranscriptWorkMetrics {
        self.work_metrics
    }

    pub fn reset_work_metrics(&mut self) {
        self.work_metrics = TranscriptWorkMetrics::default();
    }

    pub(super) fn apply_incremental_event(
        &mut self,
        event: &TranscriptEvent,
    ) -> Result<(), TranscriptIntegrationError> {
        match event {
            TranscriptEvent::TurnStarted(seed) => {
                if self
                    .turn_seeds
                    .iter()
                    .any(|turn| turn.turn_id() == seed.turn_id())
                {
                    return Err(TranscriptIntegrationError::IncrementalSync(
                        "turn already exists",
                    ));
                }
                self.turn_seeds.push(seed.clone());
                self.reflow_projected_state(None)
            }
            TranscriptEvent::TurnUpdated(seed) => {
                let turn = self
                    .turn_seeds
                    .iter_mut()
                    .find(|turn| turn.turn_id() == seed.turn_id())
                    .ok_or(TranscriptIntegrationError::MissingTurn(seed.turn_id()))?;
                *turn = seed.clone();
                self.reflow_projected_state(None)
            }
            TranscriptEvent::TurnStatus {
                turn_id,
                status,
                lifecycle,
            } => {
                let turn = self
                    .turn_seeds
                    .iter_mut()
                    .find(|turn| turn.turn_id() == *turn_id)
                    .ok_or(TranscriptIntegrationError::MissingTurn(*turn_id))?;
                turn.status = *status;
                turn.lifecycle = *lifecycle;
                self.reflow_projected_state(None)
            }
            TranscriptEvent::BlockCreated(seed) => {
                if !self
                    .turn_seeds
                    .iter()
                    .any(|turn| turn.turn_id() == seed.turn_id)
                {
                    return Err(TranscriptIntegrationError::MissingTurn(seed.turn_id));
                }
                if self.blocks.get(seed.id).is_some() {
                    return Err(
                        crate::transcript_blocks::BlockStoreError::DuplicateBlock(seed.id).into(),
                    );
                }
                self.cache.apply(CacheInvalidation::BlockRemoved(seed.id));
                self.blocks.insert(
                    seed.id,
                    seed.kind,
                    seed.lifecycle,
                    seed.content.clone(),
                    seed.raw.clone(),
                );
                self.lifecycle.set_block(seed.id, seed.kind, seed.lifecycle);
                self.remeasure_block(seed.id)
            }
            TranscriptEvent::BlockAppended { id, content } => {
                self.cache.apply(CacheInvalidation::BlockAppended(*id));
                self.blocks.append(*id, content)?;
                self.remeasure_block(*id)
            }
            TranscriptEvent::BlockLifecycle { id, lifecycle } => {
                let from = self
                    .blocks
                    .get(*id)
                    .ok_or(TranscriptIntegrationError::MissingBlock(*id))?
                    .lifecycle();
                self.cache.apply(CacheInvalidation::LifecycleTransition {
                    block_id: *id,
                    from,
                    to: *lifecycle,
                });
                self.blocks.set_lifecycle(*id, *lifecycle)?;
                let block = self
                    .blocks
                    .get(*id)
                    .ok_or(TranscriptIntegrationError::MissingBlock(*id))?;
                self.lifecycle
                    .set_block(*id, block.kind(), block.lifecycle());
                self.remeasure_block(*id)
            }
            TranscriptEvent::FoldToggled { id } => {
                self.cache.apply(CacheInvalidation::FoldToggled(*id));
                self.blocks.toggle_fold(*id)?;
                self.remeasure_block(*id)
            }
            TranscriptEvent::BlockRemoved { .. }
            | TranscriptEvent::TurnRemoved { .. }
            | TranscriptEvent::Compacted { .. } => {
                self.cache.apply(self.invalidation_for(event));
                self.rebuild()
            }
        }
    }

    fn turn_updates(
        &self,
        seed: &TurnSeed,
        blocks: &[BlockSeed],
    ) -> Result<Vec<TranscriptEvent>, TranscriptIntegrationError> {
        let Some(previous) = self
            .turn_seeds
            .iter()
            .find(|turn| turn.turn_id() == seed.turn_id())
        else {
            let mut updates = Vec::with_capacity(blocks.len().saturating_add(1));
            updates.push(TranscriptEvent::TurnStarted(seed.clone()));
            updates.extend(blocks.iter().cloned().map(TranscriptEvent::BlockCreated));
            return Ok(updates);
        };

        if blocks.len() < usize::try_from(previous.replay.block_count).unwrap_or(usize::MAX) {
            return Err(TranscriptIntegrationError::IncrementalSync(
                "turn sync cannot remove blocks incrementally",
            ));
        }
        let mut updates = Vec::new();
        if previous != seed {
            updates.push(TranscriptEvent::TurnUpdated(seed.clone()));
        }
        for block in blocks {
            let Some(current) = self.blocks.get(block.id) else {
                updates.push(TranscriptEvent::BlockCreated(block.clone()));
                continue;
            };
            if current.kind() != block.kind || current.raw() != block.raw.as_ref() {
                return Err(TranscriptIntegrationError::IncrementalSync(
                    "block shape changed",
                ));
            }
            if current.lifecycle() != block.lifecycle {
                updates.push(TranscriptEvent::BlockLifecycle {
                    id: block.id,
                    lifecycle: block.lifecycle,
                });
            }
            if current.content() == block.content {
                continue;
            }
            let Some(appended) = block.content.strip_prefix(current.content()) else {
                return Err(TranscriptIntegrationError::IncrementalSync(
                    "block content was replaced",
                ));
            };
            updates.push(TranscriptEvent::BlockAppended {
                id: block.id,
                content: appended.to_owned(),
            });
        }
        Ok(updates)
    }
}
