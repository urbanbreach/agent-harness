use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use harness_core::agent::{default_model_settings_for_profile, AgentModelSettings};
use harness_core::clock::Determinism;
use harness_core::config::{resolve_configured_model_metadata, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::perm::PermissionPolicy;
use harness_core::proj::{inspect_resume_plan, SessionModeSource};
use harness_core::redact::DefaultRedactor;
use harness_core::session_title::create_default_title;
use harness_tools::coordinator_registry;
use uuid::Uuid;

use crate::cli_config::{apply_runtime_metadata, load_optional_config_with_digest_context};
use crate::cli_io::{copy_events_file, load_events_from_run_dir};
use crate::defaults::{
    DEFAULT_INTERACTIVE_RUN_NAME, DEFAULT_MOCK_PROFILE, DEFAULT_SESSION_DIR,
    RESUME_UNAVAILABLE_FALLBACK_REASON,
};
use crate::recovery::{
    inspect_session_recovery, latest_run_name, resolve_session_run_dir, select_resume_agent_id,
};
use crate::{
    bootstrap, logging,
    scenarios::{golden_path_profiles, golden_path_provider, supervisor_actor},
    CliDeps,
};

mod stream;
#[cfg(test)]
mod tests;

use stream::{prompt_wait_timeout, wait_for_prompt_completion_with_output};

#[derive(Debug, Args, Clone)]
pub struct PromptCommand {
    #[arg(long, conflicts_with_all = ["stdin", "message"])]
    pub text: Option<String>,

    #[arg(long, default_value_t = false, conflicts_with_all = ["text", "message"])]
    pub stdin: bool,

    #[arg(value_name = "TEXT", num_args = 1.., conflicts_with_all = ["text", "stdin"])]
    pub message: Vec<String>,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long)]
    pub variant: Option<String>,

    #[arg(long, default_value_t = false)]
    pub thinking: bool,

    #[arg(long, default_value_t = false, conflicts_with = "resume")]
    pub mock: bool,

    #[arg(long, conflicts_with = "resume")]
    pub profile: Option<String>,

    #[arg(long, value_name = "RUN_ID_OR_PATH", conflicts_with = "profile")]
    pub resume: Option<String>,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub print_run_dir: bool,

    #[arg(long, value_enum, default_value_t = PromptOutputFormat::Default)]
    pub format: PromptOutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PromptOutputFormat {
    Default,
    Json,
}

pub fn execute_with_io(
    cmd: PromptCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    io: &mut crate::CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let prompt_text = match resolve_prompt_text(&cmd, io.stdin) {
        Ok(prompt_text) => prompt_text,
        Err(err) => {
            let _ = writeln!(io.stderr, "prompt setup failed: {err}");
            return 2;
        }
    };

    let workspace_root = match deps.current_dir() {
        Ok(current_dir) => current_dir,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "prompt setup failed: failed to resolve current working directory: {err}"
            );
            return 2;
        }
    };

    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "prompt setup failed: failed to resolve config context: {err}"
            );
            return 2;
        }
    };

    let settings = match resolve_settings(
        &cmd,
        config_path,
        global_session_dir,
        workspace_root,
        &config_context,
        deps,
    ) {
        Ok(settings) => settings,
        Err(err) => {
            let _ = writeln!(io.stderr, "prompt setup failed: {err}");
            return 2;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(io.stderr, "failed to build async runtime: {err}");
            return 1;
        }
    };

    match runtime.block_on(run_prompt(&cmd, &settings, &prompt_text, io.stdout)) {
        Ok(outcome) => {
            if let Some(out) = &cmd.out {
                if let Err(err) = copy_events_file(&outcome.events_path, out) {
                    let _ = writeln!(io.stderr, "failed to write --out file: {err}");
                    return 1;
                }
            }

            if cmd.print_run_dir {
                let _ = writeln!(io.stdout, "{}", outcome.run_dir.display());
            }

            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "prompt failed: {err}");
            1
        }
    }
}

struct PromptSettings {
    logging_config: Option<harness_core::config::HarnessConfig>,
    coordinator_config: CoordinatorConfig,
    default_profile: String,
    deterministic: bool,
    deterministic_seed: u64,
    config_digest: String,
    workspace_root: PathBuf,
    deps: CliDeps,
}

