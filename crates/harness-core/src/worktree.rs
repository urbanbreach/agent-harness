use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORKTREE_RELATIVE_BASE: &str = ".agent-harness/worktrees";
pub const WORKTREE_BRANCH_PREFIX: &str = "harness/wt-";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktree {
    pub path: PathBuf,
    pub branch: String,
    pub repository_root: PathBuf,
    pub slug: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct ListedWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub harness_managed: bool,
    pub slug: Option<String>,
}
#[derive(Debug, Clone)]
pub struct CreateWorktreeOptions<'a> {
    pub repository_root: &'a Path,
    pub worktree_parent: Option<&'a Path>,
    pub slug: Option<&'a str>,
    pub start_point: Option<&'a str>,
}
#[derive(Debug, Clone)]
pub struct RemoveWorktreeOptions<'a> {
    pub repository_root: &'a Path,
    pub path: &'a Path,
    pub worktree_parent: Option<&'a Path>,
    pub delete_branch: bool,
    pub force: bool,
}
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("workspace is not a git repository: {path}")]
    NotAGitRepository { path: String },
    #[error("worktree path already exists: {path}")]
    PathCollision { path: String },
    #[error("branch already exists: {branch}")]
    BranchCollision { branch: String },
    #[error("refusing to remove primary worktree: {path}")]
    PrimaryWorktree { path: String },
    #[error("worktree is outside its managed parent: {path}")]
    UnsafeRemovePath { path: String },
    #[error("worktree not found: {path}")]
    NotFound { path: String },
    #[error("Git worktree operation failed: {detail}")]
    GitFailed { detail: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn create_session_worktree(
    options: CreateWorktreeOptions<'_>,
) -> Result<CreatedWorktree, WorktreeError> {
    let repository_root = repository_root(options.repository_root)?;
    let slug = options
        .slug
        .map(sanitize_slug)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
    let branch = format!("{WORKTREE_BRANCH_PREFIX}{slug}");
    let parent = parent_path(&repository_root, options.worktree_parent);
    let path = parent.join(&slug);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(WorktreeError::PathCollision {
            path: path.display().to_string(),
        });
    }
    if command(&repository_root)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()?
        .success()
    {
        return Err(WorktreeError::BranchCollision { branch });
    }
    // Resolve first: a failed ref must not create directories or be interpreted as an option.
    let revision = output(command(&repository_root).args([
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{}^{{commit}}", options.start_point.unwrap_or("HEAD")),
    ]))?;
    let revision = std::str::from_utf8(&revision)
        .map_err(|_| invalid_output())?
        .trim();
    fs::create_dir_all(&parent)?;
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(WorktreeError::PathCollision {
                path: path.display().to_string(),
            })
        }
        Err(error) => return Err(error.into()),
    }
    if let Err(error) = output(
        command(&repository_root)
            .args(["worktree", "add", "-b", &branch, "--"])
            .arg(&path)
            .arg(revision),
    ) {
        // Git owns checkout rollback. Remove only our empty reservation, never a partial checkout.
        let _ = fs::remove_dir(&path);
        return Err(error);
    }
    Ok(CreatedWorktree {
        path,
        branch,
        repository_root,
        slug,
    })
}

