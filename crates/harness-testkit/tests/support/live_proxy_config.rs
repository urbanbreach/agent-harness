use std::env;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

use super::live_events::resolve_tagged_run_dir;
use super::live_provider_parity::assert_events_show_successful_provider_turn;
use super::live_vision::LiveVisionProxyConfig;
use super::live_visual::selected_live_viewport;
use crate::{
    repo_root, shipped_agent_prompt_body, unique_temp_dir, DEFAULT_LIVE_PROXY_MODEL,
    DEFAULT_LIVE_PROXY_PROFILE, DEFAULT_LIVE_PROXY_PROMPT, DEFAULT_LIVE_PROXY_PROVIDER,
    DEFAULT_LIVE_PROXY_VARIANT, DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS,
    LIVE_PROXY_CHAT_QUESTION_PROFILE, LIVE_PROXY_CHAT_SKILL_PROFILE,
    LIVE_PROXY_CHAT_TODO_FLOW_PROFILE, LIVE_PROXY_TOOL_FLOW_PROFILE,
    LIVE_PROXY_VISION_VERIFIER_PROFILE, LIVE_TOOL_FLOW_RELATIVE_PATH,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptRequest {
    pub(crate) source_config_path: PathBuf,
    pub(crate) provider_name: String,
    pub(crate) primary_model: String,
    pub(crate) primary_variant: Option<String>,
    pub(crate) vision_model: String,
    pub(crate) profile: String,
    pub(crate) prompt_text: String,
    pub(crate) wait_timeout_ms: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveSmokeEndpoint {
    Responses,
}

impl LiveSmokeEndpoint {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Responses => crate::RESPONSES_ENDPOINT_PATH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveProxyPreflightReport {
    source_config_path: PathBuf,
    provider_name: String,
    model_id: String,
    variant: Option<String>,
    vision_model_id: String,
    profile: String,
    endpoint_path: &'static str,
    base_url: String,
    socket_address: String,
    harness_bin: PathBuf,
    viewport_preset: &'static str,
}

impl LiveProxyPreflightReport {
    pub(crate) fn summary_text(&self) -> String {
        [
            "Live proxy preflight".to_string(),
            format!("  config: {}", self.source_config_path.display()),
            format!("  provider: {}", self.provider_name),
            format!("  model: {}", self.model_id),
            format!(
                "  variant: {}",
                self.variant.as_deref().unwrap_or("<primary>")
            ),
            format!("  vision model: {}", self.vision_model_id),
            format!("  profile: {}", self.profile),
            format!("  endpoint: {}", self.endpoint_path),
            format!("  base URL: {}", self.base_url),
            format!("  reachable socket: {}", self.socket_address),
            format!("  harness bin: {}", self.harness_bin.display()),
            format!("  viewport preset: {}", self.viewport_preset),
        ]
        .join("\n")
    }
}

pub(crate) fn run_live_proxy_preflight(
    repo_root: &Path,
) -> Result<LiveProxyPreflightReport, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "live proxy preflight currently expects Linux for the TUI live lane".to_string(),
        );
    }

    let request = resolve_live_prompt_request(repo_root)?;
    let run_config = prepare_live_prompt_run_config(&request)?;
    let config = load_json5_config(&request.source_config_path)?;
    let provider = provider_from_config(&config, &request.provider_name)?;
    let base_url = provider_base_url(provider)?;
    let endpoint = resolve_live_smoke_endpoint(provider)?;
    let parsed = reqwest::Url::parse(&base_url)
        .map_err(|err| format!("failed to parse provider base_url `{base_url}`: {err}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("provider base_url `{base_url}` is missing a host"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("provider base_url `{base_url}` is missing a known port"))?;
    let socket_address = format!("{host}:{port}");
    let resolved = socket_address
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve {socket_address}: {err}"))?
        .next()
        .ok_or_else(|| format!("no socket addresses resolved for {socket_address}"))?;
    TcpStream::connect_timeout(&resolved, Duration::from_secs(2))
        .map_err(|err| format!("failed to connect to {socket_address}: {err}"))?;

    Ok(LiveProxyPreflightReport {
        source_config_path: request.source_config_path,
        provider_name: request.provider_name,
        model_id: request.primary_model,
        variant: request.primary_variant,
        vision_model_id: request.vision_model,
        profile: run_config.profile,
        endpoint_path: endpoint.path(),
        base_url,
        socket_address,
        harness_bin: crate::resolve_harness_bin(),
        viewport_preset: selected_live_viewport().name,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptRunConfig {
    pub(crate) config_path: PathBuf,
    pub(crate) provider_name: String,
    pub(crate) profile: String,
    pub(crate) model_id: String,
    pub(crate) variant: Option<String>,
    pub(crate) endpoint: LiveSmokeEndpoint,
    pub(crate) workspace_root: PathBuf,
    pub(crate) session_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveToolFlowRunConfig {
    pub(crate) tool_flow: PromptRunConfig,
    pub(crate) vision_verifier: PromptRunConfig,
    pub(crate) canonical_relative_path: PathBuf,
    pub(crate) namespaces: LiveToolFlowNamespaces,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptChatToolRunConfig {
    pub(crate) todo_flow: PromptRunConfig,
    pub(crate) question: PromptRunConfig,
    pub(crate) skill: PromptRunConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptStageResult {
    pub(crate) run_dir: PathBuf,
    pub(crate) events_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptNativeToolFlowRunConfig {
    pub(crate) create: PromptRunConfig,
    pub(crate) first_read: PromptRunConfig,
    pub(crate) edit: PromptRunConfig,
    pub(crate) final_read: PromptRunConfig,
    pub(crate) canonical_relative_path: PathBuf,
}

impl LiveToolFlowRunConfig {
    pub(crate) fn visual_test_name(&self) -> &str {
        self.namespaces.visual_test_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveToolFlowNamespaces {
    pub(crate) live_tui_session: &'static str,
    pub(crate) tool_flow_session: &'static str,
    pub(crate) vision_verifier_session: &'static str,
    pub(crate) visual_test_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveNamespaceAllocation {
    root_dir: PathBuf,
}

impl LiveNamespaceAllocation {
    pub(crate) fn allocate(prefix: &str) -> Result<Self, String> {
        let root_dir = unique_temp_dir(prefix);
        Ok(Self { root_dir })
    }

    pub(crate) fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub(crate) fn artifact_file(&self, stem: &str, ext: &str) -> PathBuf {
        self.root_dir.join(format!("{stem}.{ext}"))
    }

    pub(crate) fn session_dir(&self, session_namespace: &str) -> PathBuf {
        self.root_dir.join(".agent-harness").join(session_namespace)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFlowStage {
    Full,
}

impl ToolFlowStage {
    pub(crate) fn tools(self) -> &'static [&'static str] {
        match self {
            Self::Full => &["read", "edit"],
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Full => concat!(
                "Execute the full live tool-flow task in one session. ",
                "Use only read and edit against tmp/live_tool_flow.md."
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedLiveConfigPaths {
    pub(crate) workspace_root: PathBuf,
    pub(crate) session_dir: PathBuf,
    pub(crate) prepared_config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) enum PreparedLiveConfigContract {
    Standard(PreparedLiveConfigPaths),
    ToolFlow {
        paths: PreparedLiveConfigPaths,
        stage: ToolFlowStage,
    },
    RestrictedTools {
        paths: PreparedLiveConfigPaths,
        description: String,
        tools: Vec<String>,
    },
    VisionVerifier(PreparedLiveConfigPaths),
}

impl PreparedLiveConfigContract {
    pub(crate) fn paths(&self) -> &PreparedLiveConfigPaths {
        match self {
            Self::Standard(paths) | Self::VisionVerifier(paths) => paths,
            Self::ToolFlow { paths, .. } => paths,
            Self::RestrictedTools { paths, .. } => paths,
        }
    }
}

pub(crate) fn prepare_live_prompt_run_config(
    request: &LivePromptRequest,
) -> Result<PromptRunConfig, String> {
    prepare_prompt_run_config(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        &request.profile,
    )
}

pub(crate) fn prepare_live_tool_flow_run_config(
    request: &LivePromptRequest,
    namespaces: LiveToolFlowNamespaces,
) -> Result<LiveToolFlowRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-tool-flow-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    fs::create_dir_all(workspace_root.join("tmp")).map_err(|err| {
        format!(
            "failed to create tool-flow workspace {}: {err}",
            workspace_root.display()
        )
    })?;

    let tool_flow = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_TOOL_FLOW_PROFILE,
        PreparedLiveConfigContract::ToolFlow {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir(namespaces.tool_flow_session),
                prepared_config_path: namespace.artifact_file("tool-flow-config", "jsonc"),
            },
            stage: ToolFlowStage::Full,
        },
    )?;

    let vision_verifier = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.vision_model,
        None,
        LIVE_PROXY_VISION_VERIFIER_PROFILE,
        PreparedLiveConfigContract::VisionVerifier(PreparedLiveConfigPaths {
            workspace_root: workspace_root.clone(),
            session_dir: namespace.session_dir(namespaces.vision_verifier_session),
            prepared_config_path: namespace.artifact_file("vision-verifier-config", "jsonc"),
        }),
    )?;

    Ok(LiveToolFlowRunConfig {
        tool_flow,
        vision_verifier,
        canonical_relative_path: PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH),
        namespaces,
    })
}

pub(crate) fn prepare_live_prompt_chat_tool_run_config(
    request: &LivePromptRequest,
) -> Result<LivePromptChatToolRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-chat-tool-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    seed_project_skill(&workspace_root, "rust-best-practices")?;

    let todo_flow = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_TODO_FLOW_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir("chat-tool-todo-flow"),
                prepared_config_path: namespace
                    .artifact_file("chat-tool-todo-flow-config", "jsonc"),
            },
            description: "Execute the live chat todo flow via todowrite.".to_string(),
            tools: vec!["todowrite".to_string()],
        },
    )?;

    let question = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_QUESTION_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir("chat-tool-question"),
                prepared_config_path: namespace.artifact_file("chat-tool-question-config", "jsonc"),
            },
            description: "Execute the live question flow and stop after answering.".to_string(),
            tools: vec!["question".to_string()],
        },
    )?;

    let skill = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_SKILL_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root,
                session_dir: namespace.session_dir("chat-tool-skill"),
                prepared_config_path: namespace.artifact_file("chat-tool-skill-config", "jsonc"),
            },
            description: "Execute the live skill-loading flow and stop after the skill tool call."
                .to_string(),
            tools: vec!["skill".to_string()],
        },
    )?;

    Ok(LivePromptChatToolRunConfig {
        todo_flow,
        question,
        skill,
    })
}

fn seed_project_skill(workspace_root: &Path, skill_name: &str) -> Result<(), String> {
    let source_root = repo_root()
        .join(".agent-harness")
        .join("skills")
        .join(skill_name);
    if !source_root.exists() {
        return Err(format!(
            "required skill `{skill_name}` not found at {}",
            source_root.display()
        ));
    }

    let dest_root = workspace_root
        .join(".agent-harness")
        .join("skills")
        .join(skill_name);
    copy_dir_recursive(&source_root, &dest_root)
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest)
        .map_err(|err| format!("failed to create {}: {err}", dest.display()))?;
    for entry in
        fs::read_dir(source).map_err(|err| format!("failed to read {}: {err}", source.display()))?
    {
        let entry = entry
            .map_err(|err| format!("failed to read entry under {}: {err}", source.display()))?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| {
            format!(
                "failed to inspect {} while copying skill fixture: {err}",
                source_path.display()
            )
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &dest_path).map_err(|err| {
                format!(
                    "failed to copy {} to {}: {err}",
                    source_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn prepare_live_prompt_native_tool_flow_run_config(
    request: &LivePromptRequest,
) -> Result<LivePromptNativeToolFlowRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-native-tool-flow-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    fs::create_dir_all(workspace_root.join("tmp")).map_err(|err| {
        format!(
            "failed to create native tool-flow workspace {}: {err}",
            workspace_root.display()
        )
    })?;
    let prepare_stage = |session_namespace: &str, config_stem: &str| {
        prepare_prompt_run_config_with_contract(
            &request.source_config_path,
            &request.provider_name,
            &request.primary_model,
            request.primary_variant.as_deref(),
            LIVE_PROXY_TOOL_FLOW_PROFILE,
            PreparedLiveConfigContract::ToolFlow {
                paths: PreparedLiveConfigPaths {
                    workspace_root: workspace_root.clone(),
                    session_dir: namespace.session_dir(session_namespace),
                    prepared_config_path: namespace.artifact_file(config_stem, "jsonc"),
                },
                stage: ToolFlowStage::Full,
            },
        )
    };

    Ok(LivePromptNativeToolFlowRunConfig {
        create: prepare_stage("native-tool-create", "native-tool-create-config")?,
        first_read: prepare_stage("native-tool-first-read", "native-tool-first-read-config")?,
        edit: prepare_stage("native-tool-edit", "native-tool-edit-config")?,
        final_read: prepare_stage("native-tool-final-read", "native-tool-final-read-config")?,
        canonical_relative_path: PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH),
    })
}

pub(crate) fn resolve_live_prompt_request(repo_root: &Path) -> Result<LivePromptRequest, String> {
    let override_config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .ok()
        .map(PathBuf::from);
    let source_config_path =
        resolve_live_proxy_config_path(repo_root, override_config_path.as_deref())?;
    let mut config = load_json5_config(&source_config_path)?;
    normalize_legacy_profile_aliases(&mut config)?;
    let provider_name = env::var("HARNESS_LIVE_PROXY_PROVIDER")
        .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROVIDER.into());
    let provider = provider_from_config(&config, &provider_name)?;
    let primary_model = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_MODEL")
        .unwrap_or_else(|| first_model_from_provider(provider))?;
    let default_variant = if source_config_path == default_live_proxy_config_path(repo_root) {
        resolve_live_proxy_variant(&config, &provider_name, &primary_model)
    } else {
        None
    };
    let primary_variant = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_VARIANT")
        .transpose()?
        .or(default_variant);
    let vision_model = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_VISION_MODEL")
        .unwrap_or_else(|| Ok(primary_model.clone()))?;

    Ok(LivePromptRequest {
        source_config_path,
        provider_name,
        primary_model,
        primary_variant,
        vision_model,
        profile: env::var("HARNESS_LIVE_PROXY_PROFILE")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROFILE.into()),
        prompt_text: env::var("HARNESS_LIVE_PROXY_PROMPT")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROMPT.into()),
        wait_timeout_ms: env::var("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string()),
    })
}

pub(crate) fn resolve_live_vision_proxy_config(
    request: &LivePromptRequest,
) -> Result<LiveVisionProxyConfig, String> {
    let config = load_json5_config(&request.source_config_path)?;
    let provider = provider_from_config(&config, &request.provider_name)?;
    ensure_provider_uses_responses_compatible_mode(&provider_api_mode(provider))?;

    LiveVisionProxyConfig::new(
        request.provider_name.clone(),
        provider_base_url(provider)?,
        provider_api_key(provider)?,
        request.vision_model.clone(),
    )
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
pub(crate) fn resolve_live_vision_proxy_config_for_run(
    run_config: &PromptRunConfig,
) -> Result<LiveVisionProxyConfig, String> {
    let config = load_json5_config(&run_config.config_path)?;
    let provider = provider_from_config(&config, DEFAULT_LIVE_PROXY_PROVIDER)?;
    ensure_provider_uses_responses_compatible_mode(&provider_api_mode(provider))?;

    LiveVisionProxyConfig::new(
        run_config.provider_name.clone(),
        provider_base_url(provider)?,
        provider_api_key(provider)?,
        run_config.model_id.clone(),
    )
}

pub(crate) fn default_live_proxy_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join("configs").join("harness.example.jsonc")
}

pub(crate) fn resolve_live_proxy_config_path(
    repo_root: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let config_path = override_path
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| default_live_proxy_config_path(repo_root));
    if config_path.exists() {
        Ok(config_path)
    } else {
        Err(format!(
            "live proxy config not found at {}",
            config_path.display()
        ))
    }
}

pub(crate) fn prepare_prompt_run_config(
    source_config_path: &Path,
    provider_name: &str,
    selected_model: &str,
    selected_variant: Option<&str>,
    profile_name: &str,
) -> Result<PromptRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-session")?;
    prepare_prompt_run_config_with_contract(
        source_config_path,
        provider_name,
        selected_model,
        selected_variant,
        profile_name,
        PreparedLiveConfigContract::Standard(PreparedLiveConfigPaths {
            workspace_root: repo_root(),
            session_dir: namespace.session_dir("prompt-session"),
            prepared_config_path: namespace.artifact_file("prepared-config", "jsonc"),
        }),
    )
}

