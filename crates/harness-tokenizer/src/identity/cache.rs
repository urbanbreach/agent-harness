use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// Process-wide memo that is cleared whole when it reaches capacity before an
/// insertion.
pub(super) struct BoundedCache<V> {
    capacity: usize,
    entries: Mutex<HashMap<String, V>>,
}

impl<V: Clone> BoundedCache<V> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn get_or_insert_with(&self, key: String, compute: impl FnOnce() -> V) -> V {
        if let Some(value) = self.lock().get(&key) {
            return value.clone();
        }
        let value = compute();
        let mut entries = self.lock();
        if entries.len() >= self.capacity {
            entries.clear();
        }
        entries.insert(key, value.clone());
        value
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, V>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
