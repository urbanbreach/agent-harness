use super::*;
use harness_core::{crash_recovery::*, session_lineage::*};
use std::collections::BTreeMap;

pub(super) fn rewind(path: &Path, cwd: &Path, command: Rewind) -> Result<Value, String> {
    use harness_core::coord::{
        plan_saved_session_rewind, spawn_coordinator, CoordinatorConfig, FileSnapshotEntry,
    };
    let report = if command.dry_run {
        plan_saved_session_rewind(&crate::replay::read_bounded_history(path)?, command.cutoff)
            .map_err(|e| e.to_string())?
    } else {
        use std::io::Read;
        let snapshot = cwd.join(command.snapshot.ok_or("--snapshot is required")?);
        let mut bytes = Vec::new();
        harness_core::store::open_private_file(&snapshot)
            .map_err(|e| e.to_string())?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("snapshot document exceeds 64 MiB".into());
        }
        let snapshot: Vec<FileSnapshotEntry> =
            serde_json::from_slice(&bytes).map_err(|_| "invalid snapshot JSON")?;
        let workspace = cwd.join(command.workspace.ok_or("--workspace is required")?);
        let root = path.parent().ok_or("session parent missing")?.to_owned();
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid session id")?
            .to_owned();
        std::thread::Builder::new()
            .name("harness-snapshot-restore".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                runtime.block_on(async {
                    let coordinator = spawn_coordinator(
                        CoordinatorConfig::new(root),
                        std::sync::Arc::new(harness_core::clock::RealClock::new()),
                        std::sync::Arc::new(harness_core::redact::DefaultRedactor::default()),
                    );
                    coordinator
                        .restore_saved_session_snapshot(id, command.cutoff, workspace, snapshot)
                        .await
                        .map_err(|e| e.to_string())
                })
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "snapshot restore worker stopped")??
    };
    let mut report = serde_json::to_value(report).map_err(|e| e.to_string())?;
    report["harness_operation"] = if command.dry_run {
        "rewind-dry-run"
    } else {
        "rewind"
    }
    .into();
    report["run_dir"] = json!(path);
    report["conversation_projection_only"] = true.into();
    Ok(report)
}

pub(super) fn branch(path: &Path, cutoff: Option<u64>) -> Result<Value, String> {
    let events = crate::replay::read_bounded_history(path)?;
    let prefix = match cutoff {
        Some(seq) => validate_fork_stable_prefix(&events, seq),
        None => latest_clone_stable_prefix(&events),
    }
    .map_err(|e| e.to_string())?;
    let child = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: path,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .map_err(|e| e.to_string())?;
    let mut report = serde_json::to_value(child).map_err(|e| e.to_string())?;
    report["harness_operation"] = if cutoff.is_some() { "fork" } else { "clone" }.into();
    report["source_run_dir"] = json!(path);
    Ok(report)
}

pub(super) fn tree(
    root: &Path,
    selected: Option<&Path>,
    filter: Option<&str>,
) -> Result<Value, String> {
    let selected_id = selected
        .map(crate::replay::inspect_session)
        .transpose()?
        .map(|e| e.catalog.run_id);
    let root = selected.and_then(Path::parent).unwrap_or(root);
    let entries = crate::replay::inspect_session_catalog(root)?;
    let paths: BTreeMap<_, _> = entries
        .iter()
        .map(|e| (e.catalog.run_id.clone(), e.run_dir.clone()))
        .collect();
    let tree = project_lineage_tree(entries.into_iter().map(|e| e.catalog));
    let mut stack: Vec<_> = tree
        .roots
        .into_iter()
        .rev()
        .map(|node| (node, 0, selected_id.is_none()))
        .collect();
    let filter = filter.map(str::to_lowercase);
    let mut rows = Vec::new();
    while let Some((node, depth, inside)) = stack.pop() {
        let selected = selected_id.as_ref() == Some(&node.entry.run_id);
        let inside = inside || selected;
        let depth = if selected { 0 } else { depth };
        let path = paths.get(&node.entry.run_id);
        let matched = filter.as_ref().is_none_or(|query| {
            [
                Some(node.entry.run_id.as_str()),
                node.entry.run_name.as_deref(),
                node.entry.parent_session_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(query))
                || path.is_some_and(|p| p.to_string_lossy().to_lowercase().contains(query))
        });
        if inside && matched {
            let mut row = serde_json::to_value(node.entry).map_err(|e| e.to_string())?;
            row["run_dir"] = json!(path);
            row["depth"] = json!(depth);
            rows.push(row);
        }
        stack.extend(
            node.children
                .into_iter()
                .rev()
                .map(|node| (node, depth + 1, inside)),
        );
    }
    Ok(
        json!({"session_count":rows.len(),"harness_lineage":rows,"root":selected_id,"filter":filter}),
    )
}

pub(super) fn reopen(path: &Path) -> Result<Value, String> {
    let before = inspect_previous_crash(path);
    let recovery = if before.previous_crash_detected {
        let parent = path.parent().ok_or("session parent missing")?;
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid session id")?;
        Some(apply_crash_recovery(parent, id, false).map_err(|e| e.to_string())?)
    } else {
        None
    };
    Ok(json!({"summary":harness_core::proj::inspect_resume_plan(path),"crash_recovery":recovery}))
}

pub(super) fn continue_session(
    path: PathBuf,
    root: PathBuf,
    config: Option<&Path>,
    exit_on_finish: bool,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let plan = harness_core::proj::inspect_resume_plan(&path);
    if !plan.is_resumable {
        return Err(plan
            .resume_disabled_reason
            .unwrap_or_else(|| "session cannot be resumed".into()));
    }
    let result = crate::tui::execute_with_io(
        crate::tui::TuiCommand {
            continue_session: Some(path),
            replay: None,
            scenario: None,
            mock: false,
            yolo: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        config.map(Path::to_path_buf),
        Some(root),
        deps.config_load_context(),
        io.stderr,
    );
    if result == std::process::ExitCode::SUCCESS {
        Ok(())
    } else {
        Err("session continuation failed".into())
    }
}