pub(crate) fn prepare_prompt_run_config_with_contract(
    source_config_path: &Path,
    provider_name: &str,
    selected_model: &str,
    selected_variant: Option<&str>,
    profile_name: &str,
    contract: PreparedLiveConfigContract,
) -> Result<PromptRunConfig, String> {
    if trimmed_non_empty(provider_name).is_none() {
        return Err("provider name cannot be empty".to_string());
    }
    if trimmed_non_empty(profile_name).is_none() {
        return Err("profile name cannot be empty".to_string());
    }
    if trimmed_non_empty(selected_model).is_none() {
        return Err("selected model cannot be empty".to_string());
    }

    let mut config = load_json5_config(source_config_path)?;
    normalize_legacy_profile_aliases(&mut config)?;

    let provider = provider_from_config(&config, provider_name)?;
    let endpoint = resolve_live_smoke_endpoint(provider)?;
    let selected_model = selected_model.trim().to_string();

    rewrite_selected_provider_to_default(&mut config, provider_name)?;
    normalize_category_model_refs_to_default(&mut config)?;
    ensure_provider_model_entry(&mut config, &selected_model)?;
    ensure_provider_model_variant(&mut config, &selected_model, selected_variant)?;
    ensure_profile_model_ref(&mut config, profile_name, &selected_model)?;
    ensure_profile_variant(&mut config, profile_name, selected_variant)?;
    seed_inline_system_prompts(&mut config)?;
    disable_prepared_determinism(&mut config)?;

    let paths = contract.paths().clone();
    apply_prepared_run_paths(&mut config, &paths.session_dir, profile_name)?;
    apply_allow_permissions(&mut config)?;
    match &contract {
        PreparedLiveConfigContract::Standard(_) => {}
        PreparedLiveConfigContract::ToolFlow { stage, .. } => {
            apply_tool_flow_contract(&mut config, profile_name, *stage)?;
        }
        PreparedLiveConfigContract::RestrictedTools {
            description, tools, ..
        } => apply_restricted_tools_contract(&mut config, profile_name, description, tools)?,
        PreparedLiveConfigContract::VisionVerifier(_) => {}
    }

    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|err| format!("failed to render prepared config JSON: {err}"))?;
    fs::write(&paths.prepared_config_path, rendered).map_err(|err| {
        format!(
            "failed to write prepared config {}: {err}",
            paths.prepared_config_path.display()
        )
    })?;

    Ok(PromptRunConfig {
        config_path: paths.prepared_config_path,
        provider_name: provider_name.trim().to_string(),
        profile: profile_name.to_string(),
        model_id: selected_model,
        variant: selected_variant.map(str::to_string),
        endpoint,
        workspace_root: paths.workspace_root,
        session_dir: paths.session_dir,
    })
}

