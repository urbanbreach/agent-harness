use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use super::binary_name::binary_name;
pub(crate) use super::repo_root::repo_root;

pub(crate) fn resolve_harness_bin() -> PathBuf {
    static HARNESS_BIN_CACHE: OnceLock<PathBuf> = OnceLock::new();
    HARNESS_BIN_CACHE
        .get_or_init(|| {
            if let Ok(path) = env::var("HARNESS_BIN") {
                let harness_bin = PathBuf::from(path);
                assert!(
                    harness_bin.exists(),
                    "HARNESS_BIN points to missing path: {}",
                    harness_bin.display()
                );
                return harness_bin;
            }

            if let Some(path) = option_env!("CARGO_BIN_EXE_harness") {
                let harness_bin = PathBuf::from(path);
                if harness_bin.exists() {
                    return harness_bin;
                }
            }

            let repo = repo_root();
            let harness_bin = repo
                .join("target")
                .join("debug")
                .join(binary_name("harness"));
            let status = Command::new("cargo")
                .arg("build")
                .arg("-p")
                .arg("harness")
                .current_dir(&repo)
                .status()
                .expect("spawn cargo build -p harness");
            assert!(
                status.success(),
                "cargo build -p harness failed with status {status}"
            );
            assert!(
                harness_bin.exists(),
                "expected harness binary at {}",
                harness_bin.display()
            );
            harness_bin
        })
        .clone()
}
