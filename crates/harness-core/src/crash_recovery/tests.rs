use crate::{
    crash_recovery::*,
    event::*,
    store::{EventStore, Journal},
};
use std::io::Write;

#[test]
fn recovery_preserves_complete_records_and_quarantines_only_an_incomplete_tail(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("source");
    let path = dir.join("events.jsonl");
    let writer = interrupted_journal(temp.path())?;
    let prefix = std::fs::read(&path)?;
    assert!(!inspect_previous_crash(&dir).previous_crash_detected);
    assert!(apply_crash_recovery(temp.path(), "source", true).is_err());
    assert_eq!(std::fs::read(&path)?, prefix);
    writer.close_writer()?;
    assert_live_legacy_owner_blocks(temp.path(), &prefix)?;
    let tail = br#"{"schema_version":1,"event_id":"unfinished"#;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)?
        .write_all(tail)?;
    let marker = dir.join(".writer.lock.recovering");
    let damaged = std::fs::read(&path)?;
    std::fs::create_dir(&marker)?;
    assert!(apply_crash_recovery(temp.path(), "source", true).is_err());
    assert_eq!(std::fs::read(&path)?, damaged);
    std::fs::remove_dir(&marker)?;
    std::fs::write(&marker, b"")?;
    let report = inspect_previous_crash(&dir);
    assert_eq!(
        (
            report.previous_crash_detected,
            report.recovery_marker_present
        ),
        (true, true)
    );
    assert_eq!(std::fs::read(&path)?, damaged);
    let applied = apply_crash_recovery(temp.path(), "source", true)?;
    assert_eq!(
        (
            applied.applied,
            applied.recovered,
            applied.recovery_marker_cleared
        ),
        (true, true, true)
    );
    assert!(!marker.exists());
    assert!(!inspect_previous_crash(&dir).previous_crash_detected);
    let events = crate::store::read_events(&path)?;
    let summary = crate::proj::project_run_summary(&events)?;
    assert_eq!(summary.status, crate::proj::RunStatus::Failed);
    assert!(summary.tasks_in_flight.is_empty());
    assert!(events.iter().any(|event| matches!(&event.payload,
        EventV1::EditRejected(edit) if !edit.path.contains("private-historical-value"))));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ToolCallFinished(_)))
            .count(),
        1
    );
    let repaired = std::fs::read(&path)?;
    assert!(repaired.starts_with(&prefix));
    assert_quarantined_tail(&dir, tail)?;
    assert!(!apply_crash_recovery(temp.path(), "source", true)?.applied);
    assert_eq!(std::fs::read(&path)?, repaired);
    assert_complete_corruption_is_preserved(temp.path(), &prefix)?;
    let missing = temp.path().join("missing");
    assert!(!inspect_previous_crash(&missing).previous_crash_detected);
    assert!(!missing.exists());
    Ok(())
}

fn assert_quarantined_tail(
    dir: &std::path::Path,
    tail: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let backups: Vec<_> = std::fs::read_dir(&dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".recovery-tail-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(backups[0].path())?, tail);
    Ok(())
}

fn assert_live_legacy_owner_blocks(
    root: &std::path::Path,
    prefix: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = root.join("source");
    let lock_path = dir.join(".writer.lock");
    let lock_bytes = std::fs::read(&lock_path)?;
    std::fs::write(
        &lock_path,
        serde_json::to_vec(&serde_json::json!({"pid":std::process::id()}))?,
    )?;
    assert!(!inspect_previous_crash(&dir).previous_crash_detected);
    assert!(Journal::open_existing(root, "source", true).is_err());
    assert!(apply_crash_recovery(root, "source", true).is_err());
    assert_eq!(std::fs::read(dir.join("events.jsonl"))?, prefix);
    std::fs::write(&lock_path, lock_bytes)?;
    Ok(())
}

fn assert_complete_corruption_is_preserved(
    root: &std::path::Path,
    prefix: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join("source/events.jsonl");
    for corrupt in [b"{invalid}\n".as_slice(), b"{\"schema_version\":1}\n"] {
        std::fs::write(&path, [prefix, corrupt].concat())?;
        let before = std::fs::read(&path)?;
        assert!(apply_crash_recovery(root, "source", true).is_err());
        assert_eq!(std::fs::read(&path)?, before);
    }
    Ok(())
}

fn interrupted_journal(root: &std::path::Path) -> Result<Journal, Box<dyn std::error::Error>> {
    let writer = Journal::open(root, "source", true)?;
    for (index, payload) in [
        EventV1::RunStarted(RunStartedEvent {
            run_name: "crash".into(),
            workspace_root: root.to_string_lossy().into(),
        }),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task".into(),
            state: TaskScheduleState::Started,
            queue_key: None,
            metadata: None,
        }),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool".into(),
            tool_id: "read".into(),
            args_summary: "{}".into(),
            args_digest: "digest".into(),
            metadata: None,
        }),
        EventV1::EditProposed(EditProposedEvent {
            edit_id: "edit".into(),
            path: "https://example.test/?token=private-historical-value".into(),
            summary: "old edit".into(),
            patch_digest: "digest".into(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        writer.append(crate::lineage_tests::event(index as u64 + 1, payload).into())?;
    }
    Ok(writer)
}
