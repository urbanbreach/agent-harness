//! Resolution side of the formatter subsystem.

use std::collections::BTreeMap;
use std::path::Path;

use crate::config::FormatterConfig;

use super::{discovery::DiscoveryContext, FormatterDiscovery, BUILTIN_FORMATTERS};

#[derive(Debug, Clone)]
pub struct ResolvedFormatter {
    pub(crate) name: String,
    pub(crate) command: Vec<String>,
    pub(crate) environment: BTreeMap<String, String>,
}

/// Status entry for a single formatter, matching OpenCode's `Format.status()` shape.
#[derive(Debug, Clone)]
pub struct FormatterStatus {
    pub name: String,
    pub extensions: Vec<String>,
    pub enabled: bool,
}

/// Return a status entry for every configured/known formatter.
pub async fn formatter_status(
    config: &FormatterConfig,
    workspace_root: &Path,
    target_path: &str,
    discovery: &dyn FormatterDiscovery,
) -> Vec<FormatterStatus> {
    let _target_ext = file_extension(target_path).unwrap_or_default();

    let ruff_disabled = config.overrides.get("ruff").is_some_and(|ov| ov.disabled);
    let uv_disabled = config.overrides.get("uv").is_some_and(|ov| ov.disabled);
    let skip_both_python = ruff_disabled || uv_disabled;

    let mut names: Vec<String> = BUILTIN_FORMATTERS
        .iter()
        .map(|info| info.name.to_string())
        .collect();
    for name in config.overrides.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }

    let target_dir = Path::new(target_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let context = DiscoveryContext {
        workspace_root: workspace_root.to_path_buf(),
        target_dir,
        experimental_oxfmt: config.experimental_oxfmt,
    };

    let mut statuses = Vec::new();
    for name in names {
        if name == "oxfmt" && !config.experimental_oxfmt {
            continue;
        }
        if (name == "ruff" || name == "uv") && skip_both_python {
            continue;
        }

        let built_in = BUILTIN_FORMATTERS.iter().find(|info| info.name == name);
        let override_config = config.overrides.get(&name);

        if override_config.is_some_and(|ov| ov.disabled) {
            continue;
        }

        let effective_extensions: Vec<String> = override_config
            .and_then(|ov| ov.extensions.clone())
            .or_else(|| {
                built_in.map(|info| {
                    info.extensions
                        .iter()
                        .map(|ext| (*ext).to_string())
                        .collect()
                })
            })
            .unwrap_or_default();

        let discovered = discovery.resolve(&name, &context).await.is_some();

        statuses.push(FormatterStatus {
            name: name.clone(),
            extensions: effective_extensions,
            enabled: discovered,
        });
    }

    statuses
}

pub(in crate::coord) async fn resolve_formatters(
    config: &FormatterConfig,
    workspace_root: &Path,
    target_path: &str,
    extension: &str,
    discovery: &dyn FormatterDiscovery,
) -> Vec<ResolvedFormatter> {
    let ruff_disabled = config.overrides.get("ruff").is_some_and(|ov| ov.disabled);
    let uv_disabled = config.overrides.get("uv").is_some_and(|ov| ov.disabled);
    let skip_both_python = ruff_disabled || uv_disabled;

    let mut names: Vec<String> = BUILTIN_FORMATTERS
        .iter()
        .map(|info| info.name.to_string())
        .collect();
    for name in config.overrides.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }

    let target_dir = Path::new(target_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let context = DiscoveryContext {
        workspace_root: workspace_root.to_path_buf(),
        target_dir,
        experimental_oxfmt: config.experimental_oxfmt,
    };

    let mut resolved = Vec::new();
    for name in names {
        if name == "oxfmt" && !config.experimental_oxfmt {
            continue;
        }
        if (name == "ruff" || name == "uv") && skip_both_python {
            continue;
        }

        let built_in = BUILTIN_FORMATTERS.iter().find(|info| info.name == name);
        let override_config = config.overrides.get(&name);

        if override_config.is_some_and(|ov| ov.disabled) {
            continue;
        }

        let effective_extensions: Vec<String> = override_config
            .and_then(|ov| ov.extensions.clone())
            .or_else(|| {
                built_in.map(|info| {
                    info.extensions
                        .iter()
                        .map(|ext| (*ext).to_string())
                        .collect()
                })
            })
            .unwrap_or_default();
        if !extension_matches(extension, &effective_extensions) {
            continue;
        }

        let command = match override_config.and_then(|ov| ov.command.as_ref()) {
            Some(cmd) => cmd.clone(),
            None => match discovery.resolve(&name, &context).await {
                Some(cmd) => cmd,
                None => continue,
            },
        };
        if command.is_empty() {
            continue;
        }

        let mut environment = BTreeMap::new();
        if let Some(info) = built_in {
            if let Some(vars) = info.environment {
                for (key, value) in vars {
                    environment.insert((*key).to_string(), (*value).to_string());
                }
            }
        }
        if let Some(ov) = override_config {
            if let Some(vars) = &ov.environment {
                for (key, value) in vars {
                    environment.insert(key.clone(), value.clone());
                }
            }
        }

        resolved.push(ResolvedFormatter {
            name: name.clone(),
            command,
            environment,
        });
    }

    resolved
}

pub(crate) fn file_extension(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)
}

fn extension_matches(extension: &str, extensions: &[impl AsRef<str>]) -> bool {
    extensions
        .iter()
        .map(|ext| normalize_extension(ext.as_ref()))
        .any(|ext| ext == extension)
}

fn normalize_extension(extension: &str) -> String {
    extension
        .strip_prefix('.')
        .unwrap_or(extension)
        .to_lowercase()
}

#[cfg(test)]
pub(in crate::coord) async fn resolve_formatter_names(
    config: &FormatterConfig,
    workspace_root: &Path,
    target_path: &str,
    extension: &str,
    discovery: &dyn FormatterDiscovery,
) -> Vec<String> {
    resolve_formatters(config, workspace_root, target_path, extension, discovery)
        .await
        .into_iter()
        .map(|formatter| formatter.name)
        .collect()
}
