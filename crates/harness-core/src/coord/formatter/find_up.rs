//! Filesystem helpers for formatter discovery.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Walk from `start_dir` up to `worktree_root` (inclusive), returning every path where `name` exists.
pub async fn find_up(
    name: impl AsRef<str>,
    start_dir: impl AsRef<Path>,
    worktree_root: impl AsRef<Path>,
) -> Vec<PathBuf> {
    let name = name.as_ref();
    let start_dir = start_dir.as_ref();
    let worktree_root = worktree_root.as_ref();

    let mut found = Vec::new();
    let mut current = Some(start_dir);

    while let Some(dir) = current {
        let candidate = dir.join(name);
        if let Ok(true) = tokio::fs::try_exists(&candidate).await {
            found.push(candidate);
        }
        if dir == worktree_root {
            break;
        }
        current = dir.parent();
    }

    found
}

/// Resolve a binary on PATH using spawn_blocking wrapper around `which::which`.
pub async fn which_binary(name: impl AsRef<str>) -> Option<PathBuf> {
    let name = name.as_ref().to_string();
    match tokio::task::spawn_blocking(move || which::which(&name)).await {
        Ok(Ok(path)) => Some(path),
        _ => None,
    }
}

/// Resolve an npm-style binary: prefer `marker_dir/node_modules/.bin/name`, fall back to `which_binary(name)`.
pub async fn resolve_npm_binary(
    name: impl AsRef<str>,
    marker_dir: impl AsRef<Path>,
) -> Option<PathBuf> {
    let marker_dir = marker_dir.as_ref().to_path_buf();
    let name = name.as_ref().to_string();
    let local = marker_dir.join("node_modules").join(".bin").join(&name);
    if let Ok(true) = tokio::fs::try_exists(&local).await {
        return Some(local);
    }
    which_binary(name).await
}

/// Read and deserialize JSON from a path.
pub async fn read_json_file<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Option<T> {
    let text = read_text_file(path).await?;
    serde_json::from_str(&text).ok()
}

/// Read text from a path.
pub async fn read_text_file(path: impl AsRef<Path>) -> Option<String> {
    tokio::fs::read_to_string(path.as_ref()).await.ok()
}

/// Result of a command health check.
#[derive(Debug, Clone)]
pub struct CommandCheck {
    pub exit_code: i32,
    /// Combined captured output. If stdout is empty and stderr is non-empty,
    /// stderr is stored here instead, mirroring how callers inspect CLI tools.
    pub output: String,
}

/// Run a command with args and return a `CommandCheck`. Treat spawn failure as None.
pub async fn run_command_check(
    program: impl AsRef<str>,
    args: &[impl AsRef<str>],
) -> Option<CommandCheck> {
    let program = program.as_ref().to_string();
    let args: Vec<String> = args.iter().map(|arg| arg.as_ref().to_string()).collect();

    let mut command = tokio::process::Command::new(&program);
    command
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return None,
    };

    let output = match tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await
    {
        Ok(Ok(output)) => output,
        _ => return None,
    };

    let combined = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = if combined.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        combined
    };

    Some(CommandCheck {
        exit_code: output.status.code().unwrap_or(-1),
        output: combined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn find_up_finds_file_in_parent() {
        // arrange
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root = temp_dir.path();
        let nested = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).expect("create nested dirs");

        let marker = root.join("marker.txt");
        std::fs::write(&marker, "found").expect("write marker");

        // act
        let found = find_up("marker.txt", &nested, root).await;
        // assert
        assert_eq!(found, vec![marker]);
    }
}
