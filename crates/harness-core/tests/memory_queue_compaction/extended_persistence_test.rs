use super::*;

// ---------------------------------------------------------------------------
// Memory trace and release
// ---------------------------------------------------------------------------

#[test]
fn memory_trace_and_release_are_durable() {
    // arrange —a store with entries
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    store
        .put_scoped("tracked", "val", MemoryScope::Workspace)
        .unwrap();
    store
        .put_scoped("dropped", "val", MemoryScope::Workspace)
        .unwrap();

    // act —trace one entry and release another
    let traced = store.trace("tracked").unwrap();
    let released = store.release("dropped").unwrap();

    // assert —trace bumped timestamp, release removed entry
    assert!(traced.updated_at_unix_ms > 0);
    assert!(released);

    // And: changes are durable across reopen
    let reopened = DurableMemoryStore::for_workspace(dir.path());
    assert!(reopened.get_scoped("dropped").unwrap().is_none());
    let tracked = reopened.get_scoped("tracked").unwrap().unwrap();
    assert_eq!(tracked.value, "val");
}

// ---------------------------------------------------------------------------
// Memory scope filtering
// ---------------------------------------------------------------------------

#[test]
fn memory_search_scoped_filters_correctly_across_reopen() {
    // arrange — a store with entries in multiple scopes
    let dir = tempdir().unwrap();
    {
        let store = DurableMemoryStore::for_workspace(dir.path());
        store
            .put_scoped("g.key", "global", MemoryScope::Global)
            .unwrap();
        store
            .put_scoped("w.key", "workspace", MemoryScope::Workspace)
            .unwrap();
        store
            .put_scoped("s.key", "session", MemoryScope::Session)
            .unwrap();
    }

    // act —reopen and search with scope filter
    let reopened = DurableMemoryStore::for_workspace(dir.path());
    let global = reopened
        .search_scoped("key", Some(MemoryScope::Global))
        .unwrap();
    let workspace = reopened
        .search_scoped("key", Some(MemoryScope::Workspace))
        .unwrap();
    let session = reopened
        .search_scoped("key", Some(MemoryScope::Session))
        .unwrap();
    let all = reopened.search_scoped("key", None).unwrap();

    // assert —scope filters work correctly after reopen
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].key, "g.key");
    assert_eq!(workspace.len(), 1);
    assert_eq!(workspace[0].key, "w.key");
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].key, "s.key");
    assert_eq!(all.len(), 3);
}

// ---------------------------------------------------------------------------
// Automatic queue drain (drain returns entries in order, queue empty after)
// ---------------------------------------------------------------------------

#[test]
fn automatic_drain_empties_queue_and_returns_entries_in_order() {
    // arrange —a queue with entries after a turn completes
    let dir = tempdir().unwrap();
    let queue = DurablePromptQueue::for_session(dir.path());
    queue.enqueue("p1", "prompt 1", 1).unwrap();
    queue.enqueue("p2", "prompt 2", 2).unwrap();
    queue.enqueue("p3", "prompt 3", 3).unwrap();

    // act —automatic drain after turn completion
    let drained = queue.drain().unwrap();

    // assert —all entries returned in FIFO order, queue is empty
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0].id, "p1");
    assert_eq!(drained[1].id, "p2");
    assert_eq!(drained[2].id, "p3");
    assert!(queue.is_empty().unwrap());

    // And: a second drain returns nothing
    let second = queue.drain().unwrap();
    assert!(second.is_empty());
}

// ---------------------------------------------------------------------------
// Safe interjection drain (only interjections, FIFO untouched)
// ---------------------------------------------------------------------------

#[test]
fn safe_interjection_drain_does_not_touch_fifo_entries() {
    // arrange —a queue with FIFO and interjection entries
    let dir = tempdir().unwrap();
    let queue = DurablePromptQueue::for_session(dir.path());
    queue.enqueue("fifo1", "ordinary", 1).unwrap();
    queue
        .interject_mid_turn("inj1", "urgent1", 2, true)
        .unwrap();
    queue.enqueue("fifo2", "ordinary2", 3).unwrap();

    // act —safe interjection drain
    let drained = queue.drain_interjections().unwrap();

    // assert —only interjections drained
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, "inj1");
    assert!(drained[0].is_interjection);

    // And: FIFO entries remain untouched in order
    let remaining = queue.list().unwrap();
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].id, "fifo1");
    assert_eq!(remaining[1].id, "fifo2");
    assert!(!remaining.iter().any(|e| e.is_interjection));
}

