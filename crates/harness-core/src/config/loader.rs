use super::discovery::{
    resolve_config_layer_paths, resolve_configured_instruction_entries,
    resolve_discovered_prompt_assets, resolve_tui_config_layer_paths,
};
use super::public::{
    translate_public_runtime_root, validate_public_root_config_object, PublicTuiConfig,
};
use super::*;

const REQUIRED_INTERNAL_CONFIG_SECTIONS: [&str; 4] =
    ["integrations", "permissions", "providers", "runtime"];

const ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS: [&str; 19] = [
    "$schema",
    "agents",
    "compatibility",
    "defaultAgent",
    "default_agent",
    "hooks",
    "hashlineEdit",
    "hashline_edit",
    "integrations",
    "lsp",
    "logging",
    "modelProfile",
    "model_profile",
    "model_profiles",
    "permissions",
    "providers",
    "runtime",
    "skills",
    "ui",
];

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: HarnessConfig,
    pub paths: Vec<PathBuf>,
}

impl LoadedConfig {
    pub fn primary_path(&self) -> Option<&Path> {
        self.paths.last().map(PathBuf::as_path)
    }

    pub fn path_display(&self) -> String {
        match self.paths.as_slice() {
            [] => "<none>".to_string(),
            [path] => path.display().to_string(),
            paths => paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
}

pub fn load_config_from_file(path: &Path) -> Result<HarnessConfig, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    let (parsed, configured_instructions) =
        parse_config_from_str(&raw, path.parent().or(Some(Path::new("."))))?;
    finalize_loaded_config(parsed, Some(path), configured_instructions)
}

pub fn load_config_from_str(raw: &str) -> Result<HarnessConfig, ConfigError> {
    let (parsed, configured_instructions) = parse_config_from_str(raw, None)?;
    finalize_loaded_config(parsed, None, configured_instructions)
}

fn parse_config_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<(HarnessConfig, Vec<String>), ConfigError> {
    let root = parse_public_config_value_from_str(raw, base_dir)?;
    let (translated, configured_instructions) = translate_public_runtime_root(root)?;
    Ok((
        parse_internal_config_from_value(translated)?,
        configured_instructions,
    ))
}

pub fn load_resolved_config(
    explicit_path: Option<&Path>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    let runtime_paths = resolve_config_layer_paths(explicit_path);
    let runtime_content = env::var("HARNESS_CONFIG_CONTENT").ok();
    if runtime_paths.is_empty() && runtime_content.is_none() {
        return Ok(None);
    }

    let tui_paths = resolve_tui_config_layer_paths(explicit_path);
    let config =
        load_resolved_config_from_paths(&runtime_paths, runtime_content.as_deref(), &tui_paths)?;
    let mut paths = runtime_paths.clone();
    for path in tui_paths {
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }

    Ok(Some(LoadedConfig { config, paths }))
}

fn parse_internal_config_from_value(root: serde_json::Value) -> Result<HarnessConfig, ConfigError> {
    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_internal_root_config_object(object)?;

    let mut parsed: HarnessConfig =
        serde_json::from_value(root).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    parsed.normalize_public_config_aliases()?;
    parsed.sync_derived_runtime_sections();
    Ok(parsed)
}

fn finalize_loaded_config(
    mut parsed: HarnessConfig,
    config_path: Option<&Path>,
    configured_instructions: Vec<String>,
) -> Result<HarnessConfig, ConfigError> {
    sync_compatibility_disabled_controls(&mut parsed);
    if let Some(path) = config_path {
        resolve_discovered_prompt_assets(&mut parsed, path)?;
        import_skill_compatibility_roots(&mut parsed, path);
        import_command_compatibility_templates(&mut parsed, path)?;
        merge_mcp_compat_file(&mut parsed, path)?;
        import_hook_compatibility_files(&mut parsed, path)?;
        import_extension_manifests(&mut parsed, path)?;
    }
    if !configured_instructions.is_empty() {
        let mut resolved =
            resolve_configured_instruction_entries(&configured_instructions, config_path)?;
        resolved.extend(parsed.instruction_files.clone());
        parsed.instruction_files = resolved;
    }
    parsed.validate_references()?;
    refresh_hook_runtime_config_registry(&parsed);
    refresh_skills_config_registry(&parsed);
    refresh_lsp_config_registry(&parsed);
    refresh_integrations_config_registry(&parsed);
    refresh_profile_model_metadata_registry(&parsed)?;
    Ok(parsed)
}

fn sync_compatibility_disabled_controls(parsed: &mut HarnessConfig) {
    parsed
        .skills
        .disabled_skills
        .extend(parsed.compatibility.disabled_skills.iter().cloned());
    parsed
        .hooks
        .disabled_hooks
        .extend(parsed.compatibility.disabled_hooks.iter().cloned());
}

fn merge_mcp_compat_file(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let mut seen = BTreeSet::new();
    for base in compatibility_search_bases(config_path) {
        let compat_path = base.join(".mcp.json");
        if !compat_path.exists() || !seen.insert(compat_path.clone()) {
            continue;
        }
        merge_single_mcp_compat_file(parsed, compat_path)?;
    }
    Ok(())
}

fn merge_single_mcp_compat_file(
    parsed: &mut HarnessConfig,
    compat_path: PathBuf,
) -> Result<(), ConfigError> {
    let raw = match fs::read_to_string(&compat_path) {
        Ok(raw) => raw,
        Err(source) => {
            let error = ConfigError::ReadFile {
                path: compat_path.display().to_string(),
                source,
            };
            return record_compat_error_or_fail(
                parsed,
                CompatibilityImportKind::Mcp,
                ".mcp.json",
                compat_path,
                error,
            );
        }
    };
    let root: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(root) => root,
        Err(err) => {
            return record_compat_error_or_fail(
                parsed,
                CompatibilityImportKind::Mcp,
                ".mcp.json",
                compat_path.clone(),
                ConfigError::ParseJson5(format!("{}: {err}", compat_path.display())),
            );
        }
    };
    let translated = match translate_mcp_compat_root(&root, &compat_path) {
        Ok(translated) => translated,
        Err(error) => {
            return record_compat_error_or_fail(
                parsed,
                CompatibilityImportKind::Mcp,
                ".mcp.json",
                compat_path,
                error,
            );
        }
    };
    for (name, mut server) in translated {
        let disabled = parsed.compatibility.disabled_mcp_servers.contains(&name);
        if disabled {
            match &mut server {
                McpServerConfig::Stdio { enabled, .. } | McpServerConfig::Http { enabled, .. } => {
                    *enabled = false;
                }
            }
        }
        parsed
            .integrations
            .mcp
            .servers
            .entry(name.clone())
            .or_insert(server);
        let status = if disabled {
            CompatibilityImportStatus::disabled(
                CompatibilityImportKind::Mcp,
                name,
                compat_path.clone(),
            )
        } else {
            CompatibilityImportStatus::imported(
                CompatibilityImportKind::Mcp,
                name,
                compat_path.clone(),
            )
        };
        parsed.compatibility.imports.push(status);
    }
    Ok(())
}

