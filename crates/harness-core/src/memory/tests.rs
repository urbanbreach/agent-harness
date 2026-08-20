use super::*;
use crate::UnwrapOrAbort;

#[test]
fn put_get_survives_store_drop_and_reload() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path();

    {
        let store = DurableMemoryStore::for_workspace(workspace);
        let written = store
            .put("project.preference", "prefer nextest")
            .unwrap_or_abort();
        assert_eq!(written.key, "project.preference");
        assert_eq!(written.value, "prefer nextest");
        assert!(store.path().is_file());
    }

    let reloaded = DurableMemoryStore::for_workspace(workspace);
    let entry = reloaded
        .get("project.preference")
        .unwrap_or_abort()
        .expect("entry should survive process restart");
    assert_eq!(entry.value, "prefer nextest");
}

#[test]
fn put_updates_existing_key() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store.put("note", "v1").unwrap_or_abort();
    store.put("note", "v2").unwrap_or_abort();
    let entry = store.get("note").unwrap_or_abort().unwrap_or_abort();
    assert_eq!(entry.value, "v2");
}

#[test]
fn search_matches_key_prefix_and_substring() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store.put("prefs.editor", "helix").unwrap_or_abort();
    store.put("prefs.shell", "zsh").unwrap_or_abort();
    store.put("todo", "fix helix config").unwrap_or_abort();

    let prefix = store.search("prefs.").unwrap_or_abort();
    assert_eq!(prefix.len(), 2);

    let substring = store.search("helix").unwrap_or_abort();
    assert_eq!(substring.len(), 2);
    assert!(substring.iter().any(|entry| entry.key == "prefs.editor"));
    assert!(substring.iter().any(|entry| entry.key == "todo"));
}

#[test]
fn put_redacts_secret_like_values() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    let written = store
        .put("creds", "token sk-abcdefghijklmnopqrstuvwxyz")
        .unwrap_or_abort();
    assert!(!written.value.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(written.value.contains("[REDACTED_API_KEY]"));

    let raw = fs::read_to_string(store.path()).unwrap_or_abort();
    assert!(!raw.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(raw.contains("[REDACTED_API_KEY]"));
}

#[test]
fn empty_key_is_rejected() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    let err = store.put("   ", "x").expect_err("empty key");
    assert!(matches!(err, MemoryError::EmptyKey));
}

// ---------------------------------------------------------------------------
// Scoped memory tests
// ---------------------------------------------------------------------------

#[test]
fn put_scoped_persists_and_reloads_with_scope() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());

    // act — put entries under different scopes
    store
        .put_scoped("global.key", "global-val", MemoryScope::Global)
        .unwrap_or_abort();
    store
        .put_scoped("ws.key", "ws-val", MemoryScope::Workspace)
        .unwrap_or_abort();
    store
        .put_scoped("session.key", "session-val", MemoryScope::Session)
        .unwrap_or_abort();

    // assert — reload from disk preserves scope
    let reloaded = DurableMemoryStore::for_workspace(temp.path());
    let g = reloaded
        .get_scoped("global.key")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(g.scope, MemoryScope::Global);
    assert_eq!(g.value, "global-val");

    let w = reloaded
        .get_scoped("ws.key")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(w.scope, MemoryScope::Workspace);

    let s = reloaded
        .get_scoped("session.key")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(s.scope, MemoryScope::Session);
}

#[test]
fn search_scoped_filters_by_scope() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store
        .put_scoped("key.a", "alpha", MemoryScope::Global)
        .unwrap_or_abort();
    store
        .put_scoped("key.b", "beta", MemoryScope::Workspace)
        .unwrap_or_abort();
    store
        .put_scoped("key.c", "gamma", MemoryScope::Session)
        .unwrap_or_abort();

    // act — search with scope filter
    let global_only = store
        .search_scoped("key", Some(MemoryScope::Global))
        .unwrap_or_abort();
    let all = store.search_scoped("key", None).unwrap_or_abort();

    // assert
    assert_eq!(global_only.len(), 1);
    assert_eq!(global_only[0].key, "key.a");
    assert_eq!(all.len(), 3);
}

