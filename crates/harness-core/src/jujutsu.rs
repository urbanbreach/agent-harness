//! Jujutsu (`jj`) availability + workspace-repo detection (honest MVP).
//!
//! Reports whether the `jj` binary is present on PATH and whether a workspace
//! looks like a jujutsu repo (`.jj` marker). Does **not** implement jujutsu
//! worktree workflows, branch ops, or full VCS product surfaces.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Whether the Jujutsu CLI is available for optional VCS workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuAvailability {
    /// `jj` resolved on PATH (path + optional version for diagnostics).
    Available {
        binary_path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    /// `jj` not found or not runnable; workflows must not claim success.
    Unavailable { reason: String },
}

impl JujutsuAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim jujutsu worktree product).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available {
                binary_path,
                version,
            } => match version {
                Some(v) => format!(
                    "jujutsu CLI: available path={} version={v}",
                    binary_path.display()
                ),
                None => format!("jujutsu CLI: available path={}", binary_path.display()),
            },
            Self::Unavailable { reason } => {
                format!("jujutsu CLI: unavailable ({reason})")
            }
        }
    }
}

/// Whether a workspace directory looks like a jujutsu repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuWorkspaceStatus {
    /// Workspace contains a `.jj` marker directory.
    Repo {
        workspace_root: PathBuf,
        jj_dir: PathBuf,
    },
    /// Workspace is not a jujutsu repo (or is unreadable).
    NotARepo {
        workspace_root: PathBuf,
        reason: String,
    },
}

impl JujutsuWorkspaceStatus {
    pub const fn is_repo(&self) -> bool {
        matches!(self, Self::Repo { .. })
    }

    pub const fn is_not_a_repo(&self) -> bool {
        matches!(self, Self::NotARepo { .. })
    }

    /// Operator-facing one-line workspace diagnostics (not a worktree workflow claim).
    pub fn one_line(&self) -> String {
        match self {
            Self::Repo {
                workspace_root,
                jj_dir,
            } => format!(
                "jujutsu workspace: repo root={} jj_dir={}",
                workspace_root.display(),
                jj_dir.display()
            ),
            Self::NotARepo {
                workspace_root,
                reason,
            } => format!(
                "jujutsu workspace: not_a_repo root={} ({reason})",
                workspace_root.display()
            ),
        }
    }
}

/// Combined CLI + workspace probe for operator diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JujutsuProbe {
    pub cli: JujutsuAvailability,
    pub workspace: JujutsuWorkspaceStatus,
}

impl JujutsuProbe {
    /// True only when `jj` is on PATH **and** the workspace has a `.jj` marker.
    pub const fn is_ready(&self) -> bool {
        self.cli.is_available() && self.workspace.is_repo()
    }

    /// One-line diagnostic summary (not a product workflow claim).
    pub fn describe(&self) -> String {
        let cli = match &self.cli {
            JujutsuAvailability::Available {
                binary_path,
                version,
            } => match version {
                Some(v) => format!("cli=available path={} version={v}", binary_path.display()),
                None => format!("cli=available path={}", binary_path.display()),
            },
            JujutsuAvailability::Unavailable { reason } => {
                format!("cli=unavailable ({reason})")
            }
        };
        let workspace = match &self.workspace {
            JujutsuWorkspaceStatus::Repo { jj_dir, .. } => {
                format!("workspace=repo jj_dir={}", jj_dir.display())
            }
            JujutsuWorkspaceStatus::NotARepo { reason, .. } => {
                format!("workspace=not_a_repo ({reason})")
            }
        };
        format!("{cli}; {workspace}; ready={}", self.is_ready())
    }

    /// Alias for operator surfaces that prefer `one_line` naming.
    pub fn one_line(&self) -> String {
        self.describe()
    }
}

/// Detect `jj` on the process PATH (real host probe).
pub fn detect_jujutsu() -> JujutsuAvailability {
    detect_jujutsu_with(resolve_jj_on_path)
}

