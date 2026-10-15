//! Per-counter LRU of exact native fragment counts.

use std::collections::HashMap;

/// Most entries kept.
const MAX_ENTRIES: usize = 256;
/// Largest total weight kept: 512 KiB.
const MAX_WEIGHT: usize = 512 * 1024;
/// Fragments longer than this many UTF-16 units are never cached.
const MAX_UNITS: usize = 16384;
/// Fixed weight added to every entry.
const ENTRY_OVERHEAD: usize = 64;

/// UTF-16 length of `text`, or `None` once it is above the caching cutoff.
fn cacheable_units(text: &str) -> Option<usize> {
    // utf16 length is at least bytes/3, so long texts are rejected without a scan.
    if text.len() > MAX_UNITS * 3 {
        return None;
    }
    let units = text.encode_utf16().count();
    (units <= MAX_UNITS).then_some(units)
}

#[derive(Debug, Default)]
pub(super) struct FragmentCache {
    entries: HashMap<String, (u32, u64)>,
    weight: usize,
    tick: u64,
}

impl FragmentCache {
    pub(super) fn get(&mut self, text: &str) -> Option<u32> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.get_mut(text)?;
        entry.1 = tick;
        Some(entry.0)
    }

    /// Stores an exact count unless the fragment is above the caching cutoff.
    pub(super) fn insert(&mut self, text: &str, count: u32) {
        let Some(units) = cacheable_units(text) else {
            return;
        };
        let weight = 2 * units + ENTRY_OVERHEAD;
        if weight > MAX_WEIGHT || self.entries.contains_key(text) {
            return;
        }
        while self.entries.len() >= MAX_ENTRIES || self.weight + weight > MAX_WEIGHT {
            if !self.evict_oldest() {
                break;
            }
        }
        self.tick += 1;
        self.entries.insert(text.to_owned(), (count, self.tick));
        self.weight += weight;
    }

    fn evict_oldest(&mut self) -> bool {
        let Some(oldest) = self
            .entries
            .iter()
            .min_by_key(|(_, (_, tick))| *tick)
            .map(|(text, _)| text.clone())
        else {
            return false;
        };
        if self.entries.remove(&oldest).is_some() {
            let units = cacheable_units(&oldest).unwrap_or(oldest.len());
            self.weight = self.weight.saturating_sub(2 * units + ENTRY_OVERHEAD);
        }
        true
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}