#[test]
fn consolidate_merges_source_into_target_scope() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store
        .put_scoped("temp.a", "val-a", MemoryScope::Session)
        .unwrap_or_abort();
    store
        .put_scoped("temp.b", "val-b", MemoryScope::Session)
        .unwrap_or_abort();
    store
        .put_scoped("perm.c", "val-c", MemoryScope::Workspace)
        .unwrap_or_abort();

    // act — consolidate session into workspace
    let count = store
        .consolidate(MemoryScope::Session, MemoryScope::Workspace)
        .unwrap_or_abort();

    // assert — entries moved to workspace scope
    assert_eq!(count, 2);
    let a = store
        .get_scoped("temp.a")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(a.scope, MemoryScope::Workspace);
    let b = store
        .get_scoped("temp.b")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(b.scope, MemoryScope::Workspace);
    let c = store
        .get_scoped("perm.c")
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(c.scope, MemoryScope::Workspace);
}

#[test]
fn trace_bumps_timestamp_without_changing_value() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    let entry = store
        .put_scoped("tracked", "original", MemoryScope::Workspace)
        .unwrap_or_abort();
    let original_ts = entry.updated_at_unix_ms;

    // act — trace the entry
    let traced = store.trace("tracked").unwrap_or_abort();

    // assert — value unchanged, timestamp bumped
    assert_eq!(traced.value, "original");
    assert!(traced.updated_at_unix_ms > original_ts);
}

#[test]
fn trace_returns_not_found_for_missing_key() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());

    // act
    let err = store.trace("nonexistent").unwrap_err();

    // assert
    assert!(matches!(err, MemoryError::NotFound { .. }));
}

#[test]
fn release_drops_single_entry() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store.put("keep", "val").unwrap_or_abort();
    store.put("drop", "val").unwrap_or_abort();

    // act
    let removed = store.release("drop").unwrap_or_abort();
    let missing = store.release("nope").unwrap_or_abort();

    // assert
    assert!(removed);
    assert!(!missing);
    assert!(store.get("drop").unwrap_or_abort().is_none());
    assert!(store.get("keep").unwrap_or_abort().is_some());
}

#[test]
fn release_scope_drops_all_entries_in_scope() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store
        .put_scoped("s1", "v1", MemoryScope::Session)
        .unwrap_or_abort();
    store
        .put_scoped("s2", "v2", MemoryScope::Session)
        .unwrap_or_abort();
    store
        .put_scoped("w1", "v1", MemoryScope::Workspace)
        .unwrap_or_abort();

    // act
    let removed = store.release_scope(MemoryScope::Session).unwrap_or_abort();

    // assert
    assert_eq!(removed, 2);
    assert!(store.get_scoped("s1").unwrap_or_abort().is_none());
    assert!(store.get_scoped("s2").unwrap_or_abort().is_none());
    assert!(store.get_scoped("w1").unwrap_or_abort().is_some());
}

#[test]
fn list_by_scope_groups_entries() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    store
        .put_scoped("g1", "v1", MemoryScope::Global)
        .unwrap_or_abort();
    store
        .put_scoped("w1", "v1", MemoryScope::Workspace)
        .unwrap_or_abort();
    store
        .put_scoped("w2", "v2", MemoryScope::Workspace)
        .unwrap_or_abort();

    // act
    let grouped = store.list_by_scope().unwrap_or_abort();

    // assert
    assert_eq!(grouped.get(&MemoryScope::Global).unwrap_or_abort().len(), 1);
    assert_eq!(
        grouped.get(&MemoryScope::Workspace).unwrap_or_abort().len(),
        2
    );
    assert!(!grouped.contains_key(&MemoryScope::Session));
}

#[test]
fn put_scoped_redacts_secrets_before_persistence() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    let secret = "sk-abcdefghijklmnopqrstuvwxyz";

    // act
    let entry = store
        .put_scoped("api.token", secret, MemoryScope::Global)
        .unwrap_or_abort();

    // assert — redacted in return value and on disk
    assert!(!entry.value.contains(secret));
    assert!(entry.value.contains("[REDACTED_API_KEY]"));
    let raw = fs::read_to_string(store.path()).unwrap_or_abort();
    assert!(!raw.contains(secret));
    assert!(raw.contains("[REDACTED_API_KEY]"));
}

#[test]
fn malformed_store_returns_parse_error() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    std::fs::create_dir_all(store.path().parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(store.path(), "{ not valid json").unwrap_or_abort();

    // act
    let err = store.put("key", "val").unwrap_err();

    // assert
    assert!(matches!(err, MemoryError::Parse { .. }));
}

#[test]
fn unsupported_version_returns_version_error() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = DurableMemoryStore::for_workspace(temp.path());
    std::fs::create_dir_all(store.path().parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(store.path(), r#"{"version": 999, "entries": {}}"#).unwrap_or_abort();

    // act
    let err = store.get("key").unwrap_err();

    // assert
    assert!(matches!(
        err,
        MemoryError::UnsupportedVersion { version: 999, .. }
    ));
}
