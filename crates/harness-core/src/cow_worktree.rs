// allow: SIZE_OK — COW detect + file/tree clone + worktree overlay product APIs
//! Copy-on-write / reflink fast-path for worktree materialization.
//!
//! Detects host COW/reflink availability, clones single files or directory trees
//! with real filesystem side effects, and applies optional overlays into git
//! worktree checkouts. Does **not** replace `git worktree add`; when COW is
//! unavailable the APIs return structured `Unavailable` (fail-closed, no silent
//! full-file copy).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Structured COW/reflink availability for workspace materialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CowWorktreeAvailability {
    /// Reflink/COW clone succeeded under a probe directory.
    Available {
        mechanism: CowMechanism,
        platform: String,
    },
    /// No usable COW/reflink path; fall back to ordinary copy/`git worktree`.
    Unavailable { reason: String, platform: String },
}

/// Detected COW mechanism (best-effort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CowMechanism {
    /// Linux FICLONE / `cp --reflink=auto` style clone.
    LinuxReflink,
    /// macOS clonefile-style COW (not proven by this MVP probe).
    MacosClonefile,
    /// Explicit full-file copy fallback marker (never reported as Available).
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

impl CowWorktreeAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim git worktree product).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available {
                mechanism,
                platform,
            } => format!(
                "COW worktree fastpath: available ({}, platform={platform})",
                mechanism.as_str()
            ),
            Self::Unavailable { reason, platform } => {
                format!("COW worktree fastpath: unavailable (platform={platform}; {reason})")
            }
        }
    }
}

/// Detect COW/reflink availability using a temporary probe under `probe_parent`.
///
/// `probe_parent` should be on the same filesystem as intended worktree storage
/// (typically the repository parent). The probe creates and deletes temp files.
pub fn detect_cow_worktree_fastpath(probe_parent: &Path) -> CowWorktreeAvailability {
    let platform = current_platform_label().to_string();

    if let Err(err) = fs::create_dir_all(probe_parent) {
        return CowWorktreeAvailability::Unavailable {
            reason: format!(
                "cannot create COW probe parent {}: {err}",
                probe_parent.display()
            ),
            platform,
        };
    }

    match try_reflink_probe(probe_parent) {
        Ok(mechanism) => CowWorktreeAvailability::Available {
            mechanism,
            platform,
        },
        Err(reason) => CowWorktreeAvailability::Unavailable { reason, platform },
    }
}

/// Result of a COW file clone attempt (does not replace `git worktree add`).
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
    pub const fn is_cloned(&self) -> bool {
        matches!(self, Self::Cloned { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics for a single-file COW clone attempt.
    pub fn one_line(&self) -> String {
        match self {
            Self::Cloned {
                mechanism,
                platform,
                src,
                dst,
            } => format!(
                "COW clone: cloned {} `{}` → `{}` (platform={platform})",
                mechanism.as_str(),
                src,
                dst
            ),
            Self::Unavailable {
                reason,
                platform,
                src,
                dst,
            } => format!(
                "COW clone: unavailable `{}` → `{}` (platform={platform}; {reason})",
                src, dst
            ),
        }
    }
}

/// Operator-facing counts for COW clone attempts (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

    pub const fn has_cloned(&self) -> bool {
        self.cloned > 0
    }
}

/// Summarize a batch of COW clone results for operator surfaces.
pub fn summarize_cow_clone_outcomes(results: &[CowCloneResult]) -> CowCloneOutcomeSummary {
    let mut summary = CowCloneOutcomeSummary {
        total: results.len(),
        ..CowCloneOutcomeSummary::default()
    };
    for result in results {
        match result {
            CowCloneResult::Cloned { .. } => {
                summary.cloned = summary.cloned.saturating_add(1);
            }
            CowCloneResult::Unavailable { .. } => {
                summary.unavailable = summary.unavailable.saturating_add(1);
            }
        }
    }
    summary
}

/// Result of a recursive COW directory-tree clone (real filesystem side effects).
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
    pub const fn is_cloned(&self) -> bool {
        matches!(self, Self::Cloned { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Cloned {
                mechanism,
                platform,
                src,
                dst,
                files_cloned,
                dirs_created,
            } => format!(
                "COW tree: cloned {} files / {} dirs `{}` → `{}` ({}, platform={platform})",
                files_cloned,
                dirs_created,
                src,
                dst,
                mechanism.as_str()
            ),
            Self::Unavailable {
                reason,
                platform,
                src,
                dst,
            } => format!(
                "COW tree: unavailable `{}` → `{}` (platform={platform}; {reason})",
                src, dst
            ),
        }
    }
}

/// Product report for applying COW fastpath after a git worktree exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CowWorktreeFastpathReport {
    pub availability: CowWorktreeAvailability,
    pub overlays: Vec<CowCloneResult>,
    pub tree_clone: Option<CowTreeCloneResult>,
}

