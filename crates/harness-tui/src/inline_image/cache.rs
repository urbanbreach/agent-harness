use std::collections::HashMap;

use super::capability::GraphicsProtocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageCacheKey {
    pub source_hash: u64,
    pub target_width: u16,
    pub target_height: u16,
    pub protocol: GraphicsProtocol,
}

impl ImageCacheKey {
    pub fn new(source: &[u8], width: u16, height: u16, protocol: GraphicsProtocol) -> Self {
        let source_hash = source.iter().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        Self {
            source_hash,
            target_width: width,
            target_height: height,
            protocol,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedImage {
    pub key: ImageCacheKey,
    pub rendered_bytes: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub created_at_tick: u64,
}

pub struct ImageCache {
    entries: HashMap<ImageCacheKey, CachedImage>,
    capacity: usize,
    total_bytes: usize,
    max_bytes: usize,
}

impl ImageCache {
    pub fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            total_bytes: 0,
            max_bytes,
        }
    }

    pub fn get(&self, key: &ImageCacheKey) -> Option<&CachedImage> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, image: CachedImage) {
        let key = image.key;
        let image_bytes = image.rendered_bytes.len();
        if let Some(previous) = self.entries.remove(&key) {
            self.total_bytes -= previous.rendered_bytes.len();
        }
        while (!self.entries.is_empty()
            && (self.entries.len() >= self.capacity
                || self.total_bytes.saturating_add(image_bytes) > self.max_bytes))
            || (self.entries.is_empty() && image_bytes > self.max_bytes)
        {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.created_at_tick)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                if let Some(removed) = self.entries.remove(&oldest) {
                    self.total_bytes -= removed.rendered_bytes.len();
                }
            } else {
                return;
            }
        }
        if self.capacity > 0 && image_bytes <= self.max_bytes {
            self.total_bytes += image_bytes;
            self.entries.insert(key, image);
        }
    }

    pub fn invalidate_resize(&mut self) {
        self.clear();
    }

    pub fn invalidate_protocol(&mut self, protocol: GraphicsProtocol) {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, image)| (image.key.protocol == protocol).then_some(*key))
            .collect();
        for key in keys {
            if let Some(image) = self.entries.remove(&key) {
                self.total_bytes -= image.rendered_bytes.len();
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_bytes = 0;
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new(16, 64 * 1024 * 1024)
    }
}
