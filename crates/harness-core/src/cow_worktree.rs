//! Explicit clones use the filesystem's COW primitive; inspection never writes probe files.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CowMechanism {
    LinuxReflink,
    MacosClonefile,
    FullCopy,
}
impl CowMechanism {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxReflink => "linux_reflink",
            Self::MacosClonefile => "macos_clonefile",
            Self::FullCopy => "full_copy",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CowWorktreeAvailability {
    Available {
        mechanism: CowMechanism,
        platform: String,
    },
    Unavailable {
        reason: String,
        platform: String,
    },
}
impl CowWorktreeAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available {
                mechanism,
                platform,
            } => format!("COW worktree fastpath: {} ({platform})", mechanism.as_str()),
            Self::Unavailable { reason, platform } => {
                format!("COW worktree fastpath: unavailable ({platform}): {reason}")
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CowCloneResult {
    Cloned {
        mechanism: CowMechanism,
        platform: String,
        src: String,
        dst: String,
    },
    Unavailable {
        reason: String,
        platform: String,
        src: String,
        dst: String,
    },
}
impl CowCloneResult {
    pub fn is_cloned(&self) -> bool {
        matches!(self, Self::Cloned { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_cloned()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Cloned {
                mechanism,
                src,
                dst,
                ..
            } => format!("COW clone: {src} -> {dst} ({})", mechanism.as_str()),
            Self::Unavailable {
                reason, src, dst, ..
            } => format!("COW clone: unavailable {src} -> {dst}: {reason}"),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CowCloneOutcomeSummary {
    pub cloned: usize,
    pub unavailable: usize,
    pub total: usize,
}
impl CowCloneOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "COW clone outcomes: {} cloned, {} unavailable ({} total)",
            self.cloned, self.unavailable, self.total
        )
    }
}
pub fn summarize_cow_clone_outcomes(results: &[CowCloneResult]) -> CowCloneOutcomeSummary {
    let cloned = results.iter().filter(|r| r.is_cloned()).count();
    CowCloneOutcomeSummary {
        cloned,
        unavailable: results.len() - cloned,
        total: results.len(),
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CowTreeCloneResult {
    Cloned {
        mechanism: CowMechanism,
        platform: String,
        src: String,
        dst: String,
        files_cloned: usize,
        dirs_created: usize,
    },
    Unavailable {
        reason: String,
        platform: String,
        src: String,
        dst: String,
    },
}
impl CowTreeCloneResult {
    pub fn is_cloned(&self) -> bool {
        matches!(self, Self::Cloned { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_cloned()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Cloned {
                files_cloned,
                dirs_created,
                src,
                dst,
                ..
            } => format!(
                "COW tree: {src} -> {dst} ({files_cloned} files, {dirs_created} directories)"
            ),
            Self::Unavailable {
                reason, src, dst, ..
            } => format!("COW tree unavailable: {src} -> {dst}: {reason}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CowWorktreeFastpathReport {
    pub availability: CowWorktreeAvailability,
    pub overlays: Vec<CowCloneResult>,
    pub tree_clone: Option<CowTreeCloneResult>,
}
impl CowWorktreeFastpathReport {
    pub fn has_cloned_overlay(&self) -> bool {
        self.overlays.iter().any(CowCloneResult::is_cloned)
            || self
                .tree_clone
                .as_ref()
                .is_some_and(CowTreeCloneResult::is_cloned)
    }
    pub fn one_line(&self) -> String {
        format!(
            "{}; {}; {}",
            self.availability.one_line(),
            summarize_cow_clone_outcomes(&self.overlays).one_line(),
            self.tree_clone.as_ref().map_or_else(
                || "COW tree: not attempted".into(),
                CowTreeCloneResult::one_line
            )
        )
    }
}
pub fn detect_cow_worktree_fastpath(root: &Path) -> CowWorktreeAvailability {
    CowWorktreeAvailability::Unavailable {
        platform: std::env::consts::OS.into(),
        reason: if root.is_dir() {
            "filesystem COW capability is unknown until an explicit clone succeeds".into()
        } else {
            "workspace directory is unavailable".into()
        },
    }
}
pub fn try_cow_clone_file(src: &Path, dst: &Path) -> CowCloneResult {
    let platform = std::env::consts::OS.into();
    match clone_file(src, dst) {
        Ok(mechanism) => CowCloneResult::Cloned {
            mechanism,
            platform,
            src: src.display().to_string(),
            dst: dst.display().to_string(),
        },
        Err(error) => unavailable_file(src, dst, error.to_string()),
    }
}
fn unavailable_file(src: &Path, dst: &Path, reason: String) -> CowCloneResult {
    CowCloneResult::Unavailable {
        reason,
        platform: std::env::consts::OS.into(),
        src: src.display().to_string(),
        dst: dst.display().to_string(),
    }
}
pub fn try_cow_clone_tree(src: &Path, dst: &Path) -> CowTreeCloneResult {
    let platform = std::env::consts::OS.into();
    let result = clone_tree(src, dst);
    let src = src.display().to_string();
    let dst = dst.display().to_string();
    match result {
        Ok((mechanism, files_cloned, dirs_created)) => CowTreeCloneResult::Cloned {
            mechanism,
            platform,
            src,
            dst,
            files_cloned,
            dirs_created,
        },
        Err(error) => CowTreeCloneResult::Unavailable {
            reason: error.to_string(),
            platform,
            src,
            dst,
        },
    }
}
pub fn apply_cow_worktree_fastpath(
    source: &Path,
    destination: &Path,
    relative_paths: &[&str],
) -> CowWorktreeFastpathReport {
    let overlays: Vec<_> = relative_paths
        .iter()
        .map(|rel| {
            match confined_path(source, rel)
                .and_then(|src| Ok((src, confined_path(destination, rel)?)))
            {
                Ok((src, dst)) => try_cow_clone_file(&src, &dst),
                Err(error) => {
                    unavailable_file(&source.join(rel), &destination.join(rel), error.to_string())
                }
            }
        })
        .collect();
    let availability = overlays
        .iter()
        .find_map(|result| match result {
            CowCloneResult::Cloned {
                mechanism,
                platform,
                ..
            } => Some(CowWorktreeAvailability::Available {
                mechanism: *mechanism,
                platform: platform.clone(),
            }),
            _ => None,
        })
        .unwrap_or_else(|| detect_cow_worktree_fastpath(destination));
    CowWorktreeFastpathReport {
        availability,
        overlays,
        tree_clone: None,
    }
}
pub fn materialize_cow_workspace_tree(
    source: &Path,
    destination: &Path,
) -> CowWorktreeFastpathReport {
    let tree = try_cow_clone_tree(source, destination);
    let availability = match &tree {
        CowTreeCloneResult::Cloned {
            mechanism,
            platform,
            ..
        } if *mechanism != CowMechanism::FullCopy => CowWorktreeAvailability::Available {
            mechanism: *mechanism,
            platform: platform.clone(),
        },
        _ => detect_cow_worktree_fastpath(source),
    };
    CowWorktreeFastpathReport {
        availability,
        overlays: Vec::new(),
        tree_clone: Some(tree),
    }
}

fn confined_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let mut path = root.canonicalize()?;
    if relative.is_empty() {
        return Err(io::Error::other("empty overlay path"));
    }
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(io::Error::other("overlay must stay within its workspace"));
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_symlink() => {
                return Err(io::Error::other("overlay symlinks are not allowed"))
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(path)
}
fn clone_file(src: &Path, dst: &Path) -> io::Result<CowMechanism> {
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "sparc", target_arch = "sparc64"))
    ))]
    {
        use rustix::fs::{open, Mode, OFlags};
        let source = fs::File::from(open(
            src,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?);
        let metadata = source.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("clone source is not a regular file"));
        }
        let parent = dst
            .parent()
            .ok_or_else(|| io::Error::other("missing clone destination parent"))?;
        let temp = tempfile::NamedTempFile::new_in(parent)?;
        rustix::fs::ioctl_ficlone(temp.as_file(), &source)?;
        temp.as_file().set_permissions(metadata.permissions())?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(dst).map_err(|e| e.error)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(CowMechanism::LinuxReflink)
    }
    #[cfg(not(all(
        target_os = "linux",
        not(any(target_arch = "sparc", target_arch = "sparc64"))
    )))]
    {
        let _ = (src, dst);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "COW clone is unavailable on this platform",
        ))
    }
}
fn clone_tree(src: &Path, dst: &Path) -> io::Result<(CowMechanism, usize, usize)> {
    let source = src.canonicalize()?;
    let parent = dst
        .parent()
        .ok_or_else(|| io::Error::other("missing clone destination parent"))?
        .canonicalize()?;
    if !source.is_dir() || parent.starts_with(&source) || fs::symlink_metadata(dst).is_ok() {
        return Err(io::Error::other(
            "tree clone requires a directory source and a new destination outside it",
        ));
    }
    let staging = tempfile::Builder::new()
        .prefix(".cow-")
        .tempdir_in(&parent)?;
    let mut files = 0;
    let mut directories = Vec::new();
    let mut mechanism = CowMechanism::FullCopy;
    for entry in walkdir::WalkDir::new(&source)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(&source)
            .map_err(io::Error::other)?;
        let target = staging.path().join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir(&target)?;
            directories.push((target, entry.metadata()?.permissions()));
        } else if entry.file_type().is_file() {
            mechanism = clone_file(entry.path(), &target)?;
            files += 1;
        } else {
            return Err(io::Error::other("tree contains a symlink or special file"));
        }
    }
    for (path, permissions) in directories.iter().rev() {
        fs::set_permissions(path, permissions.clone())?;
    }
    fs::set_permissions(staging.path(), fs::metadata(&source)?.permissions())?;
    fs::create_dir(dst)?;
    if let Err(error) = fs::rename(staging.path(), dst) {
        let _ = fs::remove_dir(dst);
        return Err(error);
    }
    fs::File::open(parent)?.sync_all()?;
    Ok((mechanism, files, directories.len()))
}