impl CowWorktreeFastpathReport {
    pub fn one_line(&self) -> String {
        let overlay_summary = summarize_cow_clone_outcomes(&self.overlays);
        let tree = self
            .tree_clone
            .as_ref()
            .map(CowTreeCloneResult::one_line)
            .unwrap_or_else(|| "COW tree: not attempted".to_string());
        format!(
            "{}; overlays {}; {}",
            self.availability.one_line(),
            overlay_summary.one_line(),
            tree
        )
    }

    pub fn has_cloned_overlay(&self) -> bool {
        self.overlays.iter().any(CowCloneResult::is_cloned)
            || self
                .tree_clone
                .as_ref()
                .is_some_and(CowTreeCloneResult::is_cloned)
    }
}

/// Recursively COW-clone a directory tree. Skips `.git` entries. Fail-closed:
/// first COW failure rolls back the destination tree and returns `Unavailable`.
pub fn try_cow_clone_tree(src: &Path, dst: &Path) -> CowTreeCloneResult {
    let platform = current_platform_label().to_string();
    let src_label = src.display().to_string();
    let dst_label = dst.display().to_string();

    if !src.is_dir() {
        return CowTreeCloneResult::Unavailable {
            reason: format!("COW tree source is not a directory: {src_label}"),
            platform,
            src: src_label,
            dst: dst_label,
        };
    }
    if dst.exists() {
        return CowTreeCloneResult::Unavailable {
            reason: format!("COW tree destination already exists: {dst_label}"),
            platform,
            src: src_label,
            dst: dst_label,
        };
    }

    let detect = detect_cow_worktree_fastpath(src);
    let mechanism = match detect {
        CowWorktreeAvailability::Available { mechanism, .. } => mechanism,
        CowWorktreeAvailability::Unavailable { reason, .. } => {
            return CowTreeCloneResult::Unavailable {
                reason,
                platform,
                src: src_label,
                dst: dst_label,
            };
        }
    };

    if let Err(err) = fs::create_dir_all(dst) {
        return CowTreeCloneResult::Unavailable {
            reason: format!("cannot create COW tree destination {dst_label}: {err}"),
            platform,
            src: src_label,
            dst: dst_label,
        };
    }

    match cow_clone_tree_walk(src, dst) {
        Ok((files_cloned, dirs_created)) => CowTreeCloneResult::Cloned {
            mechanism,
            platform,
            src: src_label,
            dst: dst_label,
            files_cloned,
            dirs_created,
        },
        Err(reason) => {
            let _ = fs::remove_dir_all(dst);
            CowTreeCloneResult::Unavailable {
                reason,
                platform,
                src: src_label,
                dst: dst_label,
            }
        }
    }
}

/// COW-clone selected paths from the repo root into the worktree after
/// a git worktree checkout exists. Missing sources are unavailable; skip collisions.
pub fn apply_cow_worktree_fastpath(
    repository_root: &Path,
    worktree_path: &Path,
    relative_paths: &[&str],
) -> CowWorktreeFastpathReport {
    let availability = detect_cow_worktree_fastpath(worktree_path);
    let mut overlays = Vec::with_capacity(relative_paths.len());
    for rel in relative_paths {
        let src = repository_root.join(rel);
        let dst = worktree_path.join(rel);
        overlays.push(try_cow_clone_file(&src, &dst));
    }
    CowWorktreeFastpathReport {
        availability,
        overlays,
        tree_clone: None,
    }
}

/// Materialize a COW workspace tree under `dest_root` from `source_root`, then
/// report availability for operator surfaces. Real filesystem side effects.
pub fn materialize_cow_workspace_tree(
    source_root: &Path,
    dest_root: &Path,
) -> CowWorktreeFastpathReport {
    let availability = detect_cow_worktree_fastpath(source_root);
    let tree_clone = Some(try_cow_clone_tree(source_root, dest_root));
    CowWorktreeFastpathReport {
        availability,
        overlays: Vec::new(),
        tree_clone,
    }
}

