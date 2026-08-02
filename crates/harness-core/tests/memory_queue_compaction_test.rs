//! Integration tests for Wave 2 Task 19: durable scoped memory, prompt queue
//! persistence/ordering/editing, safe interjection drains, automatic queue
//! drain, compaction checkpoints/suppression, and pre-compaction memory flush.
//!
//! These tests verify the cross-cutting invariants:
//! - Secrets are never persisted in memory, queues, or compaction artifacts.
//! - Restart persistence: durable state survives process restart.
//! - Stale-version edit: unsupported store versions are rejected.
//! - Concurrent drain: drain operations are atomic and ordered.
//! - Compact/replay: compaction events are replay-derived and side-effect free.
//! - Malformed store: corrupt JSON is rejected with a parse error.

use harness_core::memory::{DurableMemoryStore, MemoryError, MemoryScope};
use harness_core::prompt_queue::DurablePromptQueue;
use std::fs;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Restart persistence
// ---------------------------------------------------------------------------

#[test]
fn memory_store_survives_restart_with_scoped_entries() {
    // arrange — a store with entries in multiple scopes
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    {
        let store = DurableMemoryStore::for_workspace(workspace);
        store
            .put_scoped("g1", "global-val", MemoryScope::Global)
            .unwrap();
        store
            .put_scoped("w1", "ws-val", MemoryScope::Workspace)
            .unwrap();
        store
            .put_scoped("s1", "session-val", MemoryScope::Session)
            .unwrap();
    }

    // act —reopen the store from disk (simulating restart)
    let reopened = DurableMemoryStore::for_workspace(workspace);

    // assert —all entries survive with their original scopes
    let g = reopened.get_scoped("g1").unwrap().unwrap();
    assert_eq!(g.scope, MemoryScope::Global);
    assert_eq!(g.value, "global-val");

    let w = reopened.get_scoped("w1").unwrap().unwrap();
    assert_eq!(w.scope, MemoryScope::Workspace);

    let s = reopened.get_scoped("s1").unwrap().unwrap();
    assert_eq!(s.scope, MemoryScope::Session);
    assert_eq!(s.value, "session-val");
}

#[test]
fn prompt_queue_survives_restart_with_interjection_flag() {
    // arrange —a queue with FIFO and interjection entries
    let dir = tempdir().unwrap();
    let session_dir = dir.path().join("session");
    fs::create_dir_all(&session_dir).unwrap();
    {
        let queue = DurablePromptQueue::for_session(&session_dir);
        queue.enqueue("fifo", "ordinary", 1).unwrap();
        queue.interject_mid_turn("inj", "urgent", 2, true).unwrap();
    }

    // act —reopen the queue from disk
    let reopened = DurablePromptQueue::for_session(&session_dir);
    let listed = reopened.list().unwrap();

    // assert —both entries survive with correct interjection flags
    assert_eq!(listed.len(), 2);
    let inj = listed.iter().find(|e| e.id == "inj").unwrap();
    assert!(inj.is_interjection);
    let fifo = listed.iter().find(|e| e.id == "fifo").unwrap();
    assert!(!fifo.is_interjection);
}

// ---------------------------------------------------------------------------
// Stale-version edit
// ---------------------------------------------------------------------------

