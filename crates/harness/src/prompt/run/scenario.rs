use super::RunCommand;
use crate::{
    cli_io::{
        wait_for_permission_id, wait_for_tool_finished, ToolFinishTerminalEvents,
        DEFAULT_EVENT_WAIT_TIMEOUT,
    },
    scenarios::{self, ScenarioName},
    CliDeps, CliIo,
};
use harness_core::{
    config::load_resolved_config_with_lookup,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo},
    event::ToolCallStatus,
    perm::PermissionDecision,
    proj::SessionModeSource,
    redact::DefaultRedactor,
    store::{self, Journal},
};
use std::{
    io::{BufRead, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn execute(
    command: RunCommand,
    scenario: ScenarioName,
    config: Option<&Path>,
    sessions: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = deps.current_dir().map_err(|e| e.to_string())?;
    let config_path = config.map(|path| root.join(path));
    let config = load_resolved_config_with_lookup(
        config_path.as_deref(),
        &deps.config_load_context(),
        &|name| deps.env_var_value(name),
    )
    .map_err(|e| e.to_string())?
    .map(|loaded| loaded.config)
    .unwrap_or_default();
    let deterministic = command.deterministic
        || config.runtime.deterministic.enabled
        || deps
            .env_var_value("HARNESS_DETERMINISTIC")
            .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes"));
    let sessions =
        deps.session_directory(&sessions.unwrap_or_else(|| config.runtime.session_dir.clone()))?;
    store::create_private_dir(&sessions).map_err(|e| e.to_string())?;
    // Generated fixtures may be replaced, but concurrent scenario writers must not reset one another.
    let lock =
        store::open_private_append(&sessions.join(".scenario.lock")).map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|e| e.to_string())?;
    let id = deterministic
        .then(|| scenarios::deterministic_run_id(config.runtime.deterministic.seed, scenario));
    if let Some(id) = &id {
        reset_fixture(&sessions, id)?;
    }
    let workspace = scenarios::create_workspace(&sessions, scenario, id.as_deref())?;
    let decision = if scenario.interactive_permissions() {
        approval(io)?
    } else {
        PermissionDecision::Allow
    };
    let mut setup = CoordinatorConfig::new(sessions);
    if let Some(data_dir) =
        harness_core::storage_paths::data_dir_from_lookup(&|name| deps.env_var_value(name))
    {
        setup.data_dir = data_dir;
    }
    setup.run_id_override = id;
    setup.formatter = Arc::new(config.formatter.clone());
    setup.hook_runtime_config = harness_core::config::HookRuntimeConfig {
        hooks: config.hooks.clone(),
        shell_allowlist: config.permissions.shell_allowlist.clone(),
        suppress_execution: deterministic,
    };
    setup.permission_policy = scenarios::default_permission_policy();
    let mut registry =
        harness_tools::coordinator_registry(config.permissions.shell_allowlist.clone());
    harness_tools::register_github_tools(&mut registry, &|key| deps.env_var_value(key));
    harness_tools::register_shell_tool(
        &mut registry,
        config.permissions.shell_allowlist.clone(),
        &|key| deps.env_var_value(key),
    );
    setup.tool_registry = Arc::new(registry);
    setup.agent_profiles = scenarios::golden_path_profiles();
    setup.provider = deps
        .provider_override()
        .unwrap_or_else(|| Arc::new(scenarios::golden_path_provider()));
    setup.session_mode_source = Some(SessionModeSource::ScenarioFixture);
    let digest = blake3::hash(&serde_json::to_vec(&config).map_err(|e| e.to_string())?)
        .to_hex()
        .to_string();
    crate::cli_config::apply_runtime_metadata(&mut setup, deterministic, &digest);
    crate::cli_io::with_output_worker(io, move |io| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let coordinator = spawn_coordinator(
                setup,
                deps.clock(deterministic),
                Arc::new(DefaultRedactor::default()),
            );
            let run = coordinator
                .start_run(scenario.as_str(), &workspace)
                .await
                .map_err(|e| e.to_string())?;
            let result = edit(&coordinator, &run, decision).await;
            match &result {
                Ok(()) => coordinator.stop_run().await,
                Err(error) => coordinator.fail_run(error).await,
            }
            .map_err(|e| e.to_string())?;
            result?;
            if let Some(path) = command.out {
                let redactor = coordinator
                    .output_redactor()
                    .await
                    .map_err(|e| e.to_string())?;
                super::super::session::export(
                    &run.events_path,
                    &root.join(path),
                    redactor.as_ref(),
                )?;
            }
            if command.print_run_dir {
                writeln!(io.stdout, "{}", run.run_dir.display())
            } else {
                writeln!(
                    io.stdout,
                    "scenario {} complete: {}",
                    scenario.as_str(),
                    run.events_path.display()
                )
            }
            .map_err(|e| e.to_string())
        })
    })
}

async fn edit(
    coordinator: &CoordinatorHandle,
    run: &RunInfo,
    decision: PermissionDecision,
) -> Result<(), String> {
    let agent = coordinator
        .spawn_agent_idle(scenarios::supervisor_actor(), "default", None)
        .await
        .map_err(|e| e.to_string())?;
    let tool = coordinator
        .request_tool_call(
            scenarios::worker_actor(agent),
            None,
            "edit",
            scenarios::golden_path_edit_args(),
        )
        .await
        .map_err(|e| e.to_string())?;
    let permission =
        wait_for_permission_id(&run.events_path, &tool, DEFAULT_EVENT_WAIT_TIMEOUT).await?;
    coordinator
        .resolve_permission(permission, decision, None)
        .await
        .map_err(|e| e.to_string())?;
    let status = wait_for_tool_finished(
        &run.events_path,
        &tool,
        Some(DEFAULT_EVENT_WAIT_TIMEOUT),
        ToolFinishTerminalEvents::Error,
    )
    .await?;
    if status == ToolCallStatus::Succeeded {
        Ok(())
    } else {
        Err(format!("scenario edit did not succeed: {status:?}"))
    }
}
fn approval(io: &mut CliIo<'_>) -> Result<PermissionDecision, String> {
    writeln!(
        io.stdout,
        "permission requested: edit demo.txt (allow/deny)"
    )
    .map_err(|e| e.to_string())?;
    io.stdout.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io.stdin
        .take(4097)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.len() > 4096 {
        return Err("permission response exceeds 4 KiB".into());
    }
    Ok(
        if matches!(
            line.trim().to_ascii_lowercase().as_str(),
            "allow" | "a" | "y"
        ) {
            PermissionDecision::Allow
        } else {
            PermissionDecision::Deny
        },
    )
}
fn reset_fixture(sessions: &Path, id: &str) -> Result<(), String> {
    let directory = sessions.join(id);
    if !directory.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let metadata = harness_core::proj::load_run_metadata(&directory)
        .ok_or("cannot replace a fixture with missing or invalid metadata")?;
    if metadata.mode_source != Some(SessionModeSource::ScenarioFixture) || metadata.run_id != id {
        return Err(
            "refusing to replace a session that is not a generated scenario fixture".into(),
        );
    }
    let _writer = Journal::open_existing(sessions, id, true).map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(directory).map_err(|e| e.to_string())
}
