//! Read-only discovery, explicit diagnostic commands, and private receipts.
use crate::redact::{DefaultRedactor, Redactor};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuAvailability {
    Available {
        binary_path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    Unavailable {
        reason: String,
    },
}
impl JujutsuAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available {
                binary_path,
                version,
            } => format!(
                "jujutsu CLI: available path={} version={}",
                binary_path.display(),
                version.as_deref().unwrap_or("unknown")
            ),
            Self::Unavailable { reason } => format!("jujutsu CLI: unavailable ({reason})"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuWorkspaceStatus {
    Repo {
        workspace_root: PathBuf,
        jj_dir: PathBuf,
    },
    NotARepo {
        workspace_root: PathBuf,
        reason: String,
    },
}
impl JujutsuWorkspaceStatus {
    pub fn is_repo(&self) -> bool {
        matches!(self, Self::Repo { .. })
    }
    pub fn is_not_a_repo(&self) -> bool {
        !self.is_repo()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Repo {
                workspace_root,
                jj_dir,
            } => format!(
                "jujutsu workspace: marker root={} jj_dir={}",
                workspace_root.display(),
                jj_dir.display()
            ),
            Self::NotARepo {
                workspace_root,
                reason,
            } => format!(
                "jujutsu workspace: unavailable root={} ({reason})",
                workspace_root.display()
            ),
        }
    }
    fn root(&self) -> &Path {
        match self {
            Self::Repo { workspace_root, .. } | Self::NotARepo { workspace_root, .. } => {
                workspace_root
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JujutsuProbe {
    pub cli: JujutsuAvailability,
    pub workspace: JujutsuWorkspaceStatus,
}
impl JujutsuProbe {
    /// Binary and marker discovery only; repository commands may still fail.
    pub fn is_ready(&self) -> bool {
        self.cli.is_available() && self.workspace.is_repo()
    }
    pub fn describe(&self) -> String {
        format!(
            "jujutsu: ready={} {}; {}",
            self.is_ready(),
            self.cli.one_line(),
            self.workspace.one_line()
        )
    }
    pub fn one_line(&self) -> String {
        self.describe()
    }
}
pub fn detect_jujutsu() -> JujutsuAvailability {
    detect_jujutsu_with(|name| {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
            .map(|dir| dir.join(if cfg!(windows) { "jj.exe" } else { name }))
            .find(|p| executable(p))
    })
}
pub fn detect_jujutsu_with<F: FnOnce(&str) -> Option<PathBuf>>(
    resolve_binary: F,
) -> JujutsuAvailability {
    match resolve_binary("jj").filter(|p| executable(p)) {
        Some(binary_path) => JujutsuAvailability::Available {
            binary_path,
            version: None,
        },
        None => JujutsuAvailability::Unavailable {
            reason: "jj executable was not found".into(),
        },
    }
}
fn executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
pub fn detect_jujutsu_workspace(root: &Path) -> JujutsuWorkspaceStatus {
    let canonical = root.canonicalize().ok().filter(|p| p.is_dir());
    if let Some(root) = canonical {
        for parent in root.ancestors() {
            let jj_dir = parent.join(".jj");
            if fs::symlink_metadata(&jj_dir).is_ok_and(|m| m.is_dir()) {
                return JujutsuWorkspaceStatus::Repo {
                    workspace_root: root,
                    jj_dir,
                };
            }
        }
    }
    JujutsuWorkspaceStatus::NotARepo {
        workspace_root: root.into(),
        reason: "no .jj directory found in workspace or parents".into(),
    }
}
pub fn probe_jujutsu(root: &Path) -> JujutsuProbe {
    JujutsuProbe {
        cli: detect_jujutsu(),
        workspace: detect_jujutsu_workspace(root),
    }
}
pub fn probe_jujutsu_with<F: FnOnce(&str) -> Option<PathBuf>>(
    root: &Path,
    resolve_binary: F,
) -> JujutsuProbe {
    JujutsuProbe {
        cli: detect_jujutsu_with(resolve_binary),
        workspace: detect_jujutsu_workspace(root),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JujutsuCommandOutcome {
    Ok {
        command: String,
        stdout_preview: String,
    },
    Unavailable {
        command: String,
        reason: String,
    },
}
impl JujutsuCommandOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_ok()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Ok {
                command,
                stdout_preview,
            } => format!("jujutsu command: ok `{command}` {stdout_preview}"),
            Self::Unavailable { command, reason } => {
                format!("jujutsu command: unavailable `{command}` ({reason})")
            }
        }
    }
}
pub fn run_jujutsu_command(probe: &JujutsuProbe, args: &[&str]) -> JujutsuCommandOutcome {
    if args.len() > 128 || args.iter().any(|s| s.len() > 4096 || s.contains('\0')) {
        return JujutsuCommandOutcome::Unavailable {
            command: "jj".into(),
            reason: "invalid command arguments".into(),
        };
    }
    let command = preview(&format!("jj {}", args.join(" ")));
    let unavailable = |reason: &str| JujutsuCommandOutcome::Unavailable {
        command: command.clone(),
        reason: reason.into(),
    };
    let JujutsuAvailability::Available { binary_path, .. } = &probe.cli else {
        return unavailable("jj executable is unavailable");
    };
    let mut process = tokio::process::Command::new(binary_path);
    process
        .args(args)
        .current_dir(probe.workspace.root())
        .env("JJ_PAGER", "cat")
        .env("NO_COLOR", "1");
    match crate::process::run_blocking(process, Duration::from_secs(10)) {
        Ok(output) if output.status.success() => {
            let mut stdout_preview = preview(&String::from_utf8_lossy(&output.stdout));
            if output.truncated {
                stdout_preview.push_str(" [truncated]");
            }
            JujutsuCommandOutcome::Ok {
                command,
                stdout_preview,
            }
        }
        Ok(_) => unavailable("jj command exited unsuccessfully"),
        Err(_) => unavailable("jj command could not complete within its process or time limits"),
    }
}
fn preview(text: &str) -> String {
    DefaultRedactor::default()
        .redact_text(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(2048)
        .collect::<String>()
        .trim()
        .into()
}
pub fn run_jujutsu_version(probe: &JujutsuProbe) -> JujutsuCommandOutcome {
    run_jujutsu_command(probe, &["--version"])
}
pub const JUJUTSU_DIAGNOSTIC_WALK: &[&[&str]] =
    &[&["--version"], &["log", "-n", "1"], &["root"], &["status"]];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JujutsuDiagnosticWalk {
    pub probe: JujutsuProbe,
    pub outcomes: Vec<JujutsuCommandOutcome>,
    pub last_command: JujutsuCommandOutcome,
}
impl JujutsuDiagnosticWalk {
    pub fn all_structured(&self) -> bool {
        self.outcomes
            .iter()
            .all(|o| o.is_ok() || o.is_unavailable())
    }
    pub fn one_line(&self) -> String {
        format!(
            "jujutsu diagnostic walk: steps={} ready={} last={}",
            self.outcomes.len(),
            self.probe.is_ready(),
            self.last_command.one_line()
        )
    }
}
/// Explicit test fixture only; this does not initialize a Jujutsu repository.
pub fn ensure_jujutsu_repo_marker(root: &Path) -> io::Result<PathBuf> {
    let path = root.join(".jj");
    crate::store::validate_private_path(&path)?;
    fs::create_dir_all(&path)?;
    Ok(path)
}
pub const JUJUTSU_DIAGNOSTIC_RECEIPT_REL: &str = ".agent-harness/jujutsu-diagnostic.receipt.json";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JujutsuDiagnosticReceipt {
    pub schema: String,
    pub ready: bool,
    pub probe_one_line: String,
    pub outcomes: Vec<JujutsuCommandOutcome>,
    pub last_command: JujutsuCommandOutcome,
    pub receipt_path: String,
}
pub fn run_jujutsu_diagnostic_walk(root: &Path) -> JujutsuDiagnosticWalk {
    run_jujutsu_diagnostic_walk_with_probe(&probe_jujutsu(root))
}
pub fn run_jujutsu_diagnostic_walk_with_probe(probe: &JujutsuProbe) -> JujutsuDiagnosticWalk {
    let outcomes: Vec<_> = JUJUTSU_DIAGNOSTIC_WALK
        .iter()
        .map(|args| run_jujutsu_command(probe, args))
        .collect();
    let last_command =
        outcomes
            .last()
            .cloned()
            .unwrap_or_else(|| JujutsuCommandOutcome::Unavailable {
                command: "jj".into(),
                reason: "empty diagnostic walk".into(),
            });
    JujutsuDiagnosticWalk {
        probe: probe.clone(),
        outcomes,
        last_command,
    }
}
pub fn write_jujutsu_diagnostic_receipt(
    path: &Path,
    walk: &JujutsuDiagnosticWalk,
) -> io::Result<JujutsuDiagnosticReceipt> {
    let receipt = JujutsuDiagnosticReceipt {
        schema: "harness-jujutsu-diagnostic-receipt-v1".into(),
        ready: walk.probe.is_ready(),
        probe_one_line: preview(&walk.probe.one_line()),
        outcomes: walk.outcomes.clone(),
        last_command: walk.last_command.clone(),
        receipt_path: path.display().to_string(),
    };
    let value = crate::redact::redact_value(
        &DefaultRedactor::default(),
        &serde_json::to_value(&receipt)?,
    );
    let receipt = serde_json::from_value(value).map_err(io::Error::other)?;
    let _lock = crate::store::lock_private_parent(path)?;
    crate::store::write_private_atomic(path, &serde_json::to_vec(&receipt)?)?;
    Ok(receipt)
}
pub fn run_jujutsu_product_with_receipt(root: &Path) -> (JujutsuDiagnosticWalk, PathBuf) {
    let mut walk = run_jujutsu_diagnostic_walk(root);
    let path = root.join(JUJUTSU_DIAGNOSTIC_RECEIPT_REL);
    if write_jujutsu_diagnostic_receipt(&path, &walk).is_err() {
        walk.last_command = JujutsuCommandOutcome::Unavailable {
            command: "jj diagnostic receipt".into(),
            reason: "cannot write private diagnostic receipt".into(),
        };
    }
    (walk, path)
}