struct PromptOutcome {
    run_dir: PathBuf,
    events_path: PathBuf,
}

fn resolve_settings(
    cmd: &PromptCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
    deps: &CliDeps,
) -> Result<PromptSettings, String> {
    if cmd.mock {
        return resolve_mock_settings(
            config_path,
            global_session_dir,
            workspace_root,
            config_context,
            deps,
        );
    }

    let loaded = load_optional_config_with_digest_context(config_path.as_deref(), config_context)?;
    let credential_store =
        harness_core::auth::CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let runtime_catalog = crate::runtime_catalog::resolve_runtime_catalog(
        loaded.as_ref().map(|loaded| loaded.config.clone()),
        loaded.as_ref().map(|loaded| loaded.digest.clone()),
        global_session_dir.clone(),
        credential_store.as_ref(),
        &|name| deps.env_var_value(name),
    )?;
    if loaded.is_none() && !runtime_catalog.has_connected_provider() {
        return Err("Connect a provider to send prompts; run `harness auth login` or use TUI `/auth`/`/connect`.".to_string());
    }

    let config = runtime_catalog.config;
    let config_digest = runtime_catalog.config_digest;

    let deterministic = Determinism::enabled(config.deterministic.enabled);
    let deterministic_seed = config.deterministic.seed;
    let default_profile = bootstrap::interactive_profile_name(&config);
    let mut coordinator_config = bootstrap::build_interactive_coordinator_config(&config)?;
    if let Some(provider) = deps.provider_override() {
        coordinator_config.provider = provider;
    }

    Ok(PromptSettings {
        logging_config: Some(config),
        coordinator_config,
        default_profile,
        deterministic,
        deterministic_seed,
        config_digest,
        workspace_root,
        deps: deps.clone(),
    })
}

fn resolve_mock_settings(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
    deps: &CliDeps,
) -> Result<PromptSettings, String> {
    let mut shell_allowlist = ShellAllowlist::default();
    let mut session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut deterministic = false;
    let mut deterministic_seed = 0;
    let mut config_digest = "none".to_string();
    let mut logging_config = None;

    if let Some(loaded) =
        load_optional_config_with_digest_context(config_path.as_deref(), config_context)?
    {
        let mut config = loaded.config;
        config.apply_session_dir_override(global_session_dir.clone());

        config_digest = loaded.digest;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        session_dir = config.paths.session_dir.clone();
        deterministic = config.deterministic.enabled;
        deterministic_seed = config.deterministic.seed;
        logging_config = Some(config);
    }

    let session_dir = global_session_dir.unwrap_or(session_dir);
    deterministic = Determinism::enabled(deterministic);

    let mut coordinator_config = CoordinatorConfig::new(session_dir);
    coordinator_config.permission_policy = default_prompt_permission_policy();
    coordinator_config.tool_registry = Arc::new(coordinator_registry(shell_allowlist));
    coordinator_config.provider = Arc::new(golden_path_provider());
    if let Some(provider) = deps.provider_override() {
        coordinator_config.provider = provider;
    }
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config.formatter = logging_config
        .as_ref()
        .map(|config| config.formatter.clone())
        .unwrap_or_default();

    Ok(PromptSettings {
        logging_config,
        coordinator_config,
        default_profile: DEFAULT_MOCK_PROFILE.to_string(),
        deterministic,
        deterministic_seed,
        config_digest,
        workspace_root,
        deps: deps.clone(),
    })
}

