//! Integration tests for edit attribution diff and blame.
//!
//! Verifies that `harness attribution diff <path>` and `blame <path>` produce
//! agent-attributed diff output, external drift is reported separately, unknown
//! paths fail closed, and snapshot-based revert semantics are not weakened.

use std::fs;

use harness_core::edit_attribution::{
    BlameResult, DiffResult, EditAttributionError, EditAttributionJournal, EditSource,
};
use harness_core::UnwrapOrAbort;

fn write_file(root: &std::path::Path, rel: &str, content: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_abort();
    }
    fs::write(&path, content).unwrap_or_abort();
}

#[test]
fn diff_shows_no_drift_when_file_matches_agent_snapshot() {
    // arrange
    // act
    // assert
    // Given: agent wrote a file
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/a.rs", b"fn main() {}\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/a.rs", b"fn main() {}\n", None)
        .unwrap_or_abort();

    // When: diff against unchanged file
    let result = journal.diff("src/a.rs").unwrap_or_abort();

    // Then
    assert!(!result.drifted);
    assert_eq!(result.agent_lines, 1);
    assert_eq!(result.external_lines, 0);
    assert!(!result.agent_snapshot_sha256.is_empty());
    assert_eq!(result.agent_snapshot_sha256, result.current_sha256);
}

#[test]
fn diff_shows_drift_when_file_modified_externally() {
    // arrange
    // act
    // assert
    // Given: agent wrote content, then external modified it
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/b.rs", b"line one\nline two\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/b.rs", b"line one\nline two\n", None)
        .unwrap_or_abort();
    write_file(root, "src/b.rs", b"line one\nline modified\n");

    // When
    let result = journal.diff("src/b.rs").unwrap_or_abort();

    // Then
    assert!(result.drifted);
    assert_eq!(result.agent_lines, 1);
    assert_eq!(result.external_lines, 1);
    assert_ne!(result.agent_snapshot_sha256, result.current_sha256);
    assert!(result.unified_diff.contains("line modified"));
}

#[test]
fn blame_attributes_unchanged_lines_to_agent() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/c.rs", b"keep\nkeep\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/c.rs", b"keep\nkeep\n", None)
        .unwrap_or_abort();

    // When
    let result: BlameResult = journal.blame("src/c.rs").unwrap_or_abort();

    // Then
    assert_eq!(result.agent_lines, 2);
    assert_eq!(result.external_lines, 0);
    assert_eq!(result.lines.len(), 2);
    assert!(result
        .lines
        .iter()
        .all(|l| l.source == EditSource::AgentTool));
}

#[test]
fn blame_attributes_external_drift_lines_separately() {
    // arrange
    // act
    // assert
    // Given: agent wrote 3 lines, external modified line 2
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/d.rs", b"line1\nline2\nline3\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/d.rs", b"line1\nline2\nline3\n", None)
        .unwrap_or_abort();
    write_file(root, "src/d.rs", b"line1\nEXTERNAL\nline3\n");

    // When
    let result = journal.blame("src/d.rs").unwrap_or_abort();

    // Then: line 2 is external, lines 1 and 3 are agent
    assert_eq!(result.agent_lines, 2);
    assert_eq!(result.external_lines, 1);
    assert_eq!(result.lines[0].source, EditSource::AgentTool);
    assert_eq!(result.lines[1].source, EditSource::External);
    assert_eq!(result.lines[2].source, EditSource::AgentTool);
    assert_eq!(result.lines[1].content, "EXTERNAL");
}

#[test]
fn blame_attributes_inserted_lines_to_external() {
    // arrange
    // act
    // assert
    // Given: agent wrote 1 line, external added a second
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/e.rs", b"agent line\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/e.rs", b"agent line\n", None)
        .unwrap_or_abort();
    write_file(root, "src/e.rs", b"agent line\nexternal line\n");

    // When
    let result = journal.blame("src/e.rs").unwrap_or_abort();

    // Then
    assert_eq!(result.agent_lines, 1);
    assert_eq!(result.external_lines, 1);
    assert_eq!(result.lines[0].source, EditSource::AgentTool);
    assert_eq!(result.lines[1].source, EditSource::External);
    assert_eq!(result.lines[1].content, "external line");
}

#[test]
fn diff_fails_closed_for_unknown_path() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let journal = EditAttributionJournal::open(dir.path()).unwrap_or_abort();

    // When
    let err = journal.diff("never-seen.rs").expect_err("unknown");

    // Then
    assert!(matches!(err, EditAttributionError::NotFound { .. }));
}

#[test]
fn blame_fails_closed_for_unknown_path() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let journal = EditAttributionJournal::open(dir.path()).unwrap_or_abort();

    // When
    let err = journal.blame("never-seen.rs").expect_err("unknown");

    // Then
    assert!(matches!(err, EditAttributionError::NotFound { .. }));
}

#[test]
fn diff_fails_closed_for_external_only_path_without_agent_snapshot() {
    // arrange
    // act
    // assert
    // Given: path observed as external only (no agent edit)
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/ext.rs", b"external\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .observe_external("src/ext.rs", b"external\n", None)
        .unwrap_or_abort();

    // When
    let err = journal.diff("src/ext.rs").expect_err("no snapshot");

    // Then
    assert!(matches!(err, EditAttributionError::NoAgentSnapshot { .. }));
}

