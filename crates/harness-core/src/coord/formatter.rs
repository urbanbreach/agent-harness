use std::path::Path;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::config::{FormatterConfig, FormatterLanguageConfig};

const FORMAT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run the configured formatter for `path` if one exists and formatting is enabled.
///
/// Failures are non-fatal and are returned as a human-readable warning string.
/// A successful run (including when no formatter is configured) returns `Ok(())`.
pub(in crate::coord) async fn run_formatter_for_path(
    config: &FormatterConfig,
    workspace_root: &Path,
    path: &str,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }

    let extension = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase);
    let extension = match extension {
        Some(ext) => ext,
        None => return Ok(()),
    };

    let language_config = match config.languages.get(&extension) {
        Some(config) => config,
        None => return Ok(()),
    };

    run_formatter_command(language_config, workspace_root, path).await
}

async fn run_formatter_command(
    config: &FormatterLanguageConfig,
    workspace_root: &Path,
    path: &str,
) -> Result<(), String> {
    if config.command.is_empty() {
        return Err("formatter command is empty".to_string());
    }

    let mut command = Command::new(&config.command[0]);
    command
        .args(&config.command[1..])
        .arg(path)
        .current_dir(workspace_root)
        .env("HARNESS_FORMATTER", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = timeout(FORMAT_TIMEOUT, command.output())
        .await
        .map_err(|_| "formatter timed out".to_string())
        .and_then(|result| result.map_err(|err| format!("failed to spawn formatter: {err}")))?;

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
        Err(format!(
            "formatter `{}` failed: {reason}",
            config.command[0]
        ))
    }
}
