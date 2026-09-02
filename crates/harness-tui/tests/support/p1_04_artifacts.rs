use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const OWNER_SOURCES: &[&str] = &[
    "crates/harness-tui/tests/p1_04_pty_recorded.rs",
    "crates/harness-tui/tests/support/p1_04_pty.rs",
    "crates/harness-tui/tests/support/p1_04_recorded_owner.rs",
    "crates/harness-tui/tests/support/p1_04_session.rs",
    "crates/harness-tui/tests/support/p1_04_terminal.rs",
    "crates/harness-tui/tests/support/p1_04_artifacts.rs",
];

pub(crate) fn artifact_root() -> PathBuf {
    if let Some(root) = std::env::var_os("HARNESS_P1_04_ARTIFACT_DIR") {
        let root = PathBuf::from(root);
        assert!(root.is_absolute(), "artifact root must be absolute");
        return root;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p1-04-native-pty")
        .join(std::process::id().to_string())
}

/// Marker proving this owner created the artifact root. The lane may
/// pre-create an empty stage directory, so emptiness also proves ownership.
const OWNERSHIP_MARKER: &str = ".harness-p1-04-artifact-root";

/// Guards `remove_dir_all` against an operator-supplied root: canonical
/// paths must stay strictly inside the repository `target/` tree or the
/// platform temporary directory (canonicalization defeats symlink
/// redirects), and protected roots are refused outright.
pub(crate) fn validated_artifact_root(candidate: &Path) -> PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repository = repository.canonicalize().unwrap_or_abort();
    let canonical = canonical_with_missing_tail(candidate);
    let temporary = std::env::temp_dir().canonicalize().unwrap_or_abort();
    assert_ne!(
        temporary,
        Path::new("/"),
        "refusing artifact roots: platform temporary directory is the filesystem root"
    );

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let protected = [
        Some(repository.parent().unwrap_or_abort().to_path_buf()),
        home,
    ];
    for forbidden in protected.into_iter().flatten() {
        let forbidden = forbidden.canonicalize().unwrap_or_abort();
        assert!(
            canonical != forbidden,
            "refusing artifact root {}: protected path",
            candidate.display()
        );
    }

    let target_root = repository.join("target");
    let inside_sanctioned_root = canonical
        .strip_prefix(&target_root)
        .or_else(|_| canonical.strip_prefix(&temporary))
        .is_ok_and(|relative| !relative.as_os_str().is_empty());
    assert!(
        inside_sanctioned_root,
        "refusing artifact root {}: must live under the repository target/ tree or the platform temporary directory",
        candidate.display()
    );
    canonical
}

/// Canonicalizes through missing tail components so a first-run root that
/// does not exist yet is still validated against its existing ancestors.
fn canonical_with_missing_tail(candidate: &Path) -> PathBuf {
    if candidate.exists() {
        return candidate.canonicalize().unwrap_or_abort();
    }
    let mut existing = candidate.to_path_buf();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(existing.file_name().unwrap_or_abort().to_os_string());
        existing = existing.parent().unwrap_or_abort().to_path_buf();
    }
    let mut canonical = existing.canonicalize().unwrap_or_abort();
    while let Some(component) = missing.pop() {
        canonical.push(component);
    }
    canonical
}

/// Deletes previous owner output under `root` only when the ownership
/// marker (or pre-created emptiness) proves this lane owns the tree.
pub(crate) fn reset_artifact_root(root: &Path) {
    if root.exists() {
        let owned = root.join(OWNERSHIP_MARKER).is_file()
            || fs::read_dir(root)
                .map(|entries| entries.count() == 0)
                .unwrap_or(false);
        assert!(
            owned,
            "refusing to reset {}: missing ownership marker {OWNERSHIP_MARKER}",
            root.display()
        );
        fs::remove_dir_all(root).unwrap_or_abort();
    }
    fs::create_dir_all(root).unwrap_or_abort();
    fs::write(root.join(OWNERSHIP_MARKER), b"").unwrap_or_abort();
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

pub(crate) fn owner_provenance() -> Value {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sources = OWNER_SOURCES
        .iter()
        .map(|relative| {
            let path = repository.join(relative);
            json!({ "path": relative, "sha256": hash_file(&path) })
        })
        .collect::<Vec<_>>();
    json!({
        "testOwner": sources.first().unwrap_or_abort(),
        "supportOwners": &sources[1..],
    })
}

pub(crate) fn assert_manifest_receipts(root: &Path, manifest: &Value) {
    for capture in manifest["captures"].as_array().unwrap_or_abort() {
        for state in ["following", "detached", "resizeBurstFinal", "reducedMotion"] {
            for artifact in ["ansi", "text", "screen"] {
                assert_receipt(root, &capture["states"][state][artifact]);
            }
        }
        assert_receipt(root, &capture["cleanup"]);
        assert_receipt(root, &capture["brandReceipt"]);
    }
    assert_receipt(root, &manifest["cleanupReceipt"]);
}

fn assert_receipt(root: &Path, value: &Value) {
    let path = root.join(value["path"].as_str().unwrap_or_abort());
    assert!(path.is_file(), "missing artifact: {}", path.display());
    assert_eq!(value["sha256"].as_str(), Some(hash_file(&path).as_str()));
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").unwrap_or_abort();
    }
    output
}
