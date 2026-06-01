use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use harness_core::clock::Determinism;
use harness_core::config::{HarnessConfig, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::ToolCallStatus;
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;

use crate::cli_config::{apply_runtime_metadata, load_optional_config_with_digest};
use crate::logging;
use crate::scenarios::{
    create_workspace, default_permission_policy, deterministic_run_id, golden_path_edit_args,
    golden_path_profiles, golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

use crate::cli_io::{
    copy_events_file, wait_for_permission_id, wait_for_tool_finished, ToolFinishTerminalEvents,
    DEFAULT_EVENT_WAIT_TIMEOUT,
};
use crate::defaults::DEFAULT_SESSION_DIR;
use crate::prompt::{PromptCommand, PromptOutputFormat};
use crate::{prompt, CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub struct RunCommand {
    #[arg(long, value_enum)]
    pub scenario: Option<ScenarioName>,

    #[arg(long, default_value_t = false)]
    pub deterministic: bool,

    #[arg(long)]
    pub session_dir: Option<PathBuf>,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub print_run_dir: bool,

    #[arg(long, default_value_t = false)]
    pub mock: bool,

    #[arg(long, default_value_t = false)]
    pub stdin: bool,

    #[arg(
        long = "continue",
        short = 'c',
        default_value_t = false,
        conflicts_with = "session"
    )]
    pub continue_session: bool,

    #[arg(long, short = 's', value_name = "RUN_ID_OR_PATH")]
    pub session: Option<String>,

    #[arg(long, short = 'm')]
    pub model: Option<String>,

    #[arg(long = "agent")]
    pub agent: Option<String>,

    #[arg(long)]
    pub variant: Option<String>,

    #[arg(long, default_value_t = false)]
    pub thinking: bool,

    #[arg(long, value_enum, default_value_t = PromptOutputFormat::Default)]
    pub format: PromptOutputFormat,

    #[arg(long = "file", short = 'f', value_name = "PATH")]
    pub files: Vec<PathBuf>,

    #[arg(long = "dangerously-skip-permissions", default_value_t = false)]
    pub dangerously_skip_permissions: bool,

    #[arg(long, short = 'i', default_value_t = false)]
    pub interactive: bool,

    #[arg(long)]
    pub command: Option<String>,

    #[arg(value_name = "MESSAGE", num_args = 0..)]
    pub message: Vec<String>,
}

struct RunSettings {
    config: Option<HarnessConfig>,
    session_dir: PathBuf,
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
}

pub fn execute_with_io(
    cmd: RunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    if cmd.scenario.is_none() {
        return execute_prompt_run(cmd, config_path, global_session_dir, io, deps);
    }

    let settings = match resolve_settings(&cmd, config_path, global_session_dir) {
        Ok(settings) => settings,
        Err(err) => {
            let _ = writeln!(io.stderr, "run setup failed: {err}");
            return 2;
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(io.stderr, "failed to build async runtime: {err}");
            return 1;
        }
    };

    match runtime.block_on(run_once(&cmd, &settings, io, deps)) {
        Ok(outcome) => {
            if let Some(out) = &cmd.out {
                if let Err(err) = copy_events_file(&outcome.events_path, out) {
                    let _ = writeln!(io.stderr, "failed to write --out file: {err}");
                    return 1;
                }
            }

            if cmd.print_run_dir {
                let _ = writeln!(io.stdout, "{}", outcome.run_dir.display());
            } else {
                let _ = writeln!(
                    io.stdout,
                    "scenario {} complete: {}",
                    scenario(&cmd).as_str(),
                    outcome.events_path.display()
                );
            }

            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "run failed: {err}");
            1
        }
    }
}

fn execute_prompt_run(
    cmd: RunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    if cmd.interactive {
        let _ = writeln!(
            io.stderr,
            "run setup failed: --interactive is not implemented in this slice"
        );
        return 2;
    }
    if cmd.command.is_some() {
        let _ = writeln!(
            io.stderr,
            "run setup failed: --command is not implemented in this slice"
        );
        return 2;
    }

    let prompt_text = match resolve_run_prompt_text(&cmd, io, deps) {
        Ok(prompt_text) => prompt_text,
        Err(err) => {
            let _ = writeln!(io.stderr, "run setup failed: {err}");
            return 2;
        }
    };

    let resume = if cmd.continue_session {
        match latest_resumable_session(&cmd, config_path.as_deref(), global_session_dir.clone()) {
            Ok(session) => Some(session),
            Err(err) => {
                let _ = writeln!(io.stderr, "run setup failed: {err}");
                return 2;
            }
        }
    } else {
        cmd.session.clone()
    };

    let prompt_cmd = PromptCommand {
        text: Some(prompt_text),
        stdin: false,
        message: Vec::new(),
        model: cmd.model.clone(),
        variant: cmd.variant.clone(),
        thinking: cmd.thinking,
        mock: cmd.mock,
        profile: cmd.agent.clone(),
        resume,
        out: cmd.out.clone(),
        print_run_dir: cmd.print_run_dir,
        format: cmd.format,
    };

    prompt::execute_with_io(
        prompt_cmd,
        config_path,
        cmd.session_dir.clone().or(global_session_dir),
        io,
        deps,
    )
}

