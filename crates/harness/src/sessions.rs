use crate::{replay::SessionInspectionEntry, CliDeps, CliIo};
use harness_core::{proj::RunStatus, redact::redact_in_place};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
mod operations;

#[derive(clap::Args)]
pub(crate) struct DashboardCommand {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    action: DashboardAction,
}
#[derive(clap::Subcommand)]
enum DashboardAction {
    List,
    Status,
    Recent {
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        limit: u32,
    },
}
pub(crate) fn dashboard(
    command: DashboardCommand,
    config: Option<&Path>,
    directory: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let cwd = deps.current_dir().map_err(|e| e.to_string())?;
    let config = config.map(|path| cwd.join(path));
    let loaded = harness_core::config::load_resolved_config_with_lookup(
        config.as_deref(),
        &deps.config_load_context(),
        &|name| deps.env_var_value(name),
    )
    .map_err(|e| e.to_string())?;
    let configured = loaded.is_some();
    let config = loaded.map(|c| c.config).unwrap_or_default();
    let root = cwd.join(directory.unwrap_or_else(|| config.runtime.session_dir.clone()));
    let redactor = crate::inspect::redactor(&config, deps)?;
    let mut rows = crate::replay::inspect_session_catalog(&root)?;
    rows.retain(SessionInspectionEntry::is_visible_in_operator_history);
    let mut report = json!({"session_dir":root,"session_count":rows.len()});
    let label = match command.action {
        DashboardAction::Status => {
            report["config_loaded"] = configured.into();
            "status"
        }
        action => {
            let label = if let DashboardAction::Recent { limit } = action {
                rows.truncate(limit as usize);
                report["limit"] = limit.into();
                "recent"
            } else {
                "list"
            };
            report["session_count"] = rows.len().into();
            report["schema_version"] = format!("harness-dashboard-{label}-v1").into();
            report["sessions"] = rows.into_iter().map(|row| json!(row.catalog)).collect();
            label
        }
    };
    redact_in_place(&redactor, &mut report);
    if command.json {
        return crate::inspect::print_json(io, &report);
    }
    writeln!(
        io.stdout,
        "dashboard {label}: {} sessions in {}",
        report["session_count"],
        report["session_dir"].as_str().unwrap_or("")
    )
    .map_err(|e| e.to_string())?;
    if let Some(rows) = report["sessions"].as_array() {
        for row in rows {
            writeln!(
                io.stdout,
                "{}\t{}\t{}",
                row["run_id"].as_str().unwrap_or("?"),
                row["status"].as_str().unwrap_or("unavailable"),
                row["provider_model"].as_str().unwrap_or("-")
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[derive(clap::Args)]
pub(crate) struct SessionsCommand {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    List(List),
    RebuildIndex,
    Search {
        query: String,
        #[command(flatten)]
        list: List,
    },
    Inspect {
        #[arg(long = "run", conflicts_with = "session")]
        run_id: Option<String>,
        #[arg(required_unless_present = "run_id")]
        session: Option<String>,
    },
    Replay {
        session: String,
    },
    /// Preview a historical cutoff, or restore supplied workspace files without changing history.
    Rewind(Rewind),
    /// Export a redacted support bundle, without provider deltas or tool payloads.
    Export(crate::exports::ExportCommand),
    Reopen {
        #[arg(long)]
        session: String,
    },
    Continue {
        session: String,
        #[arg(long)]
        exit_on_finish: bool,
    },
    Fork {
        #[arg(long)]
        source: String,
        #[arg(long)]
        cutoff: u64,
    },
    Clone {
        #[arg(long)]
        source: String,
    },
    Tree {
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        filter: Option<String>,
    },
    Import {
        #[arg(long)]
        from: PathBuf,
    },
    Discover {
        #[arg(long)]
        from: PathBuf,
    },
    CrashScan {
        #[arg(long)]
        from: Option<PathBuf>,
    },
}
#[derive(clap::Args)]
struct Rewind {
    session: String,
    #[arg(long)]
    cutoff: u64,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, required_unless_present = "dry_run")]
    workspace: Option<PathBuf>,
    #[arg(long, required_unless_present = "dry_run")]
    snapshot: Option<PathBuf>,
}
#[derive(clap::Args)]
struct List {
    #[arg(long, value_enum)]
    status: Option<Status>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    resumable: Option<bool>,
    #[arg(long, value_enum, default_value = "updated_desc")]
    sort: Sort,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long)]
    search: Option<String>,
}
#[derive(Clone, Copy, clap::ValueEnum)]
enum Status {
    Running,
    Finished,
    Failed,
    Unavailable,
}
#[derive(Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "snake_case")]
enum Sort {
    UpdatedDesc,
    UpdatedAsc,
    RunIdAsc,
    RunIdDesc,
}

