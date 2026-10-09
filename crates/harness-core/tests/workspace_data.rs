use harness_core::{
    memory::{DurableMemoryStore, MemoryScope},
    plan::*,
};
use std::{
    fs,
    sync::{Arc, Barrier},
};

#[test]
fn edit_attribution_tracks_content_drift_without_reassigning_on_mtime_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::edit_attribution::*;
    let mut tracker = EditAttributionTracker::new();
    tracker.record_agent_tool_edit("./file", b"agent", None);
    let unchanged = tracker.observe_external("file", b"agent", Some(std::time::SystemTime::now()));
    assert_eq!(unchanged.source, EditSource::AgentTool);
    tracker.observe_external("file", b"external", None);
    tracker.observe_external("another", b"external", None);
    assert_eq!(
        tracker.summary(),
        EditAttributionSummary {
            agent_tool: 0,
            external: 1,
            drift: 1,
            total: 2
        }
    );
    tracker.record_agent_tool_edit("file", b"reconciled", None);
    assert!(!tracker.is_drifted("file"));
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("file");
    std::fs::write(&path, b"reconciled")?;
    assert_eq!(
        path_content_digest12(&path)?,
        content_digest12(b"reconciled")
    );
    let (_, hash) = hash_path_contents(&path)?;
    assert_eq!(
        tracker
            .get("file")
            .ok_or("missing attribution")?
            .content_sha256,
        hash
    );
    Ok(())
}

#[test]
fn durable_attribution_keeps_other_writers_and_reverts_to_the_recorded_agent_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::edit_attribution::*;
    let temp = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    let mut journal = EditAttributionJournal::open(temp.path(), runtime.path())?;
    let mut second = EditAttributionJournal::open(temp.path(), runtime.path())?;
    assert_eq!(fs::read_dir(temp.path())?.count(), 0);
    journal.record_agent_tool_edit("source", b"one\ntwo\n", None)?;
    second.observe_external("other", b"external", None)?;
    fs::write(temp.path().join("source"), "one\nchanged\n")?;
    journal.observe_external("source", b"one\nchanged\n", None)?;
    let summary = journal.summary();
    assert_eq!((summary.total, summary.external, summary.drift), (2, 1, 1));
    let diff = journal.diff("source")?;
    assert!(diff.drifted);
    assert!(diff.unified_diff.contains("-two\n+changed"));
    let blame = journal.blame("source")?;
    assert_eq!((blame.agent_lines, blame.external_lines), (1, 1));
    let result = journal.revert_path("source")?;
    assert_eq!(result.bytes_written, 8);
    assert_eq!(fs::read(temp.path().join("source"))?, b"one\ntwo\n");
    let mut reopened = EditAttributionJournal::open(temp.path(), runtime.path())?;
    assert!(!reopened.query("source")?.drifted);
    let bytes = fs::read(reopened.journal_path())?;
    assert!(!temp.path().join(".harness").exists());
    assert!(reopened
        .record_agent_tool_edit("../escape", b"x", None)
        .is_err());
    assert!(reopened
        .record_agent_tool_edit("secret", b"api_key=should-not-persist", None)
        .is_err());
    assert_eq!(fs::read(reopened.journal_path())?, bytes);
    let records: Vec<serde_json::Value> = std::str::from_utf8(&bytes)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    for mismatched_source in [true, false] {
        let mut bad = records.clone();
        if mismatched_source {
            bad[0]["source"] = serde_json::json!("agent_tool");
        } else {
            let mut record = bad.last().ok_or("missing baseline")?.clone();
            record["seq"] = serde_json::json!(bad.len() + 1);
            record["content_sha256"] = serde_json::json!("0".repeat(64));
            record
                .as_object_mut()
                .ok_or("record is not an object")?
                .remove("agent_snapshot_hex");
            bad.push(record);
        }
        let body = bad
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        fs::write(reopened.journal_path(), body)?;
        assert!(
            EditAttributionJournal::open(temp.path(), runtime.path()).is_err(),
            "invalid attribution source or snapshot digest accepted"
        );
    }
    fs::write(reopened.journal_path(), &bytes)?;
    #[cfg(unix)]
    {
        fs::create_dir(temp.path().join("literal"))?;
        fs::write(temp.path().join("literal/name"), b"external nested")?;
        fs::write(temp.path().join("literal\\name"), b"external literal")?;
        reopened.record_agent_tool_edit("literal\\name", b"agent literal", None)?;
        reopened.record_agent_tool_edit("literal/name", b"agent nested", None)?;
        reopened.revert_path("literal\\name")?;
        assert_eq!(
            fs::read(temp.path().join("literal\\name"))?,
            b"agent literal"
        );
        assert_eq!(
            fs::read(temp.path().join("literal/name"))?,
            b"external nested"
        );
    }
    Ok(())
}