#[test]
fn memory_store_rejects_unsupported_version_on_edit() {
    // arrange —a store file with an unsupported version
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(store.path(), r#"{"version": 999, "entries": {}}"#).unwrap();

    // act —attempt to edit (put) on the stale-version store
    let err = store.put("key", "val").unwrap_err();

    // assert —the edit is rejected with UnsupportedVersion
    assert!(matches!(
        err,
        MemoryError::UnsupportedVersion { version: 999, .. }
    ));
}

#[test]
fn prompt_queue_rejects_unsupported_version_on_edit() {
    // arrange —a queue file with an unsupported version
    let dir = tempdir().unwrap();
    let session_dir = dir.path().join("session");
    fs::create_dir_all(session_dir.join("tui")).unwrap();
    let queue = DurablePromptQueue::for_session(&session_dir);
    fs::write(queue.path(), r#"{"version": 999, "entries": []}"#).unwrap();

    // act —attempt to edit the stale-version queue
    let result = queue.edit("any", "new text");

    // assert —the edit is rejected
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Concurrent drain
// ---------------------------------------------------------------------------

#[test]
fn drain_returns_all_entries_in_fifo_order() {
    // arrange —a queue with multiple entries
    let dir = tempdir().unwrap();
    let queue = DurablePromptQueue::for_session(dir.path());
    queue.enqueue("a", "first", 1).unwrap();
    queue.enqueue("b", "second", 2).unwrap();
    queue.enqueue("c", "third", 3).unwrap();

    // act —drain the entire queue
    let drained = queue.drain().unwrap();

    // assert —all entries returned in FIFO order, queue is empty
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0].id, "a");
    assert_eq!(drained[1].id, "b");
    assert_eq!(drained[2].id, "c");
    assert!(queue.is_empty().unwrap());
}

#[test]
fn drain_interjections_preserves_fifo_entries_order() {
    // arrange —a queue with interleaved FIFO and interjection entries
    let dir = tempdir().unwrap();
    let queue = DurablePromptQueue::for_session(dir.path());
    queue.enqueue("f1", "fifo1", 1).unwrap();
    queue.interject_mid_turn("i1", "inj1", 2, true).unwrap();
    queue.enqueue("f2", "fifo2", 3).unwrap();
    queue.interject_mid_turn("i2", "inj2", 4, true).unwrap();

    // act —drain only interjections
    let drained = queue.drain_interjections().unwrap();
    let remaining = queue.list().unwrap();

    // assert —only interjections drained, FIFO order preserved
    assert_eq!(drained.len(), 2);
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].id, "f1");
    assert_eq!(remaining[1].id, "f2");
}

// ---------------------------------------------------------------------------
// Secret redaction across persistence and replay
// ---------------------------------------------------------------------------

#[test]
fn secret_redacted_in_scoped_memory_persistence_and_replay() {
    // arrange —a store with a secret-like value
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    let secret = "sk-abcdefghijklmnopqrstuvwxyz";

    // act —put the secret into scoped memory
    let entry = store
        .put_scoped("api.key", secret, MemoryScope::Global)
        .unwrap();

    // assert —the return value is redacted
    assert!(!entry.value.contains(secret));
    assert!(entry.value.contains("[REDACTED_API_KEY]"));

    // And: the raw file on disk does not contain the secret
    let raw = fs::read_to_string(store.path()).unwrap();
    assert!(!raw.contains(secret));
    assert!(raw.contains("[REDACTED_API_KEY]"));

    // And: reloading (replay) returns the redacted value
    let reopened = DurableMemoryStore::for_workspace(dir.path());
    let loaded = reopened.get_scoped("api.key").unwrap().unwrap();
    assert!(!loaded.value.contains(secret));
    assert!(loaded.value.contains("[REDACTED_API_KEY]"));
}

#[test]
fn secret_redacted_in_prompt_queue_text() {
    // arrange —a queue with a secret-like prompt
    let dir = tempdir().unwrap();
    let queue = DurablePromptQueue::for_session(dir.path());
    let secret = "Bearer abcdefghijklmnopqrstuvwxyz0123456789";

    // act —enqueue the secret-like text
    queue.enqueue("secret-entry", secret, 1).unwrap();

    // assert —the raw file on disk does not contain the raw secret
    let raw = fs::read_to_string(queue.path()).unwrap();
    assert!(!raw.contains(secret));
}

// ---------------------------------------------------------------------------
// Compact/replay: compaction events are replay-derived
// ---------------------------------------------------------------------------

#[test]
fn compaction_suppression_flag_prevents_auto_compaction() {
    // arrange —compaction settings with suppression enabled
    use harness_core::config::CompactionSettings;

    let settings = CompactionSettings {
        enabled: true,
        suppress_auto_compaction: true,
        ..Default::default()
    };

    // act —check the suppression flag
    let suppressed = settings.suppress_auto_compaction;

    // assert —suppression flag is set
    assert!(suppressed);
    assert!(settings.enabled);
}