pub(crate) fn run_live_prompt_stage(
    run_config: &PromptRunConfig,
    prompt: &str,
    wait_timeout_ms: &str,
    extra_env: &[(&str, &str)],
) -> Result<LivePromptStageResult, String> {
    let harness_bin = crate::resolve_harness_bin();
    let mut command = Command::new(&harness_bin);
    command
        .arg("prompt")
        .arg("--text")
        .arg(prompt)
        .arg("--profile")
        .arg(&run_config.profile)
        .arg("--config")
        .arg(&run_config.config_path)
        .env("HARNESS_PROMPT_WAIT_TIMEOUT_MS", wait_timeout_ms)
        .current_dir(&run_config.workspace_root);
    for (name, value) in extra_env {
        command.env(name, value);
    }

    let output = command
        .output()
        .map_err(|err| format!("spawn harness prompt stage: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "prompt stage failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nPrepared config: {}\nSelected profile: {}",
            output.status.code(),
            stdout,
            stderr,
            run_config.config_path.display(),
            run_config.profile,
        ));
    }

    let session_namespace = session_namespace_name(&run_config.session_dir)?;
    let run_dir = resolve_tagged_run_dir(&run_config.session_dir, &session_namespace)?;
    let events_path = run_dir.join("events.jsonl");
    let events_body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    assert_events_show_successful_provider_turn(&run_config.provider_name, &events_body);

    Ok(LivePromptStageResult {
        run_dir,
        events_body,
    })
}