fn record_compat_error_or_fail(
    parsed: &mut HarnessConfig,
    kind: CompatibilityImportKind,
    name: impl Into<String>,
    source_path: PathBuf,
    error: ConfigError,
) -> Result<(), ConfigError> {
    if parsed.compatibility.required {
        return Err(error);
    }
    parsed
        .compatibility
        .imports
        .push(CompatibilityImportStatus::error(
            kind,
            name,
            source_path,
            false,
            error.to_string(),
        ));
    Ok(())
}

fn import_skill_compatibility_roots(parsed: &mut HarnessConfig, config_path: &Path) {
    let mut seen = BTreeSet::new();
    for base in compatibility_search_bases(config_path) {
        for root in [
            base.join(".opencode").join("skills"),
            base.join(".claude").join("skills"),
            base.join(".agents").join("skills"),
        ] {
            if !root.is_dir() || !seen.insert(root.clone()) {
                continue;
            }
            let Ok(entries) = fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.join("SKILL.md").exists() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let status = if parsed.compatibility.disabled_skills.contains(name) {
                    CompatibilityImportStatus::disabled(
                        CompatibilityImportKind::Skill,
                        name,
                        path.join("SKILL.md"),
                    )
                } else {
                    CompatibilityImportStatus::imported(
                        CompatibilityImportKind::Skill,
                        name,
                        path.join("SKILL.md"),
                    )
                };
                parsed.compatibility.imports.push(status);
            }
        }
    }
}