fn cow_clone_tree_walk(src: &Path, dst: &Path) -> Result<(usize, usize), String> {
    let mut files_cloned = 0usize;
    let mut dirs_created = 0usize;
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), dst.to_path_buf())];

    while let Some((src_dir, dst_dir)) = stack.pop() {
        let entries = fs::read_dir(&src_dir)
            .map_err(|err| format!("read COW tree dir {}: {err}", src_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("read COW tree entry: {err}"))?;
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let src_child = entry.path();
            let dst_child = dst_dir.join(&name);
            let file_type = entry
                .file_type()
                .map_err(|err| format!("stat COW tree entry: {err}"))?;
            if file_type.is_dir() {
                fs::create_dir_all(&dst_child)
                    .map_err(|err| format!("create COW tree dir {}: {err}", dst_child.display()))?;
                dirs_created = dirs_created.saturating_add(1);
                stack.push((src_child, dst_child));
            } else if file_type.is_file() {
                match try_cow_clone_file(&src_child, &dst_child) {
                    CowCloneResult::Cloned { .. } => {
                        files_cloned = files_cloned.saturating_add(1);
                    }
                    CowCloneResult::Unavailable { reason, .. } => {
                        return Err(reason);
                    }
                }
            }
        }
    }

    Ok((files_cloned, dirs_created))
}

/// Attempt a COW/reflink clone of a single file.
///
/// Fail-closed: when the platform fastpath is unavailable, returns
/// [`CowCloneResult::Unavailable`] and does not fall back to a silent full copy.
/// Does **not** create git worktrees or directory trees.
pub fn try_cow_clone_file(src: &Path, dst: &Path) -> CowCloneResult {
    let platform = current_platform_label().to_string();
    let src_label = src.display().to_string();
    let dst_label = dst.display().to_string();

    if !src.is_file() {
        return CowCloneResult::Unavailable {
            reason: format!("COW clone source is not a file: {src_label}"),
            platform,
            src: src_label,
            dst: dst_label,
        };
    }
    if dst.exists() {
        return CowCloneResult::Unavailable {
            reason: format!("COW clone destination already exists: {dst_label}"),
            platform,
            src: src_label,
            dst: dst_label,
        };
    }
    if let Some(parent) = dst.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return CowCloneResult::Unavailable {
                reason: format!(
                    "cannot create COW clone destination parent {}: {err}",
                    parent.display()
                ),
                platform,
                src: src_label,
                dst: dst_label,
            };
        }
    }

    match attempt_platform_reflink(src, dst) {
        Ok(mechanism) => CowCloneResult::Cloned {
            mechanism,
            platform,
            src: src_label,
            dst: dst_label,
        },
        Err(reason) => {
            let _ = fs::remove_file(dst);
            CowCloneResult::Unavailable {
                reason,
                platform,
                src: src_label,
                dst: dst_label,
            }
        }
    }
}

fn current_platform_label() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    }
}

fn try_reflink_probe(probe_parent: &Path) -> Result<CowMechanism, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let src = probe_parent.join(format!(".harness-cow-src-{stamp}"));
    let dst = probe_parent.join(format!(".harness-cow-dst-{stamp}"));

    write_probe_file(&src).map_err(|err| format!("failed to write COW probe source: {err}"))?;

    let result = attempt_platform_reflink(&src, &dst);
    let _ = fs::remove_file(&src);
    let _ = fs::remove_file(&dst);
    result
}

fn write_probe_file(path: &Path) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(b"harness-cow-probe-v1\n")?;
    file.sync_all()?;
    Ok(())
}