pub(crate) fn execute(
    command: SessionsCommand,
    config: Option<&Path>,
    directory: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let cwd = deps.current_dir().map_err(|e| e.to_string())?;
    let configured = crate::inspect::configured(config, deps)?;
    let root = cwd.join(directory.unwrap_or_else(|| configured.config.runtime.session_dir.clone()));
    let redactor = crate::inspect::redactor(&configured.config, deps)?;
    let resolve = |session: &str| crate::recovery::resolve_session_run_dir(session, &root, &cwd);
    let mut report = match command.action {
        Action::List(list) => list_sessions(&root, list)?,
        Action::RebuildIndex => crate::replay::rebuild_session_catalog_index(&root, &redactor)?,
        Action::Search { query, mut list } => {
            list.search = Some(query);
            list_sessions(&root, list)?
        }
        Action::Inspect { run_id, session } => {
            let path = crate::recovery::resolve_session_run_dir(
                session
                    .as_deref()
                    .or(run_id.as_deref())
                    .ok_or("missing session")?,
                &root,
                &cwd,
            )?;
            let entry = crate::replay::inspect_session(&path)?;
            json!({"run_dir":path,"catalog":entry.catalog,
                "readiness":harness_core::proj::inspect_resume_plan(&path),
                "previous_crash":harness_core::crash_recovery::inspect_previous_crash(&path)})
        }
        Action::Replay { session } => crate::replay::report(
            &crate::recovery::resolve_session_run_dir(&session, &root, &cwd)?,
        )?,
        Action::Rewind(command) => operations::rewind(&resolve(&command.session)?, &cwd, command)?,
        Action::Export(command) => {
            let path = resolve(&command.session)?;
            let bytes = crate::exports::checked_json(
                crate::exports::bundle(&path, &configured.config, true)?,
                &redactor,
            )?;
            return crate::exports::write_output(
                &bytes,
                command.output.as_deref(),
                &path,
                io,
                deps,
            );
        }
        Action::Reopen { session } => operations::reopen(&resolve(&session)?)?,
        Action::Continue {
            session,
            exit_on_finish,
        } => {
            return operations::continue_session(
                resolve(&session)?,
                root,
                config,
                exit_on_finish,
                io,
                deps,
            );
        }
        Action::Fork { source, cutoff } => operations::branch(&resolve(&source)?, Some(cutoff))?,
        Action::Clone { source } => operations::branch(&resolve(&source)?, None)?,
        Action::Tree {
            root: selected,
            filter,
        } => operations::tree(
            &root,
            selected.as_deref().map(resolve).transpose()?.as_deref(),
            filter.as_deref(),
        )?,
        Action::Import { from } => json!(
            harness_core::foreign_session::import_foreign_session_as_replay(&cwd.join(from), &root)
                .map_err(|e| e.to_string())?
        ),
        Action::Discover { from } => {
            let from = cwd.join(from);
            let candidates = harness_core::foreign_session::discover_foreign_sessions(&from)
                .map_err(|e| e.to_string())?;
            json!({"scan_root":from,"count":candidates.len(),"candidates":candidates})
        }
        Action::CrashScan { from } => {
            let from = from.map_or(root, |path| cwd.join(path));
            std::fs::read_dir(&from).map_err(|e| e.to_string())?;
            let reports = harness_core::crash_recovery::scan_previous_crashes(&from);
            json!({"scan_root":from,"summary":harness_core::crash_recovery::summarize_crash_reports(&reports),"reports":reports})
        }
    };
    redact_in_place(&redactor, &mut report);
    if !command.json
        && let Some(rows) = report.as_array()
    {
        writeln!(io.stdout, "run_id\tstatus\tresumable\ttitle").map_err(|e| e.to_string())?;
        for row in rows {
            writeln!(
                io.stdout,
                "{}\t{}\t{}\t{}",
                row["run_id"].as_str().unwrap_or("?"),
                row["status"].as_str().unwrap_or("unavailable"),
                row["is_resumable"],
                row["run_name"].as_str().unwrap_or("")
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    crate::inspect::print_json(io, &report)
}

fn list_sessions(root: &Path, list: List) -> Result<Value, String> {
    let mut entries = crate::replay::inspect_session_catalog(root)?;
    let search = list.search.as_deref().map(str::to_lowercase);
    entries.retain(|entry| {
        let catalog = &entry.catalog;
        entry.is_visible_in_operator_history()
            && list.status.is_none_or(|status| match status {
                Status::Running => catalog.status == Some(RunStatus::Running),
                Status::Finished => catalog.status == Some(RunStatus::Finished),
                Status::Failed => catalog.status == Some(RunStatus::Failed),
                Status::Unavailable => catalog.status.is_none(),
            })
            && list
                .profile
                .as_ref()
                .is_none_or(|p| catalog.profile_preset.as_ref() == Some(p))
            && list.resumable.is_none_or(|r| catalog.is_resumable == r)
            && search.as_ref().is_none_or(|query| {
                [
                    Some(catalog.run_id.as_str()),
                    catalog.run_name.as_deref(),
                    catalog.workspace_root.as_deref(),
                    catalog.profile_preset.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|s| s.to_lowercase().contains(query))
            })
    });
    match list.sort {
        Sort::UpdatedDesc => {}
        Sort::UpdatedAsc => entries.sort_by(|a, b| {
            a.sort_unix_ms
                .cmp(&b.sort_unix_ms)
                .then_with(|| a.catalog.run_id.cmp(&b.catalog.run_id))
        }),
        Sort::RunIdAsc => entries.sort_by(|a, b| a.catalog.run_id.cmp(&b.catalog.run_id)),
        Sort::RunIdDesc => entries.sort_by(|a, b| b.catalog.run_id.cmp(&a.catalog.run_id)),
    }
    let start = list
        .cursor
        .as_deref()
        .map(|value| {
            entries
                .iter()
                .position(|e| cursor(e) == value)
                .map(|i| i + 1)
                .ok_or_else(|| "session cursor is invalid or stale".to_owned())
        })
        .transpose()?
        .unwrap_or(0);
    entries
        .into_iter()
        .skip(start.saturating_add(list.offset))
        .take(list.limit.min(10_000))
        .map(|entry| {
            let mut row = serde_json::to_value(&entry.catalog).map_err(|e| e.to_string())?;
            row["run_dir"] = json!(entry.run_dir);
            row["cursor"] = cursor(&entry).into();
            Ok(row)
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}
fn cursor(entry: &SessionInspectionEntry) -> String {
    format!(
        "{:032x}:{}:{}",
        entry.sort_unix_ms,
        entry.catalog.run_id,
        blake3::hash(entry.run_dir.as_os_str().as_encoded_bytes()).to_hex()
    )
}
