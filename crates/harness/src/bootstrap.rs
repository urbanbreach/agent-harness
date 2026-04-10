use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::config::{
    load_config_from_file, named_agent_system_prompt, refresh_profile_model_metadata_registry,
    resolve_profile_model_metadata, HarnessConfig, OpenAiApiMode as CoreOpenAiApiMode,
    ProviderConfig,
};
use harness_core::coord::{CoordinatorConfig, PlanProfileConfig};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::{resolve_tool_ids_for_surface, ToolRegistry};
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_providers::{Provider, ProviderRouter};
use harness_tools::coordinator_registry_with_mcp;

const DEFAULT_INTERACTIVE_PROFILE: &str = "build";
const LEGACY_INTERACTIVE_PROFILE: &str = "deep";
const DEFAULT_LOCAL_CONFIG_PATH: &str = "harness.jsonc";
const SHIPPED_EXAMPLE_CONFIG: &str = include_str!("../../../configs/harness.example.jsonc");

const CONFIG_SEARCH_LOCATIONS: [&str; 2] = [
    "./harness.jsonc",
    "$XDG_CONFIG_HOME/harness/config.jsonc (fallback: ~/.config/harness/config.jsonc)",
];

#[derive(Debug, Clone)]
pub enum ConfigInitTarget {
    CurrentDir,
    Xdg,
    Explicit(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ConfigInitOutcome {
    pub path: PathBuf,
    pub uses_auto_discovery: bool,
}

pub fn load_harness_config(path: &Path) -> Result<HarnessConfig, String> {
    load_config_from_file(path).map_err(|err| format!("{} ({})", err, path.display()))
}

pub fn interactive_config_guidance() -> String {
    format!(
        "interactive mode requires a config file; {}. {} and defaults to the build agent while keeping the plan -> build handoff available. If you want the demo/mock UI instead, re-run with --mock",
        config_bootstrap_hint(),
        shipped_example_hint()
    )
}

pub fn config_validate_guidance() -> String {
    format!(
        "no config file found; {}. {}",
        config_bootstrap_hint(),
        shipped_example_hint()
    )
}

pub fn prompt_config_guidance() -> String {
    format!(
        "prompt mode requires a config file; {}. {}, or re-run with --mock",
        prompt_bootstrap_hint(),
        shipped_example_hint()
    )
}

pub fn models_config_guidance() -> String {
    format!(
        "models requires a config file; {}. {}",
        config_bootstrap_hint(),
        shipped_example_hint()
    )
}

pub fn init_config(target: ConfigInitTarget, force: bool) -> Result<ConfigInitOutcome, String> {
    let (path, uses_auto_discovery) = match target {
        ConfigInitTarget::CurrentDir => (PathBuf::from(DEFAULT_LOCAL_CONFIG_PATH), true),
        ConfigInitTarget::Xdg => (xdg_config_path()?, true),
        ConfigInitTarget::Explicit(path) => (path, false),
    };

    if path.exists() && !force {
        return Err(format!(
            "config already exists at {}; re-run with --force to overwrite",
            path.display()
        ));
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create config directory {}: {err}",
                parent.display()
            )
        })?;
    }

    fs::write(&path, SHIPPED_EXAMPLE_CONFIG)
        .map_err(|err| format!("failed to write config {}: {err}", path.display()))?;

    Ok(ConfigInitOutcome {
        path,
        uses_auto_discovery,
    })
}

pub fn config_init_next_steps(outcome: &ConfigInitOutcome) -> Vec<String> {
    if outcome.uses_auto_discovery {
        vec!["harness config validate".to_string(), "harness".to_string()]
    } else {
        let display = outcome.path.display();
        vec![
            format!("harness --config {display} config validate"),
            format!("harness --config {display}"),
        ]
    }
}

fn xdg_config_path() -> Result<PathBuf, String> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| {
            "cannot resolve XDG config path because neither XDG_CONFIG_HOME nor HOME is set"
                .to_string()
        })?;
    Ok(base.join("harness").join("config.jsonc"))
}

fn config_bootstrap_hint() -> String {
    format!(
        "run `harness config init` to create ./harness.jsonc, pass --config <path>, or create the config manually at {}",
        CONFIG_SEARCH_LOCATIONS.join(" or ")
    )
}

fn prompt_bootstrap_hint() -> &'static str {
    "run `harness config init` to create ./harness.jsonc, pass --config <path>, or create harness.jsonc manually"
}

fn shipped_example_hint() -> &'static str {
    "A starting point lives at configs/harness.example.jsonc"
}

