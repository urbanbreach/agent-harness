use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TRANSCRIPT_CACHE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static TRANSCRIPT_RENDER_KEY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
    static TRANSCRIPT_RENDER_EXPANSION_HASH_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
pub(crate) struct TranscriptRenderCache {
    instance_id: u64,
    epoch: u64,
    key_cache: Cell<Option<(u64, u64)>>,
    selection_key_cache: Cell<Option<(u64, u64)>>,
}

impl Default for TranscriptRenderCache {
    fn default() -> Self {
        Self {
            instance_id: NEXT_TRANSCRIPT_CACHE_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
            epoch: 0,
            key_cache: Cell::new(None),
            selection_key_cache: Cell::new(None),
        }
    }
}

impl TranscriptRenderCache {
    pub(in crate::app) fn instance_id(&self) -> u64 {
        self.instance_id
    }

    pub(in crate::app) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(in crate::app) fn cache_key(&self, stamp: u64, build_key: impl FnOnce() -> u64) -> u64 {
        if let Some((cached_stamp, cached_key)) = self.key_cache.get() {
            if cached_stamp == stamp {
                return cached_key;
            }
        }

        let key = build_key();
        self.key_cache.set(Some((stamp, key)));

        #[cfg(test)]
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(|count| count.set(count.get().saturating_add(1)));

        key
    }

    pub(in crate::app) fn selection_cache_key(
        &self,
        stamp: u64,
        build_key: impl FnOnce() -> u64,
    ) -> u64 {
        if let Some((cached_stamp, cached_key)) = self.selection_key_cache.get() {
            if cached_stamp == stamp {
                return cached_key;
            }
        }

        let key = build_key();
        self.selection_key_cache.set(Some((stamp, key)));

        key
    }

    pub(in crate::app) fn bump_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.key_cache.set(None);
        self.selection_key_cache.set(None);
    }

    #[cfg(test)]
    pub(in crate::app) fn reset_build_metrics_for_test() {
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(|count| count.set(0));
        TRANSCRIPT_RENDER_EXPANSION_HASH_COUNT.with(|count| count.set(0));
    }

    #[cfg(test)]
    pub(in crate::app) fn build_count_for_test() -> usize {
        TRANSCRIPT_RENDER_KEY_BUILD_COUNT.with(Cell::get)
    }

    #[cfg(test)]
    pub(in crate::app) fn note_expansion_hash_for_test() {
        TRANSCRIPT_RENDER_EXPANSION_HASH_COUNT
            .with(|count| count.set(count.get().saturating_add(1)));
    }

    #[cfg(test)]
    pub(in crate::app) fn expansion_hash_count_for_test() -> usize {
        TRANSCRIPT_RENDER_EXPANSION_HASH_COUNT.with(Cell::get)
    }
}
