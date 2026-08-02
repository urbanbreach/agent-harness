//! Line-level diff and blame for agent-attributed edit inspection.
//!
//! Computes a unified diff and per-line blame between the agent snapshot
//! (the content the agent last wrote) and the current workspace file bytes.
//! External drift (changes made after the agent edit) is attributed as
//! `EditSource::External`; unchanged lines retain `EditSource::AgentTool`.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use super::{sha256_hex, EditSource};

/// One line in a blame result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// 1-based line number in the current file.
    pub line_number: usize,
    /// Attribution source for this line.
    pub source: EditSource,
    /// The line content (without trailing newline).
    pub content: String,
}

/// Blame result for one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameResult {
    pub path: String,
    pub lines: Vec<BlameLine>,
    pub agent_lines: usize,
    pub external_lines: usize,
    pub drifted: bool,
}

/// Diff result for one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    pub path: String,
    pub agent_snapshot_sha256: String,
    pub current_sha256: String,
    pub drifted: bool,
    pub unified_diff: String,
    pub agent_lines: usize,
    pub external_lines: usize,
}

/// Compute a unified diff between the agent snapshot and the current file bytes.
///
/// Returns a `DiffResult` with the unified diff text, line counts, and drift
/// status. Lines that match the agent snapshot are counted as `agent_lines`;
/// inserted or replaced lines are counted as `external_lines`.
pub fn compute_diff(path: &str, snapshot: &[u8], current: &[u8]) -> DiffResult {
    let snapshot_str = String::from_utf8_lossy(snapshot);
    let current_str = String::from_utf8_lossy(current);
    let diff = TextDiff::from_lines(&snapshot_str, &current_str);
    let unified = diff.unified_diff().to_string();

    let (agent_lines, external_lines) = count_blame_lines(&diff);
    let drifted = snapshot != current;

    DiffResult {
        path: path.to_string(),
        agent_snapshot_sha256: sha256_hex(snapshot),
        current_sha256: sha256_hex(current),
        drifted,
        unified_diff: unified,
        agent_lines,
        external_lines,
    }
}

/// Compute per-line blame between the agent snapshot and the current file bytes.
///
/// Each line in the current file is attributed to `AgentTool` (if it matches
/// the agent snapshot) or `External` (if it was inserted or modified after the
/// agent edit).
pub fn compute_blame(path: &str, snapshot: &[u8], current: &[u8]) -> BlameResult {
    let snapshot_str = String::from_utf8_lossy(snapshot);
    let current_str = String::from_utf8_lossy(current);
    let diff = TextDiff::from_lines(&snapshot_str, &current_str);

    let mut blame_lines = Vec::new();
    let mut agent_lines = 0usize;
    let mut external_lines = 0usize;

    for op in diff.ops() {
        for change in diff.iter_changes(op) {
            let Some(new_idx) = change.new_index() else {
                continue;
            };
            let source = match change.tag() {
                ChangeTag::Equal => {
                    agent_lines += 1;
                    EditSource::AgentTool
                }
                ChangeTag::Insert => {
                    external_lines += 1;
                    EditSource::External
                }
                ChangeTag::Delete => continue,
            };
            let content = change.value().trim_end_matches('\n').to_string();
            blame_lines.push(BlameLine {
                line_number: new_idx + 1,
                source,
                content,
            });
        }
    }

    let drifted = snapshot != current;

    BlameResult {
        path: path.to_string(),
        lines: blame_lines,
        agent_lines,
        external_lines,
        drifted,
    }
}

fn count_blame_lines(diff: &TextDiff<'_, '_, str>) -> (usize, usize) {
    let mut agent = 0usize;
    let mut external = 0usize;
    for op in diff.ops() {
        for change in diff.iter_changes(op) {
            if change.new_index().is_none() {
                continue;
            }
            match change.tag() {
                ChangeTag::Equal => agent += 1,
                ChangeTag::Insert => external += 1,
                ChangeTag::Delete => {}
            }
        }
    }
    (agent, external)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_diff_shows_no_drift_when_identical() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"line one\nline two\n";
        // When
        let result = compute_diff("src/a.rs", snapshot, snapshot);
        // Then
        assert!(!result.drifted);
        assert_eq!(result.agent_lines, 2);
        assert_eq!(result.external_lines, 0);
        assert!(result.unified_diff.is_empty() || !result.unified_diff.contains('+'));
    }

    #[test]
    fn compute_diff_shows_drift_when_modified() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"line one\nline two\n";
        let current = b"line one\nline modified\n";
        // When
        let result = compute_diff("src/a.rs", snapshot, current);
        // Then
        assert!(result.drifted);
        assert_eq!(result.agent_lines, 1);
        assert_eq!(result.external_lines, 1);
        assert!(result.unified_diff.contains("line modified"));
    }

    #[test]
    fn compute_blame_attributes_unchanged_lines_to_agent() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"keep\nkeep\n";
        // When
        let result = compute_blame("src/a.rs", snapshot, snapshot);
        // Then
        assert_eq!(result.agent_lines, 2);
        assert_eq!(result.external_lines, 0);
        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.lines[0].source, EditSource::AgentTool);
        assert_eq!(result.lines[1].source, EditSource::AgentTool);
    }

    #[test]
    fn compute_blame_attributes_inserted_lines_to_external() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"line one\n";
        let current = b"line one\nline two\n";
        // When
        let result = compute_blame("src/a.rs", snapshot, current);
        // Then
        assert_eq!(result.agent_lines, 1);
        assert_eq!(result.external_lines, 1);
        assert_eq!(result.lines[0].source, EditSource::AgentTool);
        assert_eq!(result.lines[1].source, EditSource::External);
    }

    #[test]
    fn compute_blame_attributes_replaced_lines_to_external() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"old line\n";
        let current = b"new line\n";
        // When
        let result = compute_blame("src/a.rs", snapshot, current);
        // Then
        assert_eq!(result.agent_lines, 0);
        assert_eq!(result.external_lines, 1);
        assert_eq!(result.lines[0].source, EditSource::External);
        assert_eq!(result.lines[0].content, "new line");
    }

    #[test]
    fn compute_blame_handles_empty_current_file() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"line one\nline two\n";
        let current = b"";
        // When
        let result = compute_blame("src/a.rs", snapshot, current);
        // Then
        assert_eq!(result.agent_lines, 0);
        assert_eq!(result.external_lines, 0);
        assert!(result.lines.is_empty());
        assert!(result.drifted);
    }

    #[test]
    fn compute_blame_handles_empty_snapshot() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"";
        let current = b"new content\n";
        // When
        let result = compute_blame("src/a.rs", snapshot, current);
        // Then
        assert_eq!(result.agent_lines, 0);
        assert_eq!(result.external_lines, 1);
        assert_eq!(result.lines[0].source, EditSource::External);
    }

    #[test]
    fn compute_diff_line_numbers_are_one_based() {
        // arrange
        // act
        // assert
        // Given
        let snapshot = b"a\nb\nc\n";
        let current = b"a\nB\nc\n";
        // When
        let result = compute_blame("src/a.rs", snapshot, current);
        // Then
        assert_eq!(result.lines[0].line_number, 1);
        assert_eq!(result.lines[1].line_number, 2);
        assert_eq!(result.lines[2].line_number, 3);
        assert_eq!(result.lines[1].source, EditSource::External);
    }
}
