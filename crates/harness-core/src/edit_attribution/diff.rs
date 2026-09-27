use super::{sha256_hex, EditSource};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub line_number: usize,
    pub source: EditSource,
    pub content: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameResult {
    pub path: String,
    pub lines: Vec<BlameLine>,
    pub agent_lines: usize,
    pub external_lines: usize,
    pub drifted: bool,
}
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
pub fn compute_diff(path: &str, snapshot: &[u8], current: &[u8]) -> DiffResult {
    let old = String::from_utf8_lossy(snapshot);
    let new = String::from_utf8_lossy(current);
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_lines(old.as_ref(), new.as_ref());
    let (agent_lines, external_lines) = counts(&diff);
    DiffResult {
        path: path.into(),
        agent_snapshot_sha256: sha256_hex(snapshot),
        current_sha256: sha256_hex(current),
        drifted: snapshot != current,
        unified_diff: diff
            .unified_diff()
            .header(&format!("agent/{path}"), &format!("current/{path}"))
            .to_string(),
        agent_lines,
        external_lines,
    }
}
pub fn compute_blame(path: &str, snapshot: &[u8], current: &[u8]) -> BlameResult {
    let old = String::from_utf8_lossy(snapshot);
    let new = String::from_utf8_lossy(current);
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_lines(old.as_ref(), new.as_ref());
    let (agent_lines, external_lines) = counts(&diff);
    let lines = diff
        .iter_all_changes()
        .filter(|c| c.tag() != ChangeTag::Delete)
        .enumerate()
        .map(|(i, c)| BlameLine {
            line_number: i + 1,
            source: if c.tag() == ChangeTag::Equal {
                EditSource::AgentTool
            } else {
                EditSource::External
            },
            content: c.value().into(),
        })
        .collect();
    BlameResult {
        path: path.into(),
        lines,
        agent_lines,
        external_lines,
        drifted: snapshot != current,
    }
}
fn counts(diff: &TextDiff<'_, '_, str>) -> (usize, usize) {
    diff.iter_all_changes()
        .fold((0, 0), |(agent, external), c| match c.tag() {
            ChangeTag::Equal => (agent + 1, external),
            ChangeTag::Insert => (agent, external + 1),
            ChangeTag::Delete => (agent, external),
        })
}
