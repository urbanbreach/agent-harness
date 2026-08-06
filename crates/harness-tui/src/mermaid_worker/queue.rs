use std::collections::HashMap;
use std::fmt;

use super::cache::{CacheKey, MermaidCache};
use super::fallback::MermaidFallback;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidState {
    Pending,
    Loading,
    Rendered(String),
    Failed(MermaidFallback),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[rustfmt::skip]
pub struct MermaidRequest {
    pub id: u64, pub source: String, pub theme: u8, pub width: u16, pub deadline_tick: u64,
}

impl MermaidRequest {
    pub fn cache_key(&self) -> CacheKey {
        CacheKey::new(&self.source, self.theme, self.width)
    }
}

#[rustfmt::skip]
#[derive(Debug)]
pub enum WorkerError { UnknownRequest, ConcurrencyLimit, DeadlineExceeded, UnknownKey }

impl fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnknownRequest => "unknown Mermaid request",
            Self::ConcurrencyLimit => "Mermaid concurrency limit reached",
            Self::DeadlineExceeded => "Mermaid render deadline exceeded",
            Self::UnknownKey => "unknown Mermaid cache key",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for WorkerError {}

#[rustfmt::skip]
pub struct MermaidWorker {
    next_id: u64, pending: HashMap<u64, MermaidRequest>, states: HashMap<CacheKey, MermaidState>, cache: MermaidCache, max_concurrent: usize,
}

impl MermaidWorker {
    #[rustfmt::skip]
    pub fn new(max_concurrent: usize, cache_capacity: usize) -> Self {
        Self {
            next_id: 0,
            pending: HashMap::new(),
            states: HashMap::new(),
            cache: MermaidCache::new(cache_capacity),
            max_concurrent,
        }
    }

    pub fn submit(&mut self, source: String, theme: u8, width: u16, deadline_tick: u64) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.pending.insert(
            id,
            MermaidRequest {
                id,
                source,
                theme,
                width,
                deadline_tick,
            },
        );
        id
    }

    pub fn start_render(
        &mut self,
        request_id: u64,
        current_tick: u64,
    ) -> Result<CacheKey, WorkerError> {
        let request = self
            .pending
            .get(&request_id)
            .ok_or(WorkerError::UnknownRequest)?
            .clone();
        let key = request.cache_key();
        if let Some(entry) = self.cache.get(&key) {
            self.states
                .insert(key, MermaidState::Rendered(entry.art.clone()));
            return Ok(key);
        }
        if self.loading_count() >= self.max_concurrent {
            return Err(WorkerError::ConcurrencyLimit);
        }
        if current_tick > request.deadline_tick {
            self.states.insert(
                key,
                MermaidState::Failed(MermaidFallback::render_error(
                    "Mermaid render deadline exceeded",
                )),
            );
            return Err(WorkerError::DeadlineExceeded);
        }
        self.states.insert(key, MermaidState::Loading);
        Ok(key)
    }

    pub fn complete_render(&mut self, key: &CacheKey, art: String, tick: u64) {
        self.states
            .insert(*key, MermaidState::Rendered(art.clone()));
        self.cache.insert(*key, art, tick);
        self.pending
            .retain(|_, request| request.cache_key() != *key);
    }

    pub fn fail_render(&mut self, key: &CacheKey, message: &str) {
        self.states.insert(
            *key,
            MermaidState::Failed(MermaidFallback::render_error(message)),
        );
        self.pending
            .retain(|_, request| request.cache_key() != *key);
    }

    pub fn cancel(&mut self, request_id: u64) {
        if let Some(request) = self.pending.remove(&request_id) {
            self.states.remove(&request.cache_key());
        }
    }

    pub fn tick(&mut self, current_tick: u64) {
        let expired = self
            .pending
            .values()
            .filter(|request| current_tick > request.deadline_tick)
            .map(MermaidRequest::cache_key)
            .collect::<Vec<_>>();
        for key in expired {
            self.states.insert(
                key,
                MermaidState::Failed(MermaidFallback::render_error(
                    "Mermaid render deadline exceeded",
                )),
            );
            self.pending.retain(|_, request| request.cache_key() != key);
        }
    }

    pub fn state(&self, key: &CacheKey) -> Option<&MermaidState> {
        self.states.get(key)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn loading_count(&self) -> usize {
        self.states
            .values()
            .filter(|state| matches!(state, MermaidState::Loading))
            .count()
    }
}

impl Default for MermaidWorker {
    #[rustfmt::skip]
    fn default() -> Self {
        Self::new(2, 32)
    }
}