// ---------------------------------------------------------------------------
// Compaction checkpoint: SessionCompaction event serves as checkpoint
// ---------------------------------------------------------------------------

#[test]
fn compaction_checkpoint_event_round_trips_through_serde() {
    // arrange —a SessionCompaction event
    use harness_core::event::{EventV1, SessionCompactionEvent};

    let event = EventV1::SessionCompaction(SessionCompactionEvent {
        agent_id: "agent_001".to_string(),
        summary: "## Goal\nCompaction checkpoint".to_string(),
        first_kept_event_seq: 42,
        first_kept_request_id: Some("req_001".to_string()),
        first_kept_entry_id: None,
        tokens_before: 5000,
        tokens_after: None,
        summary_usage: None,
        summary_provider_id: None,
        summary_model_id: None,
        read_files: vec!["src/main.rs".to_string()],
        modified_files: vec!["src/lib.rs".to_string()],
        current_intent: None,
        trigger_reason: "proactive".to_string(),
        from_hook: false,
    });

    // act —serialize and deserialize (replay path)
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: EventV1 = serde_json::from_str(&json).unwrap();

    // assert —the event round-trips correctly (checkpoint integrity)
    match deserialized {
        EventV1::SessionCompaction(payload) => {
            assert_eq!(payload.agent_id, "agent_001");
            assert_eq!(payload.first_kept_event_seq, 42);
            assert_eq!(payload.tokens_before, 5000);
            assert_eq!(payload.trigger_reason, "proactive");
            assert!(!payload.from_hook);
        }
        _ => panic!("expected SessionCompaction event"),
    }
}

// ---------------------------------------------------------------------------
// Memory list_by_scope for TUI access
// ---------------------------------------------------------------------------

#[test]
fn memory_list_by_scope_groups_entries_for_tui_display() {
    // arrange — a store with entries in multiple scopes
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    store.put_scoped("g1", "v1", MemoryScope::Global).unwrap();
    store.put_scoped("g2", "v2", MemoryScope::Global).unwrap();
    store
        .put_scoped("w1", "v1", MemoryScope::Workspace)
        .unwrap();

    // act —list grouped by scope
    let grouped = store.list_by_scope().unwrap();

    // assert —entries are grouped by scope
    assert_eq!(grouped.get(&MemoryScope::Global).unwrap().len(), 2);
    assert_eq!(grouped.get(&MemoryScope::Workspace).unwrap().len(), 1);
    assert!(!grouped.contains_key(&MemoryScope::Session));
}

// ---------------------------------------------------------------------------
// Release scope (drop all entries in a scope)
// ---------------------------------------------------------------------------

#[test]
fn release_scope_drops_all_entries_in_that_scope_only() {
    // arrange — a store with entries in multiple scopes
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    store.put_scoped("s1", "v1", MemoryScope::Session).unwrap();
    store.put_scoped("s2", "v2", MemoryScope::Session).unwrap();
    store
        .put_scoped("w1", "v1", MemoryScope::Workspace)
        .unwrap();

    // act —release all session-scoped entries
    let removed = store.release_scope(MemoryScope::Session).unwrap();

    // assert —only session entries removed
    assert_eq!(removed, 2);
    assert!(store.get_scoped("s1").unwrap().is_none());
    assert!(store.get_scoped("s2").unwrap().is_none());
    assert!(store.get_scoped("w1").unwrap().is_some());
}

// ---------------------------------------------------------------------------
// Queue clear (QueueClearShared action)
// ---------------------------------------------------------------------------

#[test]
fn queue_clear_removes_all_entries_and_persists_empty_state() {
    // arrange —a queue with entries
    let dir = tempdir().unwrap();
    let path = DurablePromptQueue::default_path_for_session(dir.path());
    let queue = DurablePromptQueue::open(&path);
    queue.enqueue("a", "first", 1).unwrap();
    queue.enqueue("b", "second", 2).unwrap();

    // act —clear the queue
    let count = queue.clear().unwrap();

    // assert —all entries removed, empty state persisted
    assert_eq!(count, 2);
    assert!(queue.is_empty().unwrap());

    // And: reopen confirms empty state
    let reopened = DurablePromptQueue::open(&path);
    assert!(reopened.is_empty().unwrap());
}