pub fn list_session_worktrees(
    root: &Path,
    worktree_parent: Option<&Path>,
) -> Result<Vec<ListedWorktree>, WorktreeError> {
    let root = repository_root(root)?;
    let parent = parent_path(&root, worktree_parent);
    let parent = parent.canonicalize().unwrap_or(parent);
    let data = output(command(&root).args(["worktree", "list", "--porcelain", "-z"]))?;
    let mut entries = Vec::new();
    let mut entry = ListedWorktree::default();
    for field in data.split(|byte| *byte == 0) {
        if field.is_empty() {
            if !entry.path.as_os_str().is_empty() {
                entries.push(std::mem::take(&mut entry));
            }
        } else if let Some(path) = field.strip_prefix(b"worktree ") {
            entry.path = path_from_bytes(path)?;
            let resolved = entry
                .path
                .canonicalize()
                .unwrap_or_else(|_| entry.path.clone());
            entry.harness_managed = resolved != parent && resolved.starts_with(&parent);
            entry.slug = entry.harness_managed.then(|| {
                resolved
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        } else if let Some(branch) = field.strip_prefix(b"branch refs/heads/") {
            entry.branch = Some(String::from_utf8_lossy(branch).into_owned());
        } else if let Some(head) = field.strip_prefix(b"HEAD ") {
            entry.head = Some(String::from_utf8_lossy(head).into_owned());
        } else if field == b"bare" {
            entry.bare = true;
        } else if field == b"detached" {
            entry.detached = true;
        }
    }
    Ok(entries)
}

pub fn remove_session_worktree(options: RemoveWorktreeOptions<'_>) -> Result<(), WorktreeError> {
    let root = repository_root(options.repository_root)?;
    let target = options
        .path
        .canonicalize()
        .map_err(|_| WorktreeError::NotFound {
            path: options.path.display().to_string(),
        })?;
    let listed = list_session_worktrees(&root, options.worktree_parent)?;
    if listed
        .first()
        .is_some_and(|e| e.path.canonicalize().ok().as_ref() == Some(&target))
    {
        return Err(WorktreeError::PrimaryWorktree {
            path: target.display().to_string(),
        });
    }
    let entry = listed
        .iter()
        .find(|e| e.path.canonicalize().ok().as_ref() == Some(&target))
        .ok_or_else(|| WorktreeError::NotFound {
            path: target.display().to_string(),
        })?;
    if !entry.harness_managed {
        return Err(WorktreeError::UnsafeRemovePath {
            path: target.display().to_string(),
        });
    }
    let mut remove = command(&root);
    remove.args(["worktree", "remove"]);
    if options.force {
        remove.arg("--force");
    }
    output(remove.arg("--").arg(&target))?;
    if let Some(branch) = entry
        .branch
        .as_deref()
        .filter(|b| options.delete_branch && b.starts_with(WORKTREE_BRANCH_PREFIX))
    {
        output(command(&root).args(["branch", "-D", "--", branch]))?;
    }
    Ok(())
}
pub fn default_worktree_parent(root: &Path) -> PathBuf {
    root.join(DEFAULT_WORKTREE_RELATIVE_BASE)
}
pub fn sanitize_slug(raw: &str) -> String {
    raw.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .trim_matches('-')
        .chars()
        .take(80)
        .collect::<String>()
        .to_ascii_lowercase()
}
fn parent_path(root: &Path, parent: Option<&Path>) -> PathBuf {
    parent.map_or_else(|| default_worktree_parent(root), |path| root.join(path))
}
fn repository_root(root: &Path) -> Result<PathBuf, WorktreeError> {
    let data = output(command(root).args(["rev-parse", "--show-toplevel"])).map_err(|_| {
        WorktreeError::NotAGitRepository {
            path: root.display().to_string(),
        }
    })?;
    path_from_bytes(data.strip_suffix(b"\n").unwrap_or(&data))
}
fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotepath=false",
            "-c",
            "core.hooksPath=/dev/null",
        ]);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(key);
    }
    command
}
fn output(command: &mut Command) -> Result<Vec<u8>, WorktreeError> {
    let result = command.output()?;
    if result.status.success() {
        return Ok(result.stdout);
    }
    use crate::redact::Redactor;
    Err(WorktreeError::GitFailed {
        detail: crate::redact::DefaultRedactor::default().redact_text(&String::from_utf8_lossy(
            &result.stderr[..result.stderr.len().min(4096)],
        )),
    })
}
fn invalid_output() -> WorktreeError {
    WorktreeError::GitFailed {
        detail: "Git returned an invalid path or revision".into(),
    }
}
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, WorktreeError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
    }
    #[cfg(not(unix))]
    {
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| invalid_output())?
            .into())
    }
}
