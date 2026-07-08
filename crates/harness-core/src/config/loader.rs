// allow: SIZE_OK — config loader (JSON5 parse + translate + validate + registry refresh pipeline)
use super::discovery::{
    resolve_config_layer_paths_with_context, resolve_configured_instruction_entries,
    resolve_discovered_prompt_assets, resolve_discovered_prompt_assets_with_current_dir,
    resolve_tui_config_layer_paths_with_context, ConfigDiscoveryContext,
};
use super::public::{
    translate_public_runtime_root, validate_public_root_config_object, PublicTuiConfig,
};
use super::*;

const REQUIRED_INTERNAL_CONFIG_SECTIONS: [&str; 4] =
    ["integrations", "permissions", "providers", "runtime"];

const ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS: [&str; 23] = [
    "$schema",
    "agents",
    "defaultAgent",
    "default_agent",
    "disabled_agents",
    "disabled_providers",
    "enabled_providers",
    "formatter",
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
    "small_model",
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

#[derive(Debug, Clone)]
pub struct ConfigLoadContext {
    pub discovery: ConfigDiscoveryContext,
    pub runtime_content: Option<String>,
}

impl ConfigLoadContext {
    pub fn from_env() -> Self {
        Self {
            discovery: ConfigDiscoveryContext::from_env(),
            runtime_content: env::var("HARNESS_CONFIG_CONTENT").ok(),
        }
    }

    pub fn with_current_dir(mut self, current_dir: PathBuf) -> Self {
        self.discovery = self.discovery.with_current_dir(current_dir);
        self
    }

    pub fn apply_env_var(mut self, name: &str, value: Option<String>) -> Self {
        if name == "HARNESS_CONFIG_CONTENT" {
            self.runtime_content = value;
        } else {
            self.discovery = self.discovery.apply_env_var(name, value);
        }
        self
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

pub fn load_config_from_file_with_context(
    path: &Path,
    context: &ConfigLoadContext,
) -> Result<HarnessConfig, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    let (parsed, configured_instructions) =
        parse_config_from_str(&raw, path.parent().or(Some(Path::new("."))))?;
    finalize_loaded_config_with_current_dir(
        parsed,
        Some(path),
        configured_instructions,
        Some(context.discovery.current_dir.as_path()),
    )
}

pub fn load_config_from_str(raw: &str) -> Result<HarnessConfig, ConfigError> {
    let (parsed, configured_instructions) = parse_config_from_str(raw, None)?;
    finalize_loaded_config(parsed, None, configured_instructions)
}

#[cfg(test)]
pub(super) fn load_config_from_str_with_lookup(
    raw: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<HarnessConfig, ConfigError> {
    let (parsed, configured_instructions) = parse_config_from_str_with_lookup(raw, None, lookup)?;
    finalize_loaded_config(parsed, None, configured_instructions)
}

fn parse_config_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<(HarnessConfig, Vec<String>), ConfigError> {
    parse_config_from_str_with_lookup(raw, base_dir, &|name| env::var(name).ok())
}

fn parse_config_from_str_with_lookup(
    raw: &str,
    base_dir: Option<&Path>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<(HarnessConfig, Vec<String>), ConfigError> {
    let root = parse_public_config_value_from_str_with_lookup(raw, base_dir, lookup)?;
    let (translated, configured_instructions) = translate_public_runtime_root(root)?;
    let mut translated = translated;
    resolve_config_value_references_with_lookup(&mut translated, base_dir, lookup)?;
    Ok((
        parse_internal_config_from_value(translated)?,
        configured_instructions,
    ))
}

pub fn load_resolved_config(
    explicit_path: Option<&Path>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    load_resolved_config_with_context(explicit_path, &ConfigLoadContext::from_env())
}

pub fn load_resolved_config_with_context(
    explicit_path: Option<&Path>,
    context: &ConfigLoadContext,
) -> Result<Option<LoadedConfig>, ConfigError> {
    let runtime_paths = resolve_config_layer_paths_with_context(explicit_path, &context.discovery);
    if runtime_paths.is_empty() && context.runtime_content.is_none() {
        return Ok(None);
    }

    let tui_paths = resolve_tui_config_layer_paths_with_context(explicit_path, &context.discovery);
    let config = load_resolved_config_from_paths(
        &runtime_paths,
        context.runtime_content.as_deref(),
        &tui_paths,
        Some(context.discovery.current_dir.as_path()),
    )?;
    let mut paths = runtime_paths.clone();
    for path in tui_paths {
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }

    Ok(Some(LoadedConfig { config, paths }))
}

fn parse_internal_config_from_value(root: serde_json::Value) -> Result<HarnessConfig, ConfigError> {
    let mut root = root;
    let object = root.as_object_mut().ok_or(ConfigError::InvalidRootObject)?;
    validate_internal_root_config_object(object)?;

    let small_model = object
        .remove("small_model")
        .and_then(|value| value.as_str().map(str::to_string));

    let disabled_agents = object
        .remove("disabled_agents")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();

    let mut parsed: HarnessConfig =
        serde_json::from_value(root).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    parsed.small_model = small_model;
    parsed.disabled_agents = disabled_agents;
    parsed.normalize_public_config_aliases()?;
    parsed.sync_derived_runtime_sections();
    Ok(parsed)
}

fn finalize_loaded_config(
    mut parsed: HarnessConfig,
    config_path: Option<&Path>,
    configured_instructions: Vec<String>,
) -> Result<HarnessConfig, ConfigError> {
    if let Some(path) = config_path {
        resolve_discovered_prompt_assets(&mut parsed, path)?;
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
    set_registered_formatter_config(parsed.formatter.clone());
    Ok(parsed)
}

fn finalize_loaded_config_with_current_dir(
    mut parsed: HarnessConfig,
    config_path: Option<&Path>,
    configured_instructions: Vec<String>,
    current_dir: Option<&Path>,
) -> Result<HarnessConfig, ConfigError> {
    if let Some(path) = config_path {
        resolve_discovered_prompt_assets_with_current_dir(&mut parsed, path, current_dir)?;
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
    set_registered_formatter_config(parsed.formatter.clone());
    Ok(parsed)
}

fn load_resolved_config_from_paths(
    runtime_paths: &[PathBuf],
    runtime_content: Option<&str>,
    tui_paths: &[PathBuf],
    current_dir: Option<&Path>,
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
    finalize_loaded_config_with_current_dir(
        parsed,
        primary_path,
        configured_instructions,
        current_dir,
    )
}

fn parse_public_config_value_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<serde_json::Value, ConfigError> {
    parse_public_config_value_from_str_with_lookup(raw, base_dir, &|name| env::var(name).ok())
}

fn parse_public_config_value_from_str_with_lookup(
    raw: &str,
    base_dir: Option<&Path>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<serde_json::Value, ConfigError> {
    let mut root: serde_json::Value =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;

    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_public_root_config_object(object)?;
    resolve_config_value_references_with_lookup(&mut root, base_dir, lookup)?;
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

pub(super) fn resolve_string_reference_with_lookup(
    value: &str,
    base_dir: Option<&Path>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<String, ConfigError> {
    if let Some(reference) = value
        .strip_prefix("{env:")
        .and_then(|reference| reference.strip_suffix('}'))
    {
        return Ok(lookup(reference).unwrap_or_default());
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
        return Ok(lookup(key)
            .filter(|resolved| !resolved.is_empty())
            .unwrap_or_else(|| fallback.to_string()));
    }

    match lookup(reference) {
        Some(resolved) => Ok(resolved),
        None => Err(ConfigError::MissingEnvironmentVariable(
            reference.to_string(),
        )),
    }
}

fn resolve_config_value_references_with_lookup(
    value: &mut serde_json::Value,
    base_dir: Option<&Path>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<(), ConfigError> {
    match value {
        serde_json::Value::String(string) => {
            *string = resolve_string_reference_with_lookup(string, base_dir, lookup)?;
        }
        serde_json::Value::Array(values) => {
            for value in values {
                resolve_config_value_references_with_lookup(value, base_dir, lookup)?;
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                resolve_config_value_references_with_lookup(value, base_dir, lookup)?;
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