fn import_command_compatibility_templates(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let mut roots = Vec::new();
    for base in compatibility_search_bases(config_path) {
        for root in [
            base.join(".agent-harness").join("commands"),
            base.join(".opencode").join("command"),
            base.join(".opencode").join("commands"),
            base.join(".claude").join("commands"),
            base.join(".agents").join("commands"),
            base.join(".harness").join("commands"),
        ] {
            if !roots.iter().any(|existing| existing == &root) {
                roots.push(root);
            }
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        for root in [
            home.join(".config").join("agent-harness").join("commands"),
            home.join(".config").join("opencode").join("command"),
            home.join(".claude").join("commands"),
            home.join(".agents").join("commands"),
        ] {
            if !roots.iter().any(|existing| existing == &root) {
                roots.push(root);
            }
        }
    }

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let files = match collect_compat_markdown_files(&root) {
            Ok(files) => files,
            Err(error) => {
                record_compat_error_or_fail(
                    parsed,
                    CompatibilityImportKind::Command,
                    root.display().to_string(),
                    root,
                    error,
                )?;
                continue;
            }
        };
        for file in files {
            let Some(name) = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
            else {
                continue;
            };
            if parsed.compatibility.disabled_commands.contains(&name) {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::disabled(
                        CompatibilityImportKind::Command,
                        name,
                        file,
                    ));
                continue;
            }
            let content = match fs::read_to_string(&file) {
                Ok(content) => content.trim().to_string(),
                Err(source) => {
                    record_compat_error_or_fail(
                        parsed,
                        CompatibilityImportKind::Command,
                        name,
                        file.clone(),
                        ConfigError::ReadFile {
                            path: file.display().to_string(),
                            source,
                        },
                    )?;
                    continue;
                }
            };
            if content.is_empty() {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Command,
                        name,
                        file,
                        "command template is empty",
                    ));
                continue;
            }
            if parsed.compatibility.command_templates.contains_key(&name) {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Command,
                        name,
                        file,
                        "command template shadowed by higher-precedence root",
                    ));
                continue;
            }
            parsed.compatibility.command_templates.insert(
                name.clone(),
                CompatibilityCommandTemplate {
                    name: name.clone(),
                    description: first_markdown_heading(&content),
                    prompt: strip_markdown_frontmatter(&content).trim().to_string(),
                    source_path: file.clone(),
                    enabled: true,
                },
            );
            parsed
                .compatibility
                .imports
                .push(CompatibilityImportStatus::imported(
                    CompatibilityImportKind::Command,
                    name,
                    file,
                ));
        }
    }
    Ok(())
}