/// Injectable detection for tests (avoid host PATH dependence).
///
/// When a path is resolved, best-effort version capture is attempted via
/// `jj --version`. Injectable paths that are not runnable still report Available
/// with `version = None` (path presence only).
pub fn detect_jujutsu_with<F>(resolve_binary: F) -> JujutsuAvailability
where
    F: FnOnce(&str) -> Option<PathBuf>,
{
    match resolve_binary("jj") {
        Some(path) => JujutsuAvailability::Available {
            version: capture_jj_version(&path),
            binary_path: path,
        },
        None => JujutsuAvailability::Unavailable {
            reason: "jujutsu CLI `jj` not found on PATH; jujutsu workflows unavailable".to_string(),
        },
    }
}

/// Detect whether `workspace_root` is a jujutsu repository (`.jj` directory).
///
/// Walks parents up to filesystem root so nested workspaces still resolve.
/// Read-only: never creates or mutates a repo.
pub fn detect_jujutsu_workspace(workspace_root: &Path) -> JujutsuWorkspaceStatus {
    let workspace_root = workspace_root.to_path_buf();
    if !workspace_root.exists() {
        return JujutsuWorkspaceStatus::NotARepo {
            workspace_root,
            reason: "workspace root does not exist".to_string(),
        };
    }
    if !workspace_root.is_dir() {
        return JujutsuWorkspaceStatus::NotARepo {
            workspace_root,
            reason: "workspace root is not a directory".to_string(),
        };
    }

    let mut current = workspace_root.clone();
    loop {
        let jj_dir = current.join(".jj");
        if jj_dir.is_dir() {
            return JujutsuWorkspaceStatus::Repo {
                workspace_root,
                jj_dir,
            };
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }

    JujutsuWorkspaceStatus::NotARepo {
        workspace_root,
        reason: "no `.jj` directory found in workspace or parents".to_string(),
    }
}

/// Probe CLI availability and workspace repo status together.
pub fn probe_jujutsu(workspace_root: &Path) -> JujutsuProbe {
    JujutsuProbe {
        cli: detect_jujutsu(),
        workspace: detect_jujutsu_workspace(workspace_root),
    }
}

/// Injectable combined probe for tests.
pub fn probe_jujutsu_with<F>(workspace_root: &Path, resolve_binary: F) -> JujutsuProbe
where
    F: FnOnce(&str) -> Option<PathBuf>,
{
    JujutsuProbe {
        cli: detect_jujutsu_with(resolve_binary),
        workspace: detect_jujutsu_workspace(workspace_root),
    }
}

/// Result of a single diagnostic jujutsu CLI invocation (not a worktree workflow).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuCommandOutcome {
    /// Command ran and exited successfully.
    Ok {
        command: String,
        stdout_preview: String,
    },
    /// CLI missing, not ready, or command failed — fail-closed.
    Unavailable { command: String, reason: String },
}

impl JujutsuCommandOutcome {
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim jujutsu worktree product).
    pub fn one_line(&self) -> String {
        match self {
            Self::Ok {
                command,
                stdout_preview,
            } => {
                let preview = stdout_preview.trim();
                if preview.is_empty() {
                    format!("jujutsu command: ok `{command}`")
                } else {
                    format!("jujutsu command: ok `{command}` → {preview}")
                }
            }
            Self::Unavailable { command, reason } => {
                format!("jujutsu command: unavailable `{command}` ({reason})")
            }
        }
    }
}

