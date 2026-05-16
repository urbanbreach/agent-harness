use serde::de::DeserializeOwned;

use super::*;

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
    pub tools: Option<Vec<String>>,
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
    let (agents, mut imports) = merge_configured_and_markdown_agents(
        &parsed.agents,
        config_path,
        &parsed.compatibility.disabled_agents,
        parsed.compatibility.required,
    )?;
    parsed.agents = agents;
    parsed.compatibility.imports.append(&mut imports);
    parsed.instruction_files = discover_instruction_files(config_path)?;
    Ok(())
}

fn merge_configured_and_markdown_agents(
    configured: &BTreeMap<String, ProfileConfig>,
    config_path: &Path,
    disabled_agents: &BTreeSet<String>,
    compatibility_required: bool,
) -> Result<
    (
        BTreeMap<String, ProfileConfig>,
        Vec<CompatibilityImportStatus>,
    ),
    ConfigError,
> {
    let (discovered, imports) =
        discover_markdown_agents(config_path, disabled_agents, compatibility_required)?;
    let agent_names = discovered
        .keys()
        .chain(configured.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = BTreeMap::new();

    for name in agent_names {
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
                profile_from_markdown_agent(markdown, fallback_model_ref)?
            }
            (None, None) => None,
        };

        if let Some(profile) = profile {
            merged.insert(name, profile);
        }
    }

    Ok((merged, imports))
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

    ProfileConfig {
        name: config
            .name
            .clone()
            .or_else(|| markdown.frontmatter.name.clone()),
        description: config.description.clone(),
        system_prompt: prompt,
        model_ref: config.model_ref.clone(),
        model_ref_explicit: config.model_ref_explicit,
        variant: config.variant.clone(),
        temperature: config.temperature,
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
        permissions: config.permissions.clone(),
        max_iters: config.max_iters,
        tool_failure_mode: config.tool_failure_mode,
        tools: config.tools.clone(),
    }
}

fn profile_from_markdown_agent(
    markdown: &MarkdownAgentFile,
    fallback_model_ref: Option<&str>,
) -> Result<Option<ProfileConfig>, ConfigError> {
    let Some(description) = markdown.frontmatter.description.clone() else {
        return Ok(None);
    };
    let model_ref = markdown
        .frontmatter
        .model_ref
        .clone()
        .or_else(|| fallback_model_ref.map(str::to_string))
        .unwrap_or_else(|| "default:default".to_string());

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
        tools: markdown.frontmatter.tools.clone().unwrap_or_default(),
    }))
}

fn discover_markdown_agents(
    config_path: &Path,
    disabled_agents: &BTreeSet<String>,
    compatibility_required: bool,
) -> Result<
    (
        BTreeMap<String, MarkdownAgentFile>,
        Vec<CompatibilityImportStatus>,
    ),
    ConfigError,
> {
    let mut agents = BTreeMap::new();
    let mut imports = Vec::new();

    for dir in agent_prompt_search_dirs(config_path) {
        if !dir.exists() {
            continue;
        }
        let compat_dir = is_compatibility_agent_dir(&dir);

        for file in markdown_files_in_dir(&dir)? {
            let name = match file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|stem| !stem.is_empty())
            {
                Some(stem) => stem.to_string(),
                None => {
                    let error = ConfigError::InvalidReference(format!(
                        "agent markdown `{}` must have a valid UTF-8 file stem",
                        file.display()
                    ));
                    if compat_dir && !compatibility_required {
                        imports.push(CompatibilityImportStatus::error(
                            CompatibilityImportKind::Agent,
                            file.display().to_string(),
                            file.clone(),
                            false,
                            error.to_string(),
                        ));
                        continue;
                    }
                    return Err(error);
                }
            };
            if agents.contains_key(&name) {
                continue;
            }
            if compat_dir && disabled_agents.contains(&name) {
                imports.push(CompatibilityImportStatus::disabled(
                    CompatibilityImportKind::Agent,
                    name,
                    file,
                ));
                continue;
            }

            let content = match fs::read_to_string(&file) {
                Ok(content) => content,
                Err(source) => {
                    let error = ConfigError::ReadMarkdownAsset {
                        path: file.display().to_string(),
                        source,
                    };
                    if compat_dir && !compatibility_required {
                        imports.push(CompatibilityImportStatus::error(
                            CompatibilityImportKind::Agent,
                            name,
                            file,
                            false,
                            error.to_string(),
                        ));
                        continue;
                    }
                    return Err(error);
                }
            };
            let (frontmatter, prompt_body) =
                match parse_markdown_frontmatter::<MarkdownAgentFrontmatter>(&file, &content) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        if compat_dir && !compatibility_required {
                            imports.push(CompatibilityImportStatus::error(
                                CompatibilityImportKind::Agent,
                                name,
                                file,
                                false,
                                error.to_string(),
                            ));
                            continue;
                        }
                        return Err(error);
                    }
                };
            if compat_dir && frontmatter.description.is_none() {
                imports.push(CompatibilityImportStatus::skipped(
                    CompatibilityImportKind::Agent,
                    name,
                    file,
                    "agent markdown missing description; not promoted to Harness profile",
                ));
                continue;
            }
            agents.insert(
                name.clone(),
                MarkdownAgentFile {
                    frontmatter,
                    prompt_body: (!prompt_body.is_empty()).then_some(prompt_body),
                },
            );
            if compat_dir {
                imports.push(CompatibilityImportStatus::imported(
                    CompatibilityImportKind::Agent,
                    name,
                    file,
                ));
            }
        }
    }

    Ok((agents, imports))
}