fn import_hook_compatibility_files(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let execute_imported_hooks = parsed.compatibility.enable_imported_hooks;
    let mut files = Vec::<(PathBuf, String, String, bool)>::new();
    for base in compatibility_search_bases(config_path) {
        for (file, allow_root_hook_map) in [
            (base.join(".claude").join("settings.json"), false),
            (base.join(".claude").join("settings.local.json"), false),
            (base.join(".opencode").join("hooks.json"), true),
            (base.join(".agents").join("hooks.json"), true),
        ] {
            if file.exists() && !files.iter().any(|(existing, _, _, _)| existing == &file) {
                files.push((
                    file.clone(),
                    compatibility_source_key(&base, &file),
                    compatibility_source_label(&base, &file),
                    allow_root_hook_map,
                ));
            }
        }
    }

    for (file, source_key, source_label, allow_root_hook_map) in files {
        let raw = match fs::read_to_string(&file) {
            Ok(raw) => raw,
            Err(source) => {
                record_compat_error_or_fail(
                    parsed,
                    CompatibilityImportKind::Hook,
                    file.display().to_string(),
                    file.clone(),
                    ConfigError::ReadFile {
                        path: file.display().to_string(),
                        source,
                    },
                )?;
                continue;
            }
        };
        let root: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(root) => root,
            Err(err) => {
                record_compat_error_or_fail(
                    parsed,
                    CompatibilityImportKind::Hook,
                    file.display().to_string(),
                    file,
                    ConfigError::ParseJson5(err.to_string()),
                )?;
                continue;
            }
        };
        let hooks = match root.get("hooks") {
            Some(hooks) => hooks.as_object(),
            None if allow_root_hook_map => root.as_object(),
            None => None,
        };
        let Some(hooks) = hooks else {
            continue;
        };
        for (event_name, entries) in hooks {
            if matches!(
                event_name.as_str(),
                "UserPromptSubmit" | "user_prompt_submit"
            ) {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Hook,
                        event_name,
                        file.clone(),
                        "prompt-submit hooks are skipped because Harness has no prompt-submission lifecycle event; refusing to run them at session start",
                    ));
                continue;
            }
            let Some(event) = compat_hook_event(event_name) else {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Hook,
                        event_name,
                        file.clone(),
                        "unsupported hook event for safe typed import",
                    ));
                continue;
            };
            if compat_hook_string_command_count(entries) > 0 {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Hook,
                        event_name,
                        file.clone(),
                        "string-form hook commands are skipped; use a safe command argv array",
                    ));
            }
            let commands = compat_hook_commands(entries);
            if commands.is_empty() {
                parsed
                    .compatibility
                    .imports
                    .push(CompatibilityImportStatus::skipped(
                        CompatibilityImportKind::Hook,
                        event_name,
                        file.clone(),
                        "hook entry has no safe command argv array",
                    ));
                continue;
            }
            for (index, command) in commands.into_iter().enumerate() {
                let id = format!("compat:{event_name}:{source_key}:{index}");
                let legacy_id = format!("compat:{event_name}:{index}");
                let legacy_source_id = format!("compat:{event_name}:{source_label}:{index}");
                if parsed.compatibility.disabled_hooks.contains(&id)
                    || parsed.hooks.disabled_hooks.contains(&id)
                    || parsed.compatibility.disabled_hooks.contains(&legacy_id)
                    || parsed.hooks.disabled_hooks.contains(&legacy_id)
                    || parsed
                        .compatibility
                        .disabled_hooks
                        .contains(&legacy_source_id)
                    || parsed.hooks.disabled_hooks.contains(&legacy_source_id)
                {
                    parsed
                        .compatibility
                        .imports
                        .push(CompatibilityImportStatus::disabled(
                            CompatibilityImportKind::Hook,
                            id,
                            file.clone(),
                        ));
                    continue;
                }
                if execute_imported_hooks {
                    parsed.hooks.lifecycle.push(LifecycleHookConfig {
                        id: Some(id.clone()),
                        event,
                        command,
                        cwd: None,
                        timeout_ms: default_hook_timeout_ms(),
                        critical: false,
                        env: BTreeMap::new(),
                    });
                    parsed
                        .compatibility
                        .imports
                        .push(CompatibilityImportStatus::imported(
                            CompatibilityImportKind::Hook,
                            id,
                            file.clone(),
                        ));
                } else {
                    parsed
                        .compatibility
                        .imports
                        .push(CompatibilityImportStatus::skipped(
                            CompatibilityImportKind::Hook,
                            id,
                            file.clone(),
                            "compatibility hook execution is disabled; set compatibility.enable_imported_hooks=true to opt in",
                        ));
                }
            }
        }
    }
    Ok(())
}