fn attempt_platform_reflink(src: &Path, dst: &Path) -> Result<CowMechanism, String> {
    #[cfg(target_os = "linux")]
    {
        return linux_reflink_clone(src, dst);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (src, dst);
        return Err("macos clonefile COW fastpath not implemented in this MVP; \
             standard git worktree / full copy only"
            .to_string());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (src, dst);
        Err(format!(
            "COW/reflink worktree fastpath unavailable on {}; use standard git worktree",
            current_platform_label()
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_reflink_clone(src: &Path, dst: &Path) -> Result<CowMechanism, String> {
    // Prefer `cp --reflink=always` when available: fails hard if COW is impossible.
    let output = std::process::Command::new("cp")
        .args(["--reflink=always", "--"])
        .arg(src)
        .arg(dst)
        .output()
        .map_err(|err| format!("cp --reflink=always not runnable: {err}"))?;

    if output.status.success() && dst.is_file() {
        let src_bytes = fs::read(src).map_err(|err| format!("read probe src: {err}"))?;
        let dst_bytes = fs::read(dst).map_err(|err| format!("read probe dst: {err}"))?;
        if src_bytes == dst_bytes {
            return Ok(CowMechanism::LinuxReflink);
        }
        return Err("reflink probe succeeded but content mismatch".to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "linux reflink unavailable (cp --reflink=always failed): {}",
        stderr.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_returns_structured_available_or_unavailable() {
        // Given
        let dir = tempdir().unwrap();

        // When
        let availability = detect_cow_worktree_fastpath(dir.path());

        // Then: always structured; never panic / silent bool
        assert!(availability.is_available() || availability.is_unavailable());
        match &availability {
            CowWorktreeAvailability::Available {
                mechanism,
                platform,
            } => {
                assert!(!platform.is_empty());
                assert_ne!(*mechanism, CowMechanism::FullCopy);
            }
            CowWorktreeAvailability::Unavailable { reason, platform } => {
                assert!(!reason.is_empty());
                assert!(!platform.is_empty());
            }
        }
    }

    #[test]
    fn missing_parent_that_cannot_be_created_is_unavailable() {
        // Given: path under a non-directory file as parent
        let dir = tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        fs::write(&file, b"x").unwrap();
        let probe = file.join("child");

        // When
        let availability = detect_cow_worktree_fastpath(&probe);

        // Then
        assert!(availability.is_unavailable());
    }

    #[test]
    fn mechanism_labels_are_stable() {
        assert_eq!(CowMechanism::LinuxReflink.as_str(), "linux_reflink");
        assert_eq!(CowMechanism::MacosClonefile.as_str(), "macos_clonefile");
        assert_eq!(CowMechanism::FullCopy.as_str(), "full_copy");
    }

    #[test]
    fn try_cow_clone_file_rejects_missing_source() {
        // Given
        let dir = tempdir().unwrap();
        let src = dir.path().join("missing.bin");
        let dst = dir.path().join("out.bin");

        // When
        let result = try_cow_clone_file(&src, &dst);

        // Then
        assert!(result.is_unavailable());
        assert!(!dst.exists());
    }

    #[test]
    fn try_cow_clone_file_rejects_existing_destination() {
        // Given
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");
        fs::write(&src, b"payload").unwrap();
        fs::write(&dst, b"existing").unwrap();

        // When
        let result = try_cow_clone_file(&src, &dst);

        // Then
        assert!(result.is_unavailable());
        assert_eq!(fs::read(&dst).unwrap(), b"existing");
    }

    #[test]
    fn try_cow_clone_file_matches_detect_availability() {
        // Given
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");
        fs::write(&src, b"harness-cow-clone-payload").unwrap();
        let detect = detect_cow_worktree_fastpath(dir.path());

        // When
        let result = try_cow_clone_file(&src, &dst);

        // Then: clone succeeds only when detect reported Available on this FS.
        match (&detect, &result) {
            (
                CowWorktreeAvailability::Available { mechanism, .. },
                CowCloneResult::Cloned {
                    mechanism: cloned_mech,
                    ..
                },
            ) => {
                assert_eq!(mechanism, cloned_mech);
                assert_eq!(fs::read(&dst).unwrap(), b"harness-cow-clone-payload");
            }
            (CowWorktreeAvailability::Unavailable { .. }, CowCloneResult::Unavailable { .. }) => {
                assert!(!dst.exists());
            }
            (detect, result) => {
                panic!("detect/clone availability mismatch: {detect:?} vs {result:?}");
            }
        }
    }

    #[test]
    fn cow_operator_diagnostics_cover_detect_clone_and_summary() {
        // Given
        let dir = tempdir().unwrap();
        let detect = detect_cow_worktree_fastpath(dir.path());
        let missing =
            try_cow_clone_file(&dir.path().join("missing.bin"), &dir.path().join("out.bin"));
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");
        fs::write(&src, b"payload").unwrap();
        let clone = try_cow_clone_file(&src, &dst);

        // When
        let summary = summarize_cow_clone_outcomes(&[missing.clone(), clone.clone()]);

        // Then
        assert!(
            detect.one_line().contains("COW worktree fastpath:")
                && (detect.one_line().contains("available")
                    || detect.one_line().contains("unavailable"))
        );
        assert!(missing.is_unavailable());
        assert!(missing.one_line().contains("COW clone: unavailable"));
        assert!(clone.one_line().contains("COW clone:"));
        assert_eq!(summary.total, 2);
        assert_eq!(summary.unavailable + summary.cloned, 2);
        assert!(summary.one_line().contains("2 total"));
        if clone.is_cloned() {
            assert!(summary.has_cloned());
            assert!(clone.one_line().contains("cloned"));
        } else {
            assert!(clone.is_unavailable());
            assert!(clone.one_line().contains("unavailable"));
        }
    }

    #[test]
    fn try_cow_clone_tree_materializes_nested_files_when_available() {
        // Given: nested source tree with real files
        let dir = tempdir().unwrap();
        let src = dir.path().join("src-tree");
        let dst = dir.path().join("dst-tree");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("root.txt"), b"root-payload").unwrap();
        fs::write(src.join("nested/child.txt"), b"child-payload").unwrap();
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(src.join(".git/config"), b"should-skip").unwrap();
        let detect = detect_cow_worktree_fastpath(dir.path());

        // When
        let result = try_cow_clone_tree(&src, &dst);

        // Then: matches detect; real files when COW works; never copies .git
        match (&detect, &result) {
            (
                CowWorktreeAvailability::Available { mechanism, .. },
                CowTreeCloneResult::Cloned {
                    mechanism: cloned_mech,
                    files_cloned,
                    dirs_created,
                    ..
                },
            ) => {
                assert_eq!(mechanism, cloned_mech);
                assert_eq!(*files_cloned, 2);
                assert!(*dirs_created >= 1);
                assert_eq!(fs::read(dst.join("root.txt")).unwrap(), b"root-payload");
                assert_eq!(
                    fs::read(dst.join("nested/child.txt")).unwrap(),
                    b"child-payload"
                );
                assert!(!dst.join(".git").exists());
            }
            (
                CowWorktreeAvailability::Unavailable { .. },
                CowTreeCloneResult::Unavailable { .. },
            ) => {
                assert!(!dst.exists());
            }
            (detect, result) => {
                panic!("detect/tree availability mismatch: {detect:?} vs {result:?}");
            }
        }
        assert!(result.one_line().contains("COW tree:"));
    }

    #[test]
    fn try_cow_clone_tree_rejects_existing_destination() {
        // Given
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"a").unwrap();
        fs::create_dir_all(&dst).unwrap();

        // When
        let result = try_cow_clone_tree(&src, &dst);

        // Then
        assert!(result.is_unavailable());
        assert!(result.one_line().contains("already exists"));
    }

    #[test]
    fn apply_cow_worktree_fastpath_overlays_real_files_into_worktree_path() {
        // Given: repo + worktree-like dest with one missing overlay file
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        let worktree = dir.path().join("worktree");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(repo.join("overlay.bin"), b"overlay-payload").unwrap();
        fs::write(repo.join("missing-in-worktree.txt"), b"local-only").unwrap();

        // When
        let report = apply_cow_worktree_fastpath(
            &repo,
            &worktree,
            &["overlay.bin", "missing-in-worktree.txt", "absent.bin"],
        );

        // Then: structured availability + real overlay outcomes
        assert!(report.availability.is_available() || report.availability.is_unavailable());
        assert_eq!(report.overlays.len(), 3);
        assert!(report.overlays[2].is_unavailable());
        assert!(report.one_line().contains("COW worktree fastpath:"));
        let detect = detect_cow_worktree_fastpath(&worktree);
        if detect.is_available() {
            assert!(report.overlays[0].is_cloned() || report.overlays[0].is_unavailable());
            if report.overlays[0].is_cloned() {
                assert_eq!(
                    fs::read(worktree.join("overlay.bin")).unwrap(),
                    b"overlay-payload"
                );
                assert!(report.has_cloned_overlay());
            }
        } else {
            assert!(report.overlays.iter().all(CowCloneResult::is_unavailable));
        }
    }

    #[test]
    fn materialize_cow_workspace_tree_reports_tree_clone() {
        // Given
        let dir = tempdir().unwrap();
        let src = dir.path().join("workspace");
        let dst = dir.path().join("cow-checkout");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/file.txt"), b"workspace").unwrap();

        // When
        let report = materialize_cow_workspace_tree(&src, &dst);

        // Then
        assert!(report.tree_clone.is_some());
        let tree = report.tree_clone.as_ref().unwrap();
        assert!(tree.one_line().contains("COW tree:"));
        if tree.is_cloned() {
            assert_eq!(fs::read(dst.join("sub/file.txt")).unwrap(), b"workspace");
            assert!(report.has_cloned_overlay());
        } else {
            assert!(!dst.exists());
        }
    }
}