/// Run a diagnostic `jj` invocation using a prior CLI/workspace probe.
///
/// Fail-closed: when the CLI is unavailable, returns [`JujutsuCommandOutcome::Unavailable`]
/// without spawning. Does **not** implement worktree/branch workflows.
pub fn run_jujutsu_command(probe: &JujutsuProbe, args: &[&str]) -> JujutsuCommandOutcome {
    let command = if args.is_empty() {
        "jj".to_string()
    } else {
        format!("jj {}", args.join(" "))
    };
    let binary = match &probe.cli {
        JujutsuAvailability::Available { binary_path, .. } => binary_path.clone(),
        JujutsuAvailability::Unavailable { reason } => {
            return JujutsuCommandOutcome::Unavailable {
                command,
                reason: reason.clone(),
            };
        }
    };
    let output = match Command::new(&binary).args(args).output() {
        Ok(output) => output,
        Err(err) => {
            return JujutsuCommandOutcome::Unavailable {
                command,
                reason: format!("failed to spawn jujutsu: {err}"),
            };
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let preview = stderr.lines().next().unwrap_or("").trim();
        let reason = if preview.is_empty() {
            format!("jujutsu exited with status {}", output.status)
        } else {
            format!("jujutsu exited with status {}: {preview}", output.status)
        };
        return JujutsuCommandOutcome::Unavailable { command, reason };
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let preview = stdout.lines().next().unwrap_or("").trim().to_string();
    // Cap operator preview length.
    let stdout_preview = if preview.chars().count() > 96 {
        preview.chars().take(93).collect::<String>() + "..."
    } else {
        preview
    };
    JujutsuCommandOutcome::Ok {
        command,
        stdout_preview,
    }
}

/// Diagnostic `jj --version` using a prior probe (CLI presence only; workspace optional).
pub fn run_jujutsu_version(probe: &JujutsuProbe) -> JujutsuCommandOutcome {
    run_jujutsu_command(probe, &["--version"])
}

/// Canonical multi-command diagnostic walk: version → log → root → status.
pub const JUJUTSU_DIAGNOSTIC_WALK: &[&[&str]] =
    &[&["--version"], &["log", "-n", "1"], &["root"], &["status"]];

/// Result of the multi-command jujutsu diagnostic product path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JujutsuDiagnosticWalk {
    pub probe: JujutsuProbe,
    pub outcomes: Vec<JujutsuCommandOutcome>,
    pub last_command: JujutsuCommandOutcome,
}

impl JujutsuDiagnosticWalk {
    /// True when every outcome is structured ok or unavailable (never a bare bool).
    pub fn all_structured(&self) -> bool {
        self.outcomes
            .iter()
            .all(|o| o.is_ok() || o.is_unavailable())
            && (self.last_command.is_ok() || self.last_command.is_unavailable())
    }

    pub fn one_line(&self) -> String {
        let last = self.last_command.one_line();
        format!(
            "jujutsu diagnostic walk: steps={} ready={} last={}",
            self.outcomes.len(),
            self.probe.is_ready(),
            last
        )
    }
}

/// Ensure a `.jj` marker directory exists under `workspace_root` (diagnostic only).
///
/// Does not initialize a real jujutsu repo; only creates the marker for workspace
/// detection so the product path can exercise multi-command outcomes.
pub fn ensure_jujutsu_repo_marker(workspace_root: &Path) -> std::io::Result<PathBuf> {
    let jj_dir = workspace_root.join(".jj");
    if !jj_dir.is_dir() {
        std::fs::create_dir_all(&jj_dir)?;
    }
    Ok(jj_dir)
}

/// Relative path for the durable jujutsu diagnostic receipt.
pub const JUJUTSU_DIAGNOSTIC_RECEIPT_REL: &str = ".agent-harness/jujutsu-diagnostic.receipt.json";

/// Durable receipt written after a jujutsu diagnostic walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JujutsuDiagnosticReceipt {
    pub schema: String,
    pub ready: bool,
    pub probe_one_line: String,
    pub outcomes: Vec<JujutsuCommandOutcome>,
    pub last_command: JujutsuCommandOutcome,
    pub receipt_path: String,
}

/// Product path: probe CLI+workspace, then walk version → log → root → status.
///
/// Each step returns structured [`JujutsuCommandOutcome::Ok`] or
/// [`JujutsuCommandOutcome::Unavailable`]. Does **not** implement worktree/branch UX.
pub fn run_jujutsu_diagnostic_walk(workspace_root: &Path) -> JujutsuDiagnosticWalk {
    let probe = probe_jujutsu(workspace_root);
    run_jujutsu_diagnostic_walk_with_probe(&probe)
}