fn import_extension_manifests(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let mut files = Vec::new();
    for base in compatibility_search_bases(config_path) {
        for root in [
            base.join(".agent-harness").join("extensions"),
            base.join(".agents").join("plugins"),
            base.join(".codex").join("plugins"),
        ] {
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.filter_map(Result::ok) {
                    let file = entry.path().join("plugin.json");
                    if file.exists() && !files.iter().any(|existing| existing == &file) {
                        files.push(file);
                    }
                }
            }
        }
        let root_manifest = base.join(".codex-plugin").join("plugin.json");
        if root_manifest.exists() && !files.iter().any(|existing| existing == &root_manifest) {
            files.push(root_manifest);
        }
    }

    for file in files {
        let raw = match fs::read_to_string(&file) {
            Ok(raw) => raw,
            Err(source) => {
                record_compat_error_or_fail(
                    parsed,
                    CompatibilityImportKind::ExtensionManifest,
                    file.display().to_string(),
                    file.clone(),
                    ConfigError::ReadFile {
                        path: file.display().to_string(),
                        source,
                    },
                )?;
                continue;
            }
        };
        let root: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(root) => root,
            Err(err) => {
                record_compat_error_or_fail(
                    parsed,
                    CompatibilityImportKind::ExtensionManifest,
                    file.display().to_string(),
                    file,
                    ConfigError::ParseJson5(err.to_string()),
                )?;
                continue;
            }
        };
        let id = root
            .get("id")
            .or_else(|| root.get("name"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or("extension")
            .to_string();
        let enabled = !parsed.compatibility.disabled_extensions.contains(&id);
        let registration = ExtensionManifestRegistration {
            id: id.clone(),
            name: root
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            version: root
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_path: file.clone(),
            enabled,
        };
        parsed
            .compatibility
            .extension_manifests
            .insert(id.clone(), registration);
        let status = if enabled {
            CompatibilityImportStatus::imported(
                CompatibilityImportKind::ExtensionManifest,
                id,
                file,
            )
        } else {
            CompatibilityImportStatus::disabled(
                CompatibilityImportKind::ExtensionManifest,
                id,
                file,
            )
        };
        parsed.compatibility.imports.push(status);
    }
    Ok(())
}

fn compatibility_search_bases(config_path: &Path) -> Vec<PathBuf> {
    let cwd = env::current_dir().ok();
    compatibility_search_bases_from(config_path, cwd.as_deref())
}

fn compatibility_search_bases_from(config_path: &Path, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(config_dir) = config_path.parent() {
        for base in config_dir.ancestors() {
            push_unique_path(&mut bases, base.to_path_buf());
            if base.join(".git").exists() {
                break;
            }
        }
    }
    if let Some(cwd) = cwd {
        for base in cwd.ancestors() {
            push_unique_path(&mut bases, base.to_path_buf());
            if base.join(".git").exists() {
                break;
            }
        }
    }
    if bases.is_empty() {
        bases.push(PathBuf::from("."));
    }
    bases
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

fn compatibility_source_key(base: &Path, path: &Path) -> String {
    let label = compatibility_source_label(base, path);
    let hash_input = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    format!("{label}-{:08x}", stable_compat_hash(&hash_input))
}

fn compatibility_source_label(base: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(base).unwrap_or(path);
    let mut key = String::new();
    for component in relative.components() {
        if let Component::Normal(value) = component {
            if !key.is_empty() {
                key.push('-');
            }
            key.push_str(&sanitize_compat_id_component(&value.to_string_lossy()));
        }
    }
    if key.is_empty() {
        sanitize_compat_id_component(&path.to_string_lossy())
    } else {
        key
    }
}

fn stable_compat_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5_u32;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn sanitize_compat_id_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "source".to_string()
    } else {
        trimmed.to_string()
    }
}

fn collect_compat_markdown_files(root: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut files = Vec::new();
    collect_compat_markdown_files_inner(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_compat_markdown_files_inner(
    root: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), ConfigError> {
    let entries = fs::read_dir(root)
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: root.display().to_string(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: root.display().to_string(),
            source,
        })?;
    for entry in entries {
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| ConfigError::ReadMarkdownAsset {
                path: path.display().to_string(),
                source,
            })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_compat_markdown_files_inner(&path, files)?;
        } else if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn first_markdown_heading(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix('#')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
    })
}