pub(crate) fn load_json5_config(config_path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read config {}: {err}", config_path.display()))?;
    json5::from_str(&raw).map_err(|err| {
        format!(
            "failed to parse JSON5 config {}: {err}",
            config_path.display()
        )
    })
}

pub(crate) fn provider_from_config<'a>(
    config: &'a Value,
    provider_name: &str,
) -> Result<&'a Value, String> {
    let providers = config
        .get("providers")
        .or_else(|| config.get("provider"))
        .and_then(Value::as_object)
        .ok_or_else(|| "config must define providers as an object".to_string())?;

    providers
        .get(provider_name)
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))
}

pub(crate) fn provider_api_mode(provider: &Value) -> String {
    provider
        .get("api_mode")
        .or_else(|| provider.get("apiMode"))
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("api_mode"))
        })
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("apiMode"))
        })
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase()
}

pub(crate) fn provider_base_url(provider: &Value) -> Result<String, String> {
    provider
        .get("base_url")
        .or_else(|| provider.get("baseUrl"))
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("base_url"))
        })
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("baseUrl"))
        })
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("baseURL"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "provider config missing non-empty `base_url`".to_string())
}

pub(crate) fn session_namespace_name(session_dir: &Path) -> Result<String, String> {
    session_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "failed to derive session namespace label from {}",
                session_dir.display()
            )
        })
}

