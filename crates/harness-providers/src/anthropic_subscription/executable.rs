//! Claude Code executable resolution.
//!
//! The harness never ships or downloads Claude Code; it runs the copy installed on this
//! machine: `CLAUDE_CODE_EXECUTABLE`, then `claude` on PATH, then the installer's default
//! locations, which a GUI-launched harness may not have on PATH.
use std::{
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableSource {
    Override,
    Path,
    /// An installer's default location outside PATH.
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCodeRun {
    pub executable: PathBuf,
    pub source: ExecutableSource,
}

/// What to tell someone who has no Claude Code yet.
pub const INSTALL_GUIDANCE: &str = "Install Claude Code (https://claude.com/claude-code, or `npm install -g @anthropic-ai/claude-code`) so `claude` is on PATH, or set CLAUDE_CODE_EXECUTABLE to the binary.";

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// The first regular file `name` resolves to on PATH, found without a shell.
pub fn find_executable_on_path(name: &str, path: Option<&str>) -> Option<PathBuf> {
    let path = path?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    let names: Vec<String> = if cfg!(windows) {
        [".com", ".exe"]
            .iter()
            .map(|ext| format!("{name}{ext}"))
            .collect()
    } else {
        vec![name.into()]
    };
    path.split(separator)
        .map(|dir| dir.trim_matches('"'))
        .filter(|dir| !dir.is_empty())
        .flat_map(|dir| names.iter().map(move |n| Path::new(dir).join(n)))
        .find(|candidate| is_file(candidate))
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The native installer's and the legacy local npm install's locations.
fn installed_locations(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    home.map(PathBuf::from)
        .map(|home| {
            vec![
                home.join(".local").join("bin").join(executable_name()),
                home.join(".claude").join("local").join(executable_name()),
            ]
        })
        .unwrap_or_default()
}

/// The installed Claude Code, or every spelling tried.
pub fn describe_claude_code_executable(
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ClaudeCodeRun, Vec<String>> {
    let mut tried = Vec::new();
    if let Some(executable) = env("CLAUDE_CODE_EXECUTABLE").filter(|v| !v.is_empty()) {
        let spelled = absolute(Path::new(&executable));
        tried.push(spelled.display().to_string());
        if is_file(&spelled) {
            return Ok(ClaudeCodeRun {
                executable: spelled,
                source: ExecutableSource::Override,
            });
        }
    }
    let path = env("PATH");
    if let Some(found) = find_executable_on_path("claude", path.as_deref()) {
        return Ok(ClaudeCodeRun {
            executable: absolute(&found),
            source: ExecutableSource::Path,
        });
    }
    tried.push(if path.is_some() {
        "claude on PATH".into()
    } else {
        "claude on PATH (PATH is unset)".into()
    });
    for candidate in installed_locations(env) {
        if is_file(&candidate) {
            return Ok(ClaudeCodeRun {
                executable: candidate,
                source: ExecutableSource::Installed,
            });
        }
        tried.push(candidate.display().to_string());
    }
    Err(tried)
}

/// The executable a turn spawns; the error names every candidate tried.
pub fn resolve_claude_code_run(
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ClaudeCodeRun, String> {
    describe_claude_code_executable(env).map_err(|tried| {
        ["Claude Code is not installed. Tried:".to_owned()]
            .into_iter()
            .chain(tried.iter().map(|t| format!("  - {t}")))
            .chain([INSTALL_GUIDANCE.to_owned()])
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Long enough to keep the probe off the per-request path, short enough that a fresh
/// `claude login` is picked up promptly.
const AMBIENT_STATUS_TTL: Duration = Duration::from_secs(30);
const AMBIENT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
static AMBIENT_STATUS: LazyLock<Mutex<Option<(Instant, bool)>>> = LazyLock::new(Mutex::default);

/// `claude auth status` exit 0, cached for 30 s; unavailable is the safe answer.
pub async fn read_ambient_claude_auth_status(
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> bool {
    if let Some((at, value)) = AMBIENT_STATUS.lock().ok().and_then(|cached| *cached)
        && at.elapsed() < AMBIENT_STATUS_TTL
    {
        return value;
    }
    let value = match describe_claude_code_executable(env) {
        Ok(run) => {
            let child = tokio::process::Command::new(&run.executable)
                .args(["auth", "status"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .status();
            matches!(tokio::time::timeout(AMBIENT_PROBE_TIMEOUT, child).await, Ok(Ok(status)) if status.success())
        }
        Err(_) => false,
    };
    if let Ok(mut cached) = AMBIENT_STATUS.lock() {
        *cached = Some((Instant::now(), value));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_prefers_override_then_path_then_installed_locations(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let bin = root.path().join("bin");
        let installed = root.path().join(".local").join("bin");
        std::fs::create_dir_all(&bin)?;
        std::fs::create_dir_all(&installed)?;
        std::fs::write(bin.join("claude"), "x")?;
        let override_path = root.path().join("override");
        std::fs::write(&override_path, "x")?;
        let (path, over, home) = (
            bin.display().to_string(),
            override_path.display().to_string(),
            root.path().display().to_string(),
        );
        let source = |vars: &[(&str, &str)]| {
            describe_claude_code_executable(&|name| {
                vars.iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            })
            .map(|run| run.source)
        };
        assert_eq!(
            source(&[("CLAUDE_CODE_EXECUTABLE", &over), ("PATH", &path)]),
            Ok(ExecutableSource::Override)
        );
        assert_eq!(
            source(&[("PATH", &path), ("HOME", &home)]),
            Ok(ExecutableSource::Path)
        );
        assert!(source(&[("HOME", &home)]).is_err());
        std::fs::write(installed.join("claude"), "x")?;
        assert_eq!(source(&[("HOME", &home)]), Ok(ExecutableSource::Installed));
        Ok(())
    }
}