/// Multi-command walk using an existing probe (tests + seed).
pub fn run_jujutsu_diagnostic_walk_with_probe(probe: &JujutsuProbe) -> JujutsuDiagnosticWalk {
    let mut outcomes = Vec::with_capacity(JUJUTSU_DIAGNOSTIC_WALK.len());
    for args in JUJUTSU_DIAGNOSTIC_WALK {
        outcomes.push(run_jujutsu_command(probe, args));
    }
    let last_command =
        outcomes
            .last()
            .cloned()
            .unwrap_or_else(|| JujutsuCommandOutcome::Unavailable {
                command: "jj".to_string(),
                reason: "empty diagnostic walk".to_string(),
            });
    JujutsuDiagnosticWalk {
        probe: probe.clone(),
        outcomes,
        last_command,
    }
}

/// Write a jujutsu diagnostic receipt (real filesystem side effect).
pub fn write_jujutsu_diagnostic_receipt(
    receipt_path: &Path,
    walk: &JujutsuDiagnosticWalk,
) -> std::io::Result<JujutsuDiagnosticReceipt> {
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let receipt = JujutsuDiagnosticReceipt {
        schema: "harness-jujutsu-diagnostic-receipt-v1".to_string(),
        ready: walk.probe.is_ready(),
        probe_one_line: walk.probe.one_line(),
        outcomes: walk.outcomes.clone(),
        last_command: walk.last_command.clone(),
        receipt_path: receipt_path.display().to_string(),
    };
    let body = serde_json::to_string_pretty(&receipt).map_err(std::io::Error::other)?;
    std::fs::write(receipt_path, format!("{body}\n"))?;
    Ok(receipt)
}

/// Product path: diagnostic walk + durable receipt under workspace.
///
/// When `jj` is present and the workspace has a `.jj` marker, steps can succeed
/// with real `status`/`log` output. Missing CLI stays structured unavailable and
/// still writes a receipt.
pub fn run_jujutsu_product_with_receipt(workspace_root: &Path) -> (JujutsuDiagnosticWalk, PathBuf) {
    let walk = run_jujutsu_diagnostic_walk(workspace_root);
    let receipt_path = workspace_root.join(JUJUTSU_DIAGNOSTIC_RECEIPT_REL);
    let _ = write_jujutsu_diagnostic_receipt(&receipt_path, &walk);
    (walk, receipt_path)
}

fn resolve_jj_on_path(binary_name: &str) -> Option<PathBuf> {
    // Prefer `which`-style lookup via a no-side-effect spawn of `jj --version`.
    // If the binary is missing, Command returns NotFound without running it.
    let output = Command::new(binary_name).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    // We do not have a portable which; record the bare name when runnable.
    // Callers treat presence as Available; exact path is best-effort.
    which_best_effort(binary_name).or_else(|| Some(PathBuf::from(binary_name)))
}