fn strip_markdown_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---\n") else {
        return content;
    };
    let Some((_, body)) = rest.split_once("\n---") else {
        return content;
    };
    body.trim_start_matches(['\n', '\r'])
}

fn compat_hook_event(name: &str) -> Option<HookLifecycleEvent> {
    match name {
        "run_started" | "SessionStart" => Some(HookLifecycleEvent::RunStarted),
        "run_finished" | "Stop" => Some(HookLifecycleEvent::RunFinished),
        "run_failed" => Some(HookLifecycleEvent::RunFailed),
        "agent_turn_started" => Some(HookLifecycleEvent::AgentTurnStarted),
        "agent_turn_finished" => Some(HookLifecycleEvent::AgentTurnFinished),
        "tool_call_started" | "PreToolUse" => Some(HookLifecycleEvent::ToolCallStarted),
        "tool_call_finished" | "PostToolUse" => Some(HookLifecycleEvent::ToolCallFinished),
        "provider_request_started" => Some(HookLifecycleEvent::ProviderRequestStarted),
        "provider_request_finished" => Some(HookLifecycleEvent::ProviderRequestFinished),
        "subagent_spawned" => Some(HookLifecycleEvent::SubagentSpawned),
        "subagent_finished" | "SubagentStop" => Some(HookLifecycleEvent::SubagentFinished),
        "permission_requested" => Some(HookLifecycleEvent::PermissionRequested),
        "permission_resolved" => Some(HookLifecycleEvent::PermissionResolved),
        _ => None,
    }
}

fn compat_hook_string_command_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(_) => 1,
        serde_json::Value::Array(items) => {
            if items.iter().all(serde_json::Value::is_string) {
                0
            } else {
                items.iter().map(compat_hook_string_command_count).sum()
            }
        }
        serde_json::Value::Object(object) => object
            .get("command")
            .or_else(|| object.get("commands"))
            .map(compat_hook_string_command_count)
            .unwrap_or_default(),
        _ => 0,
    }
}

fn compat_hook_commands(value: &serde_json::Value) -> Vec<Vec<String>> {
    match value {
        // String-form hook commands require shell tokenization, so the compatibility
        // importer deliberately skips them instead of creating an invalid argv like
        // Command::new("echo ok"). Use an explicit JSON argv array to import.
        serde_json::Value::String(_) => Vec::new(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(serde_json::Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|tokens| vec![tokens.into_iter().map(str::to_string).collect()])
            .unwrap_or_else(|| {
                items
                    .iter()
                    .flat_map(compat_hook_commands)
                    .collect::<Vec<_>>()
            }),
        serde_json::Value::Object(object) => object
            .get("command")
            .or_else(|| object.get("commands"))
            .map(compat_hook_commands)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn translate_mcp_compat_root(
    root: &serde_json::Value,
    path: &Path,
) -> Result<BTreeMap<String, McpServerConfig>, ConfigError> {
    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    let servers_value = object
        .get("mcpServers")
        .or_else(|| object.get("servers"))
        .or_else(|| object.get("mcp"));
    let Some(serde_json::Value::Object(servers)) = servers_value else {
        return Ok(BTreeMap::new());
    };
    let mut translated = BTreeMap::new();
    for (name, value) in servers {
        if let Some(server) = translate_mcp_compat_server(value, path)? {
            translated.insert(name.clone(), server);
        }
    }
    Ok(translated)
}

fn translate_mcp_compat_server(
    value: &serde_json::Value,
    path: &Path,
) -> Result<Option<McpServerConfig>, ConfigError> {
    let object = value.as_object().ok_or_else(|| {
        ConfigError::ParseJson5(format!(
            "{}: .mcp.json server entries must be objects",
            path.display()
        ))
    })?;
    let enabled = !matches!(object.get("enabled"), Some(serde_json::Value::Bool(false)))
        && !matches!(object.get("disabled"), Some(serde_json::Value::Bool(true)));
    if let Some(endpoint) = object
        .get("url")
        .or_else(|| object.get("endpoint"))
        .and_then(serde_json::Value::as_str)
    {
        return Ok(Some(McpServerConfig::Http {
            endpoint: endpoint.to_string(),
            headers: string_map_field(object.get("headers"))?,
            timeout_secs: u64_field(object.get("timeout")).unwrap_or(default_mcp_timeout_secs()),
            enabled,
        }));
    }

    let command = match object.get("command") {
        Some(serde_json::Value::String(command)) => {
            let mut command_parts = vec![command.to_string()];
            if let Some(args) = string_vec_field(object.get("args"))? {
                command_parts.extend(args);
            }
            command_parts
        }
        Some(serde_json::Value::Array(_)) => {
            string_vec_field(object.get("command"))?.unwrap_or_default()
        }
        None => return Ok(None),
        Some(_) => {
            return Err(ConfigError::ParseJson5(format!(
                "{}: .mcp.json command must be a string or string array",
                path.display()
            )));
        }
    };
    Ok(Some(McpServerConfig::Stdio {
        command,
        env: string_map_field(object.get("env").or_else(|| object.get("environment")))?,
        cwd: object
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from),
        timeout_secs: u64_field(object.get("timeout")).unwrap_or(default_mcp_timeout_secs()),
        enabled,
    }))
}

fn string_vec_field(value: Option<&serde_json::Value>) -> Result<Option<Vec<String>>, ConfigError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    ConfigError::ParseJson5(".mcp.json array values must be strings".to_string())
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        serde_json::Value::String(value) => Ok(Some(vec![value.clone()])),
        _ => Err(ConfigError::ParseJson5(
            ".mcp.json field must be a string or string array".to_string(),
        )),
    }
}

