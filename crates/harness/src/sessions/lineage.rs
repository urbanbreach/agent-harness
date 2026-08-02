// allow: SIZE_OK — CLI session management (list + export + lineage)
use crate::UnwrapOrAbort;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use harness_core::proj::SessionCatalogEntry;
use harness_core::session_lineage::{
    latest_clone_stable_prefix, materialize_child_session, project_lineage_tree,
    validate_fork_stable_prefix, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind, SessionLineageNode, StableSessionPrefix,
};
use serde::Serialize;

use crate::cli_io::load_events_from_run_dir;
use crate::replay::{inspect_session_catalog, SessionInspectionEntry};

use super::{
    ensure_session_dir_exists, resolve_session_for_write_or_tree, resumable_label, session_dir,
    status_label, write_json_output,
};

#[derive(Debug, Args, Clone)]
pub struct TreeSessionCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub root: Option<String>,

    #[arg(long, value_name = "TEXT")]
    pub filter: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct ForkSessionCommand {
    /// Run id or session directory path to fork from.
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub source: String,

    /// Stable event sequence cutoff to copy into the child session.
    #[arg(long, value_name = "SEQ")]
    pub cutoff: u64,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct CloneSessionCommand {
    /// Run id or session directory path to clone from.
    #[arg(long, value_name = "RUN_ID_OR_PATH")]
    pub source: String,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SessionTreeJson {
    harness_lineage: Vec<SessionTreeJsonRow>,
    session_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SessionTreeJsonRow {
    depth: usize,
    run_dir: PathBuf,
    #[serde(flatten)]
    catalog: SessionCatalogEntry,
}

#[derive(Debug, Clone)]
struct SessionTreeRow {
    depth: usize,
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
}

#[derive(Debug, Clone, Serialize)]
struct ChildSessionWriteReport {
    harness_operation: &'static str,
    child_run_id: String,
    child_run_dir: PathBuf,
    source_run_id: Option<String>,
    source_run_dir: PathBuf,
    source_cutoff_seq: u64,
    event_count: usize,
    artifact_count: usize,
    warnings: Vec<String>,
    errors: Vec<String>,
}

pub(super) fn tree_sessions(
    command: TreeSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let default_session_dir = session_dir(global_session_dir);
    let (session_dir, root_run_id) = match command.root.as_deref() {
        Some(root) => match resolve_session_for_write_or_tree(&default_session_dir, root) {
            Ok(session) => {
                let session_dir = session
                    .run_dir
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| default_session_dir.clone());
                (session_dir, Some(session.catalog.run_id))
            }
            Err(err) => {
                let _ = writeln!(stderr, "Harness session tree failed: {err}");
                return 1;
            }
        },
        None => {
            if let Err(code) = ensure_session_dir_exists(&default_session_dir, stderr) {
                return code;
            }
            (default_session_dir, None)
        }
    };

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session tree failed: {err}");
            return 1;
        }
    };
    let rows = collect_tree_rows(entries, root_run_id.as_deref(), command.filter.as_deref());

    if command.json {
        let report = SessionTreeJson {
            session_count: rows.len(),
            harness_lineage: rows
                .iter()
                .map(|row| SessionTreeJsonRow {
                    depth: row.depth,
                    run_dir: row.run_dir.clone(),
                    catalog: row.catalog.clone(),
                })
                .collect(),
            root: root_run_id,
            filter: command.filter,
        };
        write_json_output(&report, None, stdout, stderr)
    } else {
        let _ = write!(
            stdout,
            "{}",
            render_human_session_tree(&rows, root_run_id.as_deref(), command.filter.as_deref())
        );
        0
    }
}

pub(super) fn fork_session(
    command: ForkSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    write_child_session(
        "fork",
        &command.source,
        global_session_dir,
        command.json,
        stdout,
        stderr,
        |events| validate_fork_stable_prefix(events, command.cutoff),
    )
}

pub(super) fn clone_session(
    command: CloneSessionCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    write_child_session(
        "clone",
        &command.source,
        global_session_dir,
        command.json,
        stdout,
        stderr,
        latest_clone_stable_prefix,
    )
}

pub(super) fn write_child_session(
    operation: &'static str,
    source: &str,
    global_session_dir: Option<PathBuf>,
    json: bool,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    stable_prefix: impl FnOnce(
        &[harness_core::event::EventEnvelopeV1],
    ) -> Result<
        StableSessionPrefix,
        harness_core::session_lineage::SessionLineageError,
    >,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    let source = match resolve_session_for_write_or_tree(&session_dir, source) {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
        }
    };
    let events = match load_events_from_run_dir(&source.run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "Harness session {operation} failed: failed to read source session {}: {err}",
                source.catalog.run_id
            );
            return 1;
        }
    };
    let stable_prefix = match stable_prefix(&events) {
        Ok(prefix) => prefix,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
        }
    };
    let result = match materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source.run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    }) {
        Ok(result) => result,
        Err(err) => {
            let _ = writeln!(stderr, "Harness session {operation} failed: {err}");
            return 1;
        }
    };

    let report = ChildSessionWriteReport {
        harness_operation: operation,
        child_run_id: result.child_run_id,
        child_run_dir: result.child_run_dir,
        source_run_id: result.source_run_id,
        source_run_dir: source.run_dir,
        source_cutoff_seq: result.source_cutoff_seq,
        event_count: result.event_count,
        artifact_count: result.artifact_count,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    if json {
        write_json_output(&report, None, stdout, stderr)
    } else {
        let _ = writeln!(stdout, "Harness session {operation} created");
        let _ = writeln!(stdout, "child_run_id: {}", report.child_run_id);
        let _ = writeln!(stdout, "child_run_dir: {}", report.child_run_dir.display());
        if let Some(source_run_id) = report.source_run_id.as_deref() {
            let _ = writeln!(stdout, "source_run_id: {source_run_id}");
        }
        let _ = writeln!(
            stdout,
            "source_run_dir: {}",
            report.source_run_dir.display()
        );
        let _ = writeln!(stdout, "source_cutoff_seq: {}", report.source_cutoff_seq);
        let _ = writeln!(stdout, "events: {}", report.event_count);
        let _ = writeln!(stdout, "artifacts: {}", report.artifact_count);
        0
    }
}

