//! Wave 0 freeze receipt: record the checkout identity used for parity work.
//!
//! The receipt is written to `target/parity-freeze/receipt.json` and contains
//! the branch, HEAD, toolchain, reference binary digest, and any expected dirty
//! files. Subsequent runs verify the recorded values so that parity evidence can
//! be tied to a single immutable source revision.

#![allow(
    clippy::panic,
    reason = "fail-closed receipt validation helpers use explicit panics"
)]

use harness::UnwrapOrAbort;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::repo_root;

const REFERENCE_BINARY_REL: &str = "inspirations/grok-build/target/debug/xai-grok-pager";
const PINNED_REFERENCE_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const PINNED_REFERENCE_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";
const RECEIPT_DIR_REL: &str = "target/parity-freeze";
const RECEIPT_FILE: &str = "receipt-v2.json";

fn reference_binary_path(root: &Path) -> PathBuf {
    root.join(REFERENCE_BINARY_REL)
}

fn receipt_path(root: &Path) -> PathBuf {
    root.join(RECEIPT_DIR_REL).join(RECEIPT_FILE)
}

#[allow(clippy::panic, reason = "fail-closed test helper")]
fn file_sha256(path: &Path) -> String {
    let bytes =
        std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[allow(clippy::panic, reason = "fail-closed test helper")]
fn read_git_identity(root: &Path) -> (String, String) {
    let head_path = root.join(".git").join("HEAD");
    let head_ref = std::fs::read_to_string(&head_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", head_path.display()))
        .trim()
        .to_owned();

    if let Some(branch_ref) = head_ref.strip_prefix("ref: ") {
        let branch_path = branch_ref
            .split('/')
            .next_back()
            .unwrap_or("HEAD")
            .to_owned();
        let resolved_path = root.join(".git").join(branch_ref);
        let head = std::fs::read_to_string(&resolved_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", resolved_path.display()))
            .trim()
            .to_owned();
        (branch_path, head)
    } else {
        ("HEAD".to_owned(), head_ref)
    }
}

#[allow(clippy::panic, reason = "fail-closed test helper")]
fn read_toolchain(root: &Path) -> String {
    let path = root.join("rust-toolchain.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
        .trim()
        .to_owned()
}

#[allow(clippy::panic, reason = "fail-closed test helper")]
fn read_dirty_files(root: &Path) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for args in [
        ["diff", "--name-only", "-z", "HEAD"].as_slice(),
        ["ls-files", "--others", "--exclude-standard", "-z"].as_slice(),
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("failed to inspect worktree with git: {error}"));
        assert!(
            output.status.success(),
            "git worktree inspection failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for path in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            paths.insert(
                String::from_utf8(path.to_vec()).unwrap_or_else(|error| {
                    panic!("git returned a non-UTF-8 worktree path: {error}")
                }),
            );
        }
    }
    paths.into_iter().collect()
}

#[allow(clippy::panic, reason = "fail-closed test helper")]
fn build_receipt() -> Value {
    let root = repo_root();
    let binary_path = reference_binary_path(&root);
    let (branch, head) = read_git_identity(&root);
    let dirty_files = read_dirty_files(&root);

    serde_json::json!({
        "schema_version": "harness-parity-freeze-receipt-v2",
        "branch": branch,
        "head": head,
        "reference_binary_rel": REFERENCE_BINARY_REL,
        "reference_binary_path": binary_path.display().to_string(),
        "reference_binary_sha256": file_sha256(&binary_path),
        "reference_binary_version": PINNED_REFERENCE_VERSION,
        "toolchain": read_toolchain(&root),
        "rust_toolchain_file": "rust-toolchain.toml",
        "dirty_files": dirty_files,
        "safety_stash": "recorded manually; see stash list",
        "created_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_abort()
            .as_secs(),
    })
}

#[test]
fn parity_freeze_reference_binary_matches_pinned_digest() {
    // arrange
    let root = repo_root();
    let path = reference_binary_path(&root);

    // act
    let actual = file_sha256(&path);

    // assert
    assert!(
        path.is_file(),
        "pinned reference binary is missing: {}",
        path.display()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("metadata for {}: {e}", path.display()))
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "reference binary is not executable: {}",
            path.display()
        );
    }

    assert_eq!(
        actual, PINNED_REFERENCE_SHA256,
        "reference binary digest mismatch; if the reference changed, update the contract and receipt"
    );
}

#[test]
fn parity_freeze_receipt_records_checkout_identity() {
    // arrange
    let root = repo_root();
    let receipt = receipt_path(&root);

    // act: create receipt on first run, then verify it matches current checkout
    if !receipt.is_file() {
        let parent = receipt.parent().unwrap_or_abort();
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
        let doc = build_receipt();
        let json = serde_json::to_string_pretty(&doc).unwrap_or_abort();
        std::fs::write(&receipt, json)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", receipt.display()));
    }

    let raw = std::fs::read_to_string(&receipt)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", receipt.display()));
    let doc: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", receipt.display()));

    let (current_branch, current_head) = read_git_identity(&root);
    let current_dirty = read_dirty_files(&root);
    let recorded_dirty: Vec<String> = doc["dirty_files"]
        .as_array()
        .unwrap_or_else(|| panic!("receipt missing dirty_files array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("dirty_files entry must be a string"))
                .to_owned()
        })
        .collect();

    // assert
    assert_eq!(
        doc["reference_binary_sha256"].as_str(),
        Some(PINNED_REFERENCE_SHA256),
        "receipt reference digest mismatch"
    );
    assert_eq!(
        doc["reference_binary_version"].as_str(),
        Some(PINNED_REFERENCE_VERSION),
        "receipt reference version mismatch"
    );
    assert_eq!(
        doc["head"].as_str(),
        Some(current_head.as_str()),
        "receipt HEAD mismatch; parity evidence must come from one revision"
    );
    assert_eq!(
        doc["branch"].as_str(),
        Some(current_branch.as_str()),
        "receipt branch mismatch"
    );
    assert_eq!(
        recorded_dirty, current_dirty,
        "receipt dirty_files mismatch; the checkout changed after the parity freeze was created"
    );
}
