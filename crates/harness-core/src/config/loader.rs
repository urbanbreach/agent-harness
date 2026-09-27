use super::ordered::OrderedValue;
use super::*;
use serde_json::Value;
use std::io::Read;

pub fn load_config_from_str(raw: &str) -> Result<HarnessConfig, ConfigError> {
    load(raw, Path::new("."), None)
}

pub fn load_config_from_file(path: &Path) -> Result<HarnessConfig, ConfigError> {
    load(
        &read_text(path)?,
        path.parent().unwrap_or(Path::new(".")),
        path.parent(),
    )
}
pub fn load_config_from_file_with_context(
    path: &Path,
    context: &ConfigLoadContext,
) -> Result<HarnessConfig, ConfigError> {
    load(
        &read_text(path)?,
        path.parent().unwrap_or(Path::new(".")),
        Some(&context.discovery.current_dir),
    )
}
fn load(raw: &str, base: &Path, workspace: Option<&Path>) -> Result<HarnessConfig, ConfigError> {
    let mut instructions = Vec::new();
    let mut config = normalize::normalize(parse_layer(raw, base, &mut instructions)?)?;
    config.instruction_files = instructions;
    if let Some(workspace) = workspace {
        instruction_files(&mut config, workspace)?;
    }
    registries::register(&config)?;
    Ok(config)
}

pub(super) fn read_text(path: &Path) -> Result<String, ConfigError> {
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(4_194_305).read_to_string(&mut text))
        .map_err(|error| ConfigError(format!("cannot read {}: {error}", path.display())))?;
    if text.len() > 4_194_304 {
        return Err(ConfigError("configuration input exceeds 4 MiB".into()));
    }
    Ok(text)
}

pub(super) fn parse_layer(
    raw: &str,
    base: &Path,
    instructions: &mut Vec<InstructionFile>,
) -> Result<OrderedValue, ConfigError> {
    parse_layer_with_lookup(raw, base, instructions, &|name| std::env::var(name).ok())
}
pub(super) fn parse_layer_with_lookup(
    raw: &str,
    base: &Path,
    instructions: &mut Vec<InstructionFile>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<OrderedValue, ConfigError> {
    let ordered = prepare_layer(raw, base, lookup)?;
    let mut value = ordered.json()?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ConfigError("expected an object".into()))?;
    if let Some(value) = object.remove("instructions") {
        let entries = match value {
            Value::String(text) => vec![Value::String(text)],
            Value::Array(entries) => entries,
            _ => {
                return Err(ConfigError(
                    "instructions must be text or a list of text".into(),
                ))
            }
        };
        for entry in entries {
            let text = entry
                .as_str()
                .ok_or_else(|| ConfigError("instructions must contain text".into()))?;
            let path = base.join(text);
            let content = if path.is_file() {
                read_text(&path)?
            } else {
                text.into()
            };
            if !content.trim().is_empty() {
                instructions.push(InstructionFile {
                    path: if path.is_file() {
                        path
                    } else {
                        "<configured instructions>".into()
                    },
                    content,
                });
            }
        }
    }
    Ok(ordered.replace(value))
}

/// Expanded public input for config inspection, before defaults and profile resolution.
pub fn config_layer_value(
    raw: &str,
    base: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Value, ConfigError> {
    prepare_layer(raw, base, lookup)?.json()
}
fn prepare_layer(
    raw: &str,
    base: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<OrderedValue, ConfigError> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err(ConfigError("configuration input exceeds 4 MiB".into()));
    }
    let mut ordered = OrderedValue::parse(raw)?;
    for (alias, canonical) in [
        ("providers", "provider"),
        ("agents", "agent"),
        ("permissions", "permission"),
        ("hashlineEdit", "hashline_edit"),
        ("smallModel", "small_model"),
        ("modelProfile", "model_profile"),
        ("model_profiles", "model_profile"),
    ] {
        ordered.rename(alias, canonical)?;
    }
    let mut value = ordered.json()?;
    super::references::expand(&mut value, base, lookup)?;
    runtime::normalize_aliases(&mut value)?;
    Ok(ordered.replace(value))
}

pub(super) fn instruction_files(
    config: &mut HarnessConfig,
    directory: &Path,
) -> Result<(), ConfigError> {
    for base in discovery::search_roots(directory) {
        let path = base.join("AGENTS.md");
        if path.is_file() && !config.instruction_files.iter().any(|i| i.path == path) {
            config.instruction_files.push(InstructionFile {
                content: read_text(&path)?,
                path,
            });
        }
    }
    Ok(())
}

impl HarnessConfig {
    pub fn apply_session_dir_override(&mut self, directory: Option<PathBuf>) {
        if let Some(directory) = directory {
            self.runtime.session_dir = directory.clone();
            self.paths.session_dir = directory;
        }
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        let runtime = &self.runtime;
        let tasks = &runtime.background_tasks;
        if [
            tasks.default_concurrency,
            tasks.provider_concurrency,
            tasks.model_concurrency,
        ]
        .contains(&0)
            || runtime.permissions.ask_timeout_ms == 0
            || runtime.prompt.wait_timeout_ms == 0
            || runtime.session_dir.as_os_str().is_empty()
            || runtime.provider_retry.base_delay_ms > runtime.provider_retry.max_delay_ms
        {
            return Err(ConfigError(
                "invalid runtime concurrency, timeout, session path, or retry limits".into(),
            ));
        }
        for (name, profile) in &self.agents {
            if profile.max_iters == Some(0)
                || profile
                    .temperature
                    .is_some_and(|n| !n.is_finite() || !(0.0..=2.0).contains(&n))
                || profile
                    .top_p
                    .is_some_and(|n| !n.is_finite() || !(0.0..=1.0).contains(&n))
            {
                return Err(ConfigError(format!(
                    "invalid iteration or sampling limits for agent {name}"
                )));
            }
            if profile.model_ref != "mock:default" || self.providers.contains_key("mock") {
                resolve_model_selection(self, &profile.model_ref, profile.variant.as_deref())?;
            }
        }
        configured_model_profile_catalog(self)?;
        if self.hooks.lifecycle.len() > 64 {
            return Err(ConfigError("at most 64 lifecycle hooks are allowed".into()));
        }
        for hook in &self.hooks.lifecycle {
            hook.validate()?;
        }
        Ok(())
    }
}