fn resolve_run_prompt_text(
    cmd: &RunCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<String, String> {
    let mut parts = Vec::new();
    if !cmd.message.is_empty() {
        parts.push(cmd.message.join(" "));
    }

    if cmd.stdin || !io.stdin_is_terminal() {
        let mut stdin = String::new();
        io.stdin
            .read_to_string(&mut stdin)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
        let trimmed = stdin.trim_end_matches(['\r', '\n']);
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        } else if cmd.stdin && parts.is_empty() {
            return Err("stdin was empty; pass a positional message or pipe a prompt".to_string());
        }
    }

    if parts.is_empty() {
        return Err("no prompt text provided; pass a positional message or pipe stdin".to_string());
    }

    let current_dir = deps
        .current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    for file in &cmd.files {
        let resolved = if file.is_absolute() {
            file.clone()
        } else {
            current_dir.join(file)
        };
        if !resolved.exists() {
            return Err(format!("--file path does not exist: {}", file.display()));
        }
    }

    let mut prompt_text = parts.join("\n");
    for file in &cmd.files {
        prompt_text.push_str("\n@");
        prompt_text.push_str(&file.to_string_lossy());
    }
    Ok(prompt_text)
}

fn latest_resumable_session(
    cmd: &RunCommand,
    config_path: Option<&Path>,
    global_session_dir: Option<PathBuf>,
) -> Result<String, String> {
    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .or_else(|| {
            load_optional_config_with_digest(config_path)
                .ok()
                .flatten()
                .map(|loaded| loaded.config.paths.session_dir)
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR));

    let entries = fs::read_dir(&session_dir).map_err(|err| {
        format!(
            "failed to read session dir {}: {err}",
            session_dir.display()
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read session entry: {err}"))?;
        let path = entry.path();
        if !path.join("events.jsonl").is_file() {
            continue;
        }
        let recovery = crate::recovery::inspect_session_recovery(&path)?;
        if !recovery.resumable {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        candidates.push((modified, recovery.run_id));
    }

    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    candidates
        .into_iter()
        .map(|(_, run_id)| run_id)
        .next()
        .ok_or_else(|| "no resumable sessions found for --continue".to_string())
}

fn scenario(cmd: &RunCommand) -> ScenarioName {
    cmd.scenario
        .expect("scenario runner dispatch requires --scenario")
}

fn resolve_settings(
    cmd: &RunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<RunSettings, String> {
    let mut shell_allowlist = ShellAllowlist::default();
    let mut config_session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut config_deterministic = false;
    let mut config_seed = 0;
    let mut config_digest = "none".to_string();
    let mut loaded_config = None;

    if let Some(loaded) = load_optional_config_with_digest(config_path.as_deref())? {
        let config = loaded.config;
        config_digest = loaded.digest;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        loaded_config = Some(config);
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic || Determinism::enabled(config_deterministic);

    Ok(RunSettings {
        config: loaded_config,
        session_dir,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
    })
}

struct RunOutcome {
    run_dir: PathBuf,
    events_path: PathBuf,
}

async fn run_once(
    cmd: &RunCommand,
    settings: &RunSettings,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<RunOutcome, String> {
    let scenario = scenario(cmd);
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let deterministic_run_id = settings
        .deterministic
        .then(|| deterministic_run_id(settings.seed, scenario));

    if let Some(run_id) = &deterministic_run_id {
        let stale_run_dir = settings.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    let workspace = create_workspace(
        &settings.session_dir,
        scenario,
        deterministic_run_id.as_deref(),
    )?;

    let clock = deps.clock(settings.deterministic);

    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = deps
        .provider_override()
        .unwrap_or_else(|| Arc::new(golden_path_provider()));
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config.run_id_override = deterministic_run_id;
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(scenario.as_str(), &workspace)
        .await
        .map_err(|err| err.to_string())?;

    if let Some(config) = &settings.config {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }

    coordinator
        .spawn_agent(supervisor_actor(), "planner", None)
        .await
        .map_err(|err| err.to_string())?;

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .map_err(|err| err.to_string())?;

    let tool_call_id = coordinator
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit",
            golden_path_edit_args(),
        )
        .await
        .map_err(|err| err.to_string())?;

    let permission_id =
        wait_for_permission_id(&run.events_path, &tool_call_id, DEFAULT_EVENT_WAIT_TIMEOUT).await?;
    let decision = if scenario.interactive_permissions() {
        interactive_permission_decision(&permission_id, io)?
    } else {
        PermissionDecision::Allow
    };

    coordinator
        .resolve_permission(permission_id, decision, None)
        .await
        .map_err(|err| err.to_string())?;

    let tool_status = wait_for_tool_finished(
        &run.events_path,
        &tool_call_id,
        Some(DEFAULT_EVENT_WAIT_TIMEOUT),
        ToolFinishTerminalEvents::Ignore,
    )
    .await?;
    if tool_status != ToolCallStatus::Succeeded {
        let failure = format!("tool call did not succeed: {tool_status:?}");
        coordinator
            .fail_run(failure.clone())
            .await
            .map_err(|err| err.to_string())?;
        return Err(failure);
    }

    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    Ok(RunOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

fn interactive_permission_decision(
    permission_id: &str,
    io: &mut CliIo<'_>,
) -> Result<PermissionDecision, String> {
    writeln!(
        io.stdout,
        "permission requested: {permission_id} (allow/deny)"
    )
    .map_err(|err| format!("failed to write interactive permission prompt: {err}"))?;
    let mut input = String::new();
    io.stdin
        .read_line(&mut input)
        .map_err(|err| format!("failed to read interactive permission input: {err}"))?;

    let normalized = input.trim().to_ascii_lowercase();
    if normalized == "allow" || normalized == "a" || normalized == "y" {
        Ok(PermissionDecision::Allow)
    } else {
        Ok(PermissionDecision::Deny)
    }
}

#[cfg(test)]
mod tests {
    use super::{run_once, RunCommand, RunSettings};
    use crate::cli_io::load_events_file;
    use crate::replay::summarize_session;
    use crate::scenarios::{deterministic_run_id, ScenarioName};
    use crate::CliIo;
    use harness_core::config::ShellAllowlist;
    use harness_core::event::ToolCallStatus as EventToolCallStatus;
    use harness_core::event::{EventV1, PermissionDecision as EventPermissionDecision};
    use harness_core::proj::RunStatus;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn test_io() -> (Cursor<Vec<u8>>, Vec<u8>, Vec<u8>) {
        (Cursor::new(Vec::new()), Vec::new(), Vec::new())
    }

    fn test_run_command(scenario: ScenarioName) -> RunCommand {
        RunCommand {
            scenario: Some(scenario),
            deterministic: true,
            session_dir: None,
            out: None,
            print_run_dir: false,
            mock: false,
            stdin: false,
            continue_session: false,
            session: None,
            model: None,
            agent: None,
            variant: None,
            thinking: false,
            format: crate::prompt::PromptOutputFormat::Default,
            files: Vec::new(),
            dangerously_skip_permissions: false,
            interactive: false,
            command: None,
            message: Vec::new(),
        }
    }

    #[tokio::test]
    async fn deterministic_golden_path_twice_produces_identical_sha256_digest() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            config: None,
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 42,
            config_digest: "none".to_string(),
        };
        let command = test_run_command(ScenarioName::GoldenPath);

        let (mut stdin, mut stdout, mut stderr) = test_io();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let run_a = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("first deterministic run");
        let digest_a = sha256_hex(&std::fs::read(&run_a.events_path).expect("read first jsonl"));

        let run_b = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("second deterministic run");
        let digest_b = sha256_hex(&std::fs::read(&run_b.events_path).expect("read second jsonl"));

        assert_eq!(digest_a, digest_b);
    }

    #[test]
    fn deterministic_run_id_is_stable_for_seed_and_scenario() {
        let a = deterministic_run_id(7, ScenarioName::GoldenPath);
        let b = deterministic_run_id(7, ScenarioName::GoldenPath);
        let c = deterministic_run_id(8, ScenarioName::GoldenPath);
        let d = deterministic_run_id(7, ScenarioName::GoldenPathInteractive);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[tokio::test]
    async fn deterministic_run_writes_stable_meta_json_with_null_created_at() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            config: None,
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 99,
            config_digest: "none".to_string(),
        };
        let command = test_run_command(ScenarioName::GoldenPath);

        let (mut stdin, mut stdout, mut stderr) = test_io();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let run_a = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("first deterministic run");
        let meta_a = std::fs::read(run_a.run_dir.join("meta.json")).expect("read first meta");

        let run_b = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("second deterministic run");
        let meta_b = std::fs::read(run_b.run_dir.join("meta.json")).expect("read second meta");

        assert_eq!(meta_a, meta_b);
        let value: serde_json::Value = serde_json::from_slice(&meta_a).expect("parse meta json");
        assert!(value.get("created_at").is_some());
        assert!(value.get("created_at").unwrap().is_null());
    }

    #[tokio::test]
    async fn replay_summary_matches_expected_values_for_golden_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            config: None,
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 2026,
            config_digest: "none".to_string(),
        };
        let command = test_run_command(ScenarioName::GoldenPath);

        let (mut stdin, mut stdout, mut stderr) = test_io();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let run = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("deterministic golden path run");
        let summary = summarize_session(&run.run_dir).expect("replay summary");

        assert_eq!(summary.status, RunStatus::Finished);
        assert_eq!(summary.counts_by_type.get("run_finished"), Some(&1));
        assert_eq!(summary.counts_by_type.get("edit_applied"), Some(&1));
        assert!(summary.pending_permissions.is_empty());
        assert!(summary.tasks_in_flight.is_empty());
    }

    #[tokio::test]
    async fn edit_applied_diff_refs_match_artifact_written() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            config: None,
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 2027,
            config_digest: "none".to_string(),
        };
        let command = test_run_command(ScenarioName::GoldenPath);

        let (mut stdin, mut stdout, mut stderr) = test_io();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let run = run_once(&command, &settings, &mut io, &crate::CliDeps::real())
            .await
            .expect("deterministic golden path run");
        let events = load_events_file(&run.events_path).expect("load events");

        let (diff_path, diff_digest) = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::EditApplied(data) => {
                    Some((data.diff_rel_path.clone(), data.diff_digest.clone()))
                }
                _ => None,
            })
            .expect("edit applied event");

        assert!(diff_path.is_some(), "EditApplied.diff_rel_path must be set");
        assert!(diff_digest.is_some(), "EditApplied.diff_digest must be set");

        let diff_path = diff_path.expect("diff path");
        let diff_digest = diff_digest.expect("diff digest");
        let artifact_digest = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::ArtifactWritten(data) if data.path == diff_path => {
                    Some(data.digest.clone())
                }
                _ => None,
            })
            .expect("artifact_written for diff path");

        assert_eq!(artifact_digest, diff_digest);
    }

    #[tokio::test]
    async fn interactive_golden_path_deny_emits_edit_rejected_without_applying_file() {
        // arrange
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let session_dir = temp_dir.path().join("sessions");
        let settings = RunSettings {
            config: None,
            session_dir: session_dir.clone(),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 2028,
            config_digest: "none".to_string(),
        };
        let command = test_run_command(ScenarioName::GoldenPathInteractive);

        let mut stdin = Cursor::new(b"deny\n".to_vec());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        // act
        let err = match run_once(&command, &settings, &mut io, &crate::CliDeps::real()).await {
            Ok(_) => panic!("denied edit should fail the interactive scenario"),
            Err(err) => err,
        };

        // assert
        assert!(err.contains("tool call did not succeed: Failed"));

        let run_id = deterministic_run_id(2028, ScenarioName::GoldenPathInteractive);
        let run_dir = session_dir.join(&run_id);
        let events = load_events_file(&run_dir.join("events.jsonl")).expect("load events");
        let tool_call_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::ToolCallRequested(data) if data.tool_id == "edit" => {
                    Some(data.tool_call_id.clone())
                }
                _ => None,
            })
            .expect("edit tool call request");
        let permission_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
                {
                    Some(data.permission_id.clone())
                }
                _ => None,
            })
            .expect("edit permission request");

        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.permission_id == permission_id
                        && data.decision == EventPermissionDecision::Deny
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::EditRejected(data) if data.edit_id == "edit-golden-path"
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id == tool_call_id
                        && data.status == EventToolCallStatus::Failed
            )
        }));
        assert!(!events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::EditApplied(data) if data.edit_id == "edit-golden-path"
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::RunFailed(data) if data.error.contains("tool call did not succeed: Failed")
            )
        }));
        let summary = summarize_session(&run_dir).expect("replay summary");
        assert_eq!(summary.status, RunStatus::Failed);
        assert_eq!(summary.counts_by_type.get("run_failed"), Some(&1));

        let workspace = session_dir
            .join("workspaces")
            .join(format!("golden_path_interactive-{run_id}"));
        let demo = std::fs::read_to_string(workspace.join("demo.txt")).expect("read demo file");
        assert_eq!(demo, "alpha\nbeta\ngamma\n");
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }
}
