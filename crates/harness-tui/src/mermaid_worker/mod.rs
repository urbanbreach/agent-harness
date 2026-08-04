//! Bounded Mermaid render workers: queue, cache, timeout, text fallback.

pub mod cache;
pub mod fallback;
pub mod queue;

pub use cache::{CacheKey, MermaidCache};
pub use fallback::MermaidFallback;
pub use queue::{MermaidRequest, MermaidState, MermaidWorker, WorkerError};
