//! Execution side of the formatter subsystem.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::config::FormatterConfig;

use super::resolver::{file_extension, resolve_formatters, ResolvedFormatter};

const FORMAT_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_PLACEHOLDER: &str = "$FILE";

/// Run the configured formatters for `path` using real PATH discovery.
///
/// Failures are non-fatal and are returned as a human-readable warning string.
/// A successful run (including when no formatter is configured) returns `Ok(())`.
pub async fn run_formatter_for_path(
    config: &FormatterConfig,
    workspace_root: &Path,
    path: &str,
) -> Result<(), String> {
    run_formatter_for_path_with_discovery(
        config,
        workspace_root,
        path,
        &super::RealFormatterDiscovery,
    )
    .await
}

/// Run the configured formatters for `path` with the supplied discovery backend.
pub(in crate::coord) async fn run_formatter_for_path_with_discovery(
    config: &FormatterConfig,
    workspace_root: &Path,
    path: &str,
    discovery: &dyn super::FormatterDiscovery,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }

    let extension = match file_extension(path) {
        Some(ext) => ext,
        None => return Ok(()),
    };

    let _canonical_target = validate_path_inside_workspace(workspace_root, path).await?;

    let formatters = resolve_formatters(config, workspace_root, path, &extension, discovery).await;
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
        .stdin(std::process::Stdio::null())
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
