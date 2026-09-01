use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

const P0_06_PROVENANCE_PATHS: &[&str] = &[
    "crates/harness-tui/tests/pty_e2e.rs",
    "crates/harness-tui/tests/support/p0_06_artifact_support.rs",
    "crates/harness-tui/tests/support/p0_06_artifacts.rs",
    "crates/harness-tui/tests/support/p0_06_terminal_emulator.rs",
    "crates/harness/tests/test_lanes_script_test.rs",
    "scripts/test-lanes.sh",
    "scripts/qa/p0-06-xterm.test.mjs",
    "scripts/qa/web-terminal-visual-qa.mjs",
    "scripts/qa/web-terminal-visual-qa.test.mjs",
];

pub(crate) fn assert_manifest_receipts(root: &Path, manifest: &Value) {
    let captures = manifest["captures"].as_array().unwrap_or_abort();
    for capture in captures {
        for phase in ["before", "after"] {
            for artifact in ["ansi", "text", "screen"] {
                assert_receipt(root, &capture[phase][artifact]);
            }
        }
        assert_receipt(root, &capture["cleanup"]);
    }
    assert_receipt(root, &manifest["cleanupReceipt"]);
}

pub(crate) fn write_json(path: impl AsRef<Path>, value: &Value) {
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(value).unwrap_or_abort()
        ),
    )
    .unwrap_or_abort();
}

pub(crate) fn receipt(root: &Path, path: &Path) -> Value {
    json!({
        "path": path.strip_prefix(root).unwrap_or_abort(),
        "bytes": fs::metadata(path).unwrap_or_abort().len(),
        "sha256": hash_file(path),
    })
}

pub(crate) fn hash_file(path: &Path) -> String {
    digest_hex(fs::read(path).unwrap_or_abort())
}

pub(crate) fn source_tree() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    source_tree_at(&root)
}

fn source_tree_at(root: &Path) -> Value {
    let head = git(root, &["rev-parse", "HEAD"]);
    let head_tree = git(root, &["rev-parse", "HEAD^{tree}"]);
    let (working_tree_sha256, files) = tracked_working_tree_hash(root);
    let p0_06_diff_sha256 = p0_06_diff_hash(root);
    let mut status_args = vec!["status", "--porcelain=v1", "--untracked-files=all", "--"];
    status_args.extend(P0_06_PROVENANCE_PATHS);
    let status_sha256 = digest_hex(git(root, &status_args));
    json!({
        "head": head,
        "headTree": head_tree,
        "workingTreeSha256": working_tree_sha256,
        "files": files,
        "statusSha256": status_sha256,
        "p0_06DiffSha256": p0_06_diff_sha256,
    })
}

fn git(root: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap_or_abort()
            .stdout,
    )
    .unwrap_or_abort()
    .trim()
    .to_string()
}

fn tracked_working_tree_hash(root: &Path) -> (String, usize) {
    let listed = Command::new("git")
        .args(["ls-files", "--cached", "-z"])
        .current_dir(root)
        .output()
        .unwrap_or_abort()
        .stdout;
    let mut paths = listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    let mut digest = Sha256::new();
    let mut files = 0;
    for path in paths {
        let relative = String::from_utf8(path.to_vec()).unwrap_or_abort();
        let absolute = root.join(&relative);
        if !absolute.is_file() {
            continue;
        }
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(fs::read(absolute).unwrap_or_abort());
        digest.update([0]);
        files += 1;
    }
    (digest_bytes_hex(digest.finalize()), files)
}

fn p0_06_diff_hash(root: &Path) -> String {
    let mut digest = Sha256::new();
    let mut diff_args = vec!["diff", "--binary", "HEAD", "--"];
    diff_args.extend(P0_06_PROVENANCE_PATHS);
    digest.update(
        Command::new("git")
            .args(diff_args)
            .current_dir(root)
            .output()
            .unwrap_or_abort()
            .stdout,
    );

    let mut untracked_args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    untracked_args.extend(P0_06_PROVENANCE_PATHS);
    let listed = Command::new("git")
        .args(untracked_args)
        .current_dir(root)
        .output()
        .unwrap_or_abort()
        .stdout;
    let mut paths = listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    for path in paths {
        let relative = String::from_utf8(path.to_vec()).unwrap_or_abort();
        digest.update(b"untracked\0");
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(fs::read(root.join(&relative)).unwrap_or_abort());
        digest.update([0]);
    }
    digest_bytes_hex(digest.finalize())
}

fn assert_receipt(root: &Path, receipt: &Value) {
    let path = root.join(receipt["path"].as_str().unwrap_or_abort());
    assert!(
        path.is_file(),
        "manifest artifact is missing: {}",
        path.display()
    );
    assert_eq!(receipt["sha256"].as_str(), Some(hash_file(&path).as_str()));
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    digest_bytes_hex(Sha256::digest(bytes))
}

fn digest_bytes_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes.as_ref() {
        write!(&mut output, "{byte:02x}").unwrap_or_abort();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{source_tree_at, P0_06_PROVENANCE_PATHS};
    use harness_tui::UnwrapOrAbort;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn source_tree_ignores_unrelated_untracked_files() {
        // Given: a repository with a committed P0-06 owner.
        let repository = tempdir().unwrap_or_abort();
        initialize_repository(repository.path());
        let owner = repository.path().join(P0_06_PROVENANCE_PATHS[0]);
        fs::create_dir_all(owner.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(&owner, "owner\n").unwrap_or_abort();
        commit(repository.path(), "owner");
        let before = source_tree_at(repository.path());

        // When: an unrelated untracked report is added.
        let report = repository.path().join("research report");
        fs::create_dir_all(&report).unwrap_or_abort();
        fs::write(report.join("SYNTHESIS.md"), "unrelated\n").unwrap_or_abort();
        let after = source_tree_at(repository.path());

        // Then: provenance remains byte-for-byte stable.
        assert_eq!(before, after);
    }

    #[test]
    fn source_tree_binds_explicit_p0_06_diff_state() {
        // Given: a repository with a committed P0-06 owner.
        let repository = tempdir().unwrap_or_abort();
        initialize_repository(repository.path());
        let owner = repository.path().join(P0_06_PROVENANCE_PATHS[0]);
        fs::create_dir_all(owner.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(&owner, "owner\n").unwrap_or_abort();
        commit(repository.path(), "owner");
        let before = source_tree_at(repository.path());

        // When: the explicit P0-06 owner diff changes.
        fs::write(owner, "changed owner\n").unwrap_or_abort();
        let after = source_tree_at(repository.path());

        // Then: the P0-06 diff digest changes.
        assert_ne!(before["p0_06DiffSha256"], after["p0_06DiffSha256"]);
    }

    fn initialize_repository(root: &Path) {
        run_git(root, ["init", "-q"]);
        run_git(root, ["config", "user.email", "test@example.com"]);
        run_git(root, ["config", "user.name", "P0-06 tests"]);
    }

    fn commit(root: &Path, message: &str) {
        run_git(root, ["add", "."]);
        run_git(root, ["commit", "-qm", message]);
    }

    fn run_git<const N: usize>(root: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap_or_abort();
        assert!(output.status.success(), "git failed: {output:?}");
    }
}
