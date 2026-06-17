mod discovery;
mod registry;

pub use discovery::{FormatterDiscovery, WhichFormatterDiscovery};
pub use registry::BUILTIN_FORMATTERS;

#[cfg(test)]
pub(in crate::coord) use discovery::FakeFormatterDiscovery;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::config::FormatterConfig;

const FORMAT_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_PLACEHOLDER: &str = "$FILE";

/// Run the configured formatters for `path` using real PATH discovery.
///
/// Failures are non-fatal and are returned as a human-readable warning string.
/// A successful run (including when no formatter is configured) returns `Ok(())`.
pub(in crate::coord) async fn run_formatter_for_path(
    config: &FormatterConfig,
    workspace_root: &Path,
    path: &str,
) -> Result<(), String> {
    run_formatter_for_path_with_discovery(config, workspace_root, path, &WhichFormatterDiscovery)
        .await
}

/// Run the configured formatters for `path` with the supplied discovery backend.
pub(in crate::coord) async fn run_formatter_for_path_with_discovery(
    config: &FormatterConfig,
    workspace_root: &Path,
    path: &str,
    discovery: &dyn FormatterDiscovery,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }

    let extension = match file_extension(path) {
        Some(ext) => ext,
        None => return Ok(()),
    };

    let _canonical_target = validate_path_inside_workspace(workspace_root, path).await?;

    let formatters = resolve_formatters(config, &extension, discovery).await;
    if formatters.is_empty() {
        return Ok(());
    }

    let mut warnings = Vec::new();
    for formatter in formatters {
        if let Err(warning) = run_single_formatter(formatter, workspace_root, path).await {
            warnings.push(warning);
        }
    }

    if warnings.is_empty() {
        Ok(())
    } else {
        Err(warnings.join("\n"))
    }
}

#[derive(Debug, Clone)]
struct ResolvedFormatter {
    #[allow(dead_code)]
    name: String,
    command: Vec<String>,
    environment: BTreeMap<String, String>,
}

async fn resolve_formatters(
    config: &FormatterConfig,
    extension: &str,
    discovery: &dyn FormatterDiscovery,
) -> Vec<ResolvedFormatter> {
    let ruff_disabled = config.overrides.get("ruff").is_some_and(|ov| ov.disabled);
    let uv_disabled = config
        .overrides
        .get("uvformat")
        .is_some_and(|ov| ov.disabled);
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
    names.sort();
    names.dedup();

    let mut resolved = Vec::new();
    for name in names {
        if name == "oxfmt" && !config.experimental_oxfmt {
            continue;
        }
        if (name == "ruff" || name == "uvformat") && skip_both_python {
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

        let (command, requires_path_discovery) =
            match override_config.and_then(|ov| ov.command.as_ref()) {
                Some(cmd) => (cmd.clone(), false),
                None => match built_in {
                    Some(info) => (
                        info.command.iter().map(|arg| (*arg).to_string()).collect(),
                        true,
                    ),
                    None => continue,
                },
            };
        if command.is_empty() {
            continue;
        }

        if requires_path_discovery && !discovery.is_on_path(&name).await {
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

async fn run_single_formatter(
    formatter: ResolvedFormatter,
    workspace_root: &Path,
    path: &str,
) -> Result<(), String> {
    let mut args = Vec::with_capacity(formatter.command.len() + 1);
    let mut saw_placeholder = false;
    for arg in &formatter.command {
        if arg.contains(FILE_PLACEHOLDER) {
            args.push(arg.replace(FILE_PLACEHOLDER, path));
            saw_placeholder = true;
        } else {
            args.push(arg.clone());
        }
    }
    if !saw_placeholder {
        args.push(path.to_string());
    }

    if args.is_empty() {
        return Err("formatter command is empty".to_string());
    }

    let mut command = Command::new(&args[0]);
    command
        .args(&args[1..])
        .current_dir(workspace_root)
        .env("HARNESS_FORMATTER", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in &formatter.environment {
        command.env(key, value);
    }

    let program = args[0].clone();
    let output = timeout(FORMAT_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("formatter `{}` timed out", program))
        .and_then(|result| {
            result.map_err(|err| format!("formatter `{}` failed to spawn: {err}", program))
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let reason = if stderr.is_empty() {
            stdout.to_string()
        } else {
            stderr.to_string()
        };
        Err(format!("formatter `{}` failed: {reason}", program))
    }
}

async fn validate_path_inside_workspace(
    workspace_root: &Path,
    path: &str,
) -> Result<PathBuf, String> {
    let canonical_workspace = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|err| format!("failed to resolve workspace root: {err}"))?;

    let input = Path::new(path);
    let relative = if input.is_absolute() {
        input
            .strip_prefix(&canonical_workspace)
            .map_err(|_| "path must be inside workspace root".to_string())?
    } else {
        input
    };

    let candidate = canonical_workspace.join(relative);
    let canonical_candidate = tokio::fs::canonicalize(&candidate)
        .await
        .map_err(|err| format!("failed to resolve target file: {err}"))?;

    if !canonical_candidate.starts_with(&canonical_workspace) {
        return Err(format!(
            "path {} escapes workspace root {}",
            canonical_candidate.display(),
            canonical_workspace.display()
        ));
    }

    Ok(canonical_candidate)
}

fn file_extension(path: &str) -> Option<String> {
    Path::new(path)
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
    extension: &str,
    discovery: &dyn FormatterDiscovery,
) -> Vec<String> {
    resolve_formatters(config, extension, discovery)
        .await
        .into_iter()
        .map(|formatter| formatter.name)
        .collect()
}