fn string_map_field(
    value: Option<&serde_json::Value>,
) -> Result<BTreeMap<String, String>, ConfigError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value.as_object().ok_or_else(|| {
        ConfigError::ParseJson5(".mcp.json map fields must be objects".to_string())
    })?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| {
                    ConfigError::ParseJson5(".mcp.json map values must be strings".to_string())
                })
        })
        .collect()
}

fn u64_field(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(serde_json::Value::as_u64)
}

fn load_resolved_config_from_paths(
    runtime_paths: &[PathBuf],
    runtime_content: Option<&str>,
    tui_paths: &[PathBuf],
) -> Result<HarnessConfig, ConfigError> {
    let mut merged: Option<serde_json::Value> = None;
    let mut configured_instructions = Vec::new();

    for path in runtime_paths {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        let root = parse_public_config_value_from_str(&raw, path.parent())?;
        let (fragment, instructions) = translate_public_runtime_root(root)?;
        configured_instructions.extend(instructions);
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    if let Some(runtime_content) = runtime_content {
        let root = parse_public_config_value_from_str(runtime_content, None)?;
        let (fragment, instructions) = translate_public_runtime_root(root)?;
        configured_instructions.extend(instructions);
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    let merged = merged.ok_or_else(|| ConfigError::ReadFile {
        path: "<merged-config>".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "no config files resolved"),
    })?;
    let primary_path = runtime_paths.last().map(PathBuf::as_path);
    let mut parsed = parse_internal_config_from_value(merged)?;
    if !tui_paths.is_empty() {
        let tui = load_merged_tui_config_from_files(tui_paths)?;
        apply_public_tui_config(&mut parsed, tui);
    }
    finalize_loaded_config(parsed, primary_path, configured_instructions)
}

fn parse_public_config_value_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<serde_json::Value, ConfigError> {
    let mut root: serde_json::Value =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;

    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_public_root_config_object(object)?;
    resolve_config_value_references(&mut root, base_dir)?;
    Ok(root)
}

fn load_merged_tui_config_from_files(paths: &[PathBuf]) -> Result<PublicTuiConfig, ConfigError> {
    let mut merged: Option<serde_json::Value> = None;

    for path in paths {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        let fragment: serde_json::Value =
            json5::from_str(&raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    serde_json::from_value(merged.unwrap_or_else(|| serde_json::json!({})))
        .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

fn apply_public_tui_config(parsed: &mut HarnessConfig, tui: PublicTuiConfig) {
    parsed.ui.keybindings = tui.keybindings;
}

pub(super) fn resolve_string_reference(
    value: &str,
    base_dir: Option<&Path>,
) -> Result<String, ConfigError> {
    if let Some(reference) = value
        .strip_prefix("{env:")
        .and_then(|reference| reference.strip_suffix('}'))
    {
        return Ok(env::var(reference).unwrap_or_default());
    }

    if let Some(reference) = value
        .strip_prefix("{file:")
        .and_then(|reference| reference.strip_suffix('}'))
    {
        let trimmed = reference.trim();
        if trimmed.is_empty() {
            return Ok(value.to_string());
        }

        let path = PathBuf::from(trimmed);
        let resolved_path = if path.is_absolute() {
            path
        } else if let Some(base_dir) = base_dir {
            base_dir.join(path)
        } else {
            path
        };

        return fs::read_to_string(&resolved_path).map_err(|source| ConfigError::ReadFile {
            path: resolved_path.display().to_string(),
            source,
        });
    }

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

    match env::var(reference) {
        Ok(resolved) => Ok(resolved),
        Err(_) => Err(ConfigError::MissingEnvironmentVariable(
            reference.to_string(),
        )),
    }
}

fn resolve_config_value_references(
    value: &mut serde_json::Value,
    base_dir: Option<&Path>,
) -> Result<(), ConfigError> {
    match value {
        serde_json::Value::String(string) => {
            *string = resolve_string_reference(string, base_dir)?;
        }
        serde_json::Value::Array(values) => {
            for value in values {
                resolve_config_value_references(value, base_dir)?;
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                resolve_config_value_references(value, base_dir)?;
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }

    Ok(())
}

fn validate_internal_root_config_object(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ConfigError> {
    if !object.contains_key("agents") {
        let mut missing = REQUIRED_INTERNAL_CONFIG_SECTIONS.to_vec();
        missing.push("agents");
        missing.sort_unstable();
        return Err(ConfigError::MissingRequiredSections(missing.join(", ")));
    }

    let mut missing = REQUIRED_INTERNAL_CONFIG_SECTIONS
        .iter()
        .copied()
        .filter(|key| !object.contains_key(*key))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(ConfigError::MissingRequiredSections(missing.join(", ")));
    }

    let mut unknown = object
        .keys()
        .filter(|key| !ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(ConfigError::UnknownTopLevelKeys(format!(
            "unknown top-level config keys: {}; expected only {}",
            format_backticked_list(unknown.iter().copied()),
            format_backticked_list(ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS)
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compatibility_search_bases_from;

    #[test]
    fn compatibility_search_bases_prefer_explicit_config_roots_before_cwd() {
        let temp = tempfile::tempdir().expect("tempdir");
        let launch_repo = temp.path().join("launch-repo");
        let config_repo = temp.path().join("config-repo");
        let config_dir = config_repo.join("configs");
        std::fs::create_dir_all(launch_repo.join(".git")).expect("launch git dir");
        std::fs::create_dir_all(config_repo.join(".git")).expect("config git dir");
        std::fs::create_dir_all(&config_dir).expect("config dir");

        let bases =
            compatibility_search_bases_from(&config_dir.join("harness.jsonc"), Some(&launch_repo));

        let config_dir_index = bases
            .iter()
            .position(|base| base == &config_dir)
            .expect("config directory is searched");
        let config_repo_index = bases
            .iter()
            .position(|base| base == &config_repo)
            .expect("config repo is searched");
        let launch_repo_index = bases
            .iter()
            .position(|base| base == &launch_repo)
            .expect("launch repo is searched after config roots");

        assert!(config_dir_index < launch_repo_index, "{bases:?}");
        assert!(config_repo_index < launch_repo_index, "{bases:?}");
    }
}
