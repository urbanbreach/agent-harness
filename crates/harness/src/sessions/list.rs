use crate::UnwrapOrAbort;
use std::fmt::Write as _;
use std::path::PathBuf;

use clap::ValueEnum;
use harness_core::proj::{RunStatus, SessionCatalogEntry};
use serde::Serialize;

use crate::replay::{
    inspect_session_catalog, rebuild_session_catalog_index, SessionInspectionEntry,
};

use super::{
    ensure_session_dir_exists, mode_source_label, resumable_label, session_dir, status_label,
    SessionsListCommand, SessionsRebuildIndexCommand, SessionsSearchCommand,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SessionStatusFilter {
    Running,
    Finished,
    Failed,
    Unavailable,
}

impl SessionStatusFilter {
    fn matches(self, status: Option<RunStatus>) -> bool {
        matches!(
            (self, status),
            (Self::Running, Some(RunStatus::Running))
                | (Self::Finished, Some(RunStatus::Finished))
                | (Self::Failed, Some(RunStatus::Failed))
                | (Self::Unavailable, None)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SessionListSort {
    #[default]
    #[value(name = "updated_desc")]
    UpdatedDesc,
    #[value(name = "updated_asc")]
    UpdatedAsc,
    #[value(name = "run_id_asc")]
    RunIdAsc,
    #[value(name = "run_id_desc")]
    RunIdDesc,
}

impl SessionListSort {
    fn sort_entries(self, entries: &mut [SessionInspectionEntry]) {
        match self {
            Self::UpdatedDesc => {
                SessionInspectionEntry::sort_by_updated_desc(entries);
            }
            Self::UpdatedAsc => {
                SessionInspectionEntry::sort_by_updated_asc(entries);
            }
            Self::RunIdAsc => {
                entries.sort_by(|left, right| {
                    left.catalog
                        .run_id
                        .cmp(&right.catalog.run_id)
                        .then_with(|| left.run_dir.cmp(&right.run_dir))
                });
            }
            Self::RunIdDesc => {
                entries.sort_by(|left, right| {
                    right
                        .catalog
                        .run_id
                        .cmp(&left.catalog.run_id)
                        .then_with(|| right.run_dir.cmp(&left.run_dir))
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SessionListJsonEntry {
    run_dir: PathBuf,
    cursor: String,
    #[serde(flatten)]
    catalog: SessionCatalogEntry,
}

impl From<&SessionInspectionEntry> for SessionListJsonEntry {
    fn from(entry: &SessionInspectionEntry) -> Self {
        Self {
            run_dir: entry.run_dir.clone(),
            cursor: entry_cursor(entry),
            catalog: entry.catalog.clone(),
        }
    }
}

pub(super) fn list_sessions(
    command: SessionsListCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let entries = match inspect_session_catalog(&session_dir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(stderr, "{err}");
            return 1;
        }
    };

    let entries = match collect_list_entries_checked(entries, &command) {
        Ok(entries) => entries,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return 1;
        }
    };

    if command.json {
        match render_json_session_list(&entries) {
            Ok(body) => {
                let _ = writeln!(stdout, "{body}");
            }
            Err(err) => {
                let _ = writeln!(stderr, "{err}");
                return 1;
            }
        }
    } else {
        let _ = write!(stdout, "{}", render_human_session_table(&entries));
    }

    0
}

pub(super) fn search_sessions(
    mut command: SessionsSearchCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    command.list.search = Some(command.query);
    list_sessions(command.list, global_session_dir, stdout, stderr)
}

pub(super) fn rebuild_index(
    command: SessionsRebuildIndexCommand,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }
    match rebuild_session_catalog_index(&session_dir) {
        Ok(report) if command.json => {
            let body = serde_json::json!({
                "entry_count": report.entries.len(),
                "journals_scanned": report.journals_scanned,
                "journals_opened": report.journals_opened,
                "recovery_reason": report.recovery_reason,
                "index_path": report.index_path,
            });
            let _ = writeln!(stdout, "{body}");
            0
        }
        Ok(report) => {
            let _ = writeln!(
                stdout,
                "rebuilt {} session entries from {} journals at {}",
                report.entries.len(),
                report.journals_scanned,
                report.index_path.display()
            );
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            1
        }
    }
}

pub(super) fn collect_list_entries(
    entries: Vec<SessionInspectionEntry>,
    command: &SessionsListCommand,
) -> Vec<SessionInspectionEntry> {
    collect_list_entries_checked(entries, command).unwrap_or_default()
}

fn collect_list_entries_checked(
    entries: Vec<SessionInspectionEntry>,
    command: &SessionsListCommand,
) -> Result<Vec<SessionInspectionEntry>, String> {
    let mut entries = entries
        .into_iter()
        .filter(SessionInspectionEntry::is_visible_in_operator_history)
        .filter(|entry| matches_list_filters(entry, command))
        .collect::<Vec<_>>();
    command.sort.sort_entries(&mut entries);
    let entries = if let Some(cursor) = command.cursor.as_deref() {
        let index = entries
            .iter()
            .position(|entry| entry_cursor(entry) == cursor)
            .ok_or_else(|| format!("session list cursor is invalid or stale: {cursor}"))?;
        entries.into_iter().skip(index + 1).collect()
    } else {
        entries
    };
    Ok(entries
        .into_iter()
        .skip(command.offset)
        .take(command.limit)
        .collect())
}

fn entry_cursor(entry: &SessionInspectionEntry) -> String {
    format!(
        "{:032x}:{}:{}",
        entry.sort_unix_ms,
        entry.catalog.run_id,
        hex_path(&entry.run_dir)
    )
}

fn hex_path(path: &std::path::Path) -> String {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(String::new(), |mut encoded, byte| {
            write!(encoded, "{byte:02x}").unwrap_or_abort();
            encoded
        })
}

fn matches_list_filters(entry: &SessionInspectionEntry, command: &SessionsListCommand) -> bool {
    command
        .status
        .is_none_or(|status| status.matches(entry.catalog.status))
        && command
            .profile
            .as_deref()
            .is_none_or(|profile| entry.catalog.profile_preset.as_deref() == Some(profile))
        && command
            .resumable
            .is_none_or(|resumable| entry.catalog.is_resumable == resumable)
        && command.search.as_deref().is_none_or(|query| {
            let query = query.to_ascii_lowercase();
            [
                entry.catalog.run_id.as_str(),
                entry.catalog.run_name.as_deref().unwrap_or_default(),
                entry.catalog.workspace_root.as_deref().unwrap_or_default(),
                entry.catalog.profile_preset.as_deref().unwrap_or_default(),
            ]
            .iter()
            .any(|value| value.to_ascii_lowercase().contains(&query))
        })
}

pub(super) fn render_json_session_list(
    entries: &[SessionInspectionEntry],
) -> Result<String, String> {
    let json_entries = entries
        .iter()
        .map(SessionListJsonEntry::from)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json_entries)
        .map_err(|err| format!("failed to serialize session list: {err}"))
}

pub(super) fn render_human_session_table(entries: &[SessionInspectionEntry]) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<7} {:<9} {:<8} {:<30} {:<20} reason",
        "run_id",
        "status",
        "run_name",
        "profile",
        "provider/model",
        "mode",
        "resume",
        "artifacts",
        "children",
        "session_path",
        "parent"
    )
    .unwrap_or_abort();

    for entry in entries {
        writeln!(
            output,
            "{:<40} {:<12} {:<16} {:<24} {:<20} {:<16} {:<7} {:<9} {:<8} {:<30} {:<20} {}",
            entry.catalog.run_id,
            status_label(entry.catalog.status),
            entry.catalog.run_name.as_deref().unwrap_or("<unavailable>"),
            entry
                .catalog
                .profile_preset
                .as_deref()
                .unwrap_or("<unavailable>"),
            entry
                .catalog
                .provider_model
                .as_deref()
                .unwrap_or("<unavailable>"),
            mode_source_label(entry.catalog.mode_source),
            resumable_label(entry.catalog.is_resumable),
            entry.artifact_count,
            entry.child_session_count,
            entry.run_dir.display(),
            entry.catalog.parent_session_id.as_deref().unwrap_or("-"),
            entry
                .catalog
                .resume_disabled_reason
                .as_deref()
                .unwrap_or("-")
        )
        .unwrap_or_abort();
    }

    output
}
