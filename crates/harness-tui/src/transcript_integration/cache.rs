use std::collections::HashMap;

use crate::shell_geometry::{ShellState, layout_for_rect};
use crate::transcript_blocks::{BlockLifecycle, BlockSnapshot, FoldState, TranscriptBlocks};
use crate::transcript_identity::BlockId;
use crate::transcript_scroll::TranscriptLayout;
use crate::transcript_timeline::{TimelineHitMap, TimelineTurn, geometry_for_rect};
use ratatui::layout::Rect;

use super::composite::TranscriptViewModel;
use super::invalidation::{CacheInvalidation, TurnSeed};

pub const DEFAULT_CACHE_CAPACITY: usize = 256;
pub const MAX_LAYOUT_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutCacheKey {
    pub block_id: BlockId,
    pub width: u16,
    pub fold_state: FoldState,
    pub lifecycle: BlockLifecycle,
}

impl LayoutCacheKey {
    pub const fn new(
        block_id: BlockId,
        width: u16,
        fold_state: FoldState,
        lifecycle: BlockLifecycle,
    ) -> Self {
        Self {
            block_id,
            width,
            fold_state,
            lifecycle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedLayout {
    pub rows: u32,
    pub cells: u32,
    pub payload_bytes: u32,
}

impl CachedLayout {
    pub fn new(rows: usize, cells: usize, payload_bytes: usize) -> Self {
        Self {
            rows: u32::try_from(rows).map_or(u32::MAX, |value| value),
            cells: u32::try_from(cells).map_or(u32::MAX, |value| value),
            payload_bytes: u32::try_from(payload_bytes).map_or(u32::MAX, |value| value),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    layout: CachedLayout,
    last_used: u64,
}

#[derive(Debug)]
pub struct BoundedLayoutCache {
    capacity: usize,
    clock: u64,
    active_width: Option<u16>,
    entries: HashMap<LayoutCacheKey, CacheEntry>,
}

impl BoundedLayoutCache {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            clock: 0,
            active_width: None,
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &LayoutCacheKey) -> Option<CachedLayout> {
        let stamp = self.next_stamp();
        let entry = self.entries.get_mut(key)?;
        entry.last_used = stamp;
        Some(entry.layout)
    }

    pub fn insert(&mut self, key: LayoutCacheKey, layout: CachedLayout) -> bool {
        let too_large = usize::try_from(layout.payload_bytes)
            .map_or(true, |size| size > MAX_LAYOUT_PAYLOAD_BYTES);
        if self.capacity == 0 || too_large {
            return false;
        }
        if self.active_width != Some(key.width) {
            self.entries.clear();
            self.active_width = Some(key.width);
        }
        let stamp = self.next_stamp();
        self.entries.insert(
            key,
            CacheEntry {
                layout,
                last_used: stamp,
            },
        );
        self.evict_if_needed();
        true
    }

    pub fn set_width(&mut self, width: u16) -> usize {
        if self.active_width == Some(width) {
            return 0;
        }
        let removed = self.entries.len();
        self.entries.clear();
        self.active_width = Some(width);
        removed
    }

    pub fn invalidate_block(&mut self, block_id: BlockId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|key, _| key.block_id != block_id);
        before.saturating_sub(self.entries.len())
    }

    pub fn clear(&mut self) -> usize {
        let removed = self.entries.len();
        self.entries.clear();
        removed
    }

    pub fn apply(
        &mut self,
        invalidation: CacheInvalidation,
    ) -> super::invalidation::InvalidationReport {
        invalidation.apply(self)
    }

    pub fn contains(&self, key: &LayoutCacheKey) -> bool {
        self.entries.contains_key(key)
    }
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn next_stamp(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key);
            let Some(oldest) = oldest else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

impl Default for BoundedLayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn build_turns(
    turns: &[TurnSeed],
    blocks: &[BlockSnapshot],
    cache: &mut BoundedLayoutCache,
    width: u16,
) -> Vec<TimelineTurn> {
    let mut row = 0;
    turns
        .iter()
        .map(|turn| {
            let height = (0..turn.replay.block_count)
                .fold(0_usize, |height, index| {
                    let id = turn.replay.block_id(index);
                    blocks
                        .iter()
                        .find(|block| block.id == id)
                        .map_or(height, |block| {
                            height.saturating_add(
                                usize::try_from(super::cached_layout(block, width, cache).rows)
                                    .map_or(usize::MAX, |value| value),
                            )
                        })
                })
                .max(1);
            let result =
                TimelineTurn::from_replay(turn.replay, row, height, turn.status, turn.lifecycle)
                    .with_label(turn.label.clone());
            row = row.saturating_add(height);
            result
        })
        .collect()
}

pub(crate) fn build_layout(
    turns: &[TimelineTurn],
    blocks: &TranscriptBlocks,
    viewport_height: u16,
    cache: &mut BoundedLayoutCache,
    width: u16,
) -> Option<TranscriptLayout> {
    let mut placements = Vec::new();
    for turn in turns {
        for index in 0..turn.replay_turn().block_count {
            let id = turn.replay_turn().block_id(index);
            if let Some(block) = blocks.get(id) {
                let snapshot = BlockSnapshot {
                    id: block.id(),
                    kind: block.kind(),
                    lifecycle: block.lifecycle(),
                    content: block.content().to_owned(),
                    fold_state: block.fold_state(),
                    raw: block.raw().cloned(),
                };
                let rows = super::cached_layout(&snapshot, width, cache).rows.max(1);
                placements.push((id, f64::from(rows)));
            }
        }
    }
    TranscriptLayout::from_heights(placements, f64::from(viewport_height.max(1))).ok()
}

pub(crate) fn empty_view(
    viewport: Rect,
    identity: &crate::transcript_identity::TranscriptIdentity,
    screen: &crate::transcript_identity::TranscriptScreenState,
    follow: crate::transcript_scroll::FollowState,
    cache: &BoundedLayoutCache,
) -> TranscriptViewModel {
    let regions = layout_for_rect(viewport, ShellState::Streaming);
    let timeline = geometry_for_rect(regions.transcript_viewport, &[], 0);
    TranscriptViewModel {
        viewport,
        identity: identity.clone(),
        turns: Vec::new(),
        blocks: Vec::new(),
        hit_map: TimelineHitMap::from_geometry(&timeline),
        timeline,
        layout: None,
        scroll_top: 0.0,
        follow,
        screen: screen.clone(),
        lifecycle: Vec::new(),
        cache_entries: cache.len(),
    }
}
