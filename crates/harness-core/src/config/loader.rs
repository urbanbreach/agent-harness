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

const ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS: [&str; 15] = [
    "$schema",
    "agents",
    "defaultAgent",
    "default_agent",
    "hooks",
    "hashlineEdit",
    "hashline_edit",
    "integrations",
    "lsp",
    "logging",
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
    Ok(parsed)
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
