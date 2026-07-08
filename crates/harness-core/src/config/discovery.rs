// allow: SIZE_OK — config discovery pipeline (XDG/workspace/env path resolution + layer merging + asset discovery)
use serde::de::DeserializeOwned;

use super::*;

#[derive(Debug, Clone)]
pub struct ConfigDiscoveryContext {
    pub current_dir: PathBuf,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub runtime_config_path: Option<PathBuf>,
    pub tui_config_path: Option<PathBuf>,
}

impl ConfigDiscoveryContext {
    pub fn from_env() -> Self {
        Self {
            current_dir: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: env::var_os("HOME").map(PathBuf::from),
            runtime_config_path: env::var_os("HARNESS_CONFIG")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            tui_config_path: env::var_os("HARNESS_TUI_CONFIG")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn with_current_dir(mut self, current_dir: PathBuf) -> Self {
        self.current_dir = current_dir;
        self
    }

    pub fn apply_env_var(mut self, name: &str, value: Option<String>) -> Self {
        match name {
            "XDG_CONFIG_HOME" => self.xdg_config_home = value.map(PathBuf::from),
            "HOME" => self.home = value.map(PathBuf::from),
            "HARNESS_CONFIG" => {
                self.runtime_config_path =
                    value.filter(|value| !value.is_empty()).map(PathBuf::from);
            }
            "HARNESS_TUI_CONFIG" => {
                self.tui_config_path = value.filter(|value| !value.is_empty()).map(PathBuf::from);
            }
            _ => {}
        }
        self
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct MarkdownAgentFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(alias = "systemPrompt", alias = "prompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "model_ref", alias = "modelRef", alias = "model")]
    pub model_ref: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f32>,
    #[serde(alias = "topP")]
    pub top_p: Option<f32>,
    pub mode: Option<AgentMode>,
    pub hidden: Option<bool>,
    pub color: Option<String>,
    pub options: BTreeMap<String, serde_json::Value>,
    pub permissions: Option<ProfilePermissions>,
    #[serde(alias = "maxIters", alias = "steps", alias = "maxSteps")]
    pub max_iters: Option<usize>,
    #[serde(alias = "toolFailureMode")]
    pub tool_failure_mode: Option<ToolFailureMode>,
    pub tools: Option<PublicAgentTools>,
    #[serde(default, alias = "enabled")]
    pub enable: Option<bool>,
    #[serde(default)]
    pub disable: bool,
    #[serde(default, alias = "smallModel")]
    pub use_small_model: bool,
}

#[derive(Debug, Clone)]
struct MarkdownAgentFile {
    frontmatter: MarkdownAgentFrontmatter,
    prompt_body: Option<String>,
}

pub(super) fn resolve_discovered_prompt_assets(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    resolve_discovered_prompt_assets_with_current_dir(parsed, config_path, None)
}

pub(super) fn resolve_discovered_prompt_assets_with_current_dir(
    parsed: &mut HarnessConfig,
    config_path: &Path,
    current_dir: Option<&Path>,
) -> Result<(), ConfigError> {
    let small_model_ref = parsed.small_model.as_deref();
    let disabled_agents = parsed.disabled_agents.clone();
    parsed.agents = merge_configured_and_markdown_agents(
        &parsed.agents,
        config_path,
        current_dir,
        small_model_ref,
        &disabled_agents,
    )?;
    parsed.instruction_files = discover_instruction_files(config_path, current_dir)?;
    Ok(())
}

fn merge_configured_and_markdown_agents(
    configured: &BTreeMap<String, ProfileConfig>,
    config_path: &Path,
    current_dir: Option<&Path>,
    small_model_ref: Option<&str>,
    disabled_agents: &BTreeSet<String>,
) -> Result<BTreeMap<String, ProfileConfig>, ConfigError> {
    let discovered = discover_markdown_agents(config_path, current_dir)?;
    let agent_names = discovered
        .keys()
        .chain(configured.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = BTreeMap::new();

    for name in agent_names {
        if disabled_agents.contains(&name) {
            continue;
        }
        let profile = match (discovered.get(&name), configured.get(&name)) {
            (Some(markdown), Some(config)) => {
                Some(merge_markdown_agent_with_config(config, markdown))
            }
            (None, Some(config)) => Some(config.clone()),
            (Some(markdown), None) => {
                let fallback_model_ref = configured
                    .values()
                    .next()
                    .map(|profile| profile.model_ref.as_str());
                profile_from_markdown_agent(markdown, fallback_model_ref, small_model_ref)?
            }
            (None, None) => None,
        };

        if let Some(profile) = profile {
            if profile.enabled == Some(false) {
                continue;
            }
            merged.insert(name, profile);
        }
    }

    Ok(merged)
}

fn merge_markdown_agent_with_config(
    config: &ProfileConfig,
    markdown: &MarkdownAgentFile,
) -> ProfileConfig {
    let prompt = config
        .system_prompt
        .clone()
        .or_else(|| markdown.prompt_body.clone())
        .or_else(|| markdown.frontmatter.system_prompt.clone());

    // `model_ref` is a `String` (not `Option<String>`), so it always has a
    // value. Use `model_ref_explicit` to determine if JSON config explicitly
    // set the model. When JSON config did not explicitly set it, fall back to
    // the markdown frontmatter value.
    let model_ref = if config.model_ref_explicit {
        config.model_ref.clone()
    } else {
        markdown
            .frontmatter
            .model_ref
            .clone()
            .unwrap_or_else(|| config.model_ref.clone())
    };

    // `tool_failure_mode` is a `ToolFailureMode` (not `Option`), so checking
    // for the default is ambiguous: the serde default is
    // `ContinueAsToolMessage` but `Default::default()` is `FailTurn`. We
    // accept the ambiguity: if the config has `ContinueAsToolMessage` (the
    // serde default), fall back to the markdown value. This means a user who
    // explicitly sets `continue_as_tool_message` in JSON config would have
    // their value overridden by markdown — an accepted trade-off to avoid
    // adding a `tool_failure_mode_explicit` flag to all construction sites.
    let tool_failure_mode = if config.tool_failure_mode == ToolFailureMode::ContinueAsToolMessage {
        markdown
            .frontmatter
            .tool_failure_mode
            .unwrap_or(config.tool_failure_mode)
    } else {
        config.tool_failure_mode
    };

    ProfileConfig {
        name: config
            .name
            .clone()
            .or_else(|| markdown.frontmatter.name.clone()),
        description: if config.description.is_empty() {
            markdown.frontmatter.description.clone().unwrap_or_default()
        } else {
            config.description.clone()
        },
        system_prompt: prompt,
        model_ref,
        model_ref_explicit: config.model_ref_explicit || markdown.frontmatter.model_ref.is_some(),
        variant: config
            .variant
            .clone()
            .or_else(|| markdown.frontmatter.variant.clone()),
        temperature: config.temperature.or(markdown.frontmatter.temperature),
        top_p: config.top_p.or(markdown.frontmatter.top_p),
        mode: if matches!(config.mode, AgentMode::All) {
            markdown.frontmatter.mode.unwrap_or(config.mode)
        } else {
            config.mode
        },
        hidden: config.hidden || markdown.frontmatter.hidden.unwrap_or(false),
        color: config
            .color
            .clone()
            .or_else(|| markdown.frontmatter.color.clone()),
        options: if config.options.is_empty() {
            markdown.frontmatter.options.clone()
        } else {
            config.options.clone()
        },
        permissions: config
            .permissions
            .clone()
            .or_else(|| markdown.frontmatter.permissions.clone()),
        max_iters: config.max_iters.or(markdown.frontmatter.max_iters),
        tool_failure_mode,
        tools: if config.tools.is_empty() {
            markdown
                .frontmatter
                .tools
                .clone()
                .map(|t| t.tool_ids())
                .unwrap_or_default()
        } else {
            config.tools.clone()
        },
        enabled: config.enabled.or_else(|| {
            if markdown.frontmatter.disable || markdown.frontmatter.enable == Some(false) {
                Some(false)
            } else {
                None
            }
        }),
    }
}

fn profile_from_markdown_agent(
    markdown: &MarkdownAgentFile,
    fallback_model_ref: Option<&str>,
    small_model_ref: Option<&str>,
) -> Result<Option<ProfileConfig>, ConfigError> {
    if markdown.frontmatter.disable || markdown.frontmatter.enable == Some(false) {
        return Ok(None);
    }
    let Some(description) = markdown.frontmatter.description.clone() else {
        return Ok(None);
    };
    let model_ref = if markdown.frontmatter.use_small_model {
        markdown
            .frontmatter
            .model_ref
            .clone()
            .or_else(|| small_model_ref.map(str::to_string))
            .or_else(|| fallback_model_ref.map(str::to_string))
            .unwrap_or_else(|| "default:default".to_string())
    } else {
        markdown
            .frontmatter
            .model_ref
            .clone()
            .or_else(|| fallback_model_ref.map(str::to_string))
            .unwrap_or_else(|| "default:default".to_string())
    };

    Ok(Some(ProfileConfig {
        name: markdown.frontmatter.name.clone(),
        description,
        system_prompt: markdown
            .prompt_body
            .clone()
            .or_else(|| markdown.frontmatter.system_prompt.clone()),
        model_ref,
        model_ref_explicit: markdown.frontmatter.model_ref.is_some(),
        variant: markdown.frontmatter.variant.clone(),
        temperature: markdown.frontmatter.temperature,
        top_p: markdown.frontmatter.top_p,
        mode: markdown.frontmatter.mode.unwrap_or_default(),
        hidden: markdown.frontmatter.hidden.unwrap_or(false),
        color: markdown.frontmatter.color.clone(),
        options: markdown.frontmatter.options.clone(),
        permissions: markdown.frontmatter.permissions.clone(),
        max_iters: markdown.frontmatter.max_iters,
        tool_failure_mode: markdown.frontmatter.tool_failure_mode.unwrap_or_default(),
        tools: markdown
            .frontmatter
            .tools
            .clone()
            .map(|t| t.tool_ids())
            .unwrap_or_default(),
        enabled: None,
    }))
}

fn discover_markdown_agents(
    config_path: &Path,
    current_dir: Option<&Path>,
) -> Result<BTreeMap<String, MarkdownAgentFile>, ConfigError> {
    let mut agents = BTreeMap::new();

    for dir in agent_prompt_search_dirs(config_path, current_dir) {
        if !dir.exists() {
            continue;
        }

        for file in markdown_files_in_dir(&dir)? {
            let name = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|stem| !stem.is_empty())
                .ok_or_else(|| {
                    ConfigError::InvalidReference(format!(
                        "agent markdown `{}` must have a valid UTF-8 file stem",
                        file.display()
                    ))
                })?
                .to_string();

            let content =
                fs::read_to_string(&file).map_err(|source| ConfigError::ReadMarkdownAsset {
                    path: file.display().to_string(),
                    source,
                })?;
            let (frontmatter, prompt_body) =
                parse_markdown_frontmatter::<MarkdownAgentFrontmatter>(&file, &content)?;
            agents.insert(
                name,
                MarkdownAgentFile {
                    frontmatter,
                    prompt_body: (!prompt_body.is_empty()).then_some(prompt_body),
                },
            );
        }
    }

    Ok(agents)
}

fn discover_instruction_files(
    config_path: &Path,
    current_dir: Option<&Path>,
) -> Result<Vec<InstructionFile>, ConfigError> {
    let mut instructions = Vec::new();
    let mut seen = BTreeSet::new();

    for path in instruction_search_paths(config_path, current_dir) {
        if !path.exists() || !seen.insert(path.clone()) {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|source| ConfigError::ReadMarkdownAsset {
                path: path.display().to_string(),
                source,
            })?;
        let content = content.trim().to_string();
        if content.is_empty() {
            continue;
        }

        instructions.push(InstructionFile { path, content });
    }

    Ok(instructions)
}

fn parse_markdown_frontmatter<T>(path: &Path, content: &str) -> Result<(T, String), ConfigError>
where
    T: DeserializeOwned + Default,
{
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Ok((T::default(), content.trim().to_string()));
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in &mut lines {
        if line == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_closing {
        return Err(ConfigError::InvalidMarkdownFrontmatter {
            path: path.display().to_string(),
            reason: "frontmatter must end with `---`".to_string(),
        });
    }

    let frontmatter_text = frontmatter_lines.join("\n");
    let frontmatter = if frontmatter_text.trim().is_empty() {
        T::default()
    } else {
        json5::from_str(&frontmatter_text).map_err(|err| {
            ConfigError::InvalidMarkdownFrontmatter {
                path: path.display().to_string(),
                reason: err.to_string(),
            }
        })?
    };

    Ok((
        frontmatter,
        lines.collect::<Vec<_>>().join("\n").trim().to_string(),
    ))
}

fn markdown_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut files = Vec::new();
    collect_markdown_files(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ConfigError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: dir.display().to_string(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: dir.display().to_string(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }

    Ok(())
}

fn agent_prompt_search_dirs(config_path: &Path, current_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for base in discovery_search_bases(config_path, current_dir) {
        push_unique_path(&mut dirs, base.join(".agent-harness").join("agents"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut dirs, config_dir.join(".agent-harness").join("agents"));
    }

    dirs.reverse();
    dirs
}

fn instruction_search_paths(config_path: &Path, current_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in discovery_search_bases(config_path, current_dir) {
        push_unique_path(&mut paths, base.join("AGENTS.md"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut paths, config_dir.join("AGENTS.md"));
    }

    paths
}

fn discovery_search_bases(config_path: &Path, current_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut bases = Vec::new();

    if let Some(current_dir) = current_dir {
        for base in project_search_bases(current_dir) {
            push_unique_path(&mut bases, base);
        }
    }

    if let Some(config_dir) = config_path.parent() {
        for base in project_search_bases(config_dir) {
            push_unique_path(&mut bases, base);
        }
    }

    if bases.is_empty() {
        bases.push(PathBuf::from("."));
    }

    bases
}

fn project_search_bases(start: &Path) -> Vec<PathBuf> {
    let ancestors = start.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    if let Some(index) = ancestors.iter().position(|path| path.join(".git").exists()) {
        return ancestors.into_iter().take(index + 1).collect();
    }
    vec![start.to_path_buf()]
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

pub fn resolve_config_layer_paths(explicit_path: Option<&Path>) -> Vec<PathBuf> {
    resolve_config_layer_paths_with_context(explicit_path, &ConfigDiscoveryContext::from_env())
}

pub fn resolve_config_layer_paths_with_context(
    explicit_path: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Vec<PathBuf> {
    if let Some(path) = explicit_path {
        return vec![path.to_path_buf()];
    }

    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_runtime_config_path(context) {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_runtime_config_env_path(context) {
        push_unique_path(&mut paths, env_path);
    }

    for local_path in discover_project_runtime_config_paths(&context.current_dir) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

pub fn resolve_config_path(explicit_path: Option<&Path>) -> Option<PathBuf> {
    resolve_config_path_with_context(explicit_path, &ConfigDiscoveryContext::from_env())
}

pub fn resolve_config_path_with_context(
    explicit_path: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }

    resolve_config_layer_paths_with_context(None, context)
        .into_iter()
        .last()
}

pub(super) fn resolve_tui_config_layer_paths_with_context(
    explicit_path: Option<&Path>,
    context: &ConfigDiscoveryContext,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_tui_config_path(context) {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_tui_config_env_path(context) {
        push_unique_path(&mut paths, env_path);
    }

    let local_base = if context.current_dir.as_os_str().is_empty() {
        explicit_path
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        context.current_dir.clone()
    };
    for local_path in discover_project_tui_config_paths(&local_base) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

fn discover_xdg_runtime_config_path(context: &ConfigDiscoveryContext) -> Option<PathBuf> {
    config_home_dir(context).and_then(|base| {
        [
            base.join("harness").join("harness.jsonc"),
            base.join("harness").join("harness.json"),
            base.join("harness").join("config.jsonc"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn discover_xdg_tui_config_path(context: &ConfigDiscoveryContext) -> Option<PathBuf> {
    config_home_dir(context).and_then(|base| {
        [
            base.join("harness").join("tui.jsonc"),
            base.join("harness").join("tui.json"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn config_home_dir(context: &ConfigDiscoveryContext) -> Option<PathBuf> {
    context
        .xdg_config_home
        .clone()
        .or_else(|| context.home.as_ref().map(|home| home.join(".config")))
}

fn discover_runtime_config_env_path(context: &ConfigDiscoveryContext) -> Option<PathBuf> {
    context.runtime_config_path.clone()
}

fn discover_tui_config_env_path(context: &ConfigDiscoveryContext) -> Option<PathBuf> {
    context.tui_config_path.clone()
}

fn discover_project_runtime_config_paths(start: &Path) -> Vec<PathBuf> {
    discover_existing_project_config_paths(
        start,
        &[
            "harness.jsonc",
            "harness.json",
            ".agent-harness/harness.jsonc",
            ".agent-harness/harness.json",
        ],
    )
}

fn discover_project_tui_config_paths(start: &Path) -> Vec<PathBuf> {
    discover_existing_project_config_paths(
        start,
        &[
            "tui.jsonc",
            "tui.json",
            ".agent-harness/tui.jsonc",
            ".agent-harness/tui.json",
        ],
    )
}

fn discover_existing_project_config_paths(start: &Path, relative_paths: &[&str]) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in project_config_search_bases(start) {
        for relative_path in relative_paths {
            let candidate = base.join(relative_path);
            if candidate.exists() {
                paths.push(candidate);
            }
        }
    }

    paths
}

fn project_config_search_bases(start: &Path) -> Vec<PathBuf> {
    let mut bases = project_search_bases(start);
    bases.reverse();
    bases
}

pub(super) fn resolve_configured_instruction_entries(
    entries: &[String],
    config_path: Option<&Path>,
) -> Result<Vec<InstructionFile>, ConfigError> {
    let mut resolved = Vec::new();
    let base_dir = config_path.and_then(Path::parent);

    for (index, entry) in entries.iter().enumerate() {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        let candidate = base_dir
            .map(|base| base.join(trimmed))
            .filter(|path| path.exists())
            .or_else(|| {
                let path = PathBuf::from(trimmed);
                path.exists().then_some(path)
            });

        if let Some(path) = candidate {
            let content =
                fs::read_to_string(&path).map_err(|source| ConfigError::ReadMarkdownAsset {
                    path: path.display().to_string(),
                    source,
                })?;
            let content = content.trim().to_string();
            if !content.is_empty() {
                resolved.push(InstructionFile { path, content });
            }
            continue;
        }

        resolved.push(InstructionFile {
            path: PathBuf::from(format!("<config instructions {}>", index + 1)),
            content: trimmed.to_string(),
        });
    }

    Ok(resolved)
}