pub(crate) fn provider_api_key(provider: &Value) -> Result<String, String> {
    let raw = provider
        .get("api_key")
        .or_else(|| provider.get("apiKey"))
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("api_key"))
        })
        .or_else(|| {
            provider
                .get("options")
                .and_then(|options| options.get("apiKey"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider config missing non-empty `api_key`".to_string())?;

    resolve_env_reference_value(raw)
}

pub(crate) fn resolve_live_smoke_endpoint(provider: &Value) -> Result<LiveSmokeEndpoint, String> {
    let api_mode = provider_api_mode(provider);
    ensure_provider_uses_responses_compatible_mode(&api_mode)?;
    Ok(LiveSmokeEndpoint::Responses)
}

pub(crate) fn ensure_provider_uses_responses_compatible_mode(api_mode: &str) -> Result<(), String> {
    match api_mode {
        "responses" | "auto" => Ok(()),
        "chat_completions" => Err(
            "live CLI proxy E2E requires provider api_mode set to responses or auto; found chat_completions"
                .to_string(),
        ),
        other => Err(format!(
            "unsupported api_mode `{other}` for live CLI proxy E2E; expected responses or auto"
        )),
    }
}

fn first_model_from_provider(provider: &Value) -> Result<String, String> {
    let Some(models) = provider.get("models").and_then(Value::as_object) else {
        return Err(
            "provider config has no `models` object; set HARNESS_LIVE_PROXY_MODEL explicitly"
                .to_string(),
        );
    };

    if models.contains_key(DEFAULT_LIVE_PROXY_MODEL) {
        return Ok(DEFAULT_LIVE_PROXY_MODEL.to_string());
    }

    models.keys().next().cloned().ok_or_else(|| {
        "provider config has an empty `models` map; set HARNESS_LIVE_PROXY_MODEL".to_string()
    })
}

fn resolve_live_proxy_variant(
    config: &Value,
    provider_name: &str,
    model_id: &str,
) -> Option<String> {
    let provider = provider_from_config(config, provider_name).ok()?;
    let model = provider
        .get("models")
        .and_then(Value::as_object)
        .and_then(|models| models.get(model_id))
        .and_then(Value::as_object)?;

    let exposes_default_variant = model
        .get("variants")
        .and_then(Value::as_object)
        .is_some_and(|variants| variants.contains_key(DEFAULT_LIVE_PROXY_VARIANT));

    (model_id == DEFAULT_LIVE_PROXY_MODEL || exposes_default_variant)
        .then(|| DEFAULT_LIVE_PROXY_VARIANT.to_string())
}

fn rewrite_selected_provider_to_default(
    config: &mut Value,
    provider_name: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;

    let selected_provider = providers
        .get(provider_name)
        .cloned()
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))?;

    providers.insert(DEFAULT_LIVE_PROXY_PROVIDER.to_string(), selected_provider);
    Ok(())
}

fn normalize_category_model_refs_to_default(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let Some(categories_value) = root.get_mut("agents") else {
        return Ok(());
    };
    let categories = categories_value
        .as_object_mut()
        .ok_or_else(|| "config.agents must be an object".to_string())?;

    for (category_name, category_value) in categories.iter_mut() {
        let Some(category_obj) = category_value.as_object_mut() else {
            return Err(format!("agent `{category_name}` must be an object"));
        };

        let model_ref = category_obj
            .get("model_ref")
            .or_else(|| category_obj.get("modelRef"))
            .or_else(|| category_obj.get("model"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if model_ref.is_empty() {
            continue;
        }

        let model_id = model_ref
            .split_once(':')
            .or_else(|| model_ref.split_once('/'))
            .map(|(_, model_id)| model_id)
            .unwrap_or(model_ref)
            .trim();
        if model_id.is_empty() {
            continue;
        }

        category_obj.insert(
            "model_ref".to_string(),
            Value::String(format!("default:{model_id}")),
        );
        category_obj.remove("model");
        category_obj.remove("modelRef");
        category_obj
            .entry("description".to_string())
            .or_insert_with(|| Value::String(format!("{category_name} profile")));
        category_obj
            .entry("tools".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
    }

    Ok(())
}

fn normalize_legacy_profile_aliases(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    if !root.contains_key("providers") {
        if let Some(provider_alias) = root.get("provider").cloned() {
            root.insert("providers".to_string(), provider_alias);
        }
    }

    if !root.contains_key("agents") {
        if let Some(agent_alias) = root.get("agent").cloned() {
            root.insert("agents".to_string(), agent_alias);
        }
    }

    let default_profile = root
        .get("default_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if let Some(default_profile) = default_profile {
        let ui = root
            .entry("ui".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .ok_or_else(|| "config.ui must be an object".to_string())?;
        ui.entry("default_profile".to_string())
            .or_insert_with(|| Value::String(default_profile));
    }

    Ok(())
}

fn ensure_profile_model_ref(
    config: &mut Value,
    profile_name: &str,
    model_id: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    let categories = root
        .entry("agents".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.agents must be an object".to_string())?;

    let mut profile = categories.get(profile_name).cloned().unwrap_or_else(|| {
        json!({
            "description": "Live proxy smoke profile",
            "tools": []
        })
    });

    let profile_obj = profile
        .as_object_mut()
        .ok_or_else(|| format!("agent `{profile_name}` must be an object"))?;
    profile_obj.insert(
        "model_ref".to_string(),
        Value::String(format!("default:{model_id}")),
    );
    profile_obj
        .entry("description".to_string())
        .or_insert_with(|| Value::String("Live proxy smoke profile".to_string()));
    profile_obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    categories.insert(profile_name.to_string(), profile);
    Ok(())
}

fn seed_inline_system_prompts(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;

    let build_prompt = shipped_agent_prompt_body("build");

    for (profile_name, profile_value) in categories.iter_mut() {
        let profile = profile_value
            .as_object_mut()
            .ok_or_else(|| format!("agent `{profile_name}` must be an object"))?;
        if profile.contains_key("system_prompt") || profile.contains_key("systemPrompt") {
            continue;
        }

        profile.insert(
            "system_prompt".to_string(),
            Value::String(build_prompt.clone()),
        );
    }

    Ok(())
}

fn ensure_profile_variant(
    config: &mut Value,
    profile_name: &str,
    selected_variant: Option<&str>,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be an object"))?;

    match selected_variant.and_then(trimmed_non_empty) {
        Some(variant) => {
            profile.insert("variant".to_string(), Value::String(variant.to_string()));
        }
        _ => {
            profile.remove("variant");
        }
    }

    Ok(())
}

fn ensure_provider_model_entry(config: &mut Value, model_id: &str) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;
    let provider = providers
        .get_mut(DEFAULT_LIVE_PROXY_PROVIDER)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` must be an object"))?;
    let models = provider
        .entry("models".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` models must be an object")
        })?;

    if models.contains_key(model_id) {
        return Ok(());
    }

    let mut prepared_model = models
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let prepared_model_obj = prepared_model.as_object_mut().ok_or_else(|| {
        format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` model entries must be objects")
    })?;
    prepared_model_obj.insert(
        "display_name".to_string(),
        Value::String(format!("Prepared {model_id}")),
    );
    models.insert(model_id.to_string(), prepared_model);
    Ok(())
}

fn ensure_provider_model_variant(
    config: &mut Value,
    model_id: &str,
    selected_variant: Option<&str>,
) -> Result<(), String> {
    let Some(selected_variant) = selected_variant.and_then(trimmed_non_empty) else {
        return Ok(());
    };

    if selected_variant != DEFAULT_LIVE_PROXY_VARIANT {
        return Ok(());
    }

    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;
    let provider = providers
        .get_mut(DEFAULT_LIVE_PROXY_PROVIDER)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` must be an object"))?;
    let models = provider
        .get_mut("models")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` models must be an object")
        })?;
    let model = models
        .get_mut(model_id)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` is missing model `{model_id}`")
        })?;
    let variants = model
        .entry("variants".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("model `{model_id}` variants must be an object"))?;

    variants
        .entry(DEFAULT_LIVE_PROXY_VARIANT.to_string())
        .or_insert_with(|| {
            json!({
                "display_name": "Live signoff",
                "metadata": {
                    "reasoning_effort": "low",
                    "text_verbosity": "low",
                    "recommended_for": "live_proxy",
                }
            })
        });

    Ok(())
}

fn apply_prepared_run_paths(
    config: &mut Value,
    session_dir: &Path,
    profile_name: &str,
) -> Result<(), String> {
    fs::create_dir_all(session_dir).map_err(|err| {
        format!(
            "failed to create prepared session dir {}: {err}",
            session_dir.display()
        )
    })?;

    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let runtime = root
        .entry("runtime".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime must be an object".to_string())?;
    runtime.insert(
        "session_dir".to_string(),
        Value::String(session_dir.display().to_string()),
    );

    let ui = root
        .entry("ui".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.ui must be an object".to_string())?;
    ui.insert(
        "default_profile".to_string(),
        Value::String(profile_name.to_string()),
    );
    root.insert(
        "default_agent".to_string(),
        Value::String(profile_name.to_string()),
    );

    Ok(())
}

fn apply_allow_permissions(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let permissions = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions must be an object".to_string())?;
    let defaults = permissions
        .entry("defaults".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions.defaults must be an object".to_string())?;
    defaults.insert("edit".to_string(), Value::String("allow".to_string()));
    defaults.insert("shell".to_string(), Value::String("allow".to_string()));
    defaults.insert("network".to_string(), Value::String("allow".to_string()));
    defaults.insert("question".to_string(), Value::String("allow".to_string()));
    Ok(())
}

fn disable_prepared_determinism(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let runtime = root
        .entry("runtime".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime must be an object".to_string())?;
    let deterministic = runtime
        .entry("deterministic".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime.deterministic must be an object".to_string())?;
    deterministic.insert("enabled".to_string(), Value::Bool(false));
    Ok(())
}

fn apply_tool_flow_contract(
    config: &mut Value,
    profile_name: &str,
    stage: ToolFlowStage,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let permissions = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions must be an object".to_string())?;
    permissions.insert(
        "shell_allowlist".to_string(),
        json!({
            "executables": ["sh"],
            "cwd_roots": ["."],
        }),
    );

    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be present and be an object"))?;
    profile.insert(
        "description".to_string(),
        Value::String(stage.description().to_string()),
    );
    profile.insert(
        "permissions".to_string(),
        json!({
            "edit": "allow",
            "shell": "allow",
            "network": "allow",
            "question": "allow",
        }),
    );
    profile.insert(
        "tools".to_string(),
        Value::Array(
            stage
                .tools()
                .iter()
                .map(|tool| Value::String((*tool).to_string()))
                .collect(),
        ),
    );

    Ok(())
}

fn apply_restricted_tools_contract(
    config: &mut Value,
    profile_name: &str,
    description: &str,
    tools: &[String],
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be present and be an object"))?;
    profile.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    profile.insert(
        "permissions".to_string(),
        json!({
            "edit": "allow",
            "shell": "allow",
            "network": "allow",
        }),
    );
    profile.insert(
        "tools".to_string(),
        Value::Array(tools.iter().cloned().map(Value::String).collect()),
    );
    Ok(())
}

pub(crate) fn resolve_env_reference_value(value: &str) -> Result<String, String> {
    if !(value.starts_with("${") && value.ends_with('}')) {
        return Ok(value.to_string());
    }

    let reference = &value[2..value.len() - 1];
    if reference.is_empty() {
        return Ok(value.to_string());
    }

    if let Some((key, fallback)) = reference.split_once(":-") {
        if key.is_empty() {
            return Ok(value.to_string());
        }
        return Ok(env::var(key)
            .ok()
            .filter(|resolved| !resolved.is_empty())
            .unwrap_or_else(|| fallback.to_string()));
    }

    env::var(reference).map_err(|_| {
        format!("environment variable `{reference}` required by live proxy api_key is not set")
    })
}

fn resolve_trimmed_env_var(name: &str) -> Option<Result<String, String>> {
    env::var(name)
        .ok()
        .and_then(|value| trimmed_non_empty(&value).map(str::to_string))
        .map(Ok)
}

fn trimmed_non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}
