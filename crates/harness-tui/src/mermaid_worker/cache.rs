use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub content_hash: u64,
    pub theme: u8,
    pub width: u16,
}

impl CacheKey {
    pub fn new(content: &str, theme: u8, width: u16) -> Self {
        let mut hash = 0xcbf29ce484222325;
        for byte in content.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Self::from_parts(hash, theme, width)
    }

    pub const fn from_parts(content_hash: u64, theme: u8, width: u16) -> Self {
        Self {
            content_hash,
            theme,
            width,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub key: CacheKey,
    pub art: String,
    pub created_at_tick: u64,
}

pub struct MermaidCache {
    entries: HashMap<CacheKey, CacheEntry>,
    capacity: usize,
}

impl MermaidCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
        }
    }

    pub fn get(&self, key: &CacheKey) -> Option<&CacheEntry> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, key: CacheKey, art: String, tick: u64) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            if let Some(oldest) = self
                .entries
                .values()
                .min_by_key(|entry| entry.created_at_tick)
            {
                let oldest_key = oldest.key;
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(
            key,
            CacheEntry {
                key,
                art,
                created_at_tick: tick,
            },
        );
    }

    pub fn invalidate_theme(&mut self, theme: u8) {
        self.entries.retain(|key, _| key.theme != theme);
    }

    pub fn invalidate_width(&mut self, width: u16) {
        self.entries.retain(|key, _| key.width != width);
    }

    pub fn invalidate_content(&mut self, content_hash: u64) {
        self.entries
            .retain(|key, _| key.content_hash != content_hash);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for MermaidCache {
    fn default() -> Self {
        Self::new(32)
    }
}