async fn run_prompt(
    cmd: &PromptCommand,
    settings: &PromptSettings,
    prompt_text: &str,
    stdout: &mut dyn Write,
) -> Result<PromptOutcome, String> {
    if let Some(selector) = &cmd.resume {
        return run_resumed_prompt(cmd, settings, selector, prompt_text, stdout).await;
    }

    let mut coordinator_config = settings.coordinator_config.clone();
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    coordinator_config.run_id_override = Some(if settings.deterministic {
        format!("prompt_{:016x}", settings.deterministic_seed)
    } else {
        let entropy = format!(
            "{}:{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        format!(
            "prompt_{}",
            Uuid::new_v5(&Uuid::NAMESPACE_OID, entropy.as_bytes()).simple()
        )
    });

    if let Some(run_id) = &coordinator_config.run_id_override {
        let stale_run_dir = coordinator_config.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    fs::create_dir_all(&coordinator_config.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let clock = settings.deps.clock(settings.deterministic);

    let run_name = create_default_title(clock.as_ref(), false);
    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(run_name, settings.workspace_root.clone())
        .await
        .map_err(|err| err.to_string())?;

    if let Some(config) = &settings.logging_config {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }

    let profile_name = cmd
        .profile
        .clone()
        .unwrap_or_else(|| settings.default_profile.clone());
    let model_override = resolve_prompt_model_override(cmd, settings, &profile_name)?;
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), profile_name, None)
        .await
        .map_err(|err| err.to_string())?;

    let request_id = match model_override {
        Some(model_override) => {
            coordinator
                .request_agent_turn_with_model(
                    user_actor(),
                    agent_id,
                    prompt_text.to_string(),
                    model_override.model_ref,
                    Some(model_override.model_settings),
                )
                .await
        }
        None => {
            coordinator
                .request_agent_turn(user_actor(), agent_id, prompt_text.to_string())
                .await
        }
    }
    .map_err(|err| err.to_string())?;
    let event_store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion_with_output(
        event_store,
        &request_id,
        wait_timeout,
        cmd.thinking,
        cmd.format,
        stdout,
    )
    .await;
    let stop_result = coordinator.stop_run().await;

    wait_result?;
    stop_result.map_err(|err| err.to_string())?;

    Ok(PromptOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

async fn run_resumed_prompt(
    cmd: &PromptCommand,
    settings: &PromptSettings,
    selector: &str,
    prompt_text: &str,
    stdout: &mut dyn Write,
) -> Result<PromptOutcome, String> {
    let mut coordinator_config = settings.coordinator_config.clone();
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );
    coordinator_config.run_id_override = None;

    let run_dir = resolve_session_run_dir(selector, &coordinator_config.session_dir)?;
    let recovery = inspect_session_recovery(&run_dir)?;
    if !recovery.resumable {
        let reason = recovery
            .resume_disabled_reason
            .clone()
            .unwrap_or_else(|| RESUME_UNAVAILABLE_FALLBACK_REASON.to_string());
        return Err(format!(
            "resume is disabled for {}: {reason}",
            recovery.run_id
        ));
    }

    let resume_plan = inspect_resume_plan(&run_dir);
    let historical_events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    let resume_agent_id =
        select_resume_agent_id(&resume_plan, &historical_events, &recovery.run_id)?;
    let resume_profile = resume_plan
        .known_agents
        .get(&resume_agent_id)
        .cloned()
        .unwrap_or_else(|| settings.default_profile.clone());
    let run_name = recovery
        .run_name
        .clone()
        .or_else(|| latest_run_name(&historical_events))
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_RUN_NAME.to_string());

    let session_dir = run_dir.parent().ok_or_else(|| {
        format!(
            "failed to resolve parent session directory for {}",
            run_dir.display()
        )
    })?;
    fs::create_dir_all(session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;
    coordinator_config.session_dir = session_dir.to_path_buf();

    let clock = settings.deps.clock(settings.deterministic);

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .resume_run(recovery.run_id.clone(), run_name)
        .await
        .map_err(|err| err.to_string())?;

    if let Some(config) = &settings.logging_config {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }

    let model_override = resolve_prompt_model_override(cmd, settings, &resume_profile)?;
    let request_id = match model_override {
        Some(model_override) => {
            coordinator
                .request_agent_turn_with_model(
                    user_actor(),
                    resume_agent_id,
                    prompt_text.to_string(),
                    model_override.model_ref,
                    Some(model_override.model_settings),
                )
                .await
        }
        None => {
            coordinator
                .request_agent_turn(user_actor(), resume_agent_id, prompt_text.to_string())
                .await
        }
    }
    .map_err(|err| err.to_string())?;
    let event_store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion_with_output(
        event_store,
        &request_id,
        wait_timeout,
        cmd.thinking,
        cmd.format,
        stdout,
    )
    .await;
    let stop_result = coordinator.stop_run().await;

    wait_result?;
    stop_result.map_err(|err| err.to_string())?;

    Ok(PromptOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

fn user_actor() -> EventActor {
    EventActor::new(ActorKind::User, Some("agent-supervisor".to_string()))
}

fn resolve_prompt_text<R: Read + ?Sized>(
    cmd: &PromptCommand,
    stdin_reader: &mut R,
) -> Result<String, String> {
    if let Some(text) = cmd.text.clone() {
        return Ok(text);
    }

    if !cmd.message.is_empty() {
        return Ok(cmd.message.join(" "));
    }

    if cmd.stdin {
        let mut stdin = String::new();
        stdin_reader
            .read_to_string(&mut stdin)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
        let trimmed = stdin.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return Err(
                "stdin was empty; pass --text, positional TEXT, or pipe a prompt into --stdin"
                    .to_string(),
            );
        }
        return Ok(trimmed.to_string());
    }

    Err("no prompt text provided; pass --text, positional TEXT, or --stdin".to_string())
}

fn default_prompt_permission_policy() -> PermissionPolicy {
    use harness_core::config::PermissionMode;

    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptModelOverride {
    model_ref: Option<String>,
    model_settings: AgentModelSettings,
}

fn resolve_prompt_model_override(
    cmd: &PromptCommand,
    settings: &PromptSettings,
    profile_name: &str,
) -> Result<Option<PromptModelOverride>, String> {
    if cmd.model.is_none() && cmd.variant.is_none() && !cmd.thinking {
        return Ok(None);
    }

    let mut model_settings = default_model_settings_for_profile(profile_name);
    let mut model_ref_override = None;

    if let Some(config) = settings.logging_config.as_ref() {
        let (provider, model) = if let Some(model_ref) = cmd.model.as_deref() {
            parse_cli_model_ref(model_ref)?
        } else {
            let profile = config.agents.get(profile_name).ok_or_else(|| {
                format!("unknown agent `{profile_name}` while resolving prompt model override")
            })?;
            parse_cli_model_ref(&profile.model_ref)?
        };

        let resolved =
            resolve_configured_model_metadata(config, &provider, &model, cmd.variant.as_deref())
                .map_err(|err| err.to_string())?;

        model_settings.variant = resolved.variant.clone();
        model_settings.reasoning_effort = resolved.reasoning_effort.clone();
        model_settings.text_verbosity = resolved.text_verbosity.clone();
        model_settings.thinking = resolved.thinking.clone();
        model_settings.reasoning_summary = if resolved
            .resolution
            .capabilities
            .supports_reasoning_summaries
            && model_settings.reasoning_effort.is_some()
        {
            Some("auto".to_string())
        } else {
            None
        };

        if cmd.thinking && model_settings.reasoning_summary.is_none() {
            model_settings.reasoning_summary = Some("auto".to_string());
        }

        if cmd.model.is_some() || cmd.variant.is_some() || cmd.thinking {
            model_ref_override = Some(format!("{}:{}", resolved.provider, resolved.model));
        }
    } else {
        if let Some(model_ref) = cmd.model.as_deref() {
            let (provider, model) = parse_cli_model_ref(model_ref)?;
            model_ref_override = Some(format!("{provider}:{model}"));
        }
        if let Some(variant) = cmd.variant.as_ref() {
            model_settings.variant = Some(variant.clone());
        }
        if cmd.thinking && model_settings.reasoning_summary.is_none() {
            model_settings.reasoning_summary = Some("auto".to_string());
        }
    }

    Ok(Some(PromptModelOverride {
        model_ref: model_ref_override,
        model_settings,
    }))
}

fn parse_cli_model_ref(model_ref: &str) -> Result<(String, String), String> {
    let normalized = model_ref.trim();
    let Some((provider, model)) = normalized
        .split_once(':')
        .or_else(|| normalized.split_once('/'))
    else {
        return Err(format!(
            "invalid model selector `{normalized}`; use `<provider>:<model>`"
        ));
    };

    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(format!(
            "invalid model selector `{normalized}`; use `<provider>:<model>`"
        ));
    }

    Ok((provider.to_string(), model.to_string()))
}