fn discover_instruction_files(config_path: &Path) -> Result<Vec<InstructionFile>, ConfigError> {
    let mut instructions = Vec::new();
    let mut seen = BTreeSet::new();

    for path in instruction_search_paths(config_path) {
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
        let file_type = entry
            .file_type()
            .map_err(|source| ConfigError::ReadMarkdownAsset {
                path: path.display().to_string(),
                source,
            })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }

        if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }

    Ok(())
}

fn agent_prompt_search_dirs(config_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for base in discovery_search_bases(config_path) {
        push_unique_path(&mut dirs, base.join(".agent-harness").join("agents"));
        push_unique_path(&mut dirs, base.join(".opencode").join("agent"));
        push_unique_path(&mut dirs, base.join(".opencode").join("agents"));
        push_unique_path(&mut dirs, base.join(".claude").join("agents"));
        push_unique_path(&mut dirs, base.join(".agents").join("agents"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut dirs, config_dir.join(".agent-harness").join("agents"));
        push_unique_path(&mut dirs, config_dir.join(".opencode").join("agent"));
        push_unique_path(&mut dirs, config_dir.join(".opencode").join("agents"));
        push_unique_path(&mut dirs, config_dir.join(".claude").join("agents"));
        push_unique_path(&mut dirs, config_dir.join(".agents").join("agents"));
    }

    dirs
}

fn is_compatibility_agent_dir(dir: &Path) -> bool {
    let text = dir.to_string_lossy();
    text.contains("/.opencode/")
        || text.contains("/.claude/")
        || text.contains("/.agents/")
        || text.starts_with(".opencode/")
        || text.starts_with(".claude/")
        || text.starts_with(".agents/")
}

fn instruction_search_paths(config_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in discovery_search_bases(config_path) {
        push_unique_path(&mut paths, base.join("AGENTS.md"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut paths, config_dir.join("AGENTS.md"));
    }

    paths
}

fn discovery_search_bases(config_path: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        for base in project_search_bases(&cwd) {
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
    if let Some(path) = explicit_path {
        return vec![path.to_path_buf()];
    }

    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_runtime_config_path() {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_runtime_config_env_path() {
        push_unique_path(&mut paths, env_path);
    }

    for local_path in discover_project_runtime_config_paths(
        env::current_dir()
            .ok()
            .as_deref()
            .unwrap_or_else(|| Path::new(".")),
    ) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

pub fn resolve_config_path(explicit_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }

    resolve_config_layer_paths(None).into_iter().last()
}

pub(super) fn resolve_tui_config_layer_paths(explicit_path: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_tui_config_path() {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_tui_config_env_path() {
        push_unique_path(&mut paths, env_path);
    }

    let local_base = env::current_dir().unwrap_or_else(|_| {
        explicit_path
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    for local_path in discover_project_tui_config_paths(&local_base) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

fn discover_xdg_runtime_config_path() -> Option<PathBuf> {
    config_home_dir().and_then(|base| {
        [
            base.join("harness").join("harness.jsonc"),
            base.join("harness").join("harness.json"),
            base.join("harness").join("config.jsonc"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn discover_xdg_tui_config_path() -> Option<PathBuf> {
    config_home_dir().and_then(|base| {
        [
            base.join("harness").join("tui.jsonc"),
            base.join("harness").join("tui.json"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn config_home_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn discover_runtime_config_env_path() -> Option<PathBuf> {
    env::var_os("HARNESS_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn discover_tui_config_env_path() -> Option<PathBuf> {
    env::var_os("HARNESS_TUI_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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