#[test]
fn memory_updates_are_atomic_redacted_and_preserve_other_writers(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    let store = DurableMemoryStore::for_runtime(runtime.path());
    assert!(store.search("")?.is_empty());
    store.flush_existing()?;
    assert_eq!(fs::read_dir(temp.path())?.count(), 0);
    let start = Arc::new(Barrier::new(3));
    let writers: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|key| {
            let store = store.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                store.put(key, "parallel")
            })
        })
        .collect();
    start.wait();
    for writer in writers {
        writer.join().map_err(|_| "memory worker failed")??;
    }
    assert_eq!(store.search("PARALLEL")?.len(), 2);
    assert_eq!(fs::read_dir(temp.path())?.count(), 0);
    let original = store.put_scoped(
        " session ",
        r#"{"api_key":"private-value","note":"retain"}"#,
        MemoryScope::Session,
    )?;
    assert!(!fs::read_to_string(store.path())?.contains("private-value"));
    assert!(original.value.contains("retain"));
    assert!(store.trace("session")?.updated_at_unix_ms > original.updated_at_unix_ms);
    assert_eq!(
        store.consolidate(MemoryScope::Session, MemoryScope::Workspace)?,
        1
    );
    assert_eq!(
        store.search_scoped("", Some(MemoryScope::Session))?.len(),
        0
    );
    assert!(store.release("first")?);
    assert!(!store.release("absent")?);
    assert_eq!(store.release_scope(MemoryScope::Workspace)?, 2);
    fs::write(store.path(), b"{invalid}")?;
    assert!(store.put("refused", "value").is_err());
    assert_eq!(fs::read(store.path())?, b"{invalid}");
    Ok(())
}

#[test]
fn plan_projection_never_creates_paths_or_follows_symlinks(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let active = project_plan_list(temp.path(), Some("../Live//Run"));
    assert_eq!(active.len(), 1);
    assert!(!active[0].exists);
    assert_eq!(
        active[0].path,
        temp.path().join("plans/Live-Run.md").to_string_lossy()
    );
    assert_eq!(fs::read_dir(temp.path())?.count(), 0);
    let directory = temp.path().join(PLAN_DIR);
    fs::create_dir_all(&directory)?;
    fs::write(directory.join("older.md"), b"# plan")?;
    fs::write(directory.join("Live-Run.md"), b"# active")?;
    fs::write(directory.join("ignored.txt"), b"ignore")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(directory.join("older.md"), directory.join("linked.md"))?;
    let entries = project_plan_list(temp.path(), Some("Live-Run"));
    assert_eq!(entries.len(), 2);
    assert_eq!(
        (
            entries[0].slug.as_str(),
            entries[0].is_active,
            entries[0].byte_len
        ),
        ("Live-Run", true, Some(8))
    );
    #[cfg(unix)]
    {
        let linked = temp.path().join("linked-runtime");
        std::os::unix::fs::symlink(temp.path(), &linked)?;
        let linked_entries = project_plan_list(&linked, Some("Live-Run"));
        assert_eq!(linked_entries.len(), 1);
        assert!(!linked_entries[0].exists);
    }
    Ok(())
}
