#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheBounds {
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl CacheBounds {
    pub fn image_defaults() -> Self {
        Self {
            max_entries: 16,
            max_bytes: 64 * 1024 * 1024,
        }
    }
    pub fn mermaid_defaults() -> Self {
        Self {
            max_entries: 32,
            max_bytes: 8 * 1024 * 1024,
        }
    }
    pub fn transcript_defaults() -> Self {
        Self {
            max_entries: 1000,
            max_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub cache_entries: usize,
    pub cache_bytes: usize,
    pub active_workers: u8,
    pub active_subprocesses: u8,
    pub allocated_allocations: u64,
    pub tick: u64,
}

impl ResourceSnapshot {
    pub fn new() -> Self {
        Self {
            cache_entries: 0,
            cache_bytes: 0,
            active_workers: 0,
            active_subprocesses: 0,
            allocated_allocations: 0,
            tick: 0,
        }
    }
    pub fn memory_growth_rate(&self, prev: &Self, ticks_elapsed: u64) -> u64 {
        if ticks_elapsed == 0 {
            0
        } else {
            (self.cache_bytes.saturating_sub(prev.cache_bytes) as u64)
                .checked_div(ticks_elapsed)
                .unwrap_or(0)
        }
    }
    pub fn has_sustained_growth(&self, samples: &[Self]) -> bool {
        let Some(first) = samples.first() else {
            return false;
        };
        let last = samples.last().copied().unwrap_or(*first);
        samples
            .windows(2)
            .all(|pair| pair[1].cache_bytes >= pair[0].cache_bytes)
            && last.cache_bytes.saturating_sub(first.cache_bytes) > 1_048_576
    }
}

impl Default for ResourceSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ResourceBudget {
    image_bounds: CacheBounds,
    mermaid_bounds: CacheBounds,
    transcript_bounds: CacheBounds,
    max_workers: u8,
    max_subprocesses: u8,
}

impl ResourceBudget {
    pub fn defaults() -> Self {
        Self {
            image_bounds: CacheBounds::image_defaults(),
            mermaid_bounds: CacheBounds::mermaid_defaults(),
            transcript_bounds: CacheBounds::transcript_defaults(),
            max_workers: 2,
            max_subprocesses: 1,
        }
    }
    pub fn is_within_budget(&self, snapshot: &ResourceSnapshot) -> bool {
        let max_entries = self.image_bounds.max_entries
            + self.mermaid_bounds.max_entries
            + self.transcript_bounds.max_entries;
        let max_bytes = self.image_bounds.max_bytes
            + self.mermaid_bounds.max_bytes
            + self.transcript_bounds.max_bytes;
        snapshot.cache_entries <= max_entries
            && snapshot.cache_bytes <= max_bytes
            && snapshot.active_workers <= self.max_workers
            && snapshot.active_subprocesses <= self.max_subprocesses
    }
    pub fn prompt_cancellation_invariant(
        &self,
        before: &ResourceSnapshot,
        after: &ResourceSnapshot,
    ) -> bool {
        after.active_workers <= before.active_workers
            && after.active_subprocesses <= before.active_subprocesses
    }
    pub fn image_bounds(&self) -> CacheBounds {
        self.image_bounds
    }
    pub fn mermaid_bounds(&self) -> CacheBounds {
        self.mermaid_bounds
    }
    pub fn transcript_bounds(&self) -> CacheBounds {
        self.transcript_bounds
    }
}
