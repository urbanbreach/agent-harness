//! Multi-path edit attribution product scenario (real workspace files + journal).

use std::fs;
use std::path::Path;

use super::{
    AttributedEdit, EditAttributionError, EditAttributionJournal, EditAttributionSummary,
    EditSource, EDIT_ATTRIBUTION_JOURNAL_REL,
};

/// Product result of multi-path agent-tool vs external vs drift attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiPathEditAttribution {
    pub summary: EditAttributionSummary,
    pub first_line: Option<String>,
    pub last_line: Option<String>,
    pub entries: Vec<AttributedEdit>,
    pub journal_path: String,
}

impl MultiPathEditAttribution {
    pub fn one_line(&self) -> String {
        self.summary.one_line()
    }
}

/// Product path: multi-path attribution with durable journal side effects.
///
/// Writes real workspace files and records them into
/// [`.agent-harness/edit-attribution.jsonl`](EDIT_ATTRIBUTION_JOURNAL_REL):
/// - `src/agent.rs` — agent-tool only
/// - `src/external.rs` — external only
/// - `src/drift.rs` — agent then external content drift
///
/// Exposes queryable summary counts (agent / external / drift) and supports
/// path-level revert via [`EditAttributionJournal::revert_path`].
pub fn run_multi_path_edit_attribution_product(
    workspace_root: &Path,
) -> Result<MultiPathEditAttribution, EditAttributionError> {
    let agent_bytes = b"agent-product-bytes";
    let external_bytes = b"external-product-bytes";
    let drifted_agent = b"agent-product-bytes-v1";
    let drifted_external = b"agent-product-bytes-v2-external";

    let agent_rel = "src/agent.rs";
    let external_rel = "src/external.rs";
    let drift_rel = "src/drift.rs";

    write_workspace_file(workspace_root, agent_rel, agent_bytes)?;
    write_workspace_file(workspace_root, external_rel, external_bytes)?;
    write_workspace_file(workspace_root, drift_rel, drifted_agent)?;

    let mut journal = EditAttributionJournal::open(workspace_root)?;
    journal.record_agent_tool_edit(agent_rel, agent_bytes, None)?;
    journal.observe_external(external_rel, external_bytes, None)?;
    journal.record_agent_tool_edit(drift_rel, drifted_agent, None)?;

    // External drift on disk, then observe.
    write_workspace_file(workspace_root, drift_rel, drifted_external)?;
    journal.observe_external(drift_rel, drifted_external, None)?;

    let summary = journal.summary();
    let entries: Vec<AttributedEdit> = journal.list().into_iter().cloned().collect();
    let first_line = entries.first().map(AttributedEdit::one_line);
    let last_line = entries.last().map(AttributedEdit::one_line);
    let journal_path = journal.journal_path().display().to_string();

    // Sanity: product contract — multi-path with all three buckets.
    debug_assert!(summary.agent_tool >= 1);
    debug_assert!(summary.external >= 1);
    debug_assert!(summary.drift >= 1);

    // Query API must resolve each path.
    let _ = journal.query(agent_rel)?;
    let _ = journal.query(external_rel)?;
    let drift_q = journal.query(drift_rel)?;
    if drift_q.source != EditSource::External || !drift_q.drifted {
        return Err(EditAttributionError::Parse {
            path: journal_path.clone(),
            detail: format!(
                "expected drift path source=external drifted=true, got source={:?} drifted={}",
                drift_q.source, drift_q.drifted
            ),
        });
    }

    Ok(MultiPathEditAttribution {
        summary,
        first_line,
        last_line,
        entries,
        journal_path,
    })
}

fn write_workspace_file(
    workspace_root: &Path,
    relative: &str,
    bytes: &[u8],
) -> Result<(), EditAttributionError> {
    let path = workspace_root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EditAttributionError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(&path, bytes).map_err(|source| EditAttributionError::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_attribution::EDIT_ATTRIBUTION_JOURNAL_REL;

    #[test]
    fn multi_path_product_writes_journal_and_workspace_files() {
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // When
        let product = run_multi_path_edit_attribution_product(root).expect("product");

        // Then: real workspace files
        assert!(root.join("src/agent.rs").is_file());
        assert!(root.join("src/external.rs").is_file());
        assert_eq!(
            fs::read(root.join("src/drift.rs")).expect("drift"),
            b"agent-product-bytes-v2-external"
        );

        // Then: durable journal under .agent-harness/
        let journal = root.join(EDIT_ATTRIBUTION_JOURNAL_REL);
        assert!(
            journal.is_file(),
            "missing journal at {}",
            journal.display()
        );
        assert!(product.journal_path.contains("edit-attribution.jsonl"));

        // Then: multi-path summary with agent + external + drift
        assert_eq!(product.summary.total, 3);
        assert_eq!(product.summary.agent_tool, 1);
        assert_eq!(product.summary.external, 1);
        assert_eq!(product.summary.drift, 1);
        assert!(product.summary.has_agent_tool());
        assert!(product.summary.has_external());
        assert!(product.summary.has_drift());
        assert_eq!(product.entries.len(), 3);

        let first = product.first_line.as_deref().expect("first");
        assert!(
            first.contains("source=agent_tool") && first.contains("agent.rs"),
            "first={first}"
        );
        let last = product.last_line.as_deref().expect("last");
        assert!(
            last.contains("source=external")
                && (last.contains("external.rs") || last.contains("drift.rs")),
            "last={last}"
        );
        assert!(product.one_line().contains("agent-tool"));
        assert!(product.one_line().contains("external"));
        assert!(product.one_line().contains("drift"));

        // Then: query + revert product APIs work on the same journal
        let mut journal = EditAttributionJournal::open(root).expect("reopen");
        let q = journal.query("src/drift.rs").expect("query drift");
        assert!(q.drifted);
        let reverted = journal.revert_path("src/drift.rs").expect("revert");
        assert_eq!(reverted.path, "src/drift.rs");
        assert_eq!(
            fs::read(root.join("src/drift.rs")).expect("restored"),
            b"agent-product-bytes-v1"
        );
        assert_eq!(
            journal.query("src/drift.rs").expect("after").source,
            EditSource::AgentTool
        );
    }
}