pub fn build_interactive_coordinator_config(
    cfg: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut coordinator_config = CoordinatorConfig::new(cfg.paths.session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::from_config(cfg);
    let tool_registry = coordinator_registry_with_mcp(
        cfg.permissions.shell_allowlist.clone(),
        cfg.integrations.mcp.clone(),
    );
    let auto_tool_ids = auto_mcp_tool_ids(&tool_registry);
    coordinator_config.tool_registry = Arc::new(tool_registry);
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.provider = Arc::new(build_provider_router(cfg)?);
    coordinator_config.agent_profiles =
        interactive_agent_profiles_with_extra_tools(cfg, &auto_tool_ids)?;
    coordinator_config.plan_profiles = interactive_plan_profiles(cfg);
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    cfg.ui
        .default_profile
        .clone()
        .or_else(|| {
            cfg.agents
                .contains_key(DEFAULT_INTERACTIVE_PROFILE)
                .then(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
        })
        .or_else(|| {
            cfg.agents
                .contains_key(LEGACY_INTERACTIVE_PROFILE)
                .then(|| LEGACY_INTERACTIVE_PROFILE.to_string())
        })
        .or_else(|| cfg.agents.keys().next().cloned())
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
}

fn build_provider_router(cfg: &HarnessConfig) -> Result<ProviderRouter, String> {
    let providers = cfg
        .providers
        .iter()
        .map(|(provider_id, provider)| {
            build_provider(provider_id, provider).map(|provider| (provider_id.clone(), provider))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    Ok(ProviderRouter::new(providers))
}

fn build_provider(
    provider_id: &str,
    provider: &ProviderConfig,
) -> Result<Arc<dyn Provider>, String> {
    let ProviderConfig::OpenAiCompatible(provider) = provider;

    OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: map_openai_api_mode(provider.api_mode.clone()),
        timeout_ms: provider.timeout_ms,
        headers: provider.headers.clone(),
    })
    .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
    .map_err(|err| format!("failed to build provider `{provider_id}`: {err}"))
}

fn map_openai_api_mode(mode: CoreOpenAiApiMode) -> ProviderOpenAiApiMode {
    match mode {
        CoreOpenAiApiMode::Responses => ProviderOpenAiApiMode::Responses,
        CoreOpenAiApiMode::ChatCompletions => ProviderOpenAiApiMode::ChatCompletions,
        CoreOpenAiApiMode::Auto => ProviderOpenAiApiMode::Auto,
    }
}

fn default_interactive_system_prompt(profile_name: &str, description: &str) -> String {
    named_agent_system_prompt(profile_name)
        .map(str::to_string)
        .unwrap_or_else(|| format!("You are the {profile_name} agent. {description}"))
}

fn append_capability_notes(system_prompt: String, degraded_features: &[String]) -> String {
    let mut notes = Vec::new();

    if degraded_features
        .iter()
        .any(|feature| feature == "tool_calls")
    {
        notes.push(
            "Capability note: this model declares `supports_tool_calls: false`, so runtime tool calls are disabled for this profile. Answer directly without attempting tool invocations."
                .to_string(),
        );
    }

    if degraded_features
        .iter()
        .any(|feature| feature == "reasoning_summaries")
    {
        notes.push(
            "Capability note: this model declares `supports_reasoning_summaries: false`, so visible thinking summaries are unavailable for this profile."
                .to_string(),
        );
    }

    if notes.is_empty() {
        system_prompt
    } else {
        format!("{system_prompt}\n\n{}", notes.join("\n"))
    }
}

fn configured_system_prompt(
    cfg: &HarnessConfig,
    profile_name: &str,
    profile_cfg: &harness_core::config::ProfileConfig,
    degraded_features: &[String],
) -> Result<String, String> {
    let base_prompt = profile_cfg.system_prompt.clone().unwrap_or_else(|| {
        default_interactive_system_prompt(profile_name, &profile_cfg.description)
    });
    append_instruction_files(&base_prompt, &cfg.instructions)
        .map(|prompt| append_capability_notes(prompt, degraded_features))
}

fn append_instruction_files(
    base_prompt: &str,
    instruction_paths: &[String],
) -> Result<String, String> {
    if instruction_paths.is_empty() {
        return Ok(base_prompt.to_string());
    }

    let mut rendered = Vec::with_capacity(instruction_paths.len());
    for instruction_path in instruction_paths {
        let contents = fs::read_to_string(instruction_path).map_err(|err| {
            format!("failed to read instruction file `{instruction_path}`: {err}")
        })?;
        rendered.push(format!(
            "Instruction file `{instruction_path}`:\n{}",
            contents.trim()
        ));
    }

    Ok(format!(
        "{base_prompt}\n\nAdditional instructions from config:\n\n{}",
        rendered.join("\n\n")
    ))
}

pub fn interactive_agent_profiles(
    cfg: &HarnessConfig,
) -> Result<BTreeMap<String, AgentProfile>, String> {
    interactive_agent_profiles_with_extra_tools(cfg, &[])
}

fn interactive_agent_profiles_with_extra_tools(
    cfg: &HarnessConfig,
    extra_tool_ids: &[String],
) -> Result<BTreeMap<String, AgentProfile>, String> {
    refresh_profile_model_metadata_registry(cfg).map_err(|err| err.to_string())?;

    let mut profiles = BTreeMap::new();

    for (profile_name, profile_cfg) in &cfg.agents {
        let metadata =
            resolve_profile_model_metadata(cfg, profile_name).map_err(|err| err.to_string())?;
        profiles.insert(
            profile_name.clone(),
            AgentProfile {
                name: profile_name.clone(),
                category: profile_name.clone(),
                model_ref: profile_cfg.model_ref.clone(),
                system_prompt: configured_system_prompt(
                    cfg,
                    profile_name,
                    profile_cfg,
                    &metadata.degraded_features,
                )?,
                max_iters: profile_cfg.max_iters,
                temperature: profile_cfg.temperature,
                tool_failure_mode: profile_cfg.tool_failure_mode,
                tool_surface: profile_cfg.tool_surface,
                toolset: resolve_tool_ids_for_surface(
                    profile_cfg
                        .tools
                        .iter()
                        .map(String::as_str)
                        .chain(extra_tool_ids.iter().map(String::as_str)),
                    profile_cfg.tool_surface,
                ),
            },
        );
    }

    Ok(profiles)
}

fn auto_mcp_tool_ids(tool_registry: &ToolRegistry) -> Vec<String> {
    tool_registry
        .tool_ids()
        .into_iter()
        .filter(|tool_id| {
            tool_id.starts_with("mcp.")
                && !matches!(
                    tool_id
                        .splitn(4, '.')
                        .skip(2)
                        .collect::<Vec<_>>()
                        .as_slice(),
                    ["tools", "list"]
                        | ["tool", "call"]
                        | ["resources", "list"]
                        | ["resource", "read"]
                        | ["prompts", "list"]
                        | ["prompt", "get"]
                )
        })
        .collect()
}

fn interactive_plan_profiles(cfg: &HarnessConfig) -> BTreeMap<String, PlanProfileConfig> {
    cfg.agents
        .iter()
        .map(|(name, profile)| {
            (
                name.clone(),
                PlanProfileConfig {
                    plan_mode: profile.plan_mode,
                    exit_target_profile: profile.exit_target_profile.clone(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use harness_core::config::load_config_from_str;

    use super::*;
    use tempfile::tempdir;

    fn config_fixture(agents: &str) -> HarnessConfig {
        let raw = format!(
            r#"
            {{
              providers: {{
                default: {{
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "sk-zerolimit",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {{
                    "gpt-5.4-mini": {{
                      display_name: "GPT-5.4 mini",
                    }},
                  }},
                }},
              }},
              agents: {{
                {agents}
              }},
              permissions: {{
                defaults: {{
                  edit: "ask",
                  shell: "ask",
                  network: "deny",
                  question: "ask",
                  task: "ask",
                  webfetch: "deny",
                  websearch: "deny",
                  codesearch: "deny",
                  lsp: "allow",
                }},
                shell_allowlist: {{
                  executables: ["git"],
                  cwd_roots: ["."],
                }},
              }},
              runtime: {{
                background_tasks: {{
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000,
                }},
                session_dir: ".agent-harness/sessions",
                permissions: {{
                  ask_timeout_ms: 45000,
                }},
                prompt: {{
                  wait_timeout_ms: 15000,
                }},
                deterministic: {{
                  enabled: false,
                  seed: 42,
                }},
              }},
              integrations: {{
                remote_search: {{
                  endpoint: "https://mcp.exa.ai/mcp",
                }},
              }},
            }}
            "#,
            agents = agents,
        );

        load_config_from_str(&raw).expect("fixture config should parse")
    }

    #[test]
    fn interactive_agent_profiles_preserve_default_max_iters_and_temperature() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default iteration budget",
              model_ref: "default:gpt-5.4-mini",
              temperature: 0.7,
              tools: ["fs.read"],
            },
            tool_audit: {
              description: "Longer tool audit budget",
              model_ref: "default:gpt-5.4-mini",
              max_iters: 20,
              tools: ["fs.read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["deep"].max_iters, 12);
        assert_eq!(profiles["deep"].temperature, Some(0.7));
        assert_eq!(profiles["tool_audit"].max_iters, 20);
        assert_eq!(profiles["tool_audit"].temperature, None);
    }

    #[test]
    fn interactive_agents_preserve_configured_system_prompt_in_runtime_config() {
        let configured_prompt =
            "Audit the configured tool flow exactly.\nCollect hooks evidence before signoff.";
        let configured_prompt_json = configured_prompt.replace('\n', "\\n");
        let cfg = config_fixture(&format!(
            r#"
            tool_audit: {{
              description: "Audit profile",
              system_prompt: "{configured_prompt_json}",
              model_ref: "default:gpt-5.4-mini",
              tool_surface: "native",
              tools: ["fs.read"],
            }},
            "#
        ));

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["tool_audit"].system_prompt, configured_prompt);

        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        assert_eq!(
            coordinator_config.agent_profiles["tool_audit"].system_prompt,
            configured_prompt
        );
    }

    #[test]
    fn interactive_agents_fall_back_to_generated_system_prompt_when_missing() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-5.4-mini",
              tool_surface: "native",
              tools: ["fs.read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(
            profiles["deep"].system_prompt,
            default_interactive_system_prompt("deep", "Default deep execution profile")
        );
    }

    #[test]
    fn interactive_build_and_plan_agents_use_named_default_prompts_when_missing() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Implementation lane",
              model_ref: "default:gpt-5.4-mini",
              tool_surface: "native",
              tools: ["fs.read"],
            },
            plan: {
              description: "Planning lane",
              model_ref: "default:gpt-5.4-mini",
              tool_surface: "native",
              plan_mode: true,
              exit_target_profile: "build",
              tools: ["fs.read", "plan.exit"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(
            profiles["build"].system_prompt,
            default_interactive_system_prompt("build", "Implementation lane")
        );
        assert_eq!(
            profiles["plan"].system_prompt,
            default_interactive_system_prompt("plan", "Planning lane")
        );
        assert!(profiles["plan"].system_prompt.contains("Remain read-only"));
        assert!(profiles["build"]
            .system_prompt
            .contains("Implement only the approved plan"));
    }

    #[test]
    fn interactive_agents_append_instruction_files_to_system_prompt() {
        let temp = tempdir().expect("tempdir");
        let instruction_path = temp.path().join("instructions.md");
        fs::write(
            &instruction_path,
            "Honor CONTRIBUTING.md before touching workspace files.",
        )
        .expect("write instruction file");

        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default deep execution profile",
              model: "default/gpt-5.4-mini",
              prompt: "Start with a focused read-only pass.",
              steps: 9,
              permission: {
                edit: "deny",
                bash: { "*": "ask" }
              },
              tools: ["fs.read"],
            },
            "#,
        );
        let mut cfg = cfg;
        cfg.instructions = vec![instruction_path.display().to_string()];

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        let prompt = &profiles["deep"].system_prompt;
        assert!(prompt.contains("Start with a focused read-only pass."));
        assert!(prompt.contains("Additional instructions from config:"));
        assert!(prompt.contains("Honor CONTRIBUTING.md before touching workspace files."));
        assert_eq!(profiles["deep"].max_iters, 9);
    }

    #[test]
    fn interactive_agent_profiles_append_auto_mcp_tools() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              model_ref: "default:gpt-5.4-mini",
              tool_surface: "native",
              tools: ["fs.read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles_with_extra_tools(
            &cfg,
            &[
                "mcp.docs-rs.search_in_crate".to_string(),
                "mcp.gh_grep.searchGitHub".to_string(),
            ],
        )
        .expect("interactive profiles");

        assert!(profiles["build"].toolset.contains(&"fs.read".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"mcp.docs-rs.search_in_crate".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"mcp.gh_grep.searchGitHub".to_string()));
        assert!(!profiles["build"]
            .toolset
            .contains(&"mcp.docs-rs.tool.call".to_string()));
    }

    #[test]
    fn interactive_profile_name_defaults_to_build_when_present() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            plan: {
              description: "Plan lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "build");
    }

    #[test]
    fn interactive_profile_name_preserves_legacy_deep_fallback_without_build() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Deep lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "deep");
    }
}
