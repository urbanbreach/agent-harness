use harness_testkit::secret_scanner::{
    default_forbidden_patterns, scan_directories_for_secrets, SecretFinding,
};
use harness_testkit::UnwrapOrAbort;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[path = "support/repo_root.rs"]
mod repo_root;

use repo_root::repo_root;

mod secretscan {
    use super::*;

    #[test]
    fn secret_scan_does_not_find_api_keys_in_artifacts() {
        let repo_root = repo_root();

        let mut scan_roots = snapshot_dirs(repo_root.join("crates").as_path()).unwrap_or_abort();
        scan_roots.extend(committed_cassette_dirs(&repo_root).unwrap_or_abort());
        scan_roots.extend(explicit_artifact_roots(&repo_root));
        scan_roots.sort();

        let patterns = default_forbidden_patterns().unwrap_or_abort();
        let findings = scan_directories_for_secrets(&scan_roots, &patterns).unwrap_or_abort();

        assert!(
            findings.is_empty(),
            "secret scan found forbidden content:\n{}",
            format_findings(&repo_root, &findings)
        );
    }
}

fn explicit_artifact_roots(repo_root: &Path) -> Vec<PathBuf> {
    if std::env::var_os("HARNESS_SECRETS_SCAN_ARTIFACTS").is_none() {
        return Vec::new();
    }

    let mut roots = pty_temp_session_dirs();
    roots.push(repo_root.join("target").join("pty-visual-artifacts"));
    if let Some(path) = std::env::var_os("HARNESS_VISUAL_ARTIFACT_DIR") {
        roots.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("HARNESS_SIMULATION_ARTIFACT_DIR") {
        roots.push(PathBuf::from(path));
    }
    roots
}

fn pty_temp_session_dirs() -> Vec<PathBuf> {
    let base = std::env::temp_dir().join("harness-testkit");
    if !base.exists() {
        return Vec::new();
    }

    let mut dirs: Vec<PathBuf> = match fs::read_dir(&base) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.starts_with("pty-e2e-"))
                        .unwrap_or(false)
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    dirs.sort();
    dirs
}

fn committed_cassette_dirs(repo_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    collect_named_dirs(&repo_root.join("crates"), "cassettes", &mut found)?;
    found.retain(|path| path.components().any(|part| part.as_os_str() == "fixtures"));
    found.sort();
    Ok(found)
}

fn snapshot_dirs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    collect_named_dirs(root, "snapshots", &mut found)?;
    found.sort();
    Ok(found)
}

fn collect_named_dirs(root: &Path, wanted_name: &str, found: &mut Vec<PathBuf>) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(());
    }

    if root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == wanted_name)
        .unwrap_or(false)
    {
        found.push(root.to_path_buf());
    }

    let mut children: Vec<PathBuf> = fs::read_dir(root)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    children.sort();

    for child in children {
        collect_named_dirs(&child, wanted_name, found)?;
    }

    Ok(())
}

fn format_findings(repo_root: &Path, findings: &[SecretFinding]) -> String {
    findings
        .iter()
        .map(|finding| {
            let relative = finding
                .file
                .strip_prefix(repo_root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| finding.file.clone());
            format!(
                "- {}:{} matched `{}`",
                relative.display(),
                finding.line_number,
                finding.pattern
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