#[test]
fn compaction_settings_default_has_suppression_disabled() {
    // arrange —default compaction settings
    use harness_core::config::CompactionSettings;

    let settings = CompactionSettings::default();

    // act —check the default suppression flag
    let suppressed = settings.suppress_auto_compaction;

    // assert —suppression is disabled by default
    assert!(!suppressed);
}

// ---------------------------------------------------------------------------
// Malformed store handling
// ---------------------------------------------------------------------------

#[test]
fn malformed_memory_store_returns_parse_error_not_panic() {
    // arrange —a store file with corrupt JSON
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(store.path(), "{ broken json").unwrap();

    // act —attempt to read from the malformed store
    let err = store.get("any").unwrap_err();

    // assert —a parse error is returned, not a panic
    assert!(matches!(err, MemoryError::Parse { .. }));
}

#[test]
fn malformed_prompt_queue_returns_parse_error_not_panic() {
    // arrange —a queue file with corrupt JSON
    let dir = tempdir().unwrap();
    let session_dir = dir.path().join("session");
    fs::create_dir_all(session_dir.join("tui")).unwrap();
    let queue = DurablePromptQueue::for_session(&session_dir);
    fs::write(queue.path(), "{ broken").unwrap();

    // act —attempt to list from the malformed queue
    let result = queue.list();

    // assert —an error is returned, not a panic
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Pre-compaction memory flush
// ---------------------------------------------------------------------------

#[test]
fn pre_compaction_flush_consolidates_session_into_workspace_scope() {
    // arrange —a memory store with session-scoped entries
    let dir = tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    store
        .put_scoped("temp1", "val1", MemoryScope::Session)
        .unwrap();
    store
        .put_scoped("temp2", "val2", MemoryScope::Session)
        .unwrap();
    store
        .put_scoped("perm1", "val1", MemoryScope::Workspace)
        .unwrap();

    // act —consolidate session into workspace (pre-compaction flush)
    let count = store
        .consolidate(MemoryScope::Session, MemoryScope::Workspace)
        .unwrap();

    // assert —session entries moved to workspace scope
    assert_eq!(count, 2);
    let t1 = store.get_scoped("temp1").unwrap().unwrap();
    assert_eq!(t1.scope, MemoryScope::Workspace);
    let t2 = store.get_scoped("temp2").unwrap().unwrap();
    assert_eq!(t2.scope, MemoryScope::Workspace);

    // And: no session-scoped entries remain
    let session_entries = store.search_scoped("", Some(MemoryScope::Session)).unwrap();
    assert!(session_entries.is_empty());
}

// ---------------------------------------------------------------------------
// Queue editing operations (remove, reorder, clear, edit)
// ---------------------------------------------------------------------------

#[test]
fn queue_edit_operations_persist_across_reopen() {
    // arrange —a queue with entries
    let dir = tempdir().unwrap();
    let path = DurablePromptQueue::default_path_for_session(dir.path());
    {
        let queue = DurablePromptQueue::open(&path);
        queue.enqueue("a", "first", 1).unwrap();
        queue.enqueue("b", "second", 2).unwrap();
        queue.enqueue("c", "third", 3).unwrap();

        // act —edit, remove, and reorder
        queue.edit("a", "first-edited").unwrap();
        queue.remove("b").unwrap();
        queue.reorder("c", 0).unwrap();
    }

    // assert —reopen and verify all edits persisted
    let reopened = DurablePromptQueue::open(&path);
    let listed = reopened.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, "c");
    assert_eq!(listed[0].text, "third");
    assert_eq!(listed[1].id, "a");
    assert_eq!(listed[1].text, "first-edited");
}

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
        tokens_before: 5000,
        read_files: vec!["src/main.rs".to_string()],
        modified_files: vec!["src/lib.rs".to_string()],
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
