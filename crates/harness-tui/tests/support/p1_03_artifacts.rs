use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const OWNER_SOURCES: &[&str] = &[
    "crates/harness-tui/tests/p1_03_pty_recorded.rs",
    "crates/harness-tui/tests/support/p1_03_pty.rs",
    "crates/harness-tui/tests/support/p1_03_recorded_owner.rs",
    "crates/harness-tui/tests/support/p1_03_session.rs",
    "crates/harness-tui/tests/support/p1_03_terminal.rs",
    "crates/harness-tui/tests/support/p1_03_artifacts.rs",
];

pub(crate) fn artifact_root() -> PathBuf {
    if let Some(root) = std::env::var_os("HARNESS_P1_03_ARTIFACT_DIR") {
        let root = PathBuf::from(root);
        assert!(root.is_absolute(), "artifact root must be absolute");
        return root;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p1-03-native-pty")
        .join(std::process::id().to_string())
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

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").unwrap_or_abort();
    }
    output
}