#[test]
fn blame_fails_closed_for_external_only_path_without_agent_snapshot() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/ext2.rs", b"external\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .observe_external("src/ext2.rs", b"external\n", None)
        .unwrap_or_abort();

    // When
    let err = journal.blame("src/ext2.rs").expect_err("no snapshot");

    // Then
    assert!(matches!(err, EditAttributionError::NoAgentSnapshot { .. }));
}

#[test]
fn diff_shows_drift_when_file_deleted_after_agent_edit() {
    // arrange
    // act
    // assert
    // Given: agent wrote a file, then it was deleted
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/del.rs", b"line one\nline two\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/del.rs", b"line one\nline two\n", None)
        .unwrap_or_abort();
    fs::remove_file(root.join("src/del.rs")).unwrap_or_abort();

    // When
    let result: DiffResult = journal.diff("src/del.rs").unwrap_or_abort();

    // Then: drift detected, all agent lines gone
    assert!(result.drifted);
    assert_eq!(result.agent_lines, 0);
    assert_eq!(result.external_lines, 0);
    assert_ne!(result.agent_snapshot_sha256, result.current_sha256);
}

#[test]
fn blame_shows_no_lines_when_file_deleted_after_agent_edit() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/del2.rs", b"content\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/del2.rs", b"content\n", None)
        .unwrap_or_abort();
    fs::remove_file(root.join("src/del2.rs")).unwrap_or_abort();

    // When
    let result = journal.blame("src/del2.rs").unwrap_or_abort();

    // Then
    assert!(result.lines.is_empty());
    assert!(result.drifted);
}

#[test]
fn diff_and_blame_do_not_modify_journal_state() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/stable.rs", b"stable\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/stable.rs", b"stable\n", None)
        .unwrap_or_abort();
    let summary_before = journal.summary();

    // When: call diff and blame (read-only)
    let _ = journal.diff("src/stable.rs").unwrap_or_abort();
    let _ = journal.blame("src/stable.rs").unwrap_or_abort();

    // Then: journal state unchanged
    let summary_after = journal.summary();
    assert_eq!(summary_before, summary_after);
}

#[test]
fn revert_still_works_after_diff_and_blame() {
    // arrange
    // act
    // assert
    // Given: agent wrote, external drifted
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    let rel = "src/revert.rs";
    write_file(root, rel, b"agent-content\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit(rel, b"agent-content\n", None)
        .unwrap_or_abort();
    write_file(root, rel, b"external-drift\n");
    journal
        .observe_external(rel, b"external-drift\n", None)
        .unwrap_or_abort();

    // When: diff, blame, then revert
    let diff_result = journal.diff(rel).unwrap_or_abort();
    assert!(diff_result.drifted);
    let _ = journal.blame(rel).unwrap_or_abort();
    let reverted = journal.revert_path(rel).unwrap_or_abort();

    // Then: revert restores agent snapshot
    assert_eq!(reverted.path, rel);
    assert_eq!(
        fs::read(root.join(rel)).unwrap_or_abort(),
        b"agent-content\n"
    );
    assert_eq!(
        journal.query(rel).unwrap_or_abort().source,
        EditSource::AgentTool
    );
}

#[test]
fn diff_persists_across_journal_reload() {
    // arrange
    // act
    // assert
    // Given: agent wrote, external modified
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/persist.rs", b"original\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/persist.rs", b"original\n", None)
        .unwrap_or_abort();
    write_file(root, "src/persist.rs", b"modified\n");

    // When: reload journal and diff
    let reloaded = EditAttributionJournal::open(root).unwrap_or_abort();
    let result = reloaded.diff("src/persist.rs").unwrap_or_abort();

    // Then: agent snapshot survived reload
    assert!(result.drifted);
    assert_eq!(result.agent_lines, 0);
    assert_eq!(result.external_lines, 1);
}

#[test]
fn blame_line_numbers_are_sequential_from_one() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/lines.rs", b"a\nb\nc\nd\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/lines.rs", b"a\nb\nc\nd\n", None)
        .unwrap_or_abort();

    // When
    let result = journal.blame("src/lines.rs").unwrap_or_abort();

    // Then
    assert_eq!(result.lines.len(), 4);
    for (i, line) in result.lines.iter().enumerate() {
        assert_eq!(line.line_number, i + 1);
    }
}

#[test]
fn diff_invalid_path_escape_fails_closed() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let journal = EditAttributionJournal::open(dir.path()).unwrap_or_abort();

    // When
    let err = journal.diff("../escape.rs").expect_err("escape");

    // Then
    assert!(matches!(err, EditAttributionError::InvalidPath { .. }));
}

#[test]
fn blame_invalid_path_escape_fails_closed() {
    // arrange
    // act
    // assert
    // Given
    let dir = tempfile::tempdir().unwrap_or_abort();
    let journal = EditAttributionJournal::open(dir.path()).unwrap_or_abort();

    // When
    let err = journal.blame("../escape.rs").expect_err("escape");

    // Then
    assert!(matches!(err, EditAttributionError::InvalidPath { .. }));
}