fn collect_tree_rows(
    entries: Vec<SessionInspectionEntry>,
    root_run_id: Option<&str>,
    filter: Option<&str>,
) -> Vec<SessionTreeRow> {
    let run_dirs = entries
        .iter()
        .map(|entry| (entry.catalog.run_id.clone(), entry.run_dir.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let tree = project_lineage_tree(entries.into_iter().map(|entry| entry.catalog));
    let rows = match root_run_id {
        Some(root_run_id) => tree
            .roots
            .iter()
            .find_map(|node| flatten_from_node(node, root_run_id)),
        None => Some(
            tree.flatten()
                .into_iter()
                .map(|row| (row.depth, row.entry.clone()))
                .collect(),
        ),
    }
    .unwrap_or_default();

    let filter = filter.map(|text| text.to_ascii_lowercase());
    rows.into_iter()
        .filter_map(|(depth, catalog)| {
            let run_dir = run_dirs.get(&catalog.run_id).cloned().unwrap_or_default();
            let row = SessionTreeRow {
                depth,
                run_dir,
                catalog,
            };
            filter
                .as_deref()
                .is_none_or(|filter| tree_row_matches_filter(&row, filter))
                .then_some(row)
        })
        .collect()
}

fn flatten_from_node(
    node: &SessionLineageNode,
    root_run_id: &str,
) -> Option<Vec<(usize, SessionCatalogEntry)>> {
    if node.entry.run_id == root_run_id {
        let mut rows = Vec::new();
        flatten_node_into(node, 0, &mut rows);
        return Some(rows);
    }
    node.children
        .iter()
        .find_map(|child| flatten_from_node(child, root_run_id))
}

fn flatten_node_into(
    node: &SessionLineageNode,
    depth: usize,
    rows: &mut Vec<(usize, SessionCatalogEntry)>,
) {
    rows.push((depth, node.entry.clone()));
    for child in &node.children {
        flatten_node_into(child, depth + 1, rows);
    }
}

fn tree_row_matches_filter(row: &SessionTreeRow, filter: &str) -> bool {
    contains_normalized_filter(&row.catalog.run_id, filter)
        || optional_text_contains_normalized_filter(row.catalog.run_name.as_deref(), filter)
        || optional_text_contains_normalized_filter(
            row.catalog.parent_session_id.as_deref(),
            filter,
        )
        || contains_normalized_filter(&row.run_dir.display().to_string(), filter)
}

fn optional_text_contains_normalized_filter(value: Option<&str>, filter: &str) -> bool {
    value.is_some_and(|value| contains_normalized_filter(value, filter))
}

fn contains_normalized_filter(value: &str, filter: &str) -> bool {
    value.to_ascii_lowercase().contains(filter)
}

fn render_human_session_tree(
    rows: &[SessionTreeRow],
    root_run_id: Option<&str>,
    filter: Option<&str>,
) -> String {
    let mut output = String::new();
    writeln!(output, "Harness session lineage").unwrap_or_abort();
    if let Some(root_run_id) = root_run_id {
        writeln!(output, "root: {root_run_id}").unwrap_or_abort();
    }
    if let Some(filter) = filter {
        writeln!(output, "filter: {filter}").unwrap_or_abort();
    }
    if rows.is_empty() {
        writeln!(output, "  <no sessions>").unwrap_or_abort();
        return output;
    }

    for row in rows {
        let indent = "  ".repeat(row.depth);
        writeln!(
            output,
            "{indent}- {} status={} resume={} parent={} path={}",
            row.catalog.run_id,
            status_label(row.catalog.status),
            resumable_label(row.catalog.is_resumable),
            row.catalog.parent_session_id.as_deref().unwrap_or("-"),
            row.run_dir.display()
        )
        .unwrap_or_abort();
    }
    output
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    use super::*;

    #[test]
    fn tree_row_filter_matches_all_searchable_fields_case_insensitively() {
        // arrange
        // act
        // assert
        let mut row = SessionTreeRow {
            depth: 0,
            run_dir: PathBuf::from("/tmp/Session-Path"),
            catalog: SessionCatalogEntry {
                run_id: "RUN-ABC".into(),
                run_name: Some("RUN-ABC-name".to_string()),
                status: Some(RunStatus::Finished),
                last_updated_at: Some("42".to_string()),
                workspace_root: Some("/workspaces/RUN-ABC".to_string()),
                profile_preset: Some("worker".to_string()),
                provider_model: Some("openai/gpt-5.4".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
                artifact_count: 0,
                child_session_count: 0,
                parent_session_id: Some("PARENT-ABC".to_string()),
            },
        };
        row.catalog.run_name = Some("Named Session".to_string());

        for filter in ["run-abc", "named", "parent-abc", "session-path"] {
            assert!(
                tree_row_matches_filter(&row, filter),
                "filter `{filter}` should match the tree row"
            );
        }

        assert!(!tree_row_matches_filter(&row, "missing"));
    }
}
