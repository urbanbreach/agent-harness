#[path = "../src/mermaid_worker/mod.rs"]
mod mermaid_worker;

use mermaid_worker::{
    CacheKey, MermaidCache, MermaidFallback, MermaidState, MermaidWorker, WorkerError,
};

#[test]
fn cache_keys_hash_content_and_cache_eviction_is_oldest_first() {
    // arrange
    // act
    let key = CacheKey::new("graph", 1, 20);
    // assert
    assert_eq!(key, CacheKey::new("graph", 1, 20));
    assert_ne!(key, CacheKey::new("other", 1, 20));
    let mut cache = MermaidCache::new(2);
    let second = CacheKey::new("second", 1, 20);
    let third = CacheKey::new("third", 1, 20);
    cache.insert(key, "one".into(), 1);
    cache.insert(second, "two".into(), 2);
    cache.insert(third, "three".into(), 3);
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.len(), 2);
}

#[test]
fn cache_invalidates_each_dimension() {
    // arrange
    // act
    let mut cache = MermaidCache::new(8);
    let first = CacheKey::new("one", 1, 10);
    let second = CacheKey::new("two", 2, 10);
    cache.insert(first, "a".into(), 1);
    cache.insert(second, "b".into(), 2);
    cache.invalidate_theme(1);
    // assert
    assert!(cache.get(&first).is_none());
    cache.invalidate_width(10);
    assert!(cache.is_empty());
    cache.insert(first, "a".into(), 3);
    cache.invalidate_content(first.content_hash);
    assert!(cache.is_empty());
}

#[test]
fn fallbacks_render_bounded_text_boxes_and_errors() {
    // arrange
    // act
    let text = MermaidFallback::render_text("graph TD\nlong line", 4);
    // assert
    assert_eq!(text.as_str(), "```mermaid\ngrap\nlong\n```");
    let box_art = MermaidFallback::render_ascii_placeholder("graph TD", 20);
    assert!(box_art.as_str().starts_with("+------------------+\n|"));
    assert!(box_art.as_str().contains("graph TD"));
    let error = MermaidFallback::render_error(&"x".repeat(250));
    assert_eq!(error.as_str().chars().count(), 200);
}

#[test]
fn worker_lifecycle_enforces_limits_deadlines_and_cleanup() {
    // arrange
    let mut worker = MermaidWorker::new(1, 4);
    let first = worker.submit("graph TD".into(), 1, 20, 10);
    let second = worker.submit("graph LR".into(), 1, 20, 10);
    assert_eq!(first + 1, second);
    assert_eq!(worker.pending_count(), 2);
    let first_key = worker.start_render(first, 1).ok().unwrap();
    assert_eq!(worker.loading_count(), 1);
    assert!(matches!(
        worker.start_render(second, 1),
        Err(WorkerError::ConcurrencyLimit)
    ));
    worker.complete_render(&first_key, "svg".into(), 2);
    assert_eq!(worker.pending_count(), 1);
    assert_eq!(
        worker.state(&first_key),
        Some(&MermaidState::Rendered("svg".into()))
    );
    let duplicate = worker.submit("graph TD".into(), 1, 20, 10);
    assert_ne!(first, duplicate);
    assert!(matches!(worker.start_render(duplicate, 3), Ok(key) if key == first_key));
    assert_eq!(
        worker.state(&first_key),
        Some(&MermaidState::Rendered("svg".into()))
    );

    // act
    let failed = worker.submit("late".into(), 1, 20, 2);
    let failed_key = CacheKey::new("late", 1, 20);
    // assert
    assert!(matches!(
        worker.start_render(failed, 3),
        Err(WorkerError::DeadlineExceeded)
    ));
    assert!(matches!(
        worker.state(&failed_key),
        Some(MermaidState::Failed(_))
    ));
    let cancelled = worker.submit("cancel".into(), 1, 20, 20);
    let cancel_key = CacheKey::new("cancel", 1, 20);
    assert!(matches!(worker.start_render(cancelled, 3), Ok(key) if key == cancel_key));
    worker.cancel(cancelled);
    assert!(matches!(
        worker.start_render(cancelled, 3),
        Err(WorkerError::UnknownRequest)
    ));
    assert!(worker.state(&cancel_key).is_none());
}

#[test]
fn worker_fail_and_tick_remove_pending_without_caching_failures() {
    // arrange
    // act
    let mut worker = MermaidWorker::default();
    let failed = worker.submit("bad".into(), 1, 20, 10);
    let failed_key = worker.start_render(failed, 1).ok().unwrap();
    worker.fail_render(&failed_key, "broken");
    // assert
    assert_eq!(worker.pending_count(), 0);
    assert!(worker.state(&failed_key).is_some());
    let retry = worker.submit("bad".into(), 1, 20, 10);
    assert!(matches!(worker.start_render(retry, 1), Ok(key) if key == failed_key));
    let expired = worker.submit("expire".into(), 1, 20, 2);
    let expired_key = CacheKey::new("expire", 1, 20);
    worker.tick(3);
    assert_eq!(worker.pending_count(), 1);
    assert!(matches!(
        worker.state(&expired_key),
        Some(MermaidState::Failed(_))
    ));
    assert!(matches!(
        worker.start_render(expired, 3),
        Err(WorkerError::UnknownRequest)
    ));
}