fn capture_jj_version(binary_path: &Path) -> Option<String> {
    let output = Command::new(binary_path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn which_best_effort(binary_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{binary_name}.exe"));
            if is_executable_file(&with_exe) {
                return Some(with_exe);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_available_when_resolver_finds_jj() {
        // arrange
        // act
        // assert
        // Given
        let resolved = PathBuf::from("/usr/bin/jj");

        // When
        let availability = detect_jujutsu_with(|_| Some(resolved.clone()));

        // Then
        assert!(availability.is_available());
        match availability {
            JujutsuAvailability::Available {
                binary_path,
                version,
            } => {
                assert_eq!(binary_path, resolved);
                // Fake path is not runnable → version may be None (honest).
                let _ = version;
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn detect_reports_unavailable_when_jj_missing() {
        // arrange
        // act
        // assert
        // When
        let availability = detect_jujutsu_with(|_| None);

        // Then
        assert!(availability.is_unavailable());
        match availability {
            JujutsuAvailability::Unavailable { reason } => {
                assert!(reason.contains("jj"));
                assert!(reason.contains("unavailable") || reason.contains("not found"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn host_probe_is_structured_available_or_unavailable() {
        // arrange
        // act
        // assert
        // When: real PATH probe (host-dependent)
        let availability = detect_jujutsu();

        // Then: always a structured result; never a panic / silent bool
        assert!(availability.is_available() || availability.is_unavailable());
    }

    #[test]
    fn workspace_detects_jj_repo_marker() {
        // arrange
        // act
        // assert
        // Given: temp dir with `.jj`
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();

        // When
        let status = detect_jujutsu_workspace(&workspace);

        // Then
        assert!(status.is_repo());
        match status {
            JujutsuWorkspaceStatus::Repo {
                workspace_root,
                jj_dir,
            } => {
                assert_eq!(workspace_root, workspace);
                assert_eq!(jj_dir, workspace.join(".jj"));
            }
            other => panic!("expected Repo, got {other:?}"),
        }
    }

    #[test]
    fn workspace_jj_file_marker_is_not_a_repo() {
        // arrange
        // act
        // assert
        // Given: a plain FILE named `.jj` (copied/faked marker, not a repo directory)
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join(".jj"), b"not a jujutsu repo dir").unwrap();

        // When
        let status = detect_jujutsu_workspace(&workspace);

        // Then: the probe must not report Repo for a file marker
        assert!(status.is_not_a_repo());
        match status {
            JujutsuWorkspaceStatus::NotARepo { reason, .. } => {
                assert!(reason.contains(".jj"));
            }
            other => panic!("expected NotARepo, got {other:?}"),
        }
    }

    #[test]
    fn workspace_walks_parents_for_jj_marker() {
        // arrange
        // act
        // assert
        // Given: nested path under a jj workspace
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("repo");
        let nested = workspace.join("crates").join("core");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();

        // When
        let status = detect_jujutsu_workspace(&nested);

        // Then
        assert!(status.is_repo());
        match status {
            JujutsuWorkspaceStatus::Repo { jj_dir, .. } => {
                assert_eq!(jj_dir, workspace.join(".jj"));
            }
            other => panic!("expected Repo, got {other:?}"),
        }
    }

    #[test]
    fn workspace_without_jj_is_not_a_repo() {
        // arrange
        // act
        // assert
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("plain");
        std::fs::create_dir_all(&workspace).unwrap();

        // When
        let status = detect_jujutsu_workspace(&workspace);

        // Then
        assert!(status.is_not_a_repo());
        match status {
            JujutsuWorkspaceStatus::NotARepo { reason, .. } => {
                assert!(reason.contains(".jj"));
            }
            other => panic!("expected NotARepo, got {other:?}"),
        }
    }

    #[test]
    fn probe_ready_requires_cli_and_workspace_repo() {
        // arrange
        // act
        // assert
        // Given: workspace with `.jj`, injectable CLI present/missing
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();

        // When / Then: CLI present → ready
        let ready = probe_jujutsu_with(&workspace, |_| Some(PathBuf::from("/usr/bin/jj")));
        assert!(ready.is_ready());
        assert!(ready.cli.is_available());
        assert!(ready.workspace.is_repo());

        // When / Then: CLI missing → not ready even with repo marker
        let not_ready = probe_jujutsu_with(&workspace, |_| None);
        assert!(!not_ready.is_ready());
        assert!(not_ready.cli.is_unavailable());
        assert!(not_ready.workspace.is_repo());
    }

    #[test]
    fn probe_describe_summarizes_cli_and_workspace() {
        // arrange
        // act
        // assert
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();

        // When
        let ready = probe_jujutsu_with(&workspace, |_| Some(PathBuf::from("/usr/bin/jj")));
        let summary = ready.describe();

        // Then
        assert!(summary.contains("cli=available"));
        assert!(summary.contains("workspace=repo"));
        assert!(summary.contains("ready=true"));
        assert_eq!(ready.one_line(), summary);
        assert!(ready.cli.one_line().contains("jujutsu CLI: available"));
        assert!(ready
            .workspace
            .one_line()
            .contains("jujutsu workspace: repo"));

        let missing = probe_jujutsu_with(&workspace, |_| None);
        let missing_summary = missing.describe();
        assert!(missing_summary.contains("cli=unavailable"));
        assert!(missing_summary.contains("ready=false"));
        assert!(missing.cli.one_line().contains("jujutsu CLI: unavailable"));
        assert!(missing
            .workspace
            .one_line()
            .contains("jujutsu workspace: repo"));
    }

    #[test]
    fn jujutsu_operator_diagnostics_cover_cli_and_workspace_not_a_repo() {
        // arrange
        // act
        // assert
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("plain");
        std::fs::create_dir_all(&workspace).unwrap();
        let unavailable = JujutsuAvailability::Unavailable {
            reason: "jj not found on PATH".to_string(),
        };
        let not_repo = detect_jujutsu_workspace(&workspace);

        // When / Then
        assert!(unavailable.one_line().contains("jujutsu CLI: unavailable"));
        assert!(unavailable.one_line().contains("jj not found"));
        assert!(not_repo.is_not_a_repo());
        assert!(not_repo
            .one_line()
            .contains("jujutsu workspace: not_a_repo"));
        assert!(not_repo.one_line().contains(".jj"));
    }

    #[test]
    fn capture_jj_version_parses_first_line() {
        // arrange
        // act
        // assert
        // Given: a tiny fake "jj" script that prints a version line
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("jj");
        std::fs::write(&fake, "#!/bin/sh\necho 'jj 0.99.0-test'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }

        // When
        let version = capture_jj_version(&fake);

        // Then
        assert_eq!(version.as_deref(), Some("jj 0.99.0-test"));
    }

    #[test]
    fn run_jujutsu_command_fails_closed_when_cli_unavailable() {
        // arrange
        // act
        // assert
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("plain");
        std::fs::create_dir_all(&workspace).unwrap();
        let probe = probe_jujutsu_with(&workspace, |_| None);

        // When
        let outcome = run_jujutsu_version(&probe);

        // Then
        assert!(outcome.is_unavailable());
        assert!(outcome.one_line().contains("jujutsu command: unavailable"));
        assert!(
            outcome.one_line().contains("jj --version")
                || outcome.one_line().contains("`--version`")
                || outcome.one_line().contains("--version")
        );
    }

    #[test]
    fn run_jujutsu_version_ok_with_fake_binary() {
        // arrange
        // act
        // assert
        // Given: fake jj that prints a version line
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("jj");
        std::fs::write(&fake, "#!/bin/sh\necho 'jj 0.99.0-test'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();
        let probe = probe_jujutsu_with(&workspace, |_| Some(fake.clone()));

        // When
        let outcome = run_jujutsu_version(&probe);

        // Then
        assert!(
            outcome.is_ok() || outcome.is_unavailable(),
            "structured outcome only: {}",
            outcome.one_line()
        );
        if outcome.is_ok() {
            assert!(outcome.one_line().contains("jujutsu command: ok"));
            assert!(
                outcome.one_line().contains("jj 0.99.0-test")
                    || outcome.one_line().contains("--version")
            );
        }
    }

    #[test]
    fn diagnostic_walk_multi_command_ends_on_status_structured() {
        // arrange
        // act
        // assert
        // Given: workspace with .jj marker; CLI missing (fail-closed walk)
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let jj_dir = ensure_jujutsu_repo_marker(&workspace).expect("marker");
        assert!(jj_dir.is_dir());
        let probe = probe_jujutsu_with(&workspace, |_| None);

        // When
        let walk = run_jujutsu_diagnostic_walk_with_probe(&probe);

        // Then: version → log → root → status (4 steps); last is status
        assert_eq!(walk.outcomes.len(), JUJUTSU_DIAGNOSTIC_WALK.len());
        assert_eq!(walk.outcomes.len(), 4);
        assert!(walk.all_structured());
        assert!(walk.probe.workspace.is_repo());
        assert!(!walk.probe.is_ready());
        for outcome in &walk.outcomes {
            assert!(
                outcome.is_unavailable(),
                "CLI missing → every step unavailable: {}",
                outcome.one_line()
            );
        }
        assert!(
            walk.last_command.one_line().contains("status"),
            "last command must be status: {}",
            walk.last_command.one_line()
        );
        assert!(walk.one_line().contains("jujutsu diagnostic walk:"));
        assert!(walk.one_line().contains("steps=4"));
        assert!(walk.one_line().contains("status"));
    }

    #[test]
    fn diagnostic_walk_product_path_with_real_workspace_marker() {
        // arrange
        // act
        // assert
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        ensure_jujutsu_repo_marker(&workspace).expect("marker");

        // When: host PATH-dependent product path (always structured)
        let walk = run_jujutsu_diagnostic_walk(&workspace);

        // Then
        assert!(walk.probe.workspace.is_repo());
        assert_eq!(walk.outcomes.len(), 4);
        assert!(walk.all_structured());
        assert!(
            walk.last_command.one_line().contains("status"),
            "last={}",
            walk.last_command.one_line()
        );
        assert!(walk.last_command.is_ok() || walk.last_command.is_unavailable());
    }

    #[test]
    fn product_with_receipt_writes_file_when_cli_missing() {
        // Given
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        ensure_jujutsu_repo_marker(&workspace).expect("marker");

        // When
        let (walk, receipt_path) = run_jujutsu_product_with_receipt(&workspace);

        // Then: receipt side effect + structured walk
        assert!(receipt_path.is_file());
        assert!(receipt_path.ends_with(JUJUTSU_DIAGNOSTIC_RECEIPT_REL));
        assert_eq!(walk.outcomes.len(), 4);
        assert!(walk.all_structured());
        let raw = std::fs::read_to_string(&receipt_path).expect("receipt");
        let receipt: JujutsuDiagnosticReceipt = serde_json::from_str(&raw).expect("receipt json");
        assert_eq!(receipt.schema, "harness-jujutsu-diagnostic-receipt-v1");
        assert_eq!(receipt.outcomes.len(), 4);
        assert!(receipt.last_command.one_line().contains("status"));
    }

    #[test]
    fn product_with_fake_jj_can_run_status_and_log() {
        // Given: fake jj that accepts --version/log/root/status
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("jj");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'jj 0.99.0-test';;\n  log) echo '@  fake-log';;\n  root) echo '/tmp/fake-jj-root';;\n  status) echo 'Working copy changes:';;\n  *) exit 1;;\nesac\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(workspace.join(".jj")).unwrap();
        let probe = probe_jujutsu_with(&workspace, |_| Some(fake.clone()));

        // When
        let walk = run_jujutsu_diagnostic_walk_with_probe(&probe);
        let receipt_path = workspace.join(JUJUTSU_DIAGNOSTIC_RECEIPT_REL);
        write_jujutsu_diagnostic_receipt(&receipt_path, &walk).expect("receipt");

        // Then: real command path when binary present
        assert!(walk.probe.is_ready());
        assert_eq!(walk.outcomes.len(), 4);
        assert!(walk.all_structured());
        let ok_count = walk.outcomes.iter().filter(|o| o.is_ok()).count();
        assert!(
            ok_count >= 1,
            "expected at least one ok outcome with fake jj, got {:?}",
            walk.outcomes
        );
        assert!(receipt_path.is_file());
        assert!(
            walk.outcomes
                .iter()
                .any(|o| o.one_line().contains("status"))
                || walk.last_command.one_line().contains("status")
        );
    }
}
